//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! A peer-to-peer connection interface.

use std::cmp::min;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime};

use futures::sync::mpsc::{Receiver, Sender};
use futures::Future;
use tokio::prelude::*;
use tokio::runtime;
use tokio::timer::Interval;

use crate::common::{
    units::DataRate,
    BandwidthMode,
    CallDirection,
    CallId,
    CallMediaType,
    ConnectionState,
    DeviceId,
    FeatureLevel,
    Result,
    RingBench,
    DATA_CHANNEL_NAME,
};
use crate::core::call::Call;
use crate::core::call_mutex::CallMutex;
use crate::core::connection_fsm::{ConnectionEvent, ConnectionStateMachine};
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::core::util::ptr_as_box;
use crate::error::RingRtcError;

use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::ice_gatherer::IceGatherer;
use crate::webrtc::media::MediaStream;
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::sdp_observer::{
    create_csd_observer,
    create_ssd_observer,
    SessionDescriptionInterface,
};
use crate::webrtc::stats_observer::{create_stats_observer, StatsObserver};

/// The periodic tick interval. Used to generate stats and to retransmit data channel messages.
pub const TICK_PERIOD_SEC: u64 = 1;

/// The stats period, how often to get and log them. Assumes tick period is 1 second.
pub const STATS_PERIOD_SEC: u64 = 10;

/// Connection observer status notification types
/// Sent from the Connection to the parent Call object
#[derive(Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionObserverEvent {
    /// ICE negotiation is complete and a DataChannel is also ready.
    /// The Call uses this to know when it should transition to the
    /// Ringing state.
    ConnectedWithDataChannelBeforeAccepted,

    /// The remote side sent an accepted message via the data channel.
    ReceivedAcceptedViaDataChannel,

    /// The remote side sent a sender status message via the data channel.
    ReceivedSenderStatusViaDataChannel(bool),

    /// The remote side sent a hangup message via the data channel
    /// or via signaling.
    ReceivedHangup(signaling::Hangup),

    /// The call failed to connect during ICE negotiation.
    IceFailed,

    /// The connection temporarily disconnected and it attempting to reconnect.
    ReconnectingAfterAccepted,

    /// The connection temporarily disconnected and has now reconnecting.
    ReconnectedAfterAccepted,
}

impl Clone for ConnectionObserverEvent {
    fn clone(&self) -> Self {
        *self
    }
}

impl fmt::Display for ConnectionObserverEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Encapsulates several WebRTC objects associated with the
/// Connection object.
struct WebRtcData<T>
where
    T: Platform,
{
    /// PeerConnection object
    pc_interface:          Option<PeerConnection>,
    /// DataChannel object
    data_channel:          Option<DataChannel>,
    /// DataChannelObserver object
    data_channel_observer: Option<DataChannelObserver<T>>,
    /// Raw pointer to Connection object for PeerConnectionObserver
    connection_ptr:        Option<*mut Connection<T>>,
    /// Application-specific incoming media
    incoming_media:        Option<<T as Platform>::AppIncomingMedia>,
    /// Application specific peer connection
    app_connection:        Option<<T as Platform>::AppConnection>,
    /// Boxed copy of the stats collector object shared for callbacks.
    stats_observer:        Option<Box<StatsObserver>>,
    /// The current maximum bitrate setting for the local endpoint.
    local_max_bitrate:     DataRate,
    /// The current maximum bitrate setting for the remote endpoint.
    remote_max_bitrate:    DataRate,
}

// Send and Sync needed to share *const pointer types across threads.
unsafe impl<T> Send for WebRtcData<T> where T: Platform {}

unsafe impl<T> Sync for WebRtcData<T> where T: Platform {}

impl<T> fmt::Display for WebRtcData<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "pc_interface: {:?}, data_channel: {:?}, data_channel_observer: {:?}, connection_ptr: {:?}, local_max_bitrate: {:?}, remote_max_bitrate: {:?}",
               self.pc_interface,
               self.data_channel,
               self.data_channel_observer,
               self.connection_ptr,
               self.local_max_bitrate,
               self.remote_max_bitrate)
    }
}

impl<T> fmt::Debug for WebRtcData<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> WebRtcData<T>
where
    T: Platform,
{
    fn pc_interface(&self) -> Result<&PeerConnection> {
        match self.pc_interface.as_ref() {
            Some(v) => Ok(v),
            None => Err(RingRtcError::OptionValueNotSet(
                "pc_interface".to_string(),
                "pc_interface".to_string(),
            )
            .into()),
        }
    }

    fn data_channel(&self) -> Result<&DataChannel> {
        match self.data_channel.as_ref() {
            Some(v) => Ok(v),
            None => Err(RingRtcError::OptionValueNotSet(
                "data_channel".to_string(),
                "data_channel".to_string(),
            )
            .into()),
        }
    }
}

/// Encapsulates the FSM and runtime upon which a Connection runs.
struct Context {
    /// Runtime upon which the ConnectionStateMachine runs.
    pub worker_runtime: runtime::Runtime,
}

impl Context {
    fn new() -> Result<Self> {
        Ok(Self {
            worker_runtime: runtime::Builder::new()
                .core_threads(1)
                .name_prefix("worker".to_string())
                .build()?,
        })
    }
}

/// A mpsc::Receiver for receiving ConnectionEvents in the
/// [ConnectionStateMachine](../call_fsm/struct.CallStateMachine.html)
///
/// The event stream is the tuple (Connection, ConnectionEvent).
pub type EventStream<T> = Receiver<(Connection<T>, ConnectionEvent)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionType {
    // As a caller, the parent connection signals to all remote devices.
    // This is like "signaling mode == broadcast".
    OutgoingParent,
    // As a caller, the child connections don't signal anything.
    // This is like "signaling mode == disabled".
    OutgoingChild,
    // As a callee, the connection signals to one remote device.
    Incoming,
    // This is like "signaling mode == unicast".
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConnectionId {
    call_id:          CallId,
    remote_device_id: DeviceId,
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.call_id, self.remote_device_id)
    }
}

impl ConnectionId {
    pub fn new(call_id: CallId, remote_device_id: DeviceId) -> Self {
        Self {
            call_id,
            remote_device_id,
        }
    }

    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    pub fn remote_device_id(&self) -> DeviceId {
        self.remote_device_id
    }
}

/// Encapsulates the tick timer and runtime.
struct TickContext {
    /// Tokio runtime for background task execution of periodic ticks.
    runtime:       Option<runtime::Runtime>,
    /// Periodic tick counter.
    ticks_elapsed: u64,
}

impl TickContext {
    /// Create a new TickContext.
    pub fn new() -> Self {
        Self {
            runtime:       None,
            ticks_elapsed: 0,
        }
    }
}

/// Represents the connection between a local client and one remote
/// peer.
///
/// This object is thread-safe.
pub struct Connection<T>
where
    T: Platform,
{
    /// The parent Call object of this connection.
    call:                           Arc<CallMutex<Call<T>>>,
    /// Injects events into the [ConnectionStateMachine](../call_fsm/struct.CallStateMachine.html).
    fsm_sender:                     Sender<(Connection<T>, ConnectionEvent)>,
    /// Kept around between new() and start() so we can delay the starting of the FSM
    /// but queue events that happen while starting.
    fsm_receiver:                   Option<Receiver<(Connection<T>, ConnectionEvent)>>,
    /// Unique 64-bit number identifying the call.
    call_id:                        CallId,
    /// Device ID of the remote device.
    remote_feature_level:           Arc<CallMutex<FeatureLevel>>,
    /// Connection ID, identifying the call and remote_device.
    connection_id:                  ConnectionId,
    /// The call direction, inbound or outbound.
    direction:                      CallDirection,
    /// The current state of the call connection
    state:                          Arc<CallMutex<ConnectionState>>,
    /// Execution context for the call connection FSM
    context:                        Arc<CallMutex<Context>>,
    /// Ancillary WebRTC data.
    webrtc:                         Arc<CallMutex<WebRtcData<T>>>,
    /// Local ICE candidates waiting to be sent over signaling.
    buffered_local_ice_candidates:  Arc<CallMutex<Vec<signaling::IceCandidate>>>,
    /// Remote ICE candidates waiting to be added to the PeerConnection.
    buffered_remote_ice_candidates: Arc<CallMutex<Vec<signaling::IceCandidate>>>,
    /// Condition variable used at termination to quiesce and synchronize the FSM.
    terminate_condvar:              Arc<(Mutex<bool>, Condvar)>,
    /// This is write-once configuration and will not change.
    connection_type:                ConnectionType,
    /// Execution context for the connection periodic timer tick
    tick_context:                   Arc<CallMutex<TickContext>>,
}

impl<T> fmt::Display for Connection<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let webrtc = match self.webrtc.lock() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        let state = match self.state() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        write!(
            f,
            "thread: {:?}, connection_id: {}, direction: {}, state: {}, webrtc: ({})",
            thread::current().id(),
            self.connection_id,
            self.direction,
            state,
            webrtc
        )
    }
}

impl<T> fmt::Debug for Connection<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Drop for Connection<T>
where
    T: Platform,
{
    fn drop(&mut self) {
        if self.ref_count() == 1 {
            info!(
                "Connection: Dropping last reference: {}",
                self.connection_id
            );
        } else {
            debug!(
                "Dropping Connection: {}, ref_count: {}",
                self.connection_id,
                self.ref_count()
            );
        }
    }
}

impl<T> Clone for Connection<T>
where
    T: Platform,
{
    fn clone(&self) -> Self {
        Connection {
            call:                           Arc::clone(&self.call),
            fsm_sender:                     self.fsm_sender.clone(),
            // Clones shouldn't need the Receiver because it's only used
            // for the one reference that is used by the creator between
            // creation and starting.
            fsm_receiver:                   None,
            call_id:                        self.call_id,
            remote_feature_level:           Arc::clone(&self.remote_feature_level),
            connection_id:                  self.connection_id,
            direction:                      self.direction,
            state:                          Arc::clone(&self.state),
            context:                        Arc::clone(&self.context),
            webrtc:                         Arc::clone(&self.webrtc),
            buffered_local_ice_candidates:  Arc::clone(&self.buffered_local_ice_candidates),
            buffered_remote_ice_candidates: Arc::clone(&self.buffered_remote_ice_candidates),
            terminate_condvar:              Arc::clone(&self.terminate_condvar),
            connection_type:                self.connection_type,
            tick_context:                   Arc::clone(&self.tick_context),
        }
    }
}

impl<T> Connection<T>
where
    T: Platform,
{
    /// Create a new Connection.
    #[allow(clippy::mutex_atomic)]
    pub fn new(
        call: Call<T>,
        remote_device: DeviceId,
        connection_type: ConnectionType,
    ) -> Result<Self> {
        // Create a FSM runtime for this connection.
        let context = Context::new()?;
        let (fsm_sender, fsm_receiver) = futures::sync::mpsc::channel(256);

        let call_id = call.call_id();
        let direction = call.direction();

        let webrtc = WebRtcData {
            pc_interface:          None,
            data_channel:          None,
            data_channel_observer: None,
            connection_ptr:        None,
            incoming_media:        None,
            app_connection:        None,
            stats_observer:        None,
            local_max_bitrate:     BandwidthMode::Normal.max_bitrate(),
            remote_max_bitrate:    BandwidthMode::Normal.max_bitrate(),
        };

        let connection = Self {
            fsm_sender,
            fsm_receiver: Some(fsm_receiver),
            call_id,
            // Until otherwise detected, remotes are assumed to be multi-ring capable.
            remote_feature_level: Arc::new(CallMutex::new(
                FeatureLevel::MultiRing,
                "remote_feature_level",
            )),
            connection_id: ConnectionId::new(call_id, remote_device),
            direction,
            call: Arc::new(CallMutex::new(call, "call")),
            state: Arc::new(CallMutex::new(ConnectionState::NotYetStarted, "state")),
            context: Arc::new(CallMutex::new(context, "context")),
            webrtc: Arc::new(CallMutex::new(webrtc, "webrtc")),
            buffered_local_ice_candidates: Arc::new(CallMutex::new(
                Vec::new(),
                "buffered_local_ice_candidates",
            )),
            buffered_remote_ice_candidates: Arc::new(CallMutex::new(
                Vec::new(),
                "buffered_remote_ice_candidates",
            )),
            terminate_condvar: Arc::new((Mutex::new(false), Condvar::new())),
            connection_type,
            tick_context: Arc::new(CallMutex::new(TickContext::new(), "tick_context")),
        };

        connection.init_connection_ptr()?;

        Ok(connection)
    }

    fn start_fsm(&mut self) -> Result<()> {
        let mut context = self.context.lock()?;
        if let Some(fsm_receiver) = self.fsm_receiver.take() {
            info!("Starting Connection FSM for {}", self.connection_id);
            let connection_fsm = ConnectionStateMachine::new(fsm_receiver)?
                .map_err(|e| info!("connection state machine returned error: {}", e));
            context.worker_runtime.spawn(connection_fsm);
        } else {
            warn!(
                "Starting Connection FSM for {} more than once",
                self.connection_id
            );
        }
        Ok(())
    }

    // An outgoing parent is responsible for:
    // 1. Creating ICE gatherer that can be used multiple times (ICE forking)
    // 2. Creating an offer that can be used multiple times (call forking)
    // 3. Creating an offer that is backwards compatible between old and new clients
    // It does not need to fully configure the PeerConnection.
    pub fn start_outgoing_parent(
        &mut self,
        call_media_type: CallMediaType,
    ) -> Result<(IceGatherer, signaling::Offer)> {
        let result = (|| {
            self.set_state(ConnectionState::Starting)?;

            let webrtc = self.webrtc.lock()?;
            let peer_connection = webrtc.pc_interface()?;

            // We have to create and use the IceGatherer before calling
            // create_offer to make sure the ICE parameters are correct.
            let ice_gatherer = peer_connection.create_shared_ice_gatherer()?;
            peer_connection.use_shared_ice_gatherer(&ice_gatherer)?;

            // We have to create the DataChannel before calling create_offer to make sure the
            // data channel parameters are correct.  But we don't need to observe it.
            let _data_channel =
                peer_connection.create_data_channel(DATA_CHANNEL_NAME.to_string())?;

            let observer = create_csd_observer();
            peer_connection.create_offer(observer.as_ref());
            // This must be kept in sync with call.rs where it passes in V2 into create_connection.
            let mut offer_sdi = observer.get_result()?;

            // The only purpose of this is to start gathering ICE candidates.
            // But we need to call set_local_description before we munge it.
            // Otherwise there will be a data channel type mismatch.
            let observer = create_ssd_observer();
            peer_connection.set_local_description(observer.as_ref(), &offer_sdi);
            observer.get_result()?;

            let v2_offer_sdp = offer_sdi.to_sdp()?;
            offer_sdi.replace_rtp_data_channels_with_sctp()?;
            let v1_offer_sdp = offer_sdi.to_sdp()?;

            let offer =
                signaling::Offer::from_v2_and_v1_sdp(call_media_type, v2_offer_sdp, v1_offer_sdp)?;

            self.set_state(ConnectionState::IceGathering)?;
            Ok((ice_gatherer, offer))
        })();

        // Always start the FSM no matter what happened above because
        // close() relies on it running.
        self.start_fsm()?;
        result
    }

    // An outgoing child is responsible for:
    // 1. Using the ICE gatherer from the outgoing parent.
    // 2. Combining the offer from the parent and the answer from the remote peer
    //    to configure PeerConnection correctly.
    pub fn start_outgoing_child(
        &mut self,
        ice_gatherer: &IceGatherer,
        offer: &signaling::Offer,
        received: &signaling::ReceivedAnswer,
    ) -> Result<()> {
        let result = (|| {
            self.set_state(ConnectionState::Starting)?;

            self.set_remote_feature_level(received.sender_device_feature_level)?;

            let mut webrtc = self.webrtc.lock()?;

            // Create a stats observer object.
            let stats_observer = create_stats_observer();
            webrtc.stats_observer = Some(stats_observer);

            let peer_connection = webrtc.pc_interface()?;

            peer_connection.use_shared_ice_gatherer(&ice_gatherer)?;

            // The caller is responsible for creating the data channel (the callee listens for it).
            // Both sides will observe it.
            let data_channel =
                peer_connection.create_data_channel(DATA_CHANNEL_NAME.to_string())?;
            let data_channel_observer = DataChannelObserver::new(self.clone())?;
            unsafe { data_channel.register_observer(data_channel_observer.rffi_interface())? };

            let (answer_is_v2, answer_sdp) = received.answer.to_v2_or_v1_sdp()?;
            let offer_sdp = if answer_is_v2 {
                offer.to_v2_sdp()?
            } else {
                offer.to_v1_sdp()?
            };
            let offer_sdi = SessionDescriptionInterface::offer_from_sdp(offer_sdp)?;
            let answer_sdi = SessionDescriptionInterface::answer_from_sdp(answer_sdp)?;

            let observer = create_ssd_observer();
            peer_connection.set_local_description(observer.as_ref(), &offer_sdi);
            observer.get_result()?;

            let observer = create_ssd_observer();
            peer_connection.set_remote_description(observer.as_ref(), &answer_sdi);
            // on_data_channel and on_add_stream and on_ice_connected can all happen while
            // SetRemoteDescription is happening.  But none of those will be processed
            // until start_fsm() is called below.
            observer.get_result()?;

            // Don't enable until the call is accepted.
            peer_connection.set_outgoing_audio_enabled(false);
            // But do start incoming RTP right away so we can receive the
            // "accepted" message.
            peer_connection.set_incoming_rtp_enabled(true);

            // We have to do this once we're done with peer_connection because
            // it holds a ref to peer_connection as well.
            webrtc.data_channel = Some(data_channel);
            webrtc.data_channel_observer = Some(data_channel_observer);
            self.set_state(ConnectionState::ConnectingBeforeAccepted)?;
            Ok(())
        })();

        // Make sure we start the FSM after setting the state because the FSM
        // checks the state and because we don't want to do things (like
        // handle ICE connected events) until after everything is set up.
        // Always start the FSM no matter what happened above because
        // close() relies on it running.
        self.start_fsm()?;
        result
    }

    // An incoming connection is responsible for:
    // 1. Creating an answer to send back to the caller
    // 2. Configuring the PeerConnection with the offer and the answer,
    //    and any remote ICE candidates that came that have arrived.
    pub fn start_incoming(
        &mut self,
        received: signaling::ReceivedOffer,
        remote_ice_candidates: Vec<signaling::IceCandidate>,
    ) -> Result<signaling::Answer> {
        let result = (|| {
            self.set_state(ConnectionState::Starting)?;

            let mut webrtc = self.webrtc.lock()?;

            // Create a stats observer object.
            let stats_observer = create_stats_observer();
            webrtc.stats_observer = Some(stats_observer);

            let peer_connection = webrtc.pc_interface()?;

            let (offer_is_v2, offer_sdp) = received.offer.to_v2_or_v1_sdp()?;
            let offer_sdi = SessionDescriptionInterface::offer_from_sdp(offer_sdp)?;

            let observer = create_ssd_observer();
            peer_connection.set_remote_description(observer.as_ref(), &offer_sdi);
            // on_data_channel and on_add_stream can happen while SetRemoteDescription
            // is happening.  But they won't be processed until start_fsm() is called
            // below.
            observer.get_result()?;

            let observer = create_csd_observer();
            peer_connection.create_answer(observer.as_ref());
            let answer_sdi = observer.get_result()?;
            // Get the answer here before it is mutated with candidates by set_local_description.
            let answer_sdp = answer_sdi.to_sdp()?;
            let answer = if offer_is_v2 {
                signaling::Answer::from_v2_sdp(answer_sdp)?
            } else {
                signaling::Answer::from_v1_sdp(answer_sdp)
            };

            // Don't enable incoming RTP until accepted.
            // This should be done before we set local description to make sure
            // we don't get ICE connected really fast and allow any packets through.
            peer_connection.set_incoming_rtp_enabled(false);

            let observer = create_ssd_observer();
            peer_connection.set_local_description(observer.as_ref(), &answer_sdi);
            // on_ice_connected can happen while SetLocalDescription is happening.
            // But it won't be processed until start_fsm() is called below.
            observer.get_result()?;

            // Don't enable until call is accepted.
            peer_connection.set_outgoing_audio_enabled(false);

            ringbench!(
                RingBench::Conn,
                RingBench::WebRTC,
                format!("ice_candidates({})", remote_ice_candidates.len())
            );
            for remote_ice_candidate in remote_ice_candidates {
                peer_connection.add_ice_candidate(&remote_ice_candidate)?;
            }

            self.set_state(ConnectionState::ConnectingBeforeAccepted)?;
            Ok(answer)
        })();

        // Make sure we start the FSM after setting the state because the FSM
        // checks the state and because we don't want to do things (like
        // handle ICE connected events) until after everything is set up.
        // Always start the FSM no matter what happened above because
        // close() relies on it running.
        self.start_fsm()?;
        result
    }

    /// Return the Call identifier.
    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    pub fn remote_device_id(&self) -> DeviceId {
        self.connection_id.remote_device_id()
    }

    /// Return the connection identifier.
    pub fn id(&self) -> ConnectionId {
        self.connection_id
    }

    /// Return the Call direction.
    pub fn direction(&self) -> CallDirection {
        self.direction
    }

    /// Return the parent call, under a locked mutex.
    pub fn call(&self) -> Result<MutexGuard<'_, Call<T>>> {
        self.call.lock()
    }

    /// Return the current Call state.
    pub fn state(&self) -> Result<ConnectionState> {
        let state = self.state.lock()?;
        Ok(*state)
    }

    /// Update the current Call state.
    pub fn set_state(&self, new_state: ConnectionState) -> Result<()> {
        let mut state = self.state.lock()?;
        *state = new_state;
        if new_state == ConnectionState::ConnectedAndAccepted {
            // Now that we are accepted, we can enable outgoing audio and incoming RTP
            let webrtc = self.webrtc.lock()?;
            let pc = webrtc.pc_interface()?;
            pc.set_outgoing_audio_enabled(true);
            pc.set_incoming_rtp_enabled(true);
        }
        Ok(())
    }

    /// Return the current feature level of the remote.
    pub fn remote_feature_level(&self) -> Result<FeatureLevel> {
        let remote_feature_level = self.remote_feature_level.lock()?;
        Ok(*remote_feature_level)
    }

    /// Update the current feature level of the remote.
    pub fn set_remote_feature_level(&self, new_remote_feature_level: FeatureLevel) -> Result<()> {
        let mut remote_feature_level = self.remote_feature_level.lock()?;
        *remote_feature_level = new_remote_feature_level;
        Ok(())
    }

    /// Update the webrtc::PeerConnection interface.
    pub fn set_pc_interface(&self, pc_interface: PeerConnection) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        webrtc.pc_interface = Some(pc_interface);
        Ok(())
    }

    /// Return whether the connection has a data channel.
    pub fn has_data_channel(&self) -> Result<bool> {
        let webrtc = self.webrtc.lock()?;
        match webrtc.data_channel {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Update the DataChannel and DataChannelObserver
    pub fn set_data_channel(&self, dc: DataChannel) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;

        let observer = DataChannelObserver::new(self.clone())?;
        unsafe { dc.register_observer(observer.rffi_interface())? };
        webrtc.data_channel = Some(dc);
        webrtc.data_channel_observer = Some(observer);
        Ok(())
    }

    /// Set the incoming media.
    pub fn set_incoming_media(
        &self,
        incoming_media: <T as Platform>::AppIncomingMedia,
    ) -> Result<()> {
        // In the current application we only expect one incoming stream
        // per connection.
        let mut webrtc = self.webrtc.lock()?;
        match webrtc.incoming_media {
            Some(_) => {
                Err(RingRtcError::ActiveMediaStreamAlreadySet(self.remote_device_id()).into())
            }
            None => {
                webrtc.incoming_media = Some(incoming_media);
                Ok(())
            }
        }
    }

    /// Set the application peer connection.
    pub fn set_app_connection(&self, app_connection: <T as Platform>::AppConnection) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        match webrtc.app_connection {
            Some(_) => Err(RingRtcError::AppConnectionAlreadySet(self.remote_device_id()).into()),
            None => {
                webrtc.app_connection = Some(app_connection);
                Ok(())
            }
        }
    }

    /// Return a clone of the application peer connection.
    pub fn app_connection(&self) -> Result<<T as Platform>::AppConnection> {
        let webrtc = self.webrtc.lock()?;
        match webrtc.app_connection.as_ref() {
            Some(v) => Ok(v.clone()),
            None => Err(RingRtcError::OptionValueNotSet(
                String::from("app_connection()"),
                String::from("app_connection"),
            )
            .into()),
        }
    }

    // Only for tests
    pub fn app_connection_ptr_for_tests(&self) -> *const <T as Platform>::AppConnection {
        let webrtc = self.webrtc.lock().unwrap();
        webrtc.app_connection.as_ref().unwrap()
    }

    /// Returns `true` if the call is terminating.
    pub fn terminating(&self) -> Result<bool> {
        if let ConnectionState::Terminating = self.state()? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Clone the Connection, Box it and return a raw pointer to the Box.
    pub fn create_connection_ptr(&self) -> *mut Connection<T> {
        let connection_box = Box::new(self.clone());
        Box::into_raw(connection_box)
    }

    /// Return the internally tracked connection object pointer, for
    /// use by the PeerConnectionObserver call backs.
    pub fn get_connection_ptr(&self) -> Result<*mut Connection<T>> {
        let webrtc = self.webrtc.lock()?;
        match webrtc.connection_ptr.as_ref() {
            Some(v) => Ok(*v),
            None => Err(RingRtcError::OptionValueNotSet(
                String::from("connection_ptr()"),
                String::from("connection_ptr"),
            )
            .into()),
        }
    }

    /// Create a connection object on the heap, for use by the
    /// PeerConnectionObserver call backs.  Track it, as it needs to
    /// be freed after closing down the PeerConnection.
    fn init_connection_ptr(&self) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        webrtc.connection_ptr = Some(self.create_connection_ptr());
        Ok(())
    }

    /// Return the strong reference count on the webrtc `Arc<Mutex<>>`.
    fn ref_count(&self) -> usize {
        Arc::strong_count(&self.webrtc)
    }

    /// Sets the maximum send bitrate only for the local
    /// peer_connection. Called when a connection is first started to
    /// ensure the BandwidthMode::Normal.max_bitrate() is set in WebRTC.
    /// Does not send the remote side any message.
    pub fn set_local_max_send_bitrate(&self, local_max: DataRate) -> Result<()> {
        info!("set_local_max_send_bitrate(): local_max: {:?}", local_max);

        let mut webrtc = self.webrtc.lock()?;
        webrtc.local_max_bitrate = local_max;
        webrtc.pc_interface()?.set_max_send_bitrate(local_max)
    }

    /// Sets the maximum bitrate for all media combined.
    /// This includes sending and receiving.
    /// Sending by changing the PeerConnection.
    /// Receiving by sending a message to the remote side to send less
    /// The app can set this via the `set_bandwidth_mode` API.
    pub fn set_local_max_bitrate(&self, local_max: DataRate) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;

        if local_max == webrtc.local_max_bitrate {
            // The local bitrate has not changed, so there is nothing to do.
            return Ok(());
        }
        webrtc.local_max_bitrate = local_max;

        // Use the smallest bitrate for the session.
        let combined_max = min(local_max, webrtc.remote_max_bitrate);
        info!(
            "set_local_max_bitrate(): local: {:?} remote: {:?} combined: {:?}",
            local_max, webrtc.remote_max_bitrate, combined_max
        );

        webrtc.pc_interface()?.set_max_send_bitrate(combined_max)?;

        // Send the remote peer the current receiver status via the
        // PeerConnection DataChannel.
        webrtc
            .data_channel()?
            .send_receiver_status(self.call_id, local_max)
    }

    /// Sets the maximum sending bitrate for all media combined.
    /// Unlike set_local_max_bitrate, it does not send a message to the remote
    /// side to affect receiving.  Rather, it comes from the remote side
    /// and thus must affect local sending.
    pub fn set_remote_max_bitrate(&self, remote_max: DataRate) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;

        let remote_max = clamp(
            remote_max,
            BandwidthMode::Min.max_bitrate(),
            BandwidthMode::Max.max_bitrate(),
        );
        if remote_max == webrtc.remote_max_bitrate {
            // The remote bitrate has not changed, so there is nothing to do.
            return Ok(());
        }
        webrtc.remote_max_bitrate = remote_max;

        // Use the smallest bitrate for the session.
        let combined_max = min(webrtc.local_max_bitrate, remote_max);
        info!(
            "set_remote_max_bitrate(): local: {:?} remote: {:?} combined: {:?}",
            webrtc.local_max_bitrate, remote_max, combined_max
        );

        webrtc.pc_interface()?.set_max_send_bitrate(combined_max)
    }

    /// Creates a runtime for statistics to run a timer for the given interval
    /// duration to invoke PeerConnection::GetStats which will pass specific stats
    /// to StatsObserver::on_stats_complete.
    pub fn start_tick(&self) -> Result<()> {
        let duration = Duration::from_secs(TICK_PERIOD_SEC);

        // Define the future for stats logging.
        let mut connection = self.clone();
        let tick_future = Interval::new_interval(duration)
            .map_err(|e| error!("Connection tick Interval failed: {:?}", e))
            .for_each(move |_| {
                connection.tick().unwrap();
                Ok(())
            });

        debug!("start_tick(): starting the tick runtime");
        let mut tick_context = self.tick_context.lock()?;
        match tick_context.runtime {
            Some(_) => warn!("start_tick(): tick timer already running"),
            None => {
                // Start the tick runtime.
                let mut runtime = runtime::Builder::new()
                    .core_threads(1)
                    .name_prefix("tick-")
                    .build()?;

                runtime.spawn(tick_future);

                tick_context.runtime = Some(runtime);
            }
        }

        Ok(())
    }

    pub fn tick(&mut self) -> Result<()> {
        let mut tick_context = self.tick_context.lock()?;
        tick_context.ticks_elapsed += 1;

        let webrtc = self.webrtc.lock()?;

        if let Ok(data_channel) = webrtc.data_channel() {
            let _ = data_channel.send_latest_state();
        }

        if tick_context.ticks_elapsed % STATS_PERIOD_SEC == 0 {
            if let Some(observer) = webrtc.stats_observer.as_ref() {
                let _ = webrtc.pc_interface()?.get_stats(observer);
            } else {
                warn!("tick(): No stats_observer found");
            }
        }

        Ok(())
    }

    /// Check to see if this Connection is able to send messages.
    /// Once it is terminated it shouldn't be able to.
    pub fn can_send_messages(&self) -> bool {
        let state_result = self.state();

        match state_result {
            Ok(state) => match state {
                ConnectionState::Terminating | ConnectionState::Terminated => false,
                _ => true,
            },
            Err(_) => false,
        }
    }

    pub fn set_outgoing_audio_enabled(&self, enabled: bool) -> Result<()> {
        let webrtc = self.webrtc.lock()?;
        webrtc.pc_interface()?.set_outgoing_audio_enabled(enabled);
        Ok(())
    }

    /// Buffer local ICE candidates, and maybe send them immediately
    pub fn buffer_local_ice_candidate(&self, candidate: signaling::IceCandidate) -> Result<()> {
        info!(
            "Local ICE candidate: {}; {}",
            candidate.to_info_string(),
            candidate.to_redacted_string()
        );

        let num_ice_candidates = {
            let mut buffered_local_ice_candidates = self.buffered_local_ice_candidates.lock()?;
            buffered_local_ice_candidates.push(candidate);
            buffered_local_ice_candidates.len()
        };

        // Only when we transition from no candidates to one do we
        // need to signal the message queue that there is something
        // to send for this Connection.
        if num_ice_candidates == 1 {
            let call = self.call()?;
            let broadcast = self.connection_type == ConnectionType::OutgoingParent;
            call.send_buffered_local_ice_candidates(self.clone(), broadcast)?
        }

        Ok(())
    }

    /// Buffer remote ICE candidates.
    pub fn buffer_remote_ice_candidates(
        &self,
        ice_candidates: Vec<signaling::IceCandidate>,
    ) -> Result<()> {
        let mut pending_ice_candidates = self.buffered_remote_ice_candidates.lock()?;
        for ice_candidate in ice_candidates {
            info!(
                "Remote ICE candidate: {}; {}",
                ice_candidate.to_info_string(),
                ice_candidate.to_redacted_string()
            );
            pending_ice_candidates.push(ice_candidate);
        }
        Ok(())
    }

    /// Get the current local ICE candidates to send to the remote peer.
    pub fn take_buffered_local_ice_candidates(&self) -> Result<Vec<signaling::IceCandidate>> {
        info!("take_buffered_local_ice_candidates():");

        let mut ice_candidates = self.buffered_local_ice_candidates.lock()?;

        let copy_candidates = ice_candidates.clone();
        ice_candidates.clear();

        info!(
            "take_buffered_local_ice_candidates(): Local ICE candidates length: {}",
            copy_candidates.len()
        );

        Ok(copy_candidates)
    }

    pub fn add_remote_ice_candidates(
        &self,
        remote_ice_candidates: &[signaling::IceCandidate],
    ) -> Result<()> {
        ringbench!(
            RingBench::Conn,
            RingBench::WebRTC,
            format!("ice_candidates({})", remote_ice_candidates.len())
        );

        let webrtc = self.webrtc.lock()?;
        for remote_ice_candidate in remote_ice_candidates {
            webrtc
                .pc_interface()?
                .add_ice_candidate(remote_ice_candidate)?;
        }
        Ok(())
    }

    /// Send a hangup message to the remote peer via the
    /// PeerConnection DataChannel.
    pub fn send_hangup_via_data_channel(&self, hangup: signaling::Hangup) -> Result<()> {
        ringbench!(
            RingBench::Conn,
            RingBench::WebRTC,
            format!("dc(hangup/{})\t{}", hangup, self.connection_id)
        );

        let webrtc = self.webrtc.lock()?;
        if let Ok(data_channel) = webrtc.data_channel() {
            if let Err(e) = data_channel.send_hangup(self.call_id, hangup) {
                info!("data_channel.send_hang_up() failed: {}", e);
            }
        } else {
            info!(
                "send_hangup_via_data_channel(): id: {}, skipping, data_channel not present",
                self.connection_id
            );
        }
        Ok(())
    }

    /// Send an accepted message to the remote peer via the
    /// PeerConnection DataChannel.
    pub fn send_accepted_via_data_channel(&self) -> Result<()> {
        ringbench!(
            RingBench::Conn,
            RingBench::WebRTC,
            format!("dc(accepted)\t{}", self.connection_id)
        );

        let webrtc = self.webrtc.lock()?;
        webrtc.data_channel()?.send_accepted(self.call_id)?;
        Ok(())
    }

    /// Send the remote peer the current sender status via the
    /// PeerConnection DataChannel.
    ///
    /// # Arguments
    ///
    /// * `video_enabled` - `true` when the local side is streaming video,
    /// otherwise `false`.
    pub fn send_sender_status_via_data_channel(&self, video_enabled: bool) -> Result<()> {
        let webrtc = self.webrtc.lock()?;

        webrtc
            .data_channel()?
            .send_sender_status(self.call_id, video_enabled)
    }

    /// Notify the parent call observer about an event.
    pub fn notify_observer(&self, event: ConnectionObserverEvent) -> Result<()> {
        let mut call = self.call.lock()?;
        call.on_connection_observer_event(self.remote_device_id(), event)
    }

    /// Notify the parent call observer about an internal error.
    pub fn internal_error(&self, error: failure::Error) -> Result<()> {
        let mut call = self.call.lock()?;
        call.on_connection_observer_error(self.remote_device_id(), error)
    }

    /// Create an application-specific IncomingMedia object and store it
    /// for connect_incoming_media later.
    pub fn handle_received_incoming_media(&mut self, stream: MediaStream) -> Result<()> {
        info!(
            "handle_received_incoming_media(): id: {}",
            self.connection_id
        );

        let call = self.call.lock()?;
        let incoming_media = call.create_incoming_media(self, stream)?;
        self.set_incoming_media(incoming_media)
    }

    /// Connect incoming media (stored by handle_incoming_media) to the application connection
    pub fn connect_incoming_media(&self) -> Result<()> {
        info!("connect_incoming_media(): id: {}", self.connection_id);

        let webrtc = self.webrtc.lock()?;
        let incoming_media = match webrtc.incoming_media.as_ref() {
            Some(v) => v,
            None => {
                return Err(RingRtcError::OptionValueNotSet(
                    String::from("connect_incoming_media()"),
                    String::from("incoming_media"),
                )
                .into())
            }
        };

        let call = self.call()?;
        call.connect_incoming_media(incoming_media)
    }

    /// Send a ConnectionEvent to the internal FSM.
    fn inject_event(&mut self, event: ConnectionEvent) -> Result<()> {
        if self.fsm_sender.is_closed() {
            // The stream is closed, just eat the request
            debug!(
                "cc.inject_event(): stream is closed while sending: {}",
                event
            );
            return Ok(());
        }
        self.fsm_sender.try_send((self.clone(), event))?;
        Ok(())
    }

    /// Terminate the connection.
    ///
    /// Notify the internal FSM to terminate.
    ///
    /// `Note:` The current thread is blocked while waiting for the
    /// FSM to signal that termination is complete.
    pub fn terminate(&mut self) -> Result<()> {
        info!("terminate(): ref_count: {}", self.ref_count());

        self.set_state(ConnectionState::Terminating)?;

        self.inject_event(ConnectionEvent::Terminate)?;
        self.wait_for_terminate()?;

        self.set_state(ConnectionState::Terminated)?;

        // Stop the timer runtime, if any.
        let mut tick_context = self.tick_context.lock()?;
        if let Some(runtime) = tick_context.runtime.take() {
            info!("close(): stopping the tick runtime");
            let _ = runtime
                .shutdown_now()
                .wait()
                .map_err(|_| warn!("Problems shutting down the tick runtime"));
        }

        // Free up webrtc related resources.
        let mut webrtc = self.webrtc.lock()?;

        // dispose of the incoming media
        let _ = webrtc.incoming_media.take();

        // dispose of the stats observer
        let _ = webrtc.stats_observer.take();

        // unregister the data channel observer
        if let Some(data_channel) = webrtc.data_channel.take().as_mut() {
            if let Some(dc_observer) = webrtc.data_channel_observer.take().as_mut() {
                unsafe { data_channel.unregister_observer(dc_observer.rffi_interface()) };
            }
            data_channel.dispose();
        }

        // Free the application connection object, which is in essence
        // the PeerConnection object.  It is important to dispose of
        // the app_connection before the connection_ptr.  The
        // app_connection refers to the real PeerConnection object,
        // whose observer is using the connection_ptr.  Once the
        // PeerConnection is completely shutdown it is safe to free up
        // the connection_ptr.
        let _ = webrtc.app_connection.take();

        // Free the connection object previously used by the
        // PeerConnectionObserver.  Convert the pointer back into a
        // Box and let it go out of scope.
        match webrtc.connection_ptr.take() {
            Some(v) => {
                let _ = unsafe { ptr_as_box(v)? };
                Ok(())
            }
            None => Err(RingRtcError::OptionValueNotSet(
                String::from("close()"),
                String::from("connection_ptr"),
            )
            .into()),
        }
    }

    /// Bottom half of `close()`
    ///
    /// Waits for the FSM shutdown condition variable to signal that
    /// shutdown is complete.
    fn wait_for_terminate(&mut self) -> Result<()> {
        // Wait for terminate operation to complete
        info!("terminate(): waiting for terminate complete...");
        let &(ref mutex, ref condvar) = &*self.terminate_condvar;
        if let Ok(mut terminate_complete) = mutex.lock() {
            while !*terminate_complete {
                terminate_complete = condvar.wait(terminate_complete).map_err(|_| {
                    RingRtcError::MutexPoisoned(
                        "Connection Terminate Condition Variable".to_string(),
                    )
                })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned(
                "Connection Terminate Condition Variable".to_string(),
            )
            .into());
        }
        info!(
            "terminate(): terminate complete: ref_count: {}",
            self.ref_count()
        );
        Ok(())
    }

    /// Notification that the FSM shutdown is complete.
    ///
    /// `Note:` Called by the FSM on a worker thread after shutdown.
    pub fn notify_terminate_complete(&mut self) -> Result<()> {
        debug!("notify_terminate_complete(): notifying terminate complete...");
        let &(ref mutex, ref condvar) = &*self.terminate_condvar;
        if let Ok(mut terminate_complete) = mutex.lock() {
            *terminate_complete = true;
            condvar.notify_one();
            Ok(())
        } else {
            Err(
                RingRtcError::MutexPoisoned("Connection Terminate Condition Variable".to_string())
                    .into(),
            )
        }
    }

    /// Inject a `LocalIceCandidate` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `candidate` - Locally generated IceCandidate.
    pub fn inject_local_ice_candidate(
        &mut self,
        candidate: signaling::IceCandidate,
        force_send: bool,
    ) -> Result<()> {
        if !force_send && self.connection_type == ConnectionType::OutgoingChild {
            return Ok(());
        }
        self.inject_event(ConnectionEvent::LocalIceCandidate(candidate))?;
        Ok(())
    }

    /// Inject an `IceConnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connected(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::IceConnected)
    }

    /// Inject an `IceFailed` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_failed(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::IceFailed)
    }

    /// Inject an `IceDisconnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_disconnected(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::IceDisconnected)
    }

    /// Inject a `InternalError` event into the FSM.
    ///
    /// This is used to send an internal error notification to the
    /// observer.
    ///
    /// `Called By:` FSM when internal errors occur.
    ///
    /// Note: this function does not fail, as there is not much one
    /// can do in this case.
    pub fn inject_internal_error(&mut self, error: failure::Error, msg: &str) {
        error!("{}: {}", msg, error);
        let _ = self.inject_event(ConnectionEvent::InternalError(error));
    }

    /// Inject a `ReceivedAcceptedViaDataChannel` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_received_accepted_via_data_channel(&mut self, call_id: CallId) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedAcceptedViaDataChannel(call_id))
    }

    /// Inject a `ReceivedHangup` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_received_hangup(
        &mut self,
        call_id: CallId,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedHangup(call_id, hangup))
    }

    /// Inject a `ReceivedSenderStatusViaDataChannel` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    /// * `video_enabled` - `true` if the remote peer is streaming video.
    pub fn inject_received_sender_status_via_data_channel(
        &mut self,
        call_id: CallId,
        video_enabled: bool,
        sequence_number: Option<u64>,
    ) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedSenderStatusViaDataChannel(
            call_id,
            video_enabled,
            sequence_number,
        ))
    }

    /// Inject a `ReceivedReceiverStatusViaDataChannel` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    /// * `max_bitrate_bps` - the bitrate that the remote peer wants to use for
    /// the session.
    pub fn inject_received_receiver_status_via_data_channel(
        &mut self,
        call_id: CallId,
        max_bitrate: DataRate,
        sequence_number: Option<u64>,
    ) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedReceiverStatusViaDataChannel(
            call_id,
            max_bitrate,
            sequence_number,
        ))
    }

    /// Inject a `SendHangupViaDataChannel event into the FSM.
    pub fn inject_send_hangup_via_data_channel(&mut self, hangup: signaling::Hangup) -> Result<()> {
        self.set_state(ConnectionState::Terminating)?;
        self.inject_event(ConnectionEvent::SendHangupViaDataChannel(hangup))
    }

    /// Inject a local `Accept` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_accept(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::Accept)
    }

    /// Inject a `SendSenderStatusViaDataChannel` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// * `video_enabled` - `true` if the local peer is streaming video.
    pub fn inject_send_sender_status_via_data_channel(
        &mut self,
        video_enabled: bool,
    ) -> Result<()> {
        self.inject_event(ConnectionEvent::SendSenderStatusViaDataChannel(
            video_enabled,
        ))
    }

    /// Inject a `SetBandwidthMode` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// * `mode` - The bandwidth mode that should be used
    pub fn set_bandwidth_mode(&mut self, mode: BandwidthMode) -> Result<()> {
        self.inject_event(ConnectionEvent::SetBandwidthMode(mode))
    }

    /// Inject a `ReceivedIce` event into the FSM.
    ///
    /// `Called By:` Call object.
    pub fn inject_received_ice(&mut self, ice: signaling::Ice) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedIce(ice))
    }

    /// Inject an `ReceivedIncomingMedia` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` back thread.
    ///
    /// # Arguments
    ///
    /// * `stream` - WebRTC C++ MediaStream object.
    pub fn inject_received_incoming_media(&mut self, stream: MediaStream) -> Result<()> {
        let event = ConnectionEvent::ReceivedIncomingMedia(stream);
        self.inject_event(event)
    }

    /// Inject an `ReceivedDataChannel` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` back thread.
    ///
    /// # Arguments
    ///
    /// * `data_channel` - WebRTC C++ `DataChannel` object.
    pub fn inject_received_data_channel(&mut self, data_channel: DataChannel) -> Result<()> {
        let event = ConnectionEvent::ReceivedDataChannel(data_channel);
        self.inject_event(event)
    }

    #[allow(clippy::mutex_atomic)]
    /// Inject a synchronizing event into the FSM.
    ///
    /// Blocks the caller while the event flushes through the FSM.
    ///
    /// Note: Events ahead of this event in the FSM pipeline can
    /// generate additional error events, which will be queued behind
    /// this synchronizing event.
    #[cfg(feature = "sim")]
    fn inject_synchronize(&mut self) -> Result<()> {
        match self.state()? {
            ConnectionState::Terminated | ConnectionState::Terminating => {
                info!(
                    "connection-synchronize(): skipping synchronize while terminating or closed..."
                );
                return Ok(());
            }
            _ => {}
        }

        let sync = Arc::new((Mutex::new(false), Condvar::new()));
        let event = ConnectionEvent::Synchronize(sync.clone());

        self.inject_event(event)?;

        info!("connection-synchronize(): waiting for synchronize complete...");
        let &(ref mutex, ref condvar) = &*sync;
        if let Ok(mut sync_complete) = mutex.lock() {
            while !*sync_complete {
                sync_complete = condvar.wait(sync_complete).map_err(|_| {
                    RingRtcError::MutexPoisoned(
                        "Connection Synchronize Condition Variable".to_string(),
                    )
                })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned(
                "Connection Synchronize Condition Variable".to_string(),
            )
            .into());
        }
        info!("connection-synchronize(): complete");
        Ok(())
    }

    /// Synchronize the caller with the FSM event queue.
    ///
    /// Blocks the caller while the FSM event queue is flushed.
    ///
    /// `Called By:` Test infrastructure
    #[cfg(feature = "sim")]
    pub fn synchronize(&mut self) -> Result<()> {
        // The first sync flushes out any pending events.  This
        // event(s) could fail, which would enqueues another event to
        // the FSM, *behind* the sync event.
        self.inject_synchronize()?;

        // The second sync flushes out any error event(s) that might
        // have happened during the first sync.
        self.inject_synchronize()
    }
}

#[inline]
pub fn clamp<T: PartialOrd>(val: T, min: T, max: T) -> T {
    if val < min {
        return min;
    }
    if val > max {
        return max;
    }
    val
}
