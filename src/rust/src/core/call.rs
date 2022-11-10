//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! A peer-to-peer call connection interface.

use std::collections::{hash_map, HashMap};
use std::fmt;
use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use futures::channel::mpsc::{Receiver, Sender};
use futures::future::TryFutureExt;
use x25519_dalek::StaticSecret;

use crate::common::{
    ApplicationEvent, CallDirection, CallId, CallMediaType, CallState, DeviceId, Result,
};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call_fsm::{CallEvent, CallStateMachine};
use crate::core::call_manager::CallManager;
use crate::core::call_mutex::CallMutex;
use crate::core::connection::{Connection, ConnectionObserverEvent, ConnectionType};
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::core::util::TaskQueueRuntime;
use crate::error::RingRtcError;
use crate::webrtc::ice_gatherer::IceGatherer;
use crate::webrtc::media::MediaStream;
use crate::webrtc::peer_connection::AudioLevel;
use crate::webrtc::peer_connection_observer::NetworkRoute;

/// Encapsulates the FSM and runtime upon which a Call runs.
struct Context {
    /// Runtime upon which the CallStateMachine runs.
    pub worker_runtime: TaskQueueRuntime,
    /// Runtime that manages timing out a call.
    pub timeout_runtime: Option<TaskQueueRuntime>,
}

impl Context {
    fn new() -> Result<Self> {
        Ok(Self {
            worker_runtime: TaskQueueRuntime::new("fsm-worker")?,
            timeout_runtime: None,
        })
    }

    fn close(&mut self) {
        info!("stopping timeout runtime");
        self.timeout_runtime.take();
        info!("stopping timeout runtime: complete");
    }
}

/// Container for incoming call data, retained briefly while an
/// underlying Connection object is created and initialized.
struct PendingCall {
    pub received: signaling::ReceivedOffer,
    /// Buffer to hold received ICE candidates before the Connection
    /// object is ready.
    pub ice_candidates: Vec<signaling::IceCandidate>,
}

/// A mpsc::Receiver for receiving CallEvents in the
/// [CallStateMachine](../call_fsm/struct.CallStateMachine.html)
///
/// The event stream is the tuple (Call, CallEvent).
pub type EventStream<T> = Receiver<(Call<T>, CallEvent)>;

struct ForkingState<T>
where
    T: Platform,
{
    parent_connection: Connection<T>,
    // Used to negotiate SRTP keys with
    // the remote public key derived from the remote local secret.
    local_secret: StaticSecret,
    ice_gatherer: IceGatherer,
    offer: signaling::Offer,
}

/// Represents the set of connections between a local client and
/// 1-to-many remote peer devices for the same call recipient.
pub struct Call<T>
where
    T: Platform,
{
    /// Platform specific call manager
    call_manager: Arc<CallMutex<CallManager<T>>>,
    /// Unique 64-bit number identifying the call.
    call_id: CallId,
    /// The call direction, inbound or outbound.
    direction: CallDirection,
    /// The call media type at time of origination.
    media_type: CallMediaType,
    /// The local DeviceId of the client.
    local_device_id: DeviceId,
    /// The application specific remote peer of this call
    app_remote_peer: Arc<CallMutex<<T as Platform>::AppRemotePeer>>,
    /// The application specific context for this call
    app_call_context: Arc<CallMutex<Option<<T as Platform>::AppCallContext>>>,
    /// The current state of the call
    state: Arc<CallMutex<CallState>>,
    /// The actively connected connection.
    active_device_id: Arc<CallMutex<Option<DeviceId>>>,
    /// Pending remote offer and associated data.  Incoming calls only.
    pending_call: Arc<CallMutex<Option<PendingCall>>>,
    /// Injects events into the [CallStateMachine](../call_fsm/struct.CallStateMachine.html).
    fsm_sender: Sender<(Call<T>, CallEvent)>,
    /// Execution context for the call FSM
    fsm_context: Arc<CallMutex<Context>>,
    /// Collection of connections for this call
    connection_map: Arc<CallMutex<HashMap<DeviceId, Connection<T>>>>,
    /// Condition variable used at termination to quiesce and synchronize the FSM.
    terminate_condvar: Arc<(Mutex<bool>, Condvar)>,
    /// Whether or not an offer has been sent via messaging for this call.
    did_send_offer: Arc<AtomicBool>,
    /// Whether or not the application has already been notified of ApplicationEvent::RemoteRinging.
    /// It's a little ugly, but we need to handle the case where we get ConnectionState::ConnectedBeforeAccepted
    /// before a ConnectionState::ConnectingAfterAccepted and not notify the application twice.
    did_notify_application_of_remote_ringing: Arc<AtomicBool>,
    /// When doing call forking, the parent that must be kept alive to keep
    /// ICE candidates and signaling alive.
    /// And we also need to keep around that parent's offer that it created.
    forking: Arc<CallMutex<Option<ForkingState<T>>>>,
}

impl<T> fmt::Display for Call<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let state = match self.state() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        write!(
            f,
            "thread: {:?}, direction: {:?}, call_id: {}, state: {:?}",
            thread::current().id(),
            self.direction,
            self.call_id,
            state
        )
    }
}

impl<T> fmt::Debug for Call<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Drop for Call<T>
where
    T: Platform,
{
    fn drop(&mut self) {
        if self.ref_count() == 1 {
            info!("Call: Dropping last reference: {}", self.call_id);

            // This is the last call reference, so let the application
            // release the the remote object.
            if let Ok(call_manager) = self.call_manager() {
                if let Ok(remote_peer) = self.remote_peer() {
                    let _ = call_manager.notify_call_concluded(&remote_peer, self.call_id);
                }
            }
        } else {
            debug!(
                "Dropping Call: {}, ref_count: {}",
                self.call_id,
                self.ref_count()
            );
        }
    }
}

impl<T> Clone for Call<T>
where
    T: Platform,
{
    fn clone(&self) -> Self {
        Self {
            call_manager: Arc::clone(&self.call_manager),
            call_id: self.call_id,
            direction: self.direction,
            media_type: self.media_type,
            local_device_id: self.local_device_id,
            app_remote_peer: Arc::clone(&self.app_remote_peer),
            app_call_context: Arc::clone(&self.app_call_context),
            state: Arc::clone(&self.state),
            active_device_id: Arc::clone(&self.active_device_id),
            pending_call: Arc::clone(&self.pending_call),
            fsm_sender: self.fsm_sender.clone(),
            fsm_context: Arc::clone(&self.fsm_context),
            connection_map: Arc::clone(&self.connection_map),
            terminate_condvar: Arc::clone(&self.terminate_condvar),
            did_send_offer: Arc::clone(&self.did_send_offer),
            did_notify_application_of_remote_ringing: Arc::clone(
                &self.did_notify_application_of_remote_ringing,
            ),
            forking: Arc::clone(&self.forking),
        }
    }
}

impl<T> Call<T>
where
    T: Platform,
{
    /// Create a new Call.
    #[allow(clippy::mutex_atomic)]
    pub fn new(
        app_remote_peer: <T as Platform>::AppRemotePeer,
        call_id: CallId,
        direction: CallDirection,
        media_type: CallMediaType,
        local_device_id: DeviceId,
        call_manager: CallManager<T>,
    ) -> Result<Self> {
        info!("new(): call_id: {}", call_id);

        // create a FSM runtime for this connection
        let fsm_context = Context::new()?;
        let (fsm_sender, fsm_receiver) = futures::channel::mpsc::channel(256);
        let call_fsm = CallStateMachine::new(fsm_receiver)?
            .unwrap_or_else(|e| info!("call state machine returned error: {}", e));
        fsm_context.worker_runtime.spawn(call_fsm);

        let call = Self {
            call_manager: Arc::new(CallMutex::new(call_manager, "call_manager")),
            call_id,
            direction,
            media_type,
            local_device_id,
            app_remote_peer: Arc::new(CallMutex::new(app_remote_peer, "app_remote_peer")),
            app_call_context: Arc::new(CallMutex::new(None, "app_call_context")),
            state: Arc::new(CallMutex::new(CallState::NotYetStarted, "state")),
            active_device_id: Arc::new(CallMutex::new(None, "active_device_id")),
            pending_call: Arc::new(CallMutex::new(None, "pending_call")),
            fsm_sender,
            fsm_context: Arc::new(CallMutex::new(fsm_context, "fsm_context")),
            connection_map: Arc::new(CallMutex::new(HashMap::new(), "connection_map")),
            terminate_condvar: Arc::new((Mutex::new(false), Condvar::new())),
            did_send_offer: Arc::new(AtomicBool::new(false)),
            did_notify_application_of_remote_ringing: Arc::new(AtomicBool::new(false)),
            forking: Arc::new(CallMutex::new(None, "forking")),
        };

        Ok(call)
    }

    /// Start a timer to terminate the call if setup takes too long.
    pub fn start_timeout_timer(&self, time_out_period: Duration) -> Result<()> {
        if !time_out_period.is_zero() {
            if let Ok(mut fsm_context) = self.fsm_context.lock() {
                let timeout_runtime = TaskQueueRuntime::new("fsm-timeout")?;

                let mut call_clone = self.clone();
                let when = Instant::now() + time_out_period;
                let call_timeout_future = async move {
                    let sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(when));
                    sleep.await;
                    call_clone
                        .inject_call_timeout()
                        .unwrap_or_else(|e| error!("Inject call timeout failed: {:?}", e))
                };

                timeout_runtime.spawn(call_timeout_future);
                fsm_context.timeout_runtime = Some(timeout_runtime);
            }
        }

        Ok(())
    }

    /// Return the Call identifier.
    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    /// Return the Call direction.
    pub fn direction(&self) -> CallDirection {
        self.direction
    }

    /// Return the current Call state.
    pub fn state(&self) -> Result<CallState> {
        let state = self.state.lock()?;
        Ok(*state)
    }

    /// Update the current Call state.
    pub fn set_state(&self, new_state: CallState) -> Result<()> {
        let mut state = self.state.lock()?;
        *state = new_state;
        Ok(())
    }

    /// Set the active device ID this call is connected to.
    pub fn set_active_device_id(&self, remote_device: DeviceId) -> Result<()> {
        let mut active_device_id = self.active_device_id.lock()?;
        match *active_device_id {
            Some(v) => return Err(RingRtcError::ActiveDeviceIdAlreadySet(v).into()),
            None => *active_device_id = Some(remote_device),
        }
        Ok(())
    }

    /// Return the active device ID.
    pub fn active_device_id(&self) -> Result<DeviceId> {
        self.active_device_id.lock()?.ok_or(
            RingRtcError::OptionValueNotSet(
                String::from("active_connection"),
                String::from("active_connection"),
            )
            .into(),
        )
    }

    /// Return the active Connection this call is associated with.
    pub fn active_connection(&self) -> Result<Connection<T>> {
        let connection_map = self.connection_map.lock()?;
        match connection_map.get(&self.active_device_id()?) {
            Some(v) => Ok(v.clone()),
            None => Err(RingRtcError::ConnectionNotFound(self.active_device_id()?).into()),
        }
    }

    /// For an incoming call, create a PendingCall structure for
    /// holding the offer and ICE candidates sent by the remote side
    /// *before* the application has formally decided to accept the
    /// call.
    pub fn handle_received_offer(&self, received: signaling::ReceivedOffer) -> Result<()> {
        let mut pending_call = self.pending_call.lock()?;
        match pending_call.as_ref() {
            Some(pending) => {
                return Err(
                    RingRtcError::PendingCallAlreadySet(pending.received.sender_device_id).into(),
                )
            }
            None => {
                let pending_data = PendingCall {
                    received,
                    ice_candidates: Vec::new(),
                };
                *pending_call = Some(pending_data);
            }
        }
        Ok(())
    }

    /// Return the local Device Id associated with this call.
    pub fn local_device_id(&self) -> DeviceId {
        self.local_device_id
    }

    /// Store the application specific CallContext associated with this call.
    pub fn set_call_context(&self, call_context: <T as Platform>::AppCallContext) -> Result<()> {
        let mut app_call_context = self.app_call_context.lock()?;
        match *app_call_context {
            Some(_) => return Err(RingRtcError::AppCallContextAlreadySet(self.call_id).into()),
            None => *app_call_context = Some(call_context),
        }
        Ok(())
    }

    /// Return a clone of the call context
    pub fn call_context(&self) -> Result<<T as Platform>::AppCallContext> {
        let app_call_context = self.app_call_context.lock()?;
        match app_call_context.as_ref() {
            Some(v) => Ok(v.clone()),
            None => Err(RingRtcError::OptionValueNotSet(
                String::from("call_context()"),
                String::from("call_context"),
            )
            .into()),
        }
    }

    /// Returns `true` if the call is terminating.
    pub fn terminating(&self) -> Result<bool> {
        if let CallState::Terminating = self.state()? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Return the strong reference count on the state `Arc<Mutex<>>`.
    fn ref_count(&self) -> usize {
        Arc::strong_count(&self.state)
    }

    /// Return the call manager, under a locked mutex.
    pub fn call_manager(&self) -> Result<MutexGuard<'_, CallManager<T>>> {
        self.call_manager.lock()
    }

    /// Return the remote peer, under a locked mutex.
    pub fn remote_peer(&self) -> Result<MutexGuard<'_, <T as Platform>::AppRemotePeer>> {
        self.app_remote_peer.lock()
    }

    /// Inform the application that a call should be started.
    ///
    /// This is a pass through to the CallManager.
    pub fn handle_start_call(&self) -> Result<()> {
        let call_manager = self.call_manager()?;
        let remote_peer = self.remote_peer()?;

        call_manager.start_call(&remote_peer, self.call_id, self.direction, self.media_type)
    }

    /// Enable media flowing through the active connection and notify the application.
    pub fn accept_locally(&self) -> Result<()> {
        let mut connection = self.active_connection()?;
        connection.inject_accept()?;
        connection.enable_media()?;
        connection.start_tick()?;
        self.notify_application(ApplicationEvent::LocalAccepted)?;
        Ok(())
    }

    /// Enable media flowing through the active connection and notify the application.
    pub fn accept_remotely(&self) -> Result<()> {
        let connection = self.active_connection()?;
        connection.enable_media()?;
        connection.start_tick()?;
        self.notify_application(ApplicationEvent::RemoteAccepted)?;
        // Now that we've picked a connection, we can notify the app of the
        // network route.
        self.notify_network_route_changed(connection.network_route()?)?;
        Ok(())
    }

    /// Notify application of an event.
    ///
    /// This is a pass through to the CallManager.
    pub fn notify_application(&self, event: ApplicationEvent) -> Result<()> {
        if event == ApplicationEvent::RemoteRinging {
            let did_notify = self
                .did_notify_application_of_remote_ringing
                .swap(true, Ordering::SeqCst);
            if did_notify {
                // Don't notify the application of RemoteRinging more than once for the same call.
                return Ok(());
            }
        }

        let call_manager = self.call_manager()?;
        let remote_peer = self.remote_peer()?;

        call_manager.notify_application(&remote_peer, self.call_id, event)
    }

    /// Notify application of a change to the network route.
    ///
    /// This is a pass through to the CallManager.
    pub fn notify_network_route_changed(&self, network_route: NetworkRoute) -> Result<()> {
        let call_manager = self.call_manager()?;
        let remote_peer = self.remote_peer()?;

        call_manager.notify_network_route_changed(&remote_peer, network_route)
    }

    /// Notify application of audio levels
    ///
    /// This is a pass through to the CallManager.
    pub fn notify_audio_levels(
        &self,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        let call_manager = self.call_manager()?;
        let remote_peer = self.remote_peer()?;

        call_manager.notify_audio_levels(&remote_peer, captured_level, received_level)
    }

    /// Notify call manager of an internal error.
    ///
    pub fn internal_error(&self, error: anyhow::Error) -> Result<()> {
        let mut call_manager = self.call_manager()?;

        call_manager.internal_error(self.call_id, error)
    }

    /// Take and send the buffered ICE candidates to the remote peer.
    ///
    /// This is a pass through to the CallManager.
    pub fn send_buffered_local_ice_candidates(
        &self,
        connection: Connection<T>,
        broadcast: bool,
    ) -> Result<()> {
        let state = self.state()?;

        info!("send_buffered_local_ice_candidates(): {}", state);

        match state {
            CallState::Terminating | CallState::Terminated => {
                info!("send_buffered_local_ice_candidates(): ignoring, terminating state");
                Ok(())
            }
            _ => {
                let mut call_manager = self.call_manager()?;

                call_manager.send_buffered_local_ice_candidates(self.clone(), connection, broadcast)
            }
        }
    }

    /// Associate a MediaStream with a Connection.
    ///
    /// This is a pass through to the CallManager.
    pub fn create_incoming_media(
        &self,
        connection: &Connection<T>,
        incoming_media: MediaStream,
    ) -> Result<<T as Platform>::AppIncomingMedia> {
        let call_manager = self.call_manager()?;

        call_manager.create_incoming_media(connection, incoming_media)
    }

    /// Connect incoming media
    ///
    /// This is a pass through to the CallManager.
    pub fn connect_incoming_media(
        &self,
        incoming_media: &<T as Platform>::AppIncomingMedia,
    ) -> Result<()> {
        let call_manager = self.call_manager()?;
        let remote_peer = self.remote_peer()?;

        call_manager.connect_incoming_media(&remote_peer, &self.call_context()?, incoming_media)
    }

    /// Proceed with the current call.
    ///
    /// Outgoing Calls:
    ///
    /// - For each DeviceId:
    ///   - Create a Connection.
    ///   - Send an Offer to the remote peer.
    ///
    /// Incoming Calls:
    ///
    /// - create a Connection for the single remote DeviceId.
    /// - handle the previously stored pending Offer and ICE Candidates
    pub fn proceed(
        &mut self,
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    ) -> Result<()> {
        info!("proceed():");

        let mut call_manager = self.call_manager()?;

        match self.direction {
            // This happens after received_offer and an offer is put in self.pending_call.
            CallDirection::InComing => {
                let mut pending_call = self.pending_call.lock()?;
                if let Some(pending_call) = pending_call.take() {
                    let remote_device_id = pending_call.received.sender_device_id;
                    info!("proceed(): incoming: remote_device: {}", remote_device_id);

                    let mut connection = call_manager.create_connection(
                        self,
                        remote_device_id,
                        ConnectionType::Incoming,
                        pending_call.received.offer.latest_version(),
                        bandwidth_mode,
                        audio_levels_interval,
                    )?;
                    let answer = connection
                        .start_incoming(pending_call.received, pending_call.ice_candidates)?;
                    call_manager.send_answer(
                        self.clone(),
                        connection.clone(),
                        signaling::SendAnswer {
                            receiver_device_id: remote_device_id,
                            answer,
                        },
                    )?;

                    let mut connection_map = self.connection_map.lock()?;
                    connection_map.insert(remote_device_id, connection);

                    // For incoming calls we only have 1 connection and it is the active connection.
                    self.set_active_device_id(remote_device_id)?;
                } else {
                    return Err(RingRtcError::OptionValueNotSet(
                        "proceed()".to_owned(),
                        "pending_offer".to_owned(),
                    )
                    .into());
                }
            }
            CallDirection::OutGoing => {
                let mut parent_connection = call_manager.create_connection(
                    self,
                    0,
                    ConnectionType::OutgoingParent,
                    signaling::Version::V4,
                    bandwidth_mode,
                    audio_levels_interval,
                )?;
                let (local_secret, ice_gatherer, offer) =
                    parent_connection.start_outgoing_parent(self.media_type, bandwidth_mode)?;

                // Keep around so that it's not closed until all the connections are closed.
                *(self.forking.lock()?) = Some(ForkingState {
                    parent_connection: parent_connection.clone(),
                    local_secret,
                    ice_gatherer,
                    offer: offer.clone(),
                });

                call_manager.send_offer(self.clone(), parent_connection, offer)?;
                // If we don't do this, then hangups won't be sent.
                self.did_send_offer.store(true, Ordering::Release);
            }
        }
        Ok(())
    }

    /// Handle the received answer.
    pub fn received_answer(&self, received: signaling::ReceivedAnswer) -> Result<()> {
        let sender_device_id = received.sender_device_id;
        let mut connection_map = self.connection_map.lock()?;
        if let hash_map::Entry::Vacant(e) = connection_map.entry(sender_device_id) {
            if self.state()? == CallState::ConnectedAndAccepted
                || self.state()? == CallState::ReconnectingAfterAccepted
            {
                info!(
                    "received_answer from device {} when already accepted, so ignore",
                    sender_device_id
                );
                return Ok(());
            }
            let mut maybe_forking = self.forking.lock()?;
            if let Some(forking) = maybe_forking.as_mut() {
                info!("received_answer from device {}; forking enabled, so inject into connection_map", sender_device_id);
                let call_manager = self.call_manager()?;
                let bandwidth_mode = forking.parent_connection.local_bandwidth_mode()?;
                let audio_levels_interval = forking.parent_connection.audio_levels_interval();
                let mut child_connection = call_manager.create_connection(
                    self,
                    sender_device_id,
                    ConnectionType::OutgoingChild,
                    received.answer.latest_version(),
                    bandwidth_mode,
                    audio_levels_interval,
                )?;
                child_connection.start_outgoing_child(
                    &forking.local_secret,
                    &forking.ice_gatherer,
                    &forking.offer,
                    &received,
                )?;
                e.insert(child_connection);
                return Ok(());
            }
            info!(
                "received_answer from new device {}; forking not enabled, so fail",
                sender_device_id
            );
            return Err(RingRtcError::ConnectionNotFound(sender_device_id).into());
        };
        Ok(())
    }

    /// Handle the received ICE candidates.
    pub fn received_ice(&self, mut received: signaling::ReceivedIce) -> Result<()> {
        info!(
            "received_ice(): id: {}",
            self.call_id().format(received.sender_device_id)
        );
        let sender_device_id = received.sender_device_id;

        let mut pending_call = self.pending_call.lock()?;
        if let Some(pending_call) = pending_call.as_mut() {
            info!("received_ice(): storing in pending_call");
            pending_call
                .ice_candidates
                .append(&mut received.ice.candidates);
            Ok(())
        } else {
            let mut connection_map = self.connection_map.lock()?;
            match connection_map.get_mut(&sender_device_id) {
                Some(connection) => connection.inject_received_ice(received.ice),
                None => {
                    if self.state()? == CallState::ConnectedAndAccepted
                        || self.state()? == CallState::ReconnectingAfterAccepted
                    {
                        // This can happen when call forking is enabled.
                        info!(
                            "received_ice from device {} when already accepted, so ignore",
                            sender_device_id
                        );
                        return Ok(());
                    }
                    Err(RingRtcError::ConnectionNotFound(sender_device_id).into())
                }
            }
        }
    }

    /// Return true if at least one offer has been sent for the outgoing
    /// call or if the call is incoming.
    pub fn should_send_hangup(&self) -> bool {
        match self.direction {
            CallDirection::OutGoing => {
                // If the call is outgoing, only send hangup message if an
                // offer was actually sent out.
                self.did_send_offer.load(Ordering::Acquire)
            }
            _ => true,
        }
    }

    /// Send a Hangup to all callees via signal messaging.
    pub fn send_hangup_via_signaling_to_all(&self, hangup: signaling::Hangup) -> Result<()> {
        self.call_manager()?.send_hangup(
            self.clone(),
            self.call_id(),
            signaling::SendHangup { hangup },
        )
    }

    /// Send a Hangup on all underlying Connections via RTP data
    pub fn send_hangup_via_rtp_data_to_all(&self, hangup: signaling::Hangup) -> Result<()> {
        info!(
            "send_hangup_via_rtp_data_to_all(): call_id: {}",
            self.call_id()
        );

        let mut connection_map = self.connection_map.lock()?;
        for connection in connection_map.values_mut() {
            info!(
                "send_hangup_via_rtp_data_to_all(): call_id: {} remote_device_id: {}",
                self.call_id(),
                connection.remote_device_id()
            );
            connection.inject_send_hangup_via_rtp_data(hangup)?;
        }
        Ok(())
    }

    /// Send a Hangup on all underlying Connections via RTP data
    /// except for the given device_id.
    pub fn send_hangup_via_rtp_data_to_all_except(
        &self,
        hangup: signaling::Hangup,
        excluded_remote_device_id: DeviceId,
    ) -> Result<()> {
        info!(
            "send_hangup_via_rtp_data_to_all_except(): {} hangup: {:?} excluded remote_device_id: {}",
            self.call_id(),
            hangup,
            excluded_remote_device_id
        );

        let mut connection_map = self.connection_map.lock()?;
        for connection in connection_map.values_mut() {
            let remote_device_id = connection.remote_device_id();
            if excluded_remote_device_id != remote_device_id {
                info!(
                    "send_hangup_via_rtp_data_to_all_except(): included remote_device_id: {}",
                    remote_device_id
                );
                connection.inject_send_hangup_via_rtp_data(hangup)?;
            }
        }
        Ok(())
    }

    /// Convenience function to send a hangup using both the RTP data to currently
    /// connected remotes and signaling to all as a backup.
    pub fn send_hangup_via_rtp_data_and_signaling_to_all_except(
        &self,
        hangup: signaling::Hangup,
        excluded_remote_device_id: DeviceId,
    ) -> Result<()> {
        // Send hangup via RTP data
        self.send_hangup_via_rtp_data_to_all_except(hangup, excluded_remote_device_id)?;
        self.send_hangup_via_signaling_to_all(hangup)
    }

    /// ICE failed for a specific connection
    pub fn handle_ice_failed(&mut self, remote_device: DeviceId) -> Result<()> {
        info!("ice_failed(): id: {}", self.call_id().format(remote_device));

        if let Ok(active_device_id) = self.active_device_id() {
            // There is an active connection.
            if active_device_id == remote_device {
                // The active connection failed, close the call.
                info!("ice_failed(): active connection");
                let mut call_manager = self.call_manager()?;
                call_manager.connection_failure(self.call_id)?;
            }
        } else if self.connection_map.lock()?.len() == 1 {
            // Only one connection left for this call and it just
            // failed.
            info!("ice_failed(): last connection");
            let mut call_manager = self.call_manager()?;
            call_manager.connection_failure(self.call_id)?;
        } else {
            // Close this connection and remove it from the map
            let mut connection_map = self.connection_map.lock()?;
            if let Some(mut connection) = connection_map.remove(&remote_device) {
                info!("ice_failed(): terminating inactive connection");
                connection.terminate()?;
            }
        }

        Ok(())
    }

    /// Send a CallEvent to the internal FSM.
    ///
    /// Using the `EventPump` send a CallEvent to the internal FSM.
    fn inject_event(&mut self, event: CallEvent) -> Result<()> {
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

    /// Terminate Connections for this Call.
    fn terminate_connections(&mut self) -> Result<()> {
        // close any application specific resources
        if let Ok(call_context) = self.call_context() {
            let call_manager = self.call_manager()?;
            call_manager.disconnect_incoming_media(&call_context)?;
        }

        if let Some(mut forking) = self.forking.lock()?.take() {
            forking.parent_connection.terminate()?;
        }
        let mut connection_map = self.connection_map.lock()?;
        for (_, mut connection) in connection_map.drain() {
            info!(
                "terminate_connections(): call_id: {} remote_device_id: {}",
                self.call_id(),
                connection.remote_device_id()
            );
            // blocks as connection FSM shutsdown
            connection.terminate()?;
        }
        connection_map.clear();
        Ok(())
    }

    /// Terminate Connections for all devices except the device
    /// that accepted the call.
    pub fn terminate_connections_except_accepted(
        &mut self,
        accepted_device_id: DeviceId,
    ) -> Result<()> {
        let mut connection_map = self.connection_map.lock()?;

        info!("terminate_connections_except_accepted():");

        connection_map.retain(|_, connection| {
            if connection.remote_device_id() != accepted_device_id {
                // blocks as connection FSM shutsdown
                if let Err(e) = connection.terminate() {
                    error!("Error when closing {}", e)
                }
            }

            connection.remote_device_id() == accepted_device_id
        });

        info!(
            "terminate_connections_except_accepted(): len: {}",
            connection_map.len()
        );

        Ok(())
    }

    /// Terminate this Call.
    ///
    /// Notify the internal FSM to terminate.
    ///
    /// `Note:` The current thread is blocked while waiting for the
    /// FSM to signal that termination is complete.
    pub fn terminate(&mut self) -> Result<()> {
        let start_ref_count = self.ref_count();
        info!("terminate(): ref_count: {}", start_ref_count);

        self.set_state(CallState::Terminating)?;
        self.inject_event(CallEvent::Terminate)?;
        self.wait_for_terminate()?;

        self.terminate_connections()?;

        // close down the FSM context
        let mut fsm_context = self.fsm_context.lock()?;
        fsm_context.close();

        self.set_state(CallState::Terminated)?;

        Ok(())
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
                    RingRtcError::MutexPoisoned("Call Terminate Condition Variable".to_string())
                })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned(
                "Call Terminate Condition Variable".to_string(),
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
    pub fn terminate_complete(&mut self) -> Result<()> {
        info!("notify_terminate_complete(): notifying terminate complete...");
        let &(ref mutex, ref condvar) = &*self.terminate_condvar;
        if let Ok(mut terminate_complete) = mutex.lock() {
            *terminate_complete = true;
            condvar.notify_one();
            Ok(())
        } else {
            Err(RingRtcError::MutexPoisoned("Call Terminate Condition Variable".to_string()).into())
        }
    }

    /// Inject a `InternalError` event into the FSM.
    ///
    /// This is used to send an internal error notification to the
    /// call manager.
    ///
    /// `Called By:` FSM when internal errors occur.
    ///
    /// Note: this function does not fail, as there is not much one
    /// can do in this case.
    pub fn inject_internal_error(&mut self, error: anyhow::Error, msg: &str) {
        error!("{}: {}", msg, error);
        let _ = self.inject_event(CallEvent::InternalError(error));
    }

    /// Inject a StartCall event into the FSM.
    pub fn inject_start_call(&mut self) -> Result<()> {
        let event = CallEvent::StartCall;
        self.inject_event(event)
    }

    /// Inject a call Proceed event into the FSM.
    pub fn inject_proceed(
        &mut self,
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    ) -> Result<()> {
        let event = CallEvent::Proceed {
            bandwidth_mode,
            audio_levels_interval,
        };
        self.inject_event(event)
    }

    /// Inject an Accept Call event into the FSM.
    pub fn inject_accept_call(&mut self) -> Result<()> {
        self.inject_event(CallEvent::AcceptCall)
    }

    /// Inject a local `SendHangupViaRtpDataToAll` event into the FSM.
    pub fn inject_send_hangup_via_rtp_data_to_all(
        &mut self,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        self.set_state(CallState::Terminating)?;
        self.inject_event(CallEvent::SendHangupViaRtpDataToAll(hangup))
    }

    /// Inject a `ReceivedAnswer` event into the FSM
    pub fn inject_received_answer(&mut self, received: signaling::ReceivedAnswer) -> Result<()> {
        self.inject_event(CallEvent::ReceivedAnswer(received))
    }

    /// Inject a `ReceivedIce` event into the FSM
    pub fn inject_received_ice(&mut self, received: signaling::ReceivedIce) -> Result<()> {
        self.inject_event(CallEvent::ReceivedIce(received))
    }

    /// Inject a `ReceivedHangup` event into the FSM
    pub fn inject_received_hangup(&mut self, received: signaling::ReceivedHangup) -> Result<()> {
        self.inject_event(CallEvent::ReceivedHangup(received))
    }

    /// Inject a Connection Observer event into the FSM
    pub fn on_connection_observer_event(
        &mut self,
        remote_device_id: DeviceId,
        event: ConnectionObserverEvent,
    ) -> Result<()> {
        if !event.is_frequent() {
            info!(
                "on_connection_observer_event(): call_id: {}, remote_device_id: {}, event: {}",
                self.call_id(),
                remote_device_id,
                event
            );
        }
        self.inject_event(CallEvent::ConnectionObserverEvent(event, remote_device_id))
    }

    /// Inject a Connection Observer error into the FSM
    pub fn on_connection_observer_error(
        &mut self,
        remote_device_id: DeviceId,
        error: anyhow::Error,
    ) -> Result<()> {
        info!(
            "on_connection_observer_error(): call_id: {}, remote_device_id: {} error: {}",
            self.call_id(),
            remote_device_id,
            error
        );
        self.inject_event(CallEvent::ConnectionObserverError(error, remote_device_id))
    }

    /// Inject a local `CallTimeout` event into the FSM.
    ///
    /// `Called By:` Local timeout thread.
    ///
    pub fn inject_call_timeout(&mut self) -> Result<()> {
        let event = CallEvent::CallTimeout;
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
            CallState::Terminated | CallState::Terminating => {
                info!(
                    "call-synchronize(): skipping synchronize while terminating or terminated..."
                );
                return Ok(());
            }
            _ => {}
        }

        let sync = Arc::new((Mutex::new(false), Condvar::new()));
        let event = CallEvent::Synchronize(sync.clone());

        self.inject_event(event)?;

        info!("call-synchronize(): waiting for synchronize complete...");
        let &(ref mutex, ref condvar) = &*sync;
        if let Ok(mut sync_complete) = mutex.lock() {
            while !*sync_complete {
                sync_complete = condvar.wait(sync_complete).map_err(|_| {
                    RingRtcError::MutexPoisoned(
                        "CallConnection Synchronize Condition Variable".to_string(),
                    )
                })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned(
                "CallConnection Synchronize Condition Variable".to_string(),
            )
            .into());
        }
        info!("call-synchronize(): complete");
        Ok(())
    }

    /// Synchronize the caller with the FSM event queue.
    ///
    /// Blocks the caller while the FSM event queue is flushed.
    ///
    /// `Called By:` Test infrastructure
    #[cfg(feature = "sim")]
    pub fn synchronize(&mut self) -> Result<()> {
        // The first sync flushes out any pending events.  These
        // event(s) could fail, which would enqueue another event to
        // the FSM, *behind* the sync event.
        self.inject_synchronize()?;

        // Synchronize all connections in this call
        if let Ok(mut connection_map) = self.connection_map.lock() {
            for (_, connection) in connection_map.iter_mut() {
                info!(
                    "synchronize(): call_id: {} remote_device_id: {}",
                    self.call_id(),
                    connection.remote_device_id()
                );
                // blocks as connection FSM synchronizes
                connection.synchronize()?;
            }
        }

        // The second sync flushes out any error event(s) that might
        // have happened during the first sync.
        self.inject_synchronize()
    }

    /// Return a connection from the connection map.
    ///
    /// `Called By:` Test infrastructure
    #[cfg(feature = "sim")]
    pub fn get_connection(&self, device_id: DeviceId) -> Result<Connection<T>> {
        let connection_map = self.connection_map.lock()?;
        match connection_map.get(&device_id) {
            Some(v) => Ok(v.clone()),
            None => Err(RingRtcError::ConnectionNotFound(device_id).into()),
        }
    }

    /// Return the parent connection when forking is used.
    ///
    /// `Called By:` Test infrastructure
    #[cfg(feature = "sim")]
    pub fn get_parent_connection(&self) -> Result<Connection<T>> {
        let forking = self.forking.lock()?;
        let parent_connection = forking.as_ref().unwrap().parent_connection.clone();
        Ok(parent_connection)
    }
}
