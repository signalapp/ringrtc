//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! A peer-to-peer connection interface.

use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime};

use bytes::{BufMut, BytesMut};

use futures::channel::mpsc::{Receiver, Sender};
use futures::channel::oneshot;
use futures::future::{self, TryFutureExt};

use prost::Message;

use hkdf::Hkdf;
use rand::rngs::OsRng;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::common::{
    units::DataRate, CallDirection, CallId, CallMediaType, ConnectionState, DeviceId, Result,
    RingBench,
};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call::Call;
use crate::core::call_mutex::CallMutex;
use crate::core::connection_fsm::{ConnectionEvent, ConnectionStateMachine};
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::core::util::{ptr_as_box, redact_string, TaskQueueRuntime};
use crate::error::RingRtcError;
use crate::protobuf;

use crate::webrtc;
use crate::webrtc::ice_gatherer::IceGatherer;
use crate::webrtc::media::{
    AudioEncoderConfig, MediaStream, VideoFrame, VideoFrameMetadata, VideoSink,
};
use crate::webrtc::peer_connection::{AudioLevel, PeerConnection, SendRates};
use crate::webrtc::peer_connection_observer::{
    IceConnectionState, NetworkAdapterType, NetworkRoute, PeerConnectionObserverTrait,
    TransportProtocol,
};
use crate::webrtc::rtp;
use crate::webrtc::sdp_observer::{
    create_csd_observer, create_ssd_observer, SessionDescription, SrtpCryptoSuite, SrtpKey,
};
use crate::webrtc::stats_observer::{create_stats_observer, StatsObserver};

/// Used to generate stats, to retransmit RTP messages, and to get audio levels.
const TICK_INTERVAL_MILLIS: u64 = 200;
const TICK_INTERVAL: Duration = Duration::from_millis(TICK_INTERVAL_MILLIS);

/// How often to get and log stats
const POLL_STATS_INTERVAL: Duration = Duration::from_secs(10);
const POLL_STATS_INTERVAL_TICKS: u64 =
    POLL_STATS_INTERVAL.as_millis() as u64 / TICK_INTERVAL_MILLIS;

/// How often to retransmit RTP messages.
const SEND_RTP_DATA_MESSAGE_INTERVAL_MILLIS: u64 = 1000;
const SEND_RTP_DATA_MESSAGE_INTERVAL_TICKS: u64 =
    SEND_RTP_DATA_MESSAGE_INTERVAL_MILLIS / TICK_INTERVAL_MILLIS;

pub const RTP_DATA_PAYLOAD_TYPE: rtp::PayloadType = 101;
pub const OLD_RTP_DATA_SSRC_FOR_OUTGOING: rtp::Ssrc = 1001;
pub const OLD_RTP_DATA_SSRC_FOR_INCOMING: rtp::Ssrc = 2001;
pub const OLD_RTP_DATA_RESERVED: [u8; 4] = [0, 0, 0, 0];
pub const NEW_RTP_DATA_SSRC: rtp::Ssrc = 0xD;

/// Connection observer status notification types
/// Sent from the Connection to the parent Call object
#[derive(Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionObserverEvent {
    StateChanged(ConnectionState),

    /// The remote side sent a sender status message via RTP data
    /// and the value changed.
    RemoteSenderStatusChanged(signaling::SenderStatus),

    /// The remote side sent a hangup message via RTP data
    /// or via signaling.
    ReceivedHangup(signaling::Hangup),

    /// The ICE network route changed
    IceNetworkRouteChanged(NetworkRoute),

    AudioLevels {
        captured_level: AudioLevel,
        received_level: AudioLevel,
    },
}

impl ConnectionObserverEvent {
    // If an event is frequent, avoid logging it.
    pub fn is_frequent(&self) -> bool {
        matches!(self, ConnectionObserverEvent::AudioLevels { .. })
    }
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
    peer_connection: Option<PeerConnection>,
    last_sent_rtp_data_timestamp: rtp::Timestamp,
    /// Raw pointer to Connection object for PeerConnectionObserver
    connection_ptr: Option<webrtc::ptr::Owned<Connection<T>>>,
    /// Application-specific incoming media
    incoming_media: Option<<T as Platform>::AppIncomingMedia>,
    /// Application specific peer connection
    app_connection: Option<<T as Platform>::AppConnection>,
    /// Boxed copy of the stats collector object shared for callbacks.
    stats_observer: Option<Box<StatsObserver>>,
}

// Send and Sync needed to share *const pointer types across threads.
unsafe impl<T> Send for WebRtcData<T> where T: Platform {}

unsafe impl<T> Sync for WebRtcData<T> where T: Platform {}

impl<T> fmt::Display for WebRtcData<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "peer_connection: {:?}, connection_ptr: {:?}",
            self.peer_connection, self.connection_ptr,
        )
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
    fn peer_connection(&self) -> Result<&PeerConnection> {
        match self.peer_connection.as_ref() {
            Some(v) => Ok(v),
            None => Err(RingRtcError::OptionValueNotSet(
                "peer_connection".to_string(),
                "peer_connection".to_string(),
            )
            .into()),
        }
    }
}

/// Encapsulates the FSM and runtime upon which a Connection runs.
struct Context {
    /// Runtime upon which the ConnectionStateMachine runs.
    pub worker_runtime: TaskQueueRuntime,
}

impl Context {
    fn new() -> Result<Self> {
        Ok(Self {
            worker_runtime: TaskQueueRuntime::new("connection-fsm-worker")?,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionId {
    call_id: CallId,
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
    runtime: Option<TaskQueueRuntime>,
    /// Sender for the "cancel" event.
    cancel_sender: Option<oneshot::Sender<()>>,
}

impl TickContext {
    /// Create a new TickContext.
    pub fn new() -> Self {
        Self {
            runtime: None,
            cancel_sender: None,
        }
    }
}

// Don't send below this, even if the remote side requests lower.
const MIN_SEND_RATE: DataRate = DataRate::from_kbps(30);
// When a network route is relayed, don't send more than this.
const RELAYED_MAX_SEND_RATE: DataRate = DataRate::from_mbps(1);

/// State that decides the max send bitrate and audio configuration.
/// It's decided by a combination of the local settings, the remote settings,
/// and the network route (in particular, if it's relayed or not).
#[derive(Debug)]
pub struct BandwidthController {
    /// The current bandwidth mode being used for the local endpoint.
    pub local_mode: BandwidthMode,
    /// The max rate sent from the remote endpoint.
    pub remote_max: Option<DataRate>,
    // The current network route
    pub network_route: NetworkRoute,
}

impl BandwidthController {
    // Min of local, remote, and relay maxs, but can't go below MIN_SEND_RATE
    pub fn max_send_rate(&self) -> DataRate {
        self.local_max()
            .min_opt(self.remote_max)
            .min_opt(self.relay_max())
            .max(MIN_SEND_RATE)
    }

    pub fn audio_encoder_config(&self) -> AudioEncoderConfig {
        // Min of local and inferred remote
        self.local_mode
            .min(self.inferred_remote_mode())
            .audio_encoder_config()
    }

    fn local_max(&self) -> DataRate {
        self.local_mode.max_bitrate()
    }

    fn relay_max(&self) -> Option<DataRate> {
        if self.network_route.local_relayed || self.network_route.remote_relayed {
            Some(RELAYED_MAX_SEND_RATE)
        } else {
            None
        }
    }

    // Since the remote side doesn't tell us what audio configuration to use, we have to infer it
    // from the bandwidth sent.  But this should be used carefully.
    fn inferred_remote_mode(&self) -> BandwidthMode {
        match self.remote_max {
            Some(remote_max) if remote_max < BandwidthMode::Low.max_bitrate() => {
                BandwidthMode::VeryLow
            }
            Some(remote_max) if remote_max < BandwidthMode::Normal.max_bitrate() => {
                BandwidthMode::Low
            }
            _ => BandwidthMode::Normal,
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
    call: Arc<CallMutex<Call<T>>>,
    /// Injects events into the [ConnectionStateMachine](../call_fsm/struct.CallStateMachine.html).
    fsm_sender: Sender<(Connection<T>, ConnectionEvent)>,
    /// Kept around between new() and start() so we can delay the starting of the FSM
    /// but queue events that happen while starting.
    fsm_receiver: Option<Receiver<(Connection<T>, ConnectionEvent)>>,
    /// Unique 64-bit number identifying the call.
    call_id: CallId,
    /// Connection ID, identifying the call and remote_device.
    connection_id: ConnectionId,
    /// The call direction, inbound or outbound.
    direction: CallDirection,
    /// The current state of the call connection
    state: Arc<CallMutex<ConnectionState>>,
    /// Execution context for the call connection FSM
    context: Arc<CallMutex<Context>>,
    /// Ancillary WebRTC data.
    webrtc: Arc<CallMutex<WebRtcData<T>>>,
    /// State that decides what bandwidth to use for sending.
    bandwidth_controller: Arc<CallMutex<BandwidthController>>,
    /// The interval for audio level polling
    audio_levels_interval: Option<Duration>,
    /// Local ICE candidates waiting to be sent over signaling.
    buffered_local_ice_candidates: Arc<CallMutex<Vec<signaling::IceCandidate>>>,
    /// Condition variable used at termination to quiesce and synchronize the FSM.
    terminate_condvar: Arc<(Mutex<bool>, Condvar)>,
    /// This is write-once configuration and will not change.
    connection_type: ConnectionType,
    /// Execution context for the connection periodic timer tick
    tick_context: Arc<CallMutex<TickContext>>,
    /// The accumulated state of sending messages over RTP data
    accumulated_rtp_data_message: Arc<CallMutex<protobuf::rtp_data::Message>>,
    /// We use this to drop out-of-order messages.
    last_received_rtp_data_timestamp: Arc<CallMutex<rtp::Timestamp>>,
    // If set, all of the video frames will go here.
    // This is separate from the observer so it can bypass a thread hop.
    incoming_video_sink: Option<Box<dyn VideoSink>>,
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
            call: Arc::clone(&self.call),
            fsm_sender: self.fsm_sender.clone(),
            // Clones shouldn't need the Receiver because it's only used
            // for the one reference that is used by the creator between
            // creation and starting.
            fsm_receiver: None,
            call_id: self.call_id,
            connection_id: self.connection_id,
            direction: self.direction,
            state: Arc::clone(&self.state),
            context: Arc::clone(&self.context),
            webrtc: Arc::clone(&self.webrtc),
            bandwidth_controller: Arc::clone(&self.bandwidth_controller),
            audio_levels_interval: self.audio_levels_interval,
            buffered_local_ice_candidates: Arc::clone(&self.buffered_local_ice_candidates),
            terminate_condvar: Arc::clone(&self.terminate_condvar),
            connection_type: self.connection_type,
            tick_context: Arc::clone(&self.tick_context),
            accumulated_rtp_data_message: Arc::clone(&self.accumulated_rtp_data_message),
            last_received_rtp_data_timestamp: Arc::clone(&self.last_received_rtp_data_timestamp),
            incoming_video_sink: self.incoming_video_sink.clone(),
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
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
        incoming_video_sink: Option<Box<dyn VideoSink>>,
    ) -> Result<Self> {
        // Create a FSM runtime for this connection.
        let context = Context::new()?;
        let (fsm_sender, fsm_receiver) = futures::channel::mpsc::channel(256);

        let call_id = call.call_id();
        let direction = call.direction();

        let webrtc = WebRtcData {
            peer_connection: None,
            last_sent_rtp_data_timestamp: 0,
            connection_ptr: None,
            incoming_media: None,
            app_connection: None,
            stats_observer: None,
        };

        let connection = Self {
            fsm_sender,
            fsm_receiver: Some(fsm_receiver),
            call_id,
            connection_id: ConnectionId::new(call_id, remote_device),
            direction,
            call: Arc::new(CallMutex::new(call, "call")),
            state: Arc::new(CallMutex::new(ConnectionState::NotYetStarted, "state")),
            context: Arc::new(CallMutex::new(context, "context")),
            webrtc: Arc::new(CallMutex::new(webrtc, "webrtc")),
            bandwidth_controller: Arc::new(CallMutex::new(
                BandwidthController {
                    local_mode: bandwidth_mode,
                    remote_max: None,
                    network_route: NetworkRoute {
                        local_adapter_type: NetworkAdapterType::Unknown,
                        local_adapter_type_under_vpn: NetworkAdapterType::Unknown,
                        local_relayed: false,
                        local_relay_protocol: TransportProtocol::Unknown,
                        remote_relayed: false,
                    },
                },
                "webrtc",
            )),
            audio_levels_interval,
            buffered_local_ice_candidates: Arc::new(CallMutex::new(
                Vec::new(),
                "buffered_local_ice_candidates",
            )),
            terminate_condvar: Arc::new((Mutex::new(false), Condvar::new())),
            connection_type,
            tick_context: Arc::new(CallMutex::new(TickContext::new(), "tick_context")),
            accumulated_rtp_data_message: Arc::new(CallMutex::new(
                protobuf::rtp_data::Message::default(),
                "accumulated_rtp_data_message",
            )),
            last_received_rtp_data_timestamp: Arc::new(CallMutex::new(
                0,
                "last_received_rtp_data_timestamp",
            )),
            incoming_video_sink,
        };

        connection.init_connection_ptr()?;

        Ok(connection)
    }

    fn start_fsm(&mut self) -> Result<()> {
        let context = self.context.lock()?;
        if let Some(fsm_receiver) = self.fsm_receiver.take() {
            info!("Starting Connection FSM for {}", self.connection_id);
            let connection_fsm = ConnectionStateMachine::new(fsm_receiver)?
                .unwrap_or_else(|e| info!("connection state machine returned error: {}", e));
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
        bandwidth_mode: BandwidthMode,
    ) -> Result<(StaticSecret, IceGatherer, signaling::Offer)> {
        let result = (|| {
            self.set_state(ConnectionState::Starting)?;

            let webrtc = self.webrtc.lock()?;
            let peer_connection = webrtc.peer_connection()?;

            // We have to create and use the IceGatherer before calling
            // create_offer to make sure the ICE parameters are correct.
            let ice_gatherer = peer_connection.create_shared_ice_gatherer()?;
            peer_connection.use_shared_ice_gatherer(&ice_gatherer)?;

            let observer = create_csd_observer();
            peer_connection.create_offer(observer.as_ref());
            // This must be kept in sync with call.rs where it passes in V2 into create_connection.
            let offer = observer.get_result()?;

            // We have to do this before we pass ownership of offer_sdi into set_local_description.
            let (local_secret, local_public_key) = generate_local_secret_and_public_key()?;
            let v4_offer = offer.to_v4(local_public_key.as_bytes().to_vec(), bandwidth_mode)?;

            info!(
                "Outgoing offer codecs: {:?}, max_bitrate: {:?}",
                v4_offer.receive_video_codecs, v4_offer.max_bitrate_bps
            );

            if v4_offer.receive_video_codecs.is_empty() {
                warn!(
                    "No receive video codecs in outgoing offer. SDP:\n{}",
                    redact_string(offer.to_sdp().as_deref().unwrap_or("None"))
                );
            }

            // The only purpose of this is to start gathering ICE candidates.
            // But we need to call set_local_description before we munge it.
            let observer = create_ssd_observer();
            peer_connection.set_local_description(observer.as_ref(), offer);
            observer.get_result()?;

            let offer = signaling::Offer::from_v4(call_media_type, v4_offer)?;

            self.set_state(ConnectionState::IceGathering)?;
            Ok((local_secret, ice_gatherer, offer))
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
        local_secret: &StaticSecret,
        ice_gatherer: &IceGatherer,
        offer: &signaling::Offer,
        received: &signaling::ReceivedAnswer,
    ) -> Result<()> {
        let result = (|| {
            self.set_state(ConnectionState::Starting)?;

            // We need to always take the locks in this order. See reconfigure_send_bandwidth.
            let mut bandwidth_controller = self.bandwidth_controller.lock()?;
            let mut webrtc = self.webrtc.lock()?;

            // Create a stats observer object.
            let stats_observer = create_stats_observer(POLL_STATS_INTERVAL);
            webrtc.stats_observer = Some(stats_observer);

            let peer_connection = webrtc.peer_connection()?;

            peer_connection.use_shared_ice_gatherer(ice_gatherer)?;

            // Don't enable audio flow until the call is accepted.
            peer_connection.set_audio_recording_enabled(false);
            peer_connection.set_audio_playout_enabled(false);

            let (mut offer, mut answer, remote_public_key) =
                if let (Some(v4_offer), Some(v4_answer)) = (offer.to_v4(), received.answer.to_v4())
                {
                    // Set the remote max based on the bitrate in the answer.
                    bandwidth_controller.remote_max =
                        v4_answer.max_bitrate_bps.map(DataRate::from_bps);

                    let offer = SessionDescription::offer_from_v4(&v4_offer)?;
                    let answer = SessionDescription::answer_from_v4(&v4_answer)?;

                    info!(
                    "Incoming answer codecs: {:?}, max_bitrate: {:?}, bandwidth_controller: {:?}",
                    v4_answer.receive_video_codecs, v4_answer.max_bitrate_bps, bandwidth_controller
                );

                    (offer, answer, v4_answer.public_key)
                } else {
                    return Err(RingRtcError::UnknownSignaledProtocolVersion.into());
                };

            if let Some(remote_public_key) = remote_public_key {
                let callee_identity_key = &received.sender_identity_key;
                let caller_identity_key = &received.receiver_identity_key;
                let NegotiatedSrtpKeys {
                    offer_key,
                    answer_key,
                } = negotiate_srtp_keys(
                    local_secret,
                    &remote_public_key,
                    caller_identity_key,
                    callee_identity_key,
                )?;
                offer.disable_dtls_and_set_srtp_key(&offer_key)?;
                answer.disable_dtls_and_set_srtp_key(&answer_key)?;
            }

            let observer = create_ssd_observer();
            peer_connection.set_local_description(observer.as_ref(), offer);
            observer.get_result()?;

            let observer = create_ssd_observer();
            peer_connection.set_remote_description(observer.as_ref(), answer);
            // on_add_stream and on_ice_connected can all happen while
            // SetRemoteDescription is happening. But none of those will be processed
            // until start_fsm() is called below.
            observer.get_result()?;

            // Don't enable until the call is accepted.
            peer_connection.set_outgoing_media_enabled(false);
            // But do start incoming RTP right away so that we can receive the
            // "accepted" message.
            // Warning: we're holding the lock to webrtc_data while we
            // block on the WebRTC network thread, so we need to make
            // sure we don't grab the webrtc_data lock in
            // handle_rtp_received.
            peer_connection.receive_rtp(RTP_DATA_PAYLOAD_TYPE)?;
            peer_connection.set_incoming_media_enabled(true);

            self.apply_bandwidth_controller(&mut bandwidth_controller, &mut webrtc)?;

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

            // We need to always take the locks in this order. See reconfigure_send_bandwidth.
            let mut bandwidth_controller = self.bandwidth_controller.lock()?;
            let mut webrtc = self.webrtc.lock()?;

            // Create a stats observer object.
            let stats_observer = create_stats_observer(POLL_STATS_INTERVAL);
            webrtc.stats_observer = Some(stats_observer);

            let peer_connection = webrtc.peer_connection()?;

            // Don't enable audio flow until the call is accepted.
            peer_connection.set_audio_recording_enabled(false);
            peer_connection.set_audio_playout_enabled(false);

            let v4_offer = received.offer.to_v4();
            let (mut offer, remote_public_key) = if let Some(v4_offer) = v4_offer.as_ref() {
                // Set the remote mode based on the bitrate in the offer.
                bandwidth_controller.remote_max = v4_offer.max_bitrate_bps.map(DataRate::from_bps);

                info!(
                    "Incoming offer codecs: {:?}, max_bitrate: {:?}, bandwidth_controller: {:?}",
                    v4_offer.receive_video_codecs, v4_offer.max_bitrate_bps, bandwidth_controller
                );

                let offer = SessionDescription::offer_from_v4(v4_offer)?;

                (offer, v4_offer.public_key.clone())
            } else {
                return Err(RingRtcError::UnknownSignaledProtocolVersion.into());
            };

            let (local_secret, local_public_key) = generate_local_secret_and_public_key()?;
            let answer_key = match remote_public_key {
                None => None,
                Some(remote_public_key) => {
                    let caller_identity_key = &received.sender_identity_key;
                    let callee_identity_key = &received.receiver_identity_key;
                    let NegotiatedSrtpKeys {
                        offer_key,
                        answer_key,
                    } = negotiate_srtp_keys(
                        &local_secret,
                        &remote_public_key,
                        caller_identity_key,
                        callee_identity_key,
                    )?;
                    offer.disable_dtls_and_set_srtp_key(&offer_key)?;
                    Some(answer_key)
                }
            };

            let observer = create_ssd_observer();
            peer_connection.set_remote_description(observer.as_ref(), offer);
            // on_add_stream can happen while SetRemoteDescription
            // is happening.  But they won't be processed until start_fsm() is called
            // below.
            observer.get_result()?;

            let observer = create_csd_observer();
            peer_connection.create_answer(observer.as_ref());
            let mut answer = observer.get_result()?;
            if let Some(answer_key) = &answer_key {
                answer.disable_dtls_and_set_srtp_key(answer_key)?;
            }

            let answer_to_send = if v4_offer.is_some() {
                let v4_answer = answer.to_v4(
                    local_public_key.as_bytes().to_vec(),
                    bandwidth_controller.local_mode,
                )?;

                info!(
                    "Outgoing answer codecs: {:?}, max_bitrate: {:?}, bandwidth_controller: {:?}",
                    v4_answer.receive_video_codecs, v4_answer.max_bitrate_bps, bandwidth_controller
                );

                // We have to change the local answer to match what we send back
                answer = SessionDescription::answer_from_v4(&v4_answer)?;
                // And we have to make sure to do this again since answer_from_v4 doesn't do it.
                if let Some(answer_key) = &answer_key {
                    answer.disable_dtls_and_set_srtp_key(answer_key)?;
                }
                signaling::Answer::from_v4(v4_answer)?
            } else {
                return Err(RingRtcError::UnknownSignaledProtocolVersion.into());
            };

            // Don't enable incoming RTP until accepted.
            // This should be done before we set local description to make sure
            // we don't get ICE connected really fast and allow any packets through.
            peer_connection.set_incoming_media_enabled(false);

            let observer = create_ssd_observer();
            peer_connection.set_local_description(observer.as_ref(), answer);

            // on_ice_connected can happen while SetLocalDescription is happening.
            // But it won't be processed until start_fsm() is called below.
            observer.get_result()?;

            // Don't enable until call is accepted.
            peer_connection.set_outgoing_media_enabled(false);

            // No RTP will be processed/received until
            // peer_connection.set_incoming_media_enabled(true).
            // Warning: we're holding the lock to webrtc_data while we
            // block on the WebRTC network thread, so we need to make
            // sure we don't grab the webrtc_data lock in
            // handle_rtp_received.
            peer_connection.receive_rtp(RTP_DATA_PAYLOAD_TYPE)?;

            self.apply_bandwidth_controller(&mut bandwidth_controller, &mut webrtc)?;

            ringbench!(
                RingBench::Conn,
                RingBench::WebRtc,
                format!("ice_candidates({})", remote_ice_candidates.len())
            );

            let peer_connection = webrtc.peer_connection()?;
            self.add_and_remove_remote_ice_candidates(peer_connection, &remote_ice_candidates)?;

            self.set_state(ConnectionState::ConnectingBeforeAccepted)?;
            Ok(answer_to_send)
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

    pub fn connection_id(&self) -> ConnectionId {
        self.connection_id
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
        if *state != new_state {
            *state = new_state;
            self.notify_observer(ConnectionObserverEvent::StateChanged(new_state))?;
        }
        Ok(())
    }

    /// Return the current network route
    pub fn network_route(&self) -> Result<NetworkRoute> {
        let bandwidth_controller = self.bandwidth_controller.lock()?;
        Ok(bandwidth_controller.network_route)
    }

    /// Update the current network route.
    pub fn set_network_route(&self, network_route: NetworkRoute) -> Result<()> {
        self.update_bandwidth_controller(move |bandwidth_controller| {
            if bandwidth_controller.network_route == network_route {
                // Nothing changed
                return false;
            }
            bandwidth_controller.network_route = network_route;
            info!(
                "set_network_route(): bandwidth_controller: {:?}",
                bandwidth_controller
            );
            true
        })?;
        Ok(())
    }

    /// Update the PeerConnection.
    pub fn set_peer_connection(&self, peer_connection: PeerConnection) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        webrtc.peer_connection = Some(peer_connection);
        Ok(())
    }

    /// Return the current local bandwidth mode used for this connection.
    pub fn local_bandwidth_mode(&self) -> Result<BandwidthMode> {
        let bandwidth_controller = self.bandwidth_controller.lock()?;
        Ok(bandwidth_controller.local_mode)
    }

    /// Needed for ICE forking (we must copy this value from the parent connection
    /// to the child connection)
    pub fn audio_levels_interval(&self) -> Option<Duration> {
        self.audio_levels_interval
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

    /// Returns `true` if the call is terminating.
    pub fn terminating(&self) -> Result<bool> {
        if let ConnectionState::Terminating = self.state()? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Clone the Connection, Box it and return a raw pointer to the Box.
    pub fn create_connection_ptr(&self) -> webrtc::ptr::Owned<Connection<T>> {
        let connection_box = Box::new(self.clone());
        unsafe { webrtc::ptr::Owned::from_ptr(Box::into_raw(connection_box)) }
    }

    /// Return the internally tracked connection object pointer, for
    /// use by the PeerConnectionObserver call backs.
    pub fn get_connection_ptr(&self) -> Result<webrtc::ptr::Borrowed<Connection<T>>> {
        let webrtc = self.webrtc.lock()?;
        match webrtc.connection_ptr.as_ref() {
            Some(v) => Ok(v.borrow()),
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

    /// The remote user is updating the max_bitrate via RTP data. They are
    /// making the request so only update locally (if changed).
    pub fn set_remote_max_bitrate(&self, remote_max: DataRate) -> Result<()> {
        self.update_bandwidth_controller(move |bandwidth_controller| {
            if bandwidth_controller.remote_max == Some(remote_max) {
                // Nothing changed
                return false;
            }
            bandwidth_controller.remote_max = Some(remote_max);
            info!(
                "set_remote_max_bitrate(): bandwidth_controller: {:?}",
                bandwidth_controller
            );
            true
        })?;
        Ok(())
    }

    /// The local user is updating the bandwidth mode via the API. Update locally and
    /// send an updated bitrate to the remote.
    pub fn update_bandwidth_mode(&self, local_mode: BandwidthMode) -> Result<()> {
        let changed = self.update_bandwidth_controller(|bandwidth_controller| {
            if bandwidth_controller.local_mode == local_mode {
                // Nothing changed
                return false;
            }
            bandwidth_controller.local_mode = local_mode;
            info!(
                "update_bandwidth_mode(): bandwidth_controller: {:?}",
                bandwidth_controller
            );
            true
        })?;

        if changed {
            let mut receiver_status = protobuf::rtp_data::ReceiverStatus {
                id: Some(u64::from(self.call_id)),
                max_bitrate_bps: Some(local_mode.max_bitrate().as_bps()),
            };
            receiver_status.id = Some(u64::from(self.call_id));

            let mut webrtc = self.webrtc.lock()?;
            self.update_and_send_rtp_data_message(&mut webrtc, move |data| {
                data.receiver_status = Some(receiver_status)
            })?;
        }
        Ok(())
    }

    /// Creates a runtime for statistics to run a timer for the given interval
    /// duration to invoke PeerConnection::GetStats which will pass specific stats
    /// to StatsObserver::on_stats_complete.
    pub fn start_tick(&self) -> Result<()> {
        // Define the future for stats logging.
        let mut connection = self.clone();

        let (cancel_sender, cancel_receiver) = oneshot::channel::<()>();
        let tick_forever = async move {
            let mut interval = tokio::time::interval(TICK_INTERVAL);
            let mut ticks_elapsed = 0u64;
            loop {
                interval.tick().await;
                ticks_elapsed += 1;
                if let Err(err) = connection.tick(ticks_elapsed) {
                    warn!("connection.tick() failed: {:?}", err);
                }
            }
        };
        let tick_until_cancel = async move {
            pin_mut!(tick_forever);
            future::select(tick_forever, cancel_receiver).await;
        };
        debug!("start_tick(): starting the tick runtime");
        let mut tick_context = self.tick_context.lock()?;
        match tick_context.runtime {
            Some(_) => warn!("start_tick(): tick timer already running"),
            None => {
                // Start the tick runtime.
                let runtime = TaskQueueRuntime::new("connection-tick")?;
                runtime.spawn(tick_until_cancel);
                tick_context.runtime = Some(runtime);
                tick_context.cancel_sender = Some(cancel_sender);
            }
        }

        Ok(())
    }

    pub fn tick(&mut self, ticks_elapsed: u64) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;

        if ticks_elapsed % SEND_RTP_DATA_MESSAGE_INTERVAL_TICKS == 0 {
            self.send_latest_rtp_data_message(&mut webrtc)?;
        }

        if ticks_elapsed % POLL_STATS_INTERVAL_TICKS == 0 {
            if let Some(observer) = webrtc.stats_observer.as_ref() {
                let _ = webrtc.peer_connection()?.get_stats(observer);
            } else {
                warn!("tick(): No stats_observer found");
            }
        }

        if let Some(audio_levels_interval) = self.audio_levels_interval {
            let audio_levels_interval_ticks =
                (audio_levels_interval.as_millis() as u64) / TICK_INTERVAL_MILLIS;
            #[allow(clippy::modulo_one)]
            if ticks_elapsed % audio_levels_interval_ticks == 0 {
                let (captured_level, received_levels) =
                    webrtc.peer_connection()?.get_audio_levels();
                let received_level = received_levels
                    .first()
                    .map(|received| received.level)
                    .unwrap_or(0);
                let event = ConnectionObserverEvent::AudioLevels {
                    captured_level,
                    received_level,
                };
                if let Err(err) = self.notify_observer(event) {
                    warn!("tick(): failed to notify of audio levels: {:?}", err);
                }
            }
        }

        Ok(())
    }

    /// Check to see if this Connection is able to send messages.
    /// Once it is terminated it shouldn't be able to.
    pub fn can_send_messages(&self) -> bool {
        !matches!(
            self.state(),
            Ok(ConnectionState::Terminating) | Ok(ConnectionState::Terminated)
        )
    }

    pub fn set_outgoing_media_enabled(&self, enabled: bool) -> Result<()> {
        let webrtc = self.webrtc.lock()?;
        webrtc
            .peer_connection()?
            .set_outgoing_media_enabled(enabled);
        Ok(())
    }

    /// Buffer local ICE candidates, and maybe send them immediately
    pub fn buffer_local_ice_candidates(
        &self,
        candidates: Vec<signaling::IceCandidate>,
    ) -> Result<()> {
        let (buffered_count_before, buffered_count_after) = {
            let mut buffered_candidates = self.buffered_local_ice_candidates.lock()?;
            let buffered_count_before = buffered_candidates.len();
            for candidate in candidates {
                buffered_candidates.push(candidate);
            }
            let buffered_count_after = buffered_candidates.len();
            (buffered_count_before, buffered_count_after)
        };

        // Only when we transition from no candidates to some do we
        // need to signal the message queue that there is something
        // to send for this Connection.
        if buffered_count_before == 0 && buffered_count_after > 0 {
            let call = self.call()?;
            let broadcast = self.connection_type == ConnectionType::OutgoingParent;
            call.send_buffered_local_ice_candidates(self.clone(), broadcast)?
        }

        Ok(())
    }

    /// Get the current local ICE candidates to send to the remote peer.
    pub fn take_buffered_local_ice_candidates(&self) -> Result<Vec<signaling::IceCandidate>> {
        info!("take_buffered_local_ice_candidates():");

        let mut ice_candidates = self.buffered_local_ice_candidates.lock()?;

        let copy_candidates = ice_candidates.clone();
        ice_candidates.clear();

        Ok(copy_candidates)
    }

    pub fn handle_received_ice(&self, ice: signaling::Ice) -> Result<()> {
        let webrtc = self.webrtc.lock()?;
        let pc = webrtc.peer_connection()?;

        self.add_and_remove_remote_ice_candidates(pc, &ice.candidates)
    }

    // This is where we differentiate between received candidate additions and removals.
    fn add_and_remove_remote_ice_candidates(
        &self,
        pc: &PeerConnection,
        remote_ice_candidates: &[signaling::IceCandidate],
    ) -> Result<()> {
        let mut added_sdps = vec![];
        let mut removed_addresses = vec![];
        for candidate in remote_ice_candidates {
            if let Some(removed_address) = candidate.removed_address() {
                removed_addresses.push(removed_address);
                // We don't add a candidate if it's both added and removed because of
                // the backwards-compatibility mechanism we have that contains a dummy
                // candidate.
            } else if let Some(sdp) = candidate.v3_sdp() {
                added_sdps.push(sdp);
            }
        }

        ringbench!(
            RingBench::Conn,
            RingBench::WebRtc,
            format!(
                "ice_candidates({}); ice_candidates_removed({})",
                added_sdps.len(),
                removed_addresses.len()
            )
        );

        for added_sdp in added_sdps {
            if let Err(e) = pc.add_ice_candidate_from_sdp(&added_sdp) {
                warn!("Failed to add ICE candidate: {:?}", e);
            }
        }
        if !removed_addresses.is_empty() {
            pc.remove_ice_candidates(removed_addresses.into_iter());
        }
        Ok(())
    }

    /// Send a hangup message to the remote peer via RTP data.
    pub fn send_hangup_via_rtp_data(&self, hangup: signaling::Hangup) -> Result<()> {
        ringbench!(
            RingBench::Conn,
            RingBench::WebRtc,
            format!("dc(hangup/{})\t{}", hangup, self.connection_id)
        );

        let (hangup_type, hangup_device_id) = hangup.to_type_and_device_id();

        let hangup = protobuf::rtp_data::Hangup {
            id: Some(u64::from(self.call_id)),
            r#type: Some(hangup_type as i32),
            device_id: hangup_device_id,
        };

        let mut webrtc = self.webrtc.lock()?;
        self.update_and_send_rtp_data_message(&mut webrtc, move |data| data.hangup = Some(hangup))
    }

    /// Send an accepted message to the remote peer via RTP data.
    pub fn send_accepted_via_rtp_data(&self) -> Result<()> {
        ringbench!(
            RingBench::Conn,
            RingBench::WebRtc,
            format!("dc(accepted)\t{}", self.connection_id)
        );

        let accepted = protobuf::rtp_data::Accepted {
            id: Some(u64::from(self.call_id)),
        };

        let mut webrtc = self.webrtc.lock()?;
        self.update_and_send_rtp_data_message(&mut webrtc, move |data| {
            data.accepted = Some(accepted)
        })
    }

    fn update_bandwidth_controller(
        &self,
        update: impl FnOnce(&mut BandwidthController) -> bool,
    ) -> Result<bool> {
        // We need to always take the locks in this order. See apply_bandwidth_controller.
        let mut bandwidth_controller = self.bandwidth_controller.lock()?;

        let changed = update(&mut bandwidth_controller);
        if changed {
            // We need to always take the locks in this order. See apply_bandwidth_controller.
            let mut webrtc = self.webrtc.lock()?;
            self.apply_bandwidth_controller(&mut bandwidth_controller, &mut webrtc)?;
        }
        Ok(changed)
    }

    /// Based on the given bandwidth mode, configure the media encoders.
    /// Make sure we always take the locks in the order of (bandwidth_controller, webrtc).
    /// We require passing in &mut MutexGuard<T> instead of &mut T to remind you about this.
    fn apply_bandwidth_controller(
        &self,
        bandwidth_controller: &mut MutexGuard<BandwidthController>,
        webrtc: &mut MutexGuard<WebRtcData<T>>,
    ) -> Result<()> {
        let max_send_rate = bandwidth_controller.max_send_rate();
        let audio_encoder_config = bandwidth_controller.audio_encoder_config();
        info!("apply_bandwidth_controller(): bandwidth_controller: {:?}; max_send_rate: {:?}; audio_encoder_config: {:?}", bandwidth_controller, max_send_rate, audio_encoder_config);

        let peer_connection = webrtc.peer_connection()?;
        peer_connection.set_send_rates(SendRates {
            max: Some(max_send_rate),
            ..SendRates::default()
        })?;
        peer_connection.configure_audio_encoders(&audio_encoder_config);
        Ok(())
    }

    /// Send the remote peer the current sender status via RTP data.
    pub fn update_sender_status_from_fsm(&self, updated: signaling::SenderStatus) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        self.update_and_send_rtp_data_message(&mut webrtc, move |data| {
            let previous = data.sender_status.as_ref();
            let previous_video_enabled =
                previous.and_then(|sender_status| sender_status.video_enabled);
            let previous_sharing_screen =
                previous.and_then(|sender_status| sender_status.sharing_screen);
            data.sender_status = Some(protobuf::rtp_data::SenderStatus {
                id: Some(u64::from(self.call_id)),
                video_enabled: updated.video_enabled.or(previous_video_enabled),
                sharing_screen: updated.sharing_screen.or(previous_sharing_screen),
            });
        })
    }

    /// Populates a message using the supplied closure and sends it via RTP data.
    fn update_and_send_rtp_data_message<F>(
        &self,
        webrtc_data: &mut std::sync::MutexGuard<WebRtcData<T>>,
        populate: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut protobuf::rtp_data::Message),
    {
        let message = {
            // Merge this message into accumulated_state and send out the latest version.
            let mut state = self.accumulated_rtp_data_message.lock()?;
            populate(&mut state);
            state.seqnum = Some(state.seqnum.unwrap_or(0) + 1);
            state.clone()
        };
        info!("Sending RTP data message: {:?}", message);
        self.send_via_rtp_data(webrtc_data, &message)
    }

    /// Sends the current accumulated state via RTP data
    fn send_latest_rtp_data_message(
        &self,
        webrtc_data: &mut std::sync::MutexGuard<WebRtcData<T>>,
    ) -> Result<()> {
        let data = self.accumulated_rtp_data_message.lock()?;
        if *data != protobuf::rtp_data::Message::default() {
            self.send_via_rtp_data(webrtc_data, &data)
        } else {
            // Don't send empty messages
            Ok(())
        }
    }

    /// Send data via RTP data.
    fn send_via_rtp_data(
        &self,
        webrtc_data: &mut std::sync::MutexGuard<WebRtcData<T>>,
        data: &protobuf::rtp_data::Message,
    ) -> Result<()> {
        let mut bytes = BytesMut::with_capacity(OLD_RTP_DATA_RESERVED.len() + data.encoded_len());
        bytes.put_slice(&OLD_RTP_DATA_RESERVED);
        data.encode(&mut bytes)?;

        // At 1hz, this would take 136 years to roll over.
        webrtc_data.last_sent_rtp_data_timestamp += 1;
        let header = rtp::Header {
            pt: RTP_DATA_PAYLOAD_TYPE,
            // TODO: Once all clients are updated to accept the NEW_RTP_DATA_SSRC, use that.
            ssrc: match self.direction {
                CallDirection::InComing => OLD_RTP_DATA_SSRC_FOR_INCOMING,
                CallDirection::OutGoing => OLD_RTP_DATA_SSRC_FOR_OUTGOING,
            },
            // This has to be incremented to make sure SRTP functions properly, but rollovers are OK.
            seqnum: webrtc_data.last_sent_rtp_data_timestamp as rtp::SequenceNumber,
            // Just imagine the clock is the number of heartbeat ticks :).
            // Plus the above sequence number is too small to be useful.
            timestamp: webrtc_data.last_sent_rtp_data_timestamp,
        };
        // Warning: we're holding the lock to webrtc_data while we
        // block on the WebRTC network thread, so we need to make
        // sure we don't grab the webrtc_data lock in
        // handle_rtp_received.
        // Warning: send_rtp() can fail if there is a transient error, so don't take it
        // too seriously.  Plus, we'll resend every second anyway.
        if webrtc_data
            .peer_connection()?
            .send_rtp(header, &bytes)
            .is_err()
        {
            warn!("Could not send RTP data message.");
        }
        Ok(())
    }

    /// Notify the parent call observer about an event.
    pub fn notify_observer(&self, event: ConnectionObserverEvent) -> Result<()> {
        let mut call = self.call.lock()?;
        call.on_connection_observer_event(self.remote_device_id(), event)
    }

    /// Notify the parent call observer about an internal error.
    pub fn internal_error(&self, error: anyhow::Error) -> Result<()> {
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
        // When PeerConnection::SetRemoteDescription triggers PeerConnectionObserver::OnAddStream,
        // the MediaStream is wrapped via create_incoming_media, which does the following on different platforms:
        // - Android: wraps it in layers of JavaMediaStream/jni::JavaMediaStream.
        // - iOS: wraps it in layers of IosMediaStream/AppMediaStreamInterface/ConnectionMediaStream/RTCMediaStream.
        // - Desktop: does no additional wrapping
        // Later, when the call is accepted, the wrapped media
        // is passed to connect_incoming_media, which does the following on different platforms:
        // - iOS: The RTCMediaStream level of wrapping is passed to the app via onConnectMedia, which adds a sink to the first video track.
        // - Android: The JavaMediaStream level of wrapping is passed to the app via onConnectMedia, which adds a sink to the first video track.
        // - Desktop: Uses the PeerConnectionObserver for video sinks rather than adding its own.
        let incoming_media = call.create_incoming_media(self, stream)?;
        self.set_incoming_media(incoming_media)
    }

    /// Connect incoming media (stored by handle_incoming_media) to the application connection,
    /// and enabled audio playout, and enable outgoing and incoming RTP.
    /// Make sure that if you call this, you also notify the app that media is flowing.
    pub fn enable_media(&self) -> Result<()> {
        info!("enable_media(): id: {}", self.connection_id);

        let webrtc = self.webrtc.lock()?;
        let pc = webrtc.peer_connection()?;
        pc.set_audio_recording_enabled(true);
        pc.set_audio_playout_enabled(true);
        pc.set_outgoing_media_enabled(true);
        pc.set_incoming_media_enabled(true);

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
        if let Some(rt) = tick_context.runtime.take() {
            info!("close(): stopping the tick runtime");
            // Send the cancel event
            let sender = tick_context.cancel_sender.take().unwrap();
            let _ = sender.send(());
            // Drop the runtime to shut it down
            std::mem::drop(rt);
        }

        // Free up webrtc related resources.
        let mut webrtc = self.webrtc.lock()?;

        // This makes it safe to destroy the stats observer
        // and the Connection (which is also a PeerConnectionObserver).
        if let Ok(peer_connection) = webrtc.peer_connection() {
            peer_connection.close();
        }

        // dispose of the incoming media
        webrtc.incoming_media = None;

        // dispose of the stats observer
        webrtc.stats_observer = None;

        // Free the application connection object, which is in essence
        // the PeerConnection object.  It is important to dispose of
        // the app_connection before the connection_ptr.  The
        // app_connection refers to the real PeerConnection object,
        // whose observer is using the connection_ptr.  Once the
        // PeerConnection is completely shutdown it is safe to free up
        // the connection_ptr.
        webrtc.app_connection = None;

        // Free the connection object previously used by the
        // PeerConnectionObserver.  Convert the pointer back into a
        // Box and let it go out of scope.
        match webrtc.connection_ptr.take() {
            Some(v) => {
                let _ = unsafe { ptr_as_box(v.as_ptr() as *mut Connection<T>)? };
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
        sdp_for_logging: &str,
        relay_protocol: Option<TransportProtocol>,
    ) -> Result<()> {
        if !force_send && self.connection_type == ConnectionType::OutgoingChild {
            return Ok(());
        }

        if let Some(relay_protocol) = relay_protocol {
            info!(
                "Local ICE candidate: {}; {}; relay_protocol={:?}",
                candidate.to_info_string(),
                redact_string(sdp_for_logging),
                relay_protocol,
            );
        } else {
            info!(
                "Local ICE candidate: {}; {}",
                candidate.to_info_string(),
                redact_string(sdp_for_logging)
            );
        }

        self.inject_event(ConnectionEvent::LocalIceCandidates(vec![candidate]))?;
        Ok(())
    }

    /// Inject a `LocalIceCandidatesRemoved` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `removed_addresses` - Locally removed candidate addresses
    pub fn inject_local_ice_candidates_removed(
        &mut self,
        removed_addresses: Vec<SocketAddr>,
        force_send: bool,
    ) -> Result<()> {
        if !force_send && self.connection_type == ConnectionType::OutgoingChild {
            return Ok(());
        }

        let removed_ports: Vec<u16> = removed_addresses
            .iter()
            .map(|address| address.port())
            .collect();
        info!("Local ICE candidates removed; ports: {:?}", removed_ports,);

        let candidates = removed_addresses
            .into_iter()
            .filter_map(|removed_address| {
                signaling::IceCandidate::from_removed_address(removed_address)
                    .map_err(|e| {
                        warn!("Failed to signal removed candidate: {:?}", e);
                        e
                    })
                    .ok()
            })
            .collect();

        // This is where we make additions and removals look the same in signaling
        // where a "candidate" (really, an update) can be either an addition or removal.
        self.inject_event(ConnectionEvent::LocalIceCandidates(candidates))?;
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

    /// Inject an `IceNetworkRouteChanged` event into the FSM.
    ///
    /// `Called By:` WebRTC `IceNetworkRouteChanged` call back thread.
    pub fn inject_ice_network_route_changed(&mut self, network_route: NetworkRoute) -> Result<()> {
        self.inject_event(ConnectionEvent::IceNetworkRouteChanged(network_route))
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
    pub fn inject_internal_error(&mut self, error: anyhow::Error, msg: &str) {
        error!("{}: {}", msg, error);
        let _ = self.inject_event(ConnectionEvent::InternalError(error));
    }

    pub fn inject_received_via_rtp_data(&mut self, bytes: &[u8]) {
        if bytes.len() > (std::mem::size_of::<protobuf::rtp_data::Message>() * 2) {
            warn!("RTP data message is excessively large: {}", bytes.len());
            return;
        }

        if bytes.is_empty() {
            warn!("RTP data message has zero length");
            return;
        }

        let message = match protobuf::rtp_data::Message::decode(bytes) {
            Ok(v) => v,
            Err(e) => {
                warn!("unable to parse rx protobuf: {}", e);
                return;
            }
        };

        debug!("Received RTP data message: {:?}", message);

        let mut message_handled = false;
        let original_message = message.clone();
        if let Some(accepted) = message.accepted {
            if let CallDirection::OutGoing = self.direction() {
                self.inject_received_accepted_via_rtp_data(CallId::new(accepted.id()))
                    .unwrap_or_else(|e| warn!("unable to inject remote accepted event: {}", e));
            } else {
                warn!("Unexpected incoming accepted message: {:?}", accepted);
                self.inject_internal_error(
                    RingRtcError::RtpDataProtocol(
                        "Received 'accepted' for inbound call".to_string(),
                    )
                    .into(),
                    "",
                );
            };
            message_handled = true;
        };
        if let Some(hangup) = message.hangup {
            self.inject_received_hangup(
                CallId::new(hangup.id()),
                signaling::Hangup::from_type_and_device_id(
                    signaling::HangupType::from_i32(hangup.r#type() as i32)
                        .unwrap_or(signaling::HangupType::Normal),
                    hangup.device_id(),
                ),
            )
            .unwrap_or_else(|e| warn!("unable to inject remote hangup event: {}", e));
            message_handled = true;
        };
        if let (Some(sender_status), Some(seqnum)) = (message.sender_status, message.seqnum) {
            self.inject_received_sender_status_via_rtp_data(
                CallId::new(sender_status.id()),
                signaling::SenderStatus {
                    video_enabled: sender_status.video_enabled,
                    sharing_screen: sender_status.sharing_screen,
                },
                seqnum,
            )
            .unwrap_or_else(|e| warn!("unable to inject remote sender status event: {}", e));
            message_handled = true;
        };
        if let (Some(receiver_status), Some(seqnum)) = (message.receiver_status, message.seqnum) {
            self.inject_received_receiver_status_via_rtp_data(
                CallId::new(receiver_status.id()),
                DataRate::from_bps(receiver_status.max_bitrate_bps()),
                seqnum,
            )
            .unwrap_or_else(|e| warn!("unable to inject remote receiver status event: {}", e));
            message_handled = true;
        };
        if !message_handled {
            info!("Unhandled RTP data message: {:?}", original_message);
        }
    }

    /// Inject a `ReceivedAcceptedViaRtpData` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_received_accepted_via_rtp_data(&mut self, call_id: CallId) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedAcceptedViaRtpData(call_id))
    }

    /// Inject a `ReceivedHangup` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    fn inject_received_hangup(&mut self, call_id: CallId, hangup: signaling::Hangup) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedHangup(call_id, hangup))
    }

    /// Inject a `ReceivedSenderStatusViaRtpData` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    /// * `status` - The status of the remote peer.
    pub fn inject_received_sender_status_via_rtp_data(
        &mut self,
        call_id: CallId,
        status: signaling::SenderStatus,
        seqnum: u64,
    ) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedSenderStatusViaRtpData(
            call_id, status, seqnum,
        ))
    }

    /// Inject a `ReceivedReceiverStatusViaRtpData` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    /// * `max_bitrate_bps` - the bitrate that the remote peer wants to use for
    /// the session.
    pub fn inject_received_receiver_status_via_rtp_data(
        &mut self,
        call_id: CallId,
        max_bitrate: DataRate,
        seqnum: u64,
    ) -> Result<()> {
        self.inject_event(ConnectionEvent::ReceivedReceiverStatusViaRtpData(
            call_id,
            max_bitrate,
            seqnum,
        ))
    }

    /// Inject a `SendHangupViaRtpData event into the FSM.
    pub fn inject_send_hangup_via_rtp_data(&mut self, hangup: signaling::Hangup) -> Result<()> {
        self.set_state(ConnectionState::Terminating)?;
        self.inject_event(ConnectionEvent::SendHangupViaRtpData(hangup))
    }

    /// Inject a local `Accept` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_accept(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::Accept)
    }

    /// Inject a `UpdateSenderStatus` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// * `status` - The local peer's status.
    pub fn update_sender_status(&mut self, status: signaling::SenderStatus) -> Result<()> {
        self.inject_event(ConnectionEvent::UpdateSenderStatus(status))
    }

    /// Inject a `UpdateBandwidthMode` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// * `mode` - The bandwidth mode that should be used
    pub fn inject_update_bandwidth_mode(&mut self, bandwidth_mode: BandwidthMode) -> Result<()> {
        self.inject_event(ConnectionEvent::UpdateBandwidthMode(bandwidth_mode))
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

    #[cfg(feature = "sim")]
    pub fn last_sent_sender_status(&self) -> Option<protobuf::rtp_data::SenderStatus> {
        self.accumulated_rtp_data_message
            .lock()
            .unwrap()
            .sender_status
            .clone()
    }
}

#[cfg(feature = "sim")]
impl Connection<crate::sim::sim_platform::SimPlatform> {
    pub fn peer_connection_rffi(
        &self,
    ) -> crate::webrtc::Arc<crate::webrtc::sim::peer_connection::RffiPeerConnection> {
        let webrtc = self.webrtc.lock().unwrap();
        // This is safe because the webrtc.app_connection() is still alive when this is called.
        // Plus, this is only used by unit tests.
        unsafe {
            crate::webrtc::Arc::from_borrowed(crate::webrtc::ptr::BorrowedRc::from_ptr(
                webrtc.app_connection.as_ref().unwrap()
                    as *const crate::webrtc::sim::peer_connection::RffiPeerConnection,
            ))
        }
    }
}

impl<T> PeerConnectionObserverTrait for Connection<T>
where
    T: Platform,
{
    fn log_id(&self) -> &dyn std::fmt::Display {
        &self.connection_id
    }

    fn handle_ice_candidate_gathered(
        &mut self,
        ice_candidate: signaling::IceCandidate,
        sdp_for_logging: &str,
        relay_protocol: Option<TransportProtocol>,
    ) -> Result<()> {
        let force_send = false;
        self.inject_local_ice_candidate(ice_candidate, force_send, sdp_for_logging, relay_protocol)
    }

    fn handle_ice_candidates_removed(&mut self, removed_addresses: Vec<SocketAddr>) -> Result<()> {
        let force_send = false;
        self.inject_local_ice_candidates_removed(removed_addresses, force_send)
    }

    fn handle_ice_connection_state_changed(&mut self, new_state: IceConnectionState) -> Result<()> {
        match new_state {
            IceConnectionState::Completed | IceConnectionState::Connected => {
                self.inject_ice_connected()
            }
            IceConnectionState::Failed => self.inject_ice_failed(),
            IceConnectionState::Disconnected => self.inject_ice_disconnected(),
            _ => Ok(()),
        }
    }

    fn handle_ice_network_route_changed(&mut self, network_route: NetworkRoute) -> Result<()> {
        self.inject_ice_network_route_changed(network_route)
    }

    fn handle_incoming_media_added(&mut self, stream: MediaStream) -> Result<()> {
        self.inject_received_incoming_media(stream)
    }

    fn handle_incoming_video_frame(
        &mut self,
        track_id: u32,
        _video_frame_metadata: VideoFrameMetadata,
        video_frame: Option<VideoFrame>,
    ) -> Result<()> {
        if let (Some(incoming_video_sink), Some(video_frame)) =
            (self.incoming_video_sink.as_ref(), video_frame)
        {
            incoming_video_sink.on_video_frame(track_id, video_frame)
        }
        Ok(())
    }

    fn handle_rtp_received(&mut self, header: rtp::Header, payload: &[u8]) {
        let data = match (header.pt, header.ssrc) {
            // Old clients send with 4 bytes of reserved data.
            (
                RTP_DATA_PAYLOAD_TYPE,
                OLD_RTP_DATA_SSRC_FOR_INCOMING | OLD_RTP_DATA_SSRC_FOR_OUTGOING,
            ) => &payload[OLD_RTP_DATA_RESERVED.len()..],
            // New clients will send without 4 bytes of reserved data.
            (RTP_DATA_PAYLOAD_TYPE, NEW_RTP_DATA_SSRC) => payload,
            (pt, ssrc) => {
                warn!(
                    "Received RTP with unexpected (PT, SSRC) = ({:?}, {:?})",
                    pt, ssrc
                );
                return;
            }
        };
        // Warning: normally you wouldn't want to take a lock while being
        // called by the WebRTC network thread, but this lock
        // is only taken here and nowhere else, and no other locks
        // are taken so we can't get into a deadlock.
        let mut last_received_rtp_data_timestamp = self
            .last_received_rtp_data_timestamp
            .lock()
            .expect("Lock last_recived_rtp_data_timestamp");

        // We allow equal timestamps because old clients send
        // multiple messages with the same timestamp.
        // This shouldn't be a problem in practice.
        if header.timestamp >= *last_received_rtp_data_timestamp {
            *last_received_rtp_data_timestamp = header.timestamp;
            drop(last_received_rtp_data_timestamp);
            self.inject_received_via_rtp_data(data);
        }
    }
}

fn generate_local_secret_and_public_key() -> Result<(StaticSecret, PublicKey)> {
    let secret = StaticSecret::new(OsRng);
    let public = PublicKey::from(&secret);
    Ok((secret, public))
}

struct NegotiatedSrtpKeys {
    pub offer_key: SrtpKey,
    pub answer_key: SrtpKey,
}

fn negotiate_srtp_keys(
    local_secret: &StaticSecret,
    remote_public_key: &[u8],
    caller_identity_key: &[u8],
    callee_identity_key: &[u8],
) -> Result<NegotiatedSrtpKeys> {
    // info!("Negotiating SRTP keys using local_public_key: {:?}, remote_public_key: {:?}, caller_identity_key: {:?}, callee_identity_key: {:?}",
    //     PublicKey::from(local_secret).as_bytes(), remote_public_key, caller_identity_key, callee_identity_key);

    let remote_public_key = {
        let mut array = [0u8; 32];
        array.copy_from_slice(remote_public_key);
        PublicKey::from(array)
    };

    let shared_secret = local_secret.diffie_hellman(&remote_public_key);

    let hkdf_salt = vec![0u8; 32];
    let hkdf_info_prefix = "Signal_Calling_20200807_SignallingDH_SRTPKey_KDF";
    let mut hkdf_info = Vec::with_capacity(
        hkdf_info_prefix.len() + caller_identity_key.len() + callee_identity_key.len(),
    );
    hkdf_info.extend_from_slice(hkdf_info_prefix.as_bytes());
    hkdf_info.extend_from_slice(caller_identity_key);
    hkdf_info.extend_from_slice(callee_identity_key);
    let hkdf = Hkdf::<Sha256>::new(Some(&hkdf_salt), shared_secret.as_bytes());

    const SUITE: SrtpCryptoSuite = SrtpCryptoSuite::AeadAes256Gcm;
    const KEY_SIZE: usize = SUITE.key_size();
    const SALT_SIZE: usize = SUITE.salt_size();
    let mut okm = vec![0; KEY_SIZE + SALT_SIZE + KEY_SIZE + SALT_SIZE];
    hkdf.expand(&hkdf_info, &mut okm)
        .map_err(|_| RingRtcError::SrtpKeyNegotiationFailure)?;
    let (offer_key, okm) = okm.split_at(KEY_SIZE);
    let (offer_salt, okm) = okm.split_at(SALT_SIZE);
    let (answer_key, okm) = okm.split_at(KEY_SIZE);
    let (answer_salt, _) = okm.split_at(SALT_SIZE);

    Ok(NegotiatedSrtpKeys {
        offer_key: SrtpKey {
            suite: SUITE,
            key: offer_key.to_vec(),
            salt: offer_salt.to_vec(),
        },
        answer_key: SrtpKey {
            suite: SUITE,
            key: answer_key.to_vec(),
            salt: answer_salt.to_vec(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect(
        max_send_rate_bps: u64,
        audio_bandwidth_mode: BandwidthMode,
    ) -> (DataRate, AudioEncoderConfig) {
        (
            DataRate::from_bps(max_send_rate_bps),
            audio_bandwidth_mode.audio_encoder_config(),
        )
    }

    fn compute(
        local_mode: BandwidthMode,
        remote_max_bps: u64,
        relayed: bool,
    ) -> (DataRate, AudioEncoderConfig) {
        let controller = BandwidthController {
            local_mode,
            remote_max: Some(DataRate::from_bps(remote_max_bps)),
            network_route: NetworkRoute {
                local_adapter_type: NetworkAdapterType::Unknown,
                local_adapter_type_under_vpn: NetworkAdapterType::Unknown,
                local_relayed: relayed,
                local_relay_protocol: TransportProtocol::Unknown,
                remote_relayed: false,
            },
        };
        (
            controller.max_send_rate(),
            controller.audio_encoder_config(),
        )
    }

    #[test]
    fn bandwidth_controller() {
        use BandwidthMode::*;
        // Remote max can push down the audio and video, but only to a point.
        assert_eq!(expect(2_000_000, Normal), compute(Normal, 3_000_000, false));
        assert_eq!(expect(2_000_000, Normal), compute(Normal, 2_000_000, false));
        assert_eq!(expect(1_999_999, Low), compute(Normal, 1_999_999, false));
        assert_eq!(expect(1_000_000, Low), compute(Normal, 1_000_000, false));
        assert_eq!(expect(300_000, Low), compute(Normal, 300_000, false));
        assert_eq!(expect(299_000, VeryLow), compute(Normal, 299_000, false));
        assert_eq!(expect(30_000, VeryLow), compute(Normal, 1_000, false));

        // Local mode can also push it down
        assert_eq!(expect(300_000, Low), compute(Low, 3_000_000, false));
        assert_eq!(expect(300_000, Low), compute(Low, 2_000_000, false));
        assert_eq!(expect(300_000, Low), compute(Low, 1_999_999, false));
        assert_eq!(expect(300_000, Low), compute(Low, 1_000_000, false));
        assert_eq!(expect(300_000, Low), compute(Low, 300_000, false));
        assert_eq!(expect(299_000, VeryLow), compute(Low, 299_000, false));
        assert_eq!(expect(30_000, VeryLow), compute(Low, 1_000, false));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 3_000_000, false));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 2_000_000, false));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 1_999_999, false));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 1_000_000, false));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 300_000, false));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 299_000, false));
        assert_eq!(expect(30_000, VeryLow), compute(VeryLow, 1_000, false));

        // Being relayed can also push it down, but it doesn't affect the audio.
        assert_eq!(expect(1_000_000, Normal), compute(Normal, 3_000_000, true));
        assert_eq!(expect(1_000_000, Normal), compute(Normal, 2_000_000, true));
        assert_eq!(expect(1_000_000, Low), compute(Normal, 1_999_999, true));
        assert_eq!(expect(1_000_000, Low), compute(Normal, 1_000_000, true));
        assert_eq!(expect(300_000, Low), compute(Normal, 300_000, true));
        assert_eq!(expect(299_000, VeryLow), compute(Normal, 299_000, true));
        assert_eq!(expect(30_000, VeryLow), compute(Normal, 1_000, true));
        assert_eq!(expect(300_000, Low), compute(Low, 3_000_000, true));
        assert_eq!(expect(300_000, Low), compute(Low, 2_000_000, true));
        assert_eq!(expect(300_000, Low), compute(Low, 1_999_999, true));
        assert_eq!(expect(300_000, Low), compute(Low, 1_000_000, true));
        assert_eq!(expect(300_000, Low), compute(Low, 300_000, true));
        assert_eq!(expect(299_000, VeryLow), compute(Low, 299_000, true));
        assert_eq!(expect(30_000, VeryLow), compute(Low, 1_000, true));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 3_000_000, true));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 2_000_000, true));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 1_999_999, true));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 1_000_000, true));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 300_000, true));
        assert_eq!(expect(125_000, VeryLow), compute(VeryLow, 299_000, true));
        assert_eq!(expect(30_000, VeryLow), compute(VeryLow, 1_000, true));
    }
}
