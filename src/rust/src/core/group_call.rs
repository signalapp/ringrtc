//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use std::{
    collections::{HashMap, HashSet},
    convert::TryInto,
    mem::size_of,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use bytes::{Bytes, BytesMut};
use prost::Message;
use rand::Rng;

use crate::core::util::uuid_to_string;
use crate::{
    common::{
        actor::{Actor, Stopper},
        units::DataRate,
        Result,
    },
    core::{call_mutex::CallMutex, crypto as frame_crypto, signaling},
    error::RingRtcError,
    protobuf,
    webrtc::{
        data_channel::DataChannel,
        media::{AudioTrack, VideoTrack},
        peer_connection::PeerConnection,
        peer_connection_factory::{Certificate, IceServer, PeerConnectionFactory},
        peer_connection_observer::{
            IceConnectionState,
            PeerConnectionObserver,
            PeerConnectionObserverTrait,
        },
        rtp,
        sdp_observer::{create_ssd_observer, SessionDescription},
        stats_observer::{create_stats_observer, StatsObserver},
    },
};

// Each instance of a group_call::Client has an ID for logging and passing events
// around (such as callbacks to the Observer).  It's just very convenient to have.
pub type ClientId = u32;
// Group UUID
pub type GroupId = Vec<u8>;
// An opaque value obtained from a Groups server and provided to an SFU
pub type MembershipProof = Vec<u8>;
// User UUID plaintext
pub type UserId = Vec<u8>;
// User UUID cipher text within the context of the group
pub type UserIdCiphertext = Vec<u8>;
// Each device joined to a group call is assigned a DemuxID
// which is used for demuxing media, but also identifying
// the device.
// 0 is not a valid value
// When given as remote devices, these must have "gaps"
// That allow for enough SSRCs to be derived from them.
// Currently that gap is 16.
pub type DemuxId = u32;
// SHA256 of DER form of X.509 certificate
// Basically what you get from https://tools.ietf.org/html/rfc8122#section-5
// but with SHA256 and without the hex encoding.
// Or what you get by calling this WebRTC code:
//  auto identity = SSLIdentity::Create("name", rtc::KeyParams(), 3153600000);
//  auto fingerprint = rtc::SSLFingerprint::CreateUnique("sha-256", *identity);
pub type DtlsFingerprint = [u8; 32];

/// Converts the DTLS fingerprint into a SDP-format hex string.
/// ```
/// let fp = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31];
/// let fp = ringrtc::core::group_call::encode_fingerprint(&fp);
/// assert_eq!(fp, "00:01:02:03:04:05:06:07:08:09:0A:0B:0C:0D:0E:0F:10:11:12:13:14:15:16:17:18:19:1A:1B:1C:1D:1E:1F")
/// ```
pub fn encode_fingerprint(dtls_fingerprint: &DtlsFingerprint) -> String {
    let mut s = String::new();
    for byte in dtls_fingerprint {
        if !s.is_empty() {
            s.push(':');
        }
        let hex = format!("{:02X}", byte);
        s.push_str(&hex);
    }
    s
}

/// Parses a DTLS fingerprint from a SDP-format hex string.
/// ```
/// let bad_string = "00:11:22:33";
/// assert_eq!(ringrtc::core::group_call::decode_fingerprint(&bad_string), None);
/// let good_string = "00:01:02:03:04:05:06:07:08:09:0A:0B:0C:0D:0E:0F:10:11:12:13:14:15:16:17:18:19:1A:1B:1C:1D:1E:1F";
/// let good_fingerprint = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31];
/// assert_eq!(ringrtc::core::group_call::decode_fingerprint(&good_string), Some(good_fingerprint));
/// ```
pub fn decode_fingerprint(s: &str) -> Option<DtlsFingerprint> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 32 {
        error!("Failed to parse fingerprint: {} - bad length", s);
        return None;
    }

    let mut fp = [0; 32];
    for i in 0..32 {
        if let Ok(b) = u8::from_str_radix(parts[i], 16) {
            fp[i] = b;
        } else {
            error!("Failed to parse fingerprint: {} - bad component", s);
            return None;
        }
    }
    Some(fp)
}

pub const INVALID_CLIENT_ID: ClientId = 0;

// The callbacks from the Call to the Observer of the call.
// Some of these are more than an "observer" in that a response is needed,
// which is provided asynchronously.
pub trait Observer {
    // A response should be provided via Call.update_membership_proof.
    fn request_membership_proof(&self, client_id: ClientId);
    // A response should be provided via Call.update_group_members.
    fn request_group_members(&self, client_id: ClientId);
    // Send a signaling message to the given remote user
    fn send_signaling_message(
        &mut self,
        recipient: UserId,
        message: protobuf::group_call::DeviceToDevice,
    );

    // The following notify the observer of state changes to the local device.
    fn handle_connection_state_changed(
        &self,
        client_id: ClientId,
        connection_state: ConnectionState,
    );
    fn handle_join_state_changed(&self, client_id: ClientId, join_state: JoinState);
    fn handle_max_send_bitrate_changed(&self, _client_id: ClientId, _rate: DataRate) {}

    // The following notify the observer of state changes to the remote devices.
    fn handle_remote_devices_changed(
        &self,
        client_id: ClientId,
        remote_devices: &[RemoteDeviceState],
    );

    // Notifies the observer of changes to the list of call participants.
    fn handle_peek_changed(
        &self,
        client_id: ClientId,
        joined_members: &[UserId],
        creator: Option<UserId>,
        era_id: Option<&str>,
        max_devices: Option<u32>,
        device_count: u32,
    );

    // This is separate from handle_remote_devices_changed because everything else
    // is a pure state that can be copied, deleted, etc.
    // But the VideoTrack is a special handle which must be attached to.
    // This will be called once per demux_id after handle_remote_devices_changed
    // has been called with the demux_id included.
    fn handle_incoming_video_track(
        &mut self,
        client_id: ClientId,
        remote_demux_id: DemuxId,
        incoming_video_track: VideoTrack,
    );

    // This will be the last callback.
    // The observer can assume the Call is completely shut down and can be deleted.
    fn handle_ended(&self, client_id: ClientId, reason: EndReason);
}

// The connection states of a device connecting to a group call.
// Has a state diagram like this:
//
//      |
//      | start()
//      V
// NotConnected
//      |                        ^
//      | connect()              |
//      V                        |
//  Connecting                -->|
//      |                        |
//      | connected              | connection failed
//      V                        | or disconnect()
//  Connected                 -->|
//      |            ^           |
//      | problems   | fixed     |
//      V            |           |
// Reconnecting               -->|
//
// Currently, due to limitations of the SFU, we cannot connect until after join() is called.
// So the ConnectionState will remain Connecting until join() is called.
// But updates to members joined (via handle_peek_changed)
// will still be received even when only Connecting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnectionState {
    /// Connect() has not yet been called
    /// or disconnect() has been called
    /// or connect() was called but failed.
    NotConnected,

    /// Connect() has been called but connectivity is pending.
    Connecting,

    /// Connect() has been called and connectivity has been established.
    Connected,

    /// Connect() has been called and connection has been established.
    /// But the connectivity is temporarily failing.
    Reconnecting,
}

// The join states of a device joining a group call.
// Has a state diagram like this:
//      |
//      | start()
//      V
//  NotJoined
//      |            ^
//      | join()     |
//      V            |
//   Joining      -->|  leave() or
//      |            |  failed to join
//      | joined     |
//      V            |
//   Joined       -->|
#[derive(Clone, Debug, PartialEq)]
pub enum JoinState {
    /// Join() has not yet been called
    /// or leave() has been called
    /// or join() was called but failed.
    NotJoined,

    /// Join() has been called but a response from the SFU is pending.
    Joining,

    /// Join() has been called and a response from the SFU has been received.
    /// and a DemuxId/RequestToken has been assigned.
    Joined(DemuxId, String),
}

// The info about SFU needed in order to connect to it.
#[derive(Clone, Debug)]
pub struct SfuInfo {
    pub udp_addresses:    Vec<SocketAddr>,
    pub ice_ufrag:        String,
    pub ice_pwd:          String,
    pub dtls_fingerprint: DtlsFingerprint,
}

// The current state of the SFU conference.
#[derive(Clone, Debug, Default)]
pub struct PeekInfo {
    /// Currently joined devices
    pub devices:      Vec<PeekDeviceInfo>,
    /// The user who created the call
    pub creator:      Option<UserId>,
    /// The "era" of this group call; changes every time the last partipant leaves and someone else joins again.
    pub era_id:       Option<String>,
    /// The maximum number of devices that can join this group call.
    pub max_devices:  Option<u32>,
    /// The number of devices currently joined (including local device/user).
    pub device_count: u32,
}

#[derive(Clone, Debug)]
pub struct PeekDeviceInfo {
    pub demux_id:        DemuxId,
    pub user_id:         Option<UserId>,
    // These are basically the same as DemuxIds,
    // but the SFU uses one sometimes and the other
    // other times.
    pub short_device_id: u64,
    pub long_device_id:  String,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EndReason {
    // Normal events
    DeviceExplicitlyDisconnected = 0,
    ServerExplicitlyDisconnected,

    // Things that can go wrong
    CallManagerIsBusy,
    SfuClientFailedToJoin,
    FailedToCreatePeerConnectionFactory,
    FailedToGenerateCertificate,
    FailedToCreatePeerConnection,
    FailedToCreateDataChannel,
    FailedToStartPeerConnection,
    FailedToUpdatePeerConnection,
    FailedToSetMaxSendBitrate,
    IceFailedWhileConnecting,
    IceFailedAfterConnected,
    ServerChangedDemuxId,
    HasMaxDevices,
}

pub type BoxedPeekInfoHandler = Box<dyn FnOnce(Result<PeekInfo>) + Send + 'static>;

// The callbacks from the Client to the "SFU client" for the group call.
pub trait SfuClient {
    // This should call Client.on_sfu_client_joined when the SfuClient has joined.
    fn join(
        &mut self,
        ice_ufrag: &str,
        ice_pwd: &str,
        dtls_fingerprint: &DtlsFingerprint,
        client: Client,
    );
    fn peek(&mut self, handle_remote_devices: BoxedPeekInfoHandler);

    // Notifies the client of the new membership proof.
    fn set_membership_proof(&mut self, proof: MembershipProof);
    fn set_group_members(&mut self, members: Vec<GroupMemberInfo>);
    fn leave(&mut self, long_device_id: String);
}

// Associates a group member's UUID with their UUID ciphertext
#[derive(Clone, Debug)]
pub struct GroupMemberInfo {
    pub user_id:            UserId,
    pub user_id_ciphertext: UserIdCiphertext,
}

// The info about remote devices received from the SFU
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteDeviceState {
    pub demux_id:            DemuxId,
    pub user_id:             UserId,
    short_device_id:         u64,
    long_device_id:          String,
    pub media_keys_received: bool,
    pub audio_muted:         Option<bool>,
    pub video_muted:         Option<bool>,
    // The latest timestamp we received from an update to
    // audio_muted and video_muted.
    muted_rtp_timestamp:     Option<rtp::Timestamp>,
    // The time at which this device was added to the list of devices.
    // A combination of (added_timestamp, demux_id) can be used for a stable
    // sort of remote devices for a grid layout.
    pub added_time:          SystemTime,
    // The most recent time at which this device was primary speaker
    // Sorting using this value will give a history of who spoke.
    pub speaker_time:        Option<SystemTime>,
    pub leaving_received:    bool,
}

fn as_unix_millis(t: Option<SystemTime>) -> u64 {
    if let Some(t) = t {
        if let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) {
            d.as_millis() as u64
        } else {
            0
        }
    } else {
        0
    }
}

impl RemoteDeviceState {
    fn new(
        demux_id: DemuxId,
        user_id: UserId,
        short_device_id: u64,
        long_device_id: String,
        added_time: SystemTime,
    ) -> Self {
        Self {
            demux_id,
            user_id,
            short_device_id,
            long_device_id,
            media_keys_received: false,
            audio_muted: None,
            video_muted: None,
            muted_rtp_timestamp: None,

            added_time,
            speaker_time: None,
            leaving_received: false,
        }
    }

    pub fn speaker_time_as_unix_millis(&self) -> u64 {
        as_unix_millis(self.speaker_time)
    }

    pub fn added_time_as_unix_millis(&self) -> u64 {
        as_unix_millis(Some(self.added_time))
    }
}

/// These can be sent to the SFU to request different resolutions of
/// video for different remote dem
#[derive(Clone, Debug)]
pub struct VideoRequest {
    pub demux_id:  DemuxId,
    pub width:     u16,
    pub height:    u16,
    // If not specified, it means unrestrained framerate.
    pub framerate: Option<u16>,
}

// This must stay in sync with the data PT in SfuClient.
const RTP_DATA_PAYLOAD_TYPE: rtp::PayloadType = 101;
// This must stay in sync with the data SSRC offset in SfuClient.
const RTP_DATA_THROUGH_SFU_SSRC_OFFSET: rtp::Ssrc = 0xD;
const RTP_DATA_TO_SFU_SSRC: rtp::Ssrc = 1;

// If the local device is the only device, tell WebRTC to send as little
// as possible while keeping the bandwidth estimator going.
// It looks like the bandwidth estimator will only probe up to 100kbps,
// but that's better than nothing.  It appears to take 26 seconds to
// ramp all the way up, though.
const ALL_ALONE_SEND_BITRATE_KBPS: u64 = 1;

// The time between when a sender generates a new media send key
// and applies it.  It needs to be big enough that there is
// a high probability that receivers will receive the
// key before the sender begins using it.  But making it too big
// gives a larger window of time during which a receiver that has
// left the call may decrypt media after leaving.
// Note that the window can be almost double this value because
// only one media send key rotation can be pending at a time
// so a receiver may leave immediately after receiving a newly
// generated key and it will be able to decrypt until after
// a second rotation is applied.
const MEDIA_SEND_KEY_ROTATION_DELAY_SECS: u64 = 3;

enum KeyRotationState {
    // A key has been applied.  Nothing is pending.
    Applied,
    // A key has been generated but not yet applied.
    Pending {
        secret:                 frame_crypto::Secret,
        // Once it has been applied, another rotation needs to take place because
        // a user left the call while rotation was pending.
        needs_another_rotation: bool,
    },
}

// We want to make sure there is at most one pending request for remote devices
// going on at a time, and to only request remote devices when the data is too stale
// or if it's been too long without a response.
#[derive(Debug)]
enum RemoteDevicesRequestState {
    WaitingForMembershipProof,
    NeverRequested,
    Requested {
        // While waiting, something happend that makes us think we should ask again.
        should_request_again: bool,
        at:                   Instant,
    },
    Updated {
        at: Instant,
    },
    Failed {
        at: Instant,
    },
}

/// Represents a device connecting to an SFU and joining a group call.
#[derive(Clone)]
pub struct Client {
    // A value used for logging and passing into the Observer.
    client_id:            ClientId,
    pub group_id:         GroupId,
    // We have to leave this outside of the actor state
    // because WebRTC calls back to the PeerConnectionObserver
    // synchronously.
    frame_crypto_context: Arc<CallMutex<frame_crypto::Context>>,
    actor:                Actor<State>,
}

/// The state inside the Actor
struct State {
    // Things passed in that never change
    client_id:  ClientId,
    group_id:   GroupId,
    sfu_client: Box<dyn SfuClient>,
    observer:   Box<dyn Observer>,

    // Shared busy flag with the CallManager that might change
    busy: Arc<CallMutex<bool>>,

    // State that changes regularly and is sent to the observer
    connection_state: ConnectionState,
    join_state:       JoinState,
    remote_devices:   Vec<RemoteDeviceState>,

    // Things to control peeking
    remote_devices_request_state: RemoteDevicesRequestState,
    last_peek_info:               Option<PeekInfo>,
    known_members:                HashSet<UserId>,

    // Derived from remote_devices but stored so we can fire
    // Observer::handle_peek_changed only when it changes
    joined_members: HashSet<UserId>,

    // Things we send to other clients via heartbeats
    // These are unset until the app sets them.
    // But we err on the side of caution and don't send anything when they are unset.
    outgoing_audio_muted: Option<bool>,
    outgoing_video_muted: Option<bool>,

    // Things for controlling the PeerConnection
    local_ice_ufrag:                  String,
    local_ice_pwd:                    String,
    local_dtls_fingerprint:           DtlsFingerprint,
    sfu_info:                         Option<SfuInfo>,
    peer_connection:                  PeerConnection,
    peer_connection_observer_impl:    Box<PeerConnectionObserverImpl>,
    rtp_data_to_sfu_next_seqnum:      u32,
    rtp_data_through_sfu_next_seqnum: u32,

    // Things for getting statistics from the PeerConnection
    // Stats gathering happens only when joined
    next_stats_time: Option<Instant>,
    stats_observer:  Box<StatsObserver>,

    // We have to put this inside the actor state also because
    // we change the keys from within the actor.
    frame_crypto_context: Arc<CallMutex<frame_crypto::Context>>,

    // If we receive a media key before we know about the remote device,
    // we store it here until we do know about the remote device.
    pending_media_receive_keys: Vec<(
        UserId,
        DemuxId,
        frame_crypto::RatchetCounter,
        frame_crypto::Secret,
    )>,
    // If we generate a new media send key when a user leaves the call,
    // during the time between when we generate it and apply it, we need
    // to make sure that user that joined in that window gets that key
    // even if it hasn't been applied yet.
    // And if more than one user leaves at the same time, we want to make sure
    // we throttle the rotations so they don't happen too often.
    // Note that this has the effect of doubling the amount of time someone might
    // be able do decrypt media after leaving if they leave immediately
    // after receiving a newly generated key.
    media_send_key_rotation_state: KeyRotationState,

    // Things to control video requests.  We want to send them regularly on ticks,
    // but also limit how often they are sent "on demand".  So here's the rule:
    // once per second, you get an "on demand" one.  Any more than that and you
    // wait for the next tick.
    video_requests:                               Option<Vec<VideoRequest>>,
    on_demand_video_request_sent_since_last_tick: bool,
    speaker_rtp_timestamp:                        Option<rtp::Timestamp>,

    // If unset, will use automatic behavior
    max_send_bitrate: Option<DataRate>,

    actor: Actor<State>,
}

// The time between ticks to do periodic things like
// Request updated membership list from the SfuClient
const TICK_INTERVAL_SECS: u64 = 1;

// The stats period, how often to get and log them.
const STATS_INTERVAL_SECS: u64 = 10;

impl Client {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        group_id: GroupId,
        client_id: ClientId,
        sfu_client: Box<dyn SfuClient + Send>,
        observer: Box<dyn Observer + Send>,
        busy: Arc<CallMutex<bool>>,
        peer_connection_factory: Option<PeerConnectionFactory>,
        outgoing_audio_track: AudioTrack,
        outgoing_video_track: Option<VideoTrack>,
    ) -> Result<Self> {
        debug!("group_call::Client(outer)::new(client_id: {})", client_id);
        let stopper = Stopper::new();
        // We only send with this key until the first person joins, at which point
        // we ratchet the key forward.
        let frame_crypto_context = Arc::new(CallMutex::new(
            frame_crypto::Context::new(frame_crypto::random_secret(&mut rand::rngs::OsRng)),
            "Frame encryption context",
        ));
        let frame_crypto_context_for_outside_actor = frame_crypto_context.clone();
        let client = Self {
            client_id,
            group_id: group_id.clone(),
            actor: Actor::start(stopper, move |actor| {
                debug!("group_call::Client(inner)::new(client_id: {})", client_id);

                let peer_connection_factory = match peer_connection_factory {
                    None => {
                        match PeerConnectionFactory::new(false /* use_injectable network */) {
                            Ok(v) => v,
                            Err(err) => {
                                observer.handle_ended(
                                    client_id,
                                    EndReason::FailedToCreatePeerConnectionFactory,
                                );
                                return Err(err);
                            }
                        }
                    }
                    Some(v) => v,
                };
                let certificate = Certificate::generate().map_err(|e| {
                    observer.handle_ended(client_id, EndReason::FailedToGenerateCertificate);
                    e
                })?;

                let (peer_connection_observer_impl, peer_connection_observer) =
                    PeerConnectionObserverImpl::uninitialized()?;
                // WebRTC uses alphanumeric plus + and /, which is just barely a superset of this,
                // but we can't uses dashes due to the sfu.
                let local_ice_ufrag = random_alphanumeric(4);
                let local_ice_pwd = random_alphanumeric(22);
                let local_dtls_fingerprint = certificate.compute_fingerprint_sha256()?;
                let hide_ip = false;
                let ice_server = IceServer::none();
                let enable_dtls = true;
                let enable_rtp_data_channel = true;
                let peer_connection = peer_connection_factory
                    .create_peer_connection(
                        peer_connection_observer,
                        certificate,
                        hide_ip,
                        &ice_server,
                        outgoing_audio_track,
                        outgoing_video_track,
                        enable_dtls,
                        enable_rtp_data_channel,
                    )
                    .map_err(|e| {
                        observer.handle_ended(client_id, EndReason::FailedToCreatePeerConnection);
                        e
                    })?;
                Ok(State {
                    client_id,
                    group_id,
                    sfu_client,
                    observer,
                    busy,
                    local_ice_ufrag,
                    local_ice_pwd,

                    connection_state: ConnectionState::NotConnected,
                    join_state: JoinState::NotJoined,
                    remote_devices: Vec::new(),

                    remote_devices_request_state:
                        RemoteDevicesRequestState::WaitingForMembershipProof,
                    last_peek_info: None,

                    known_members: HashSet::new(),

                    joined_members: HashSet::new(),

                    outgoing_audio_muted: None,
                    outgoing_video_muted: None,

                    local_dtls_fingerprint,
                    sfu_info: None,
                    peer_connection_observer_impl,
                    peer_connection,
                    rtp_data_to_sfu_next_seqnum: 1,
                    rtp_data_through_sfu_next_seqnum: 1,

                    next_stats_time: None,
                    stats_observer: create_stats_observer(),

                    frame_crypto_context,
                    pending_media_receive_keys: Vec::new(),
                    media_send_key_rotation_state: KeyRotationState::Applied,

                    video_requests: None,
                    on_demand_video_request_sent_since_last_tick: false,
                    speaker_rtp_timestamp: None,

                    max_send_bitrate: None,

                    actor,
                })
            })?,
            frame_crypto_context: frame_crypto_context_for_outside_actor,
        };

        // After we have the actor, we can initialize the PeerConnectionObserverImpl
        // and kick of ticking.
        let client_clone_to_init_peer_connection_observer_impl = client.clone();
        client.actor.send(move |state| {
            state
                .peer_connection_observer_impl
                .initialize(client_clone_to_init_peer_connection_observer_impl);
        });
        Ok(client)
    }

    // Pulled into a named private method so we can call it recursively.
    fn tick(state: &mut State) {
        let now = Instant::now();

        debug!(
            "group_call::Client(inner)::tick(group_id: {})",
            state.client_id
        );

        Self::request_remote_devices_from_sfu_if_older_than(state, Duration::from_secs(10));

        if let Err(err) = Self::send_heartbeat(state) {
            warn!("Failed to send regular heartbeat: {:?}", err);
        }

        if let Some(next_stats_time) = state.next_stats_time {
            if now >= next_stats_time {
                let _ = state
                    .peer_connection
                    .get_stats(state.stats_observer.as_ref());
                state.next_stats_time = Some(now + Duration::from_secs(STATS_INTERVAL_SECS));
            }
        }

        Self::send_video_requests_to_sfu(state);
        state.on_demand_video_request_sent_since_last_tick = false;

        state
            .actor
            .send_delayed(Duration::from_secs(TICK_INTERVAL_SECS), move |state| {
                Self::tick(state)
            });
    }

    fn request_remote_devices_as_soon_as_possible(state: &mut State) {
        debug!(
            "group_call::Client::request_remote_devices_as_soon_as_possible(client_id: {})",
            state.client_id
        );

        Self::maybe_request_remote_devices(state, Duration::from_secs(0), true);
    }

    fn request_remote_devices_from_sfu_if_older_than(state: &mut State, max_age: Duration) {
        debug!(
            "group_call::Client::request_remote_devices_from_sfu_if_older_than(client_id: {}, max_age: {:?})",
            state.client_id, max_age
        );

        Self::maybe_request_remote_devices(state, max_age, false);
    }

    fn maybe_request_remote_devices(
        state: &mut State,
        max_age: Duration,
        rerequest_if_pending: bool,
    ) {
        let now = Instant::now();
        let should_request_now = match state.remote_devices_request_state {
            RemoteDevicesRequestState::WaitingForMembershipProof => false,
            RemoteDevicesRequestState::NeverRequested => true,
            RemoteDevicesRequestState::Requested {
                at: request_time, ..
            } => {
                // Timeout if we don't get a response
                now > request_time + Duration::from_secs(5)
            }
            RemoteDevicesRequestState::Updated { at: update_time } => now >= update_time + max_age,
            RemoteDevicesRequestState::Failed { at: failure_time } => {
                // Don't hammer server during failures
                now > failure_time + Duration::from_secs(1)
            }
        };
        if should_request_now {
            // We've already requested, so just wait until the next update and then request again.
            debug!("Request remote devices now.");
            let actor = state.actor.clone();
            state.sfu_client.peek(Box::new(move |peek_info| {
                actor.send(move |state| {
                    Self::set_peek_info_inner(state, peek_info);
                });
            }));
            state.remote_devices_request_state = RemoteDevicesRequestState::Requested {
                should_request_again: false,
                at:                   Instant::now(),
            };
        } else if rerequest_if_pending {
            // We've already requested, so just wait until the next update and then request again.
            debug!("Request remote devices later because there's a request pending.");
            if let RemoteDevicesRequestState::Requested { at, .. } =
                state.remote_devices_request_state
            {
                state.remote_devices_request_state = RemoteDevicesRequestState::Requested {
                    at,
                    should_request_again: true,
                }
            }
        } else {
            debug!("Just skip this request for remote devices.");
        }
    }

    pub fn connect(&self) {
        debug!(
            "group_call::Client(outer)::connect(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::connect(client_id: {})",
                state.client_id
            );

            match state.connection_state {
                ConnectionState::Connected | ConnectionState::Reconnecting => {
                    warn!("Can't connect when already connected.");
                }
                ConnectionState::Connecting => {
                    warn!("Can't connect when already connecting.");
                }
                ConnectionState::NotConnected => {
                    // Because the SfuClient currently doesn't allow connecting without joining,
                    // we just pretend to connect and wait for join() to be called.
                    Self::set_connection_state_and_notify_observer(
                        state,
                        ConnectionState::Connecting,
                    );

                    // Request group membership refresh as we start polling the participant list.
                    state.observer.request_membership_proof(state.client_id);

                    // Request the list of all group members
                    state.observer.request_group_members(state.client_id);

                    Self::tick(state);
                }
            }
        });
    }

    // Pulled into a named private method because it might be called by many methods.
    fn set_connection_state_and_notify_observer(
        state: &mut State,
        connection_state: ConnectionState,
    ) {
        debug!(
            "group_call::Client(inner)::set_connection_state_and_notify_observer(client_id: {})",
            state.client_id
        );

        state.connection_state = connection_state;
        state
            .observer
            .handle_connection_state_changed(state.client_id, connection_state);
    }

    // Pulled into a private method so we can lock/set/unlock the busy state.
    fn take_busy(state: &mut State) -> bool {
        let busy = state.busy.lock();
        match busy {
            Ok(mut busy) => {
                if *busy {
                    info!("Call Manager is busy with another call");
                    false
                } else {
                    *busy = true;
                    true
                }
            }
            Err(err) => {
                error!("Can't lock busy: {}", err);
                false
            }
        }
    }

    fn release_busy(state: &mut State) {
        let busy = state.busy.lock();
        match busy {
            Ok(mut busy) => {
                *busy = false;
            }
            Err(err) => {
                error!("Can't lock busy: {}", err);
            }
        }
    }

    pub fn join(&self) {
        debug!(
            "group_call::Client(outer)::join(client_id: {})",
            self.client_id
        );
        let callback = self.clone();
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::join(client_id: {})",
                state.client_id
            );
            match state.join_state {
                JoinState::Joined(_, _) => {
                    warn!("Can't join when already joined.");
                }
                JoinState::Joining => {
                    warn!("Can't join when already joining.");
                }
                JoinState::NotJoined => {
                    if let Some(PeekInfo{device_count, max_devices: Some(max_devices), ..}) = &state.last_peek_info {
                        if device_count >= max_devices {
                            info!("Ending group call client because there are {}/{} devices in the call.", device_count, max_devices);
                            Self::end(state, EndReason::HasMaxDevices);
                            return;
                        }
                    }
                    if Self::take_busy(state) {
                        Self::set_join_state_and_notify_observer(state, JoinState::Joining);

                        // Request group membership refresh before joining.
                        // The Join request will then proceed once SfuClient has the token.
                        state.observer.request_membership_proof(state.client_id);

                        state.sfu_client.join(
                            &state.local_ice_ufrag,
                            &state.local_ice_pwd,
                            &state.local_dtls_fingerprint,
                            callback,
                        );
                    } else {
                        Self::end(state, EndReason::CallManagerIsBusy);
                    }
                }
            }
        });
    }

    // Pulled into a named private method because it might be called by leave_inner().
    fn set_join_state_and_notify_observer(state: &mut State, join_state: JoinState) {
        debug!(
            "group_call::Client(inner)::set_join_state_and_notify_observer(client_id: {}, join_state: {:?})",
            state.client_id,
            join_state
        );
        state.join_state = join_state.clone();
        state
            .observer
            .handle_join_state_changed(state.client_id, join_state);
    }

    pub fn leave(&self) {
        debug!(
            "group_call::Client(outer)::leave(client_id: {})",
            self.client_id
        );
        self.actor.send(Self::leave_inner);
    }

    // Pulled into a named private method because it might be called by end().
    fn leave_inner(state: &mut State) {
        debug!(
            "group_call::Client(inner)::leave(client_id: {}, join_state: {:?})",
            state.client_id, state.join_state
        );

        match state.join_state {
            JoinState::NotJoined => {
                warn!("Can't leave when not joined.");
            }
            JoinState::Joining | JoinState::Joined(_, _) => {
                state.peer_connection.set_outgoing_media_enabled(false);
                state.peer_connection.set_incoming_media_enabled(false);
                Self::release_busy(state);

                if let JoinState::Joined(local_demux_id, long_device_id) = state.join_state.clone()
                {
                    state.sfu_client.leave(long_device_id);
                    Self::send_leaving_through_sfu_and_over_signaling(state, local_demux_id);
                }
                Self::set_join_state_and_notify_observer(state, JoinState::NotJoined);
                state.next_stats_time = None;
            }
        }
    }

    pub fn disconnect(&self) {
        debug!(
            "group_call::Client(outer)::disconnect(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::disconnect(client_id: {})",
                state.client_id
            );
            Self::end(state, EndReason::DeviceExplicitlyDisconnected);
        });
    }

    pub fn set_outgoing_audio_muted(&self, muted: bool) {
        debug!(
            "group_call::Client(outer)::set_audio_muted(client_id: {}, muted: {})",
            self.client_id, muted
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_audio_muted(client_id: {}, muted: {})",
                state.client_id, muted
            );
            // We don't modify the outgoing audio track.  We expect the app to handle that.
            state.outgoing_audio_muted = Some(muted);
            if let Err(err) = Self::send_heartbeat(state) {
                warn!(
                    "Failed to send heartbeat after updating audio mute state: {:?}",
                    err
                );
            }
        });
    }

    pub fn set_outgoing_video_muted(&self, muted: bool) {
        debug!(
            "group_call::Client(outer)::set_video_muted(client_id: {}, muted: {})",
            self.client_id, muted
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_video_muted(client_id: {}, muted: {})",
                state.client_id, muted
            );
            // We don't modify the outgoing video track.  We expect the app to handle that.
            state.outgoing_video_muted = Some(muted);
            if let Err(err) = Self::send_heartbeat(state) {
                warn!(
                    "Failed to send heartbeat after updating video mute state: {:?}",
                    err
                );
            }
            state.outgoing_video_muted = Some(muted);
        });
    }

    pub fn resend_media_keys(&self) {
        debug!(
            "group_call::Client(outer)::resend_media_keys(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::resend_media_keys(client_id: {})",
                state.client_id
            );

            if let JoinState::Joined(local_demux_id, _) = state.join_state {
                let user_ids: HashSet<UserId> = state
                    .remote_devices
                    .iter()
                    .map(|rd| rd.user_id.clone())
                    .collect();

                let (ratchet_counter, secret) = {
                    let frame_crypto_context = state
                        .frame_crypto_context
                        .lock()
                        .expect("Get lock for frame encryption context to advance media send key");
                    frame_crypto_context.send_state()
                };

                info!(
                    "Resending media keys to everyone (number of users: {})",
                    user_ids.len()
                );
                for user_id in user_ids {
                    Self::send_media_send_key_to_user_over_signaling(
                        state,
                        user_id,
                        local_demux_id,
                        ratchet_counter,
                        secret,
                    );
                }
            }
        });
    }

    pub fn set_max_send_bitrate(&self, rate: DataRate) {
        debug!(
            "group_call::Client(outer)::set_max_send_bitrate(client_id: {}, rate: {:?})",
            self.client_id, rate
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_max_send_bitrate(client_id: {}, rate: {:?})",
                state.client_id, rate
            );
            state.max_send_bitrate = Some(rate);
            Self::set_max_send_bitrate_inner(state, rate);
        });
    }

    fn set_max_send_bitrate_inner(state: &mut State, rate: DataRate) {
        if state.max_send_bitrate.is_none() || state.max_send_bitrate == Some(rate) {
            if rate.as_kbps() == ALL_ALONE_SEND_BITRATE_KBPS {
                info!("Disabling outgoing media because there are no other devices.");
                state.peer_connection.set_outgoing_media_enabled(false);
            } else {
                info!("Enabling outgoing media because there are other devices.");
                state.peer_connection.set_outgoing_media_enabled(true);
            }
            if state.peer_connection.set_max_send_bitrate(rate).is_err() {
                warn!("Could not set max send bitrate to {:?}", rate);
            } else {
                info!("Set max send bitrate to {:?}", rate);
                state
                    .observer
                    .handle_max_send_bitrate_changed(state.client_id, rate);
            }
        }
    }

    pub fn request_video(&self, requests: Vec<VideoRequest>) {
        debug!(
            "group_call::Client(outer)::request_video(client_id: {}, requests: {:?})",
            self.client_id, requests,
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::request_video(client_id: {})",
                state.client_id
            );
            state.video_requests = Some(requests);
            if !state.on_demand_video_request_sent_since_last_tick {
                Self::send_video_requests_to_sfu(state);
                state.on_demand_video_request_sent_since_last_tick = true;
            }
        });
    }

    fn send_video_requests_to_sfu(state: &mut State) {
        use protobuf::group_call::{
            device_to_sfu::{
                video_request_message::VideoRequest as VideoRequestProto,
                VideoRequestMessage,
            },
            DeviceToSfu,
        };
        use std::cmp::min;

        if let Some(video_requests) = &state.video_requests {
            let requests: Vec<_> = video_requests
                .iter()
                .filter_map(|request| {
                    state
                        .remote_devices
                        .iter()
                        .find(|device| device.demux_id == request.demux_id)
                        .map(|device| {
                            VideoRequestProto {
                                short_device_id: Some(device.short_device_id),
                                // We use the min because the SFU does not understand the concept of video rotation
                                // so all requests must be in terms of non-rotated video even though the apps
                                // will request in terms of rotated video.  We assume that all video is sent over the
                                // wire in landscape format with rotation metadata.
                                // If it's not, we'll have a problem.
                                height:          Some(min(request.height, request.width) as u32),
                            }
                        })
                })
                .collect();
            match encode_proto(DeviceToSfu {
                video_request: Some(VideoRequestMessage {
                    // TODO: Update the server to handle this as expected or remove this altogether.
                    // The client needs the server to sort by resolution and then cap the number after that sort.
                    // Currently, the server is sorting by audio activity and then capping the number.
                    // Two possible fixes on the server:
                    // A. Sort by resolution and then cap.
                    //    After that, the client could re-add the lines below.
                    // B. Treat the list of resolution requests as "complete" and don't use "lastN" at all.
                    //    After that, the client could remove the lines below.
                    // Note: the server can't handle a None value here, so we have to pass
                    // in a value larger than a group call would ever be.
                    // The only problem with this mechanism is that the server will send video for
                    // new remote devices that the local device hasn't yet learned about.
                    // max: Some(
                    //     requests
                    //         .iter()
                    //         .filter(|request| request.height.unwrap() > 0)
                    //         .count() as u32,
                    // ),
                    max: Some(1000000),
                    requests,
                }),
                ..DeviceToSfu::default()
            }) {
                Err(e) => {
                    warn!("Failed to encode video request: {:?}", e);
                }
                Ok(msg) => {
                    if let Err(e) = Self::send_data_to_sfu(state, &msg) {
                        warn!("Failed to send video request: {:?}", e);
                    }
                }
            }
        }
    }

    pub fn set_group_members(&self, group_members: Vec<GroupMemberInfo>) {
        debug!(
            "group_call::Client(outer)::set_group_members(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_group_members(client_id: {})",
                state.client_id
            );
            let new_members: HashSet<UserId> =
                group_members.iter().map(|i| i.user_id.clone()).collect();
            if new_members != state.known_members {
                info!("known group members changed");
                state.known_members = new_members;
                state.sfu_client.set_group_members(group_members);
                Self::request_remote_devices_as_soon_as_possible(state);
            }
        })
    }

    pub fn set_membership_proof(&self, proof: MembershipProof) {
        debug!(
            "group_call::Client(outer)::set_membership_proof(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_membership_proof(client_id: {})",
                state.client_id
            );
            state.sfu_client.set_membership_proof(proof);
            if matches!(
                state.remote_devices_request_state,
                RemoteDevicesRequestState::WaitingForMembershipProof
            ) {
                state.remote_devices_request_state = RemoteDevicesRequestState::NeverRequested;
                Self::request_remote_devices_as_soon_as_possible(state);
            }
        })
    }

    // Pulled into a named private method because it can be called in many places.
    #[allow(clippy::collapsible_if)]
    fn end(state: &mut State, reason: EndReason) {
        debug!(
            "group_call::Client(inner)::end(client_id: {})",
            state.client_id
        );

        let joining_or_joined = match state.join_state {
            JoinState::Joined(_, _) | JoinState::Joining => true,
            JoinState::NotJoined => false,
        };
        if joining_or_joined {
            // This will send an update after changing the join state.
            Self::leave_inner(state);
        }
        match state.connection_state {
            ConnectionState::NotConnected => {
                warn!("Can't disconnect when not connected.");
            }
            ConnectionState::Connecting
            | ConnectionState::Connected
            | ConnectionState::Reconnecting => {
                state.peer_connection.close();
                Self::set_connection_state_and_notify_observer(
                    state,
                    ConnectionState::NotConnected,
                );
                let _join_handles = state.actor.stopper().stop_all_without_joining();
                state.observer.handle_ended(state.client_id, reason);
            }
        }
    }

    // This should be called by the SfuClient after it has joined.
    pub fn on_sfu_client_joined(&self, result: Result<(SfuInfo, DemuxId, String)>) {
        debug!(
            "group_call::Client(outer)::on_sfu_client_joined(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::on_sfu_client_joined(client_id: {})",
                state.client_id
            );

            if let Ok((sfu_info, local_demux_id, long_device_id)) = result {
                match state.connection_state {
                    ConnectionState::NotConnected => {
                        warn!("The SFU completed joining before connect() was requested.");
                    }
                    ConnectionState::Connecting => {
                        if Self::start_peer_connection(state, &sfu_info, local_demux_id).is_err() {
                            Self::end(state, EndReason::FailedToStartPeerConnection);
                        };

                        // Set a low bitrate until we learn someone else is in the call.
                        Self::set_max_send_bitrate_inner(
                            state,
                            DataRate::from_kbps(ALL_ALONE_SEND_BITRATE_KBPS),
                        );

                        state.sfu_info = Some(sfu_info);
                    }
                    ConnectionState::Connected | ConnectionState::Reconnecting => {
                        warn!("The SFU completed joining after already being connected.");
                    }
                };
                match state.join_state {
                    JoinState::NotJoined => {
                        warn!("The SFU completed joining before join() was requested.");
                    }
                    JoinState::Joining => {
                        // The call to set_peek_info_inner needs the join state to be joined.
                        // But make sure to fire observer.handle_join_state_changed after
                        // set_peek_info_inner so that state.remote_devices are filled in.
                        state.join_state = JoinState::Joined(local_demux_id, long_device_id);
                        if let Some(peek_info) = &state.last_peek_info {
                            // TODO: Do the same processing without making it look like we just
                            // got an update from the server even though the update actually came
                            // from earlier.  For now, it's close enough.
                            let peek_info = peek_info.clone();
                            Self::set_peek_info_inner(state, Ok(peek_info));
                        }
                        state
                            .observer
                            .handle_join_state_changed(state.client_id, state.join_state.clone());
                        // We just now appeared in the participants list, and possibly even updated
                        // the eraId.
                        Self::request_remote_devices_as_soon_as_possible(state);
                        state.next_stats_time =
                            Some(Instant::now() + Duration::from_secs(STATS_INTERVAL_SECS));
                    }
                    JoinState::Joined(_, _) => {
                        warn!("The SFU completed joining more than once.");
                    }
                };
            } else {
                Self::end(state, EndReason::SfuClientFailedToJoin);
            }
        });
    }

    pub fn on_signaling_message_received(
        &self,
        sender_user_id: UserId,
        message: protobuf::group_call::DeviceToDevice,
    ) {
        debug!(
            "group_call::Client(outer)::on_signaling_message_received(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::on_signaling_message_received(client_id: {})",
                state.client_id
            );
            match message {
                protobuf::group_call::DeviceToDevice {
                    media_key:
                        Some(protobuf::group_call::device_to_device::MediaKey {
                            demux_id: Some(sender_demux_id),
                            ratchet_counter: Some(ratchet_counter),
                            secret: Some(secret_vec),
                            ..
                        }),
                    ..
                } => {
                    if secret_vec.len() != size_of::<frame_crypto::Secret>() {
                        warn!("on_signaling_message_received(): ignoring media receive key with wrong length");
                        return;
                    }
                    if let Ok(ratchet_counter) = ratchet_counter.try_into() {
                        let mut secret = frame_crypto::Secret::default();
                        secret.copy_from_slice(&secret_vec);
                        Self::add_media_receive_key_or_store_for_later(
                            state,
                            sender_user_id,
                            sender_demux_id,
                            ratchet_counter,
                            secret,
                        );
                    } else {
                        warn!("on_signaling_message_received(): ignoring media receive key with ratchet counter that's too big");
                    }
                    let known = state.remote_devices.iter().any(|rd| rd.demux_id == sender_demux_id);
                    if !known {
                        // It's likely someone this demux ID just joined.
                        debug!("Request devices because we receive a signaling message from unknown demux_id = {}", sender_demux_id);
                        Self::request_remote_devices_as_soon_as_possible(state);
                    }
                }
                protobuf::group_call::DeviceToDevice {
                    group_id: Some(group_id),
                    leaving: Some(protobuf::group_call::device_to_device::Leaving {
                        demux_id: Some(leaving_demux_id),
                        ..
                    }),
                    ..
                } => {
                    if group_id == state.group_id {
                        Self::handle_leaving_received(state, leaving_demux_id);
                    }
                }
                _ => {
                    warn!("on_signaling_message_received(): ignoring unknown message");
                }
            }
        });
    }

    // Pulled into a named private method because it's more convenient to deal with errors that way
    fn start_peer_connection(
        state: &State,
        sfu_info: &SfuInfo,
        local_demux_id: DemuxId,
    ) -> Result<()> {
        debug!(
            "group_call::Client(inner)::start_peer_connection(client_id: {})",
            state.client_id
        );

        Self::set_peer_connection_descriptions(state, sfu_info, local_demux_id, &[])?;

        for addr in &sfu_info.udp_addresses {
            state.peer_connection.add_ice_candidate_from_server(
                addr.ip(),
                addr.port(),
                false, /* tcp */
            )?;
        }

        if state
            .peer_connection
            .receive_rtp(RTP_DATA_PAYLOAD_TYPE)
            .is_err()
        {
            warn!("Could not tell PeerConnection to receive RTP");
        }

        Ok(())
    }

    pub fn set_peek_info(&self, info: Result<PeekInfo>) {
        debug!(
            "group_call::Client(outer)::set_peek_info: {}, info: {:?})",
            self.client_id, info
        );

        self.actor.send(move |state| {
            Self::set_peek_info_inner(state, info);
        });
    }

    // Most of the logic moved to inner method so this can be called by both
    // set_peek_info() and as a callback to SfuClient::request_remote_devices.
    fn set_peek_info_inner(state: &mut State, peek_info: Result<PeekInfo>) {
        debug!(
            "group_call::Client(inner)::set_peek_info_inner(client_id: {}, info: {:?} state: {:?})",
            state.client_id, peek_info, state.remote_devices_request_state
        );

        if let Err(e) = peek_info {
            warn!("Failed to request remote devices from SFU: {}", e);
            state.remote_devices_request_state =
                RemoteDevicesRequestState::Failed { at: Instant::now() };
            return;
        }
        let peek_info = peek_info.unwrap();

        let is_first_update = matches!(
            state.remote_devices_request_state,
            RemoteDevicesRequestState::NeverRequested
        );
        let should_request_again = matches!(
            state.remote_devices_request_state,
            RemoteDevicesRequestState::Requested {
                should_request_again: true,
                ..
            }
        );
        state.remote_devices_request_state =
            RemoteDevicesRequestState::Updated { at: Instant::now() };

        let old_user_ids: HashSet<UserId> =
            std::mem::replace(&mut state.joined_members, HashSet::new());
        let new_user_ids: HashSet<UserId> = peek_info
            .devices
            .iter()
            // Note: this ignores users that aren't in the group
            .filter_map(|device| device.user_id.clone())
            .collect();

        let old_era_id = match &state.last_peek_info {
            Some(PeekInfo {
                era_id: Some(era_id),
                ..
            }) => Some(era_id.clone()),
            _ => None,
        };
        if old_user_ids != new_user_ids || old_era_id != peek_info.era_id {
            let joined_members: Vec<UserId> = new_user_ids.iter().cloned().collect();
            state.observer.handle_peek_changed(
                state.client_id,
                &joined_members,
                peek_info.creator.clone(),
                peek_info.era_id.as_deref(),
                peek_info.max_devices,
                peek_info.device_count,
            )
        }

        let peek_info_to_remember = peek_info.clone();
        if let JoinState::Joined(local_demux_id, _) = state.join_state {
            // We remember these before changing state.remote_devices so we can calculate changes after.
            let old_demux_ids: HashSet<DemuxId> =
                state.remote_devices.iter().map(|rd| rd.demux_id).collect();

            // Then we update state.remote_devices by first building a map of id_pair => RemoteDeviceState
            // from the old values and then building a new Vec using either the old value (if there is one)
            // or creating a new one.
            let mut old_remote_devices_by_id_pair: HashMap<(DemuxId, UserId), RemoteDeviceState> =
                std::mem::replace(&mut state.remote_devices, Vec::new())
                    .into_iter()
                    .map(|rd| ((rd.demux_id, rd.user_id.clone()), rd))
                    .collect();
            let added_time = SystemTime::now();
            state.remote_devices = peek_info
                .devices
                .into_iter()
                .filter_map(|device| {
                    if device.demux_id == local_demux_id {
                        // Don't add a remote device to represent the local device.
                        return None;
                    }
                    if let PeekDeviceInfo {
                        demux_id,
                        user_id: Some(user_id),
                        short_device_id,
                        long_device_id,
                    } = device
                    {
                        // Keep the old one, with its state, if there is one.
                        Some(
                            match old_remote_devices_by_id_pair.remove(&(demux_id, user_id.clone()))
                            {
                                Some(existing_remote_device) => existing_remote_device,
                                None => RemoteDeviceState::new(
                                    demux_id,
                                    user_id,
                                    short_device_id,
                                    long_device_id,
                                    added_time,
                                ),
                            },
                        )
                    } else {
                        // Ignore devices of users that aren't in the group
                        None
                    }
                })
                .collect();

            // Recalculate to see the differences
            let new_demux_ids: HashSet<DemuxId> =
                state.remote_devices.iter().map(|rd| rd.demux_id).collect();

            let demux_ids_changed = old_demux_ids != new_demux_ids;
            // If demux IDs changed, let the PeerConnection know that related SSRCs changed as well
            if demux_ids_changed {
                info!(
                    "New set of demux IDs to be pushed down to PeerConnection: {:?}",
                    new_demux_ids
                );
                if let Some(sfu_info) = state.sfu_info.as_ref() {
                    let new_demux_ids: Vec<DemuxId> = new_demux_ids.iter().copied().collect();
                    let result = Self::set_peer_connection_descriptions(
                        state,
                        sfu_info,
                        local_demux_id,
                        &new_demux_ids,
                    );
                    if result.is_err() {
                        Self::end(state, EndReason::FailedToUpdatePeerConnection);
                        return;
                    }
                }
            }

            // Note: if the first call to set_peek_info is [], we still fire the
            // handle_remote_devices_changed to ensure the observer can tell the difference
            // between "we know we have no remote devices" and "we don't know what we have yet".
            if demux_ids_changed || is_first_update {
                state
                    .observer
                    .handle_remote_devices_changed(state.client_id, &state.remote_devices);
            }

            if new_user_ids != old_user_ids {
                let joined_members: Vec<UserId> = new_user_ids.iter().cloned().collect();
                state.observer.handle_peek_changed(
                    state.client_id,
                    &joined_members,
                    peek_info.creator.clone(),
                    peek_info.era_id.as_deref(),
                    peek_info.max_devices,
                    peek_info.device_count,
                )
            }
            // If someone was added, we must advance the send media key
            // and send it to everyone that was added.
            let added_demux_ids: HashSet<DemuxId> =
                new_demux_ids.difference(&old_demux_ids).copied().collect();
            let users_with_added_devices: Vec<UserId> = state
                .remote_devices
                .iter()
                .filter(|device| added_demux_ids.contains(&device.demux_id))
                .map(|device| device.user_id.clone())
                .collect();
            if !users_with_added_devices.is_empty() {
                Self::advance_media_send_key_and_send_to_users_with_added_devices(
                    state,
                    &users_with_added_devices[..],
                );
                Self::send_pending_media_send_key_to_users_with_added_devices(
                    state,
                    &users_with_added_devices[..],
                );
            }

            // If someone was removed, we must reset the send media key and send it to everyone not removed.
            let user_ids_removed: Vec<&UserId> = old_user_ids.difference(&new_user_ids).collect();
            if !user_ids_removed.is_empty() {
                Self::rotate_media_send_key_and_send_to_users_not_removed(state);
            }

            // We can't gate this behind the demux IDs changing because a forged demux ID might
            // be in there already when the non-forged one comes in.
            let pending_receive_keys =
                std::mem::replace(&mut state.pending_media_receive_keys, Vec::new());
            for (user_id, demux_id, ratchet_counter, secret) in pending_receive_keys {
                // If we the key is still pending, we'll just put this back into state.pending_media_receive_keys.
                Self::add_media_receive_key_or_store_for_later(
                    state,
                    user_id,
                    demux_id,
                    ratchet_counter,
                    secret,
                );
            }
            if new_demux_ids.len() != old_demux_ids.len() {
                // Send between 500kbps and 1mbps depending on how many other devices there are.
                // The more there are, the less we will send.
                let rate = DataRate::from_kbps(match new_demux_ids.len() {
                    // No one is here, so push it down as low as WebRTC will let us.
                    0 => ALL_ALONE_SEND_BITRATE_KBPS,
                    1..=7 => 1000, // Pretty much the default
                    _ => 500,
                });
                Self::set_max_send_bitrate_inner(state, rate);
            }
        }
        state.last_peek_info = Some(peek_info_to_remember);

        // Do this later so that we can use new_user_ids above without running into
        // referencing issues
        state.joined_members = new_user_ids;

        if should_request_again {
            // Something occured while we were waiting for this update.
            // We should request again.
            debug!("Request devices because we previously requested while a request was pending");
            Self::request_remote_devices_as_soon_as_possible(state);
        }
    }

    // Pulled into a named private method because it might be called by set_peek_info
    fn set_peer_connection_descriptions(
        state: &State,
        sfu_info: &SfuInfo,
        local_demux_id: DemuxId,
        remote_demux_ids: &[DemuxId],
    ) -> Result<()> {
        let local_description = SessionDescription::local_for_group_call(
            &state.local_ice_ufrag,
            &state.local_ice_pwd,
            &state.local_dtls_fingerprint,
            Some(local_demux_id),
        )?;
        let observer = create_ssd_observer();
        state
            .peer_connection
            .set_local_description(observer.as_ref(), local_description);
        observer.get_result()?;

        let remote_description = SessionDescription::remote_for_group_call(
            &sfu_info.ice_ufrag,
            &sfu_info.ice_pwd,
            &sfu_info.dtls_fingerprint,
            remote_demux_ids,
        )?;
        let observer = create_ssd_observer();
        state
            .peer_connection
            .set_remote_description(observer.as_ref(), remote_description);
        observer.get_result()?;
        Ok(())
    }

    fn rotate_media_send_key_and_send_to_users_not_removed(state: &mut State) {
        match state.media_send_key_rotation_state {
            KeyRotationState::Pending { secret, .. } => {
                info!("Waiting to generate a new media send key until after the pending one has been applied. client_id: {}", state.client_id);

                state.media_send_key_rotation_state = KeyRotationState::Pending {
                    secret,
                    needs_another_rotation: true,
                }
            }
            KeyRotationState::Applied => {
                info!("Generating a new random media send key because a user has been removed. client_id: {}", state.client_id);

                // First generate a new key, then wait some time, and then apply it.
                let ratchet_counter: frame_crypto::RatchetCounter = 0;
                let secret = frame_crypto::random_secret(&mut rand::rngs::OsRng);

                if let JoinState::Joined(local_demux_id, _) = state.join_state {
                    let user_ids: HashSet<UserId> = state
                        .remote_devices
                        .iter()
                        .map(|rd| rd.user_id.clone())
                        .collect();
                    info!(
                        "Sending newly rotated key to everyone (number of users: {})",
                        user_ids.len()
                    );
                    for user_id in user_ids {
                        Self::send_media_send_key_to_user_over_signaling(
                            state,
                            user_id,
                            local_demux_id,
                            ratchet_counter,
                            secret,
                        );
                    }
                }

                state.media_send_key_rotation_state = KeyRotationState::Pending {
                    secret,
                    needs_another_rotation: false,
                };
                state
                    .actor
                    .send_delayed(Duration::from_secs(MEDIA_SEND_KEY_ROTATION_DELAY_SECS), move |state| {
                        info!("Applying the new send key. client_id: {}", state.client_id);
                        {
                            let mut frame_crypto_context = state
                                .frame_crypto_context
                                .lock()
                                .expect("Get lock for frame encryption context to reset media send key");
                            frame_crypto_context.reset_send_ratchet(secret);
                        }

                        let needs_another_rotation = matches!(state.media_send_key_rotation_state, KeyRotationState::Pending{needs_another_rotation: true, ..});
                        state.media_send_key_rotation_state = KeyRotationState::Applied;
                        if needs_another_rotation {
                            Self::rotate_media_send_key_and_send_to_users_not_removed(state);
                        }
                    })
            }
        }
    }

    fn advance_media_send_key_and_send_to_users_with_added_devices(
        state: &mut State,
        users_with_added_devices: &[UserId],
    ) {
        info!(
            "Advancing current media send key because a user has been added. client_id: {}",
            state.client_id
        );

        let (ratchet_counter, secret) = {
            let mut frame_crypto_context = state
                .frame_crypto_context
                .lock()
                .expect("Get lock for frame encryption context to advance media send key");
            frame_crypto_context.advance_send_ratchet()
        };
        if let JoinState::Joined(local_demux_id, _) = state.join_state {
            info!(
                "Sending newly advanced key to users with added devices (number of users: {})",
                users_with_added_devices.len()
            );
            for user_id in users_with_added_devices {
                Self::send_media_send_key_to_user_over_signaling(
                    state,
                    user_id.to_vec(),
                    local_demux_id,
                    ratchet_counter,
                    secret,
                );
            }
        }
    }

    fn add_media_receive_key_or_store_for_later(
        state: &mut State,
        user_id: UserId,
        demux_id: DemuxId,
        ratchet_counter: frame_crypto::RatchetCounter,
        secret: frame_crypto::Secret,
    ) {
        if let Some(device) = state
            .remote_devices
            .iter_mut()
            .find(|device| device.demux_id == demux_id)
        {
            if device.user_id == user_id {
                info!(
                    "Adding media receive key from {}. client_id: {}",
                    device.demux_id, state.client_id
                );
                let mut frame_crypto_context = state
                    .frame_crypto_context
                    .lock()
                    .expect("Get lock for frame encryption context to add media receive key");
                frame_crypto_context.add_receive_secret(demux_id, ratchet_counter, secret);
                let had_media_keys = std::mem::replace(&mut device.media_keys_received, true);
                if !had_media_keys {
                    state
                        .observer
                        .handle_remote_devices_changed(state.client_id, &state.remote_devices)
                }
            } else {
                warn!("Ignoring received media key from user because the demux ID {} doesn't make sense", demux_id);
                debug!("  user_id: {}", uuid_to_string(&user_id));
            }
        } else {
            info!(
                "Storing media receive key from {} because we don't know who they are yet.",
                demux_id
            );
            state
                .pending_media_receive_keys
                .push((user_id, demux_id, ratchet_counter, secret));
        }
    }

    fn send_media_send_key_to_user_over_signaling(
        state: &mut State,
        recipient_id: UserId,
        local_demux_id: DemuxId,
        ratchet_counter: frame_crypto::RatchetCounter,
        secret: frame_crypto::Secret,
    ) {
        info!("send_media_send_key_to_user_over_signaling():");
        debug!("  recipient_id: {}", uuid_to_string(&recipient_id));

        let media_key = protobuf::group_call::device_to_device::MediaKey {
            demux_id: Some(local_demux_id),
            ratchet_counter: Some(ratchet_counter as u32),
            secret: Some(secret.to_vec()),
        };
        let message = protobuf::group_call::DeviceToDevice {
            group_id: Some(state.group_id.clone()),
            media_key: Some(media_key),
            ..Default::default()
        };

        state.observer.send_signaling_message(recipient_id, message);
    }

    fn send_pending_media_send_key_to_users_with_added_devices(
        state: &mut State,
        users_with_added_devices: &[UserId],
    ) {
        info!(
            "Sending pending media key to users with added devices (number of users: {}).",
            users_with_added_devices.len()
        );
        if let JoinState::Joined(local_demux_id, _) = state.join_state {
            if let KeyRotationState::Pending { secret, .. } = state.media_send_key_rotation_state {
                for user_id in users_with_added_devices.iter() {
                    Self::send_media_send_key_to_user_over_signaling(
                        state,
                        user_id.clone(),
                        local_demux_id,
                        0,
                        secret,
                    );
                }
            }
        }
    }

    // The format for the ciphertext is:
    // 1 (audio) or 10 (video) bytes of unencrypted media
    // N bytes of encrypted media (the rest of the given plaintext_size)
    // 1 byte RatchetCounter
    // 4 byte FrameCounter
    // 16 byte MAC
    //
    // Here is the justification for a 4 byte FrameCounter:
    // - With 30fps video with 3 layers:
    //   - an 8min call will require 17 bits
    //   - a 35hr call will require 25 bits
    //   - a 1yr call will require 33 bits
    // - So for most calls we need 3 bytes and for a small number of calls we need 4 bytes.
    // - We could use a varint mechanism to choose between 3 and 4 bytes, but that's not really
    //   worth the extra complexity.
    const FRAME_ENCRYPTION_FOOTER_LEN: usize = size_of::<frame_crypto::RatchetCounter>()
        + size_of::<u32>()
        + size_of::<frame_crypto::Mac>();

    // The portion of the frame we leave in the clear
    // to allow the SFU to forward media properly.
    fn unencrypted_media_header_len(is_audio: bool) -> usize {
        if is_audio {
            // For Opus TOC
            1
        } else {
            // For VP8 headers
            // TODO: Reduce this to 3 when it's not a key frame
            10
        }
    }

    // Called by WebRTC through PeerConnectionObserver
    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn get_ciphertext_buffer_size(plaintext_size: usize) -> usize {
        // If we get asked to encrypt a message of size greater than (usize::MAX - FRAME_ENCRYPTION_FOOTER_LEN),
        // we'd fail to write the footer in encrypt_media and the frame would be dropped.
        plaintext_size.saturating_add(Self::FRAME_ENCRYPTION_FOOTER_LEN)
    }

    // Called by WebRTC through PeerConnectionObserver
    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn encrypt_media(
        &self,
        is_audio: bool,
        plaintext: &[u8],
        ciphertext_buffer: &mut [u8],
    ) -> Result<usize> {
        let mut frame_crypto_context = self
            .frame_crypto_context
            .lock()
            .expect("Get e2ee context to encrypt media");

        let unencrypted_header_len = Self::unencrypted_media_header_len(is_audio);
        Self::encrypt(
            &mut frame_crypto_context,
            unencrypted_header_len,
            plaintext,
            ciphertext_buffer,
        )
    }

    fn encrypt_data(state: &mut State, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut frame_crypto_context = state
            .frame_crypto_context
            .lock()
            .expect("Get e2ee context to encrypt data");

        let mut ciphertext = vec![0; Self::get_ciphertext_buffer_size(plaintext.len())];
        Self::encrypt(&mut frame_crypto_context, 0, plaintext, &mut ciphertext)?;
        Ok(ciphertext)
    }

    fn encrypt(
        frame_crypto_context: &mut frame_crypto::Context,
        unencrypted_header_len: usize,
        plaintext: &[u8],
        ciphertext_buffer: &mut [u8],
    ) -> Result<usize> {
        let ciphertext_size = Self::get_ciphertext_buffer_size(plaintext.len());
        let mut plaintext = Reader::new(plaintext);
        let mut ciphertext = Writer::new(ciphertext_buffer);

        let unencrypted_header = plaintext.read_slice(unencrypted_header_len)?;
        ciphertext.write_slice(unencrypted_header)?;
        let encrypted_payload = ciphertext.write_slice(plaintext.remaining())?;

        let mut mac = frame_crypto::Mac::default();
        let (ratchet_counter, frame_counter) =
            frame_crypto_context.encrypt(encrypted_payload, unencrypted_header, &mut mac)?;
        if frame_counter > u32::MAX as u64 {
            return Err(RingRtcError::FrameCounterTooBig.into());
        }

        ciphertext.write_u8(ratchet_counter)?;
        ciphertext.write_u32(frame_counter as u32)?;
        ciphertext.write_slice(&mac)?;

        Ok(ciphertext_size)
    }

    // Called by WebRTC through PeerConnectionObserver
    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn get_plaintext_buffer_size(ciphertext_size: usize) -> usize {
        // If we get asked to decrypt a message of size less than FRAME_ENCRYPTION_FOOTER_LEN,
        // we'd fail to read the footer in encrypt_media and the frame would be dropped.
        ciphertext_size.saturating_sub(Self::FRAME_ENCRYPTION_FOOTER_LEN)
    }

    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn decrypt_media(
        &self,
        remote_demux_id: DemuxId,
        is_audio: bool,
        ciphertext: &[u8],
        plaintext_buffer: &mut [u8],
    ) -> Result<usize> {
        let mut frame_crypto_context = self
            .frame_crypto_context
            .lock()
            .expect("Get e2ee context to decrypt media");

        let unencrypted_header_len = Self::unencrypted_media_header_len(is_audio);
        Self::decrypt(
            &mut frame_crypto_context,
            remote_demux_id,
            unencrypted_header_len,
            ciphertext,
            plaintext_buffer,
        )
    }

    fn decrypt_data(&self, remote_demux_id: DemuxId, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let mut frame_crypto_context = self
            .frame_crypto_context
            .lock()
            .expect("Get e2ee context to encrypt data");

        let mut plaintext = vec![0; Self::get_plaintext_buffer_size(ciphertext.len())];
        Self::decrypt(
            &mut frame_crypto_context,
            remote_demux_id,
            0,
            ciphertext,
            &mut plaintext,
        )?;
        Ok(plaintext)
    }

    fn decrypt(
        frame_crypto_context: &mut frame_crypto::Context,
        remote_demux_id: DemuxId,
        unencrypted_header_len: usize,
        ciphertext: &[u8],
        plaintext_buffer: &mut [u8],
    ) -> Result<usize> {
        let mut ciphertext = Reader::new(ciphertext);
        let mut plaintext = Writer::new(plaintext_buffer);

        let unencrypted_header = ciphertext.read_slice(unencrypted_header_len)?;
        let mac: frame_crypto::Mac = ciphertext
            .read_slice_from_end(size_of::<frame_crypto::Mac>())?
            .try_into()?;
        let frame_counter = ciphertext.read_u32_from_end()?;
        let ratchet_counter = ciphertext.read_u8_from_end()?;

        plaintext.write_slice(unencrypted_header)?;
        let encrypted_payload = plaintext.write_slice(ciphertext.remaining())?;

        frame_crypto_context.decrypt(
            remote_demux_id,
            ratchet_counter,
            frame_counter as u64,
            encrypted_payload,
            unencrypted_header,
            &mac,
        )?;
        Ok(unencrypted_header.len() + encrypted_payload.len())
    }

    fn send_heartbeat(state: &mut State) -> Result<()> {
        let heartbeat_msg = encode_proto(
        protobuf::group_call::DeviceToDevice {
                heartbeat: Some(protobuf::group_call::device_to_device::Heartbeat {
                    audio_muted: state.outgoing_audio_muted,
                    video_muted: state.outgoing_video_muted,
                }),
                ..Default::default()
            }
        )?;
        Self::broadcast_data_through_sfu(state, &heartbeat_msg)
    }

    fn send_leaving_through_sfu_and_over_signaling(state: &mut State, local_demux_id: DemuxId) {
        use protobuf::group_call::{device_to_device::Leaving, DeviceToDevice};

        debug!(
            "group_call::Client(inner)::send_leaving_through_sfu_and_over_signaling(client_id: {}, local_demux_id: {})",
            state.client_id, local_demux_id,
        );

        let msg = DeviceToDevice {
            leaving: Some(Leaving::default()),
            ..DeviceToDevice::default()
        };
        if let Ok(encoded_msg) = encode_proto(msg) {
            if Self::broadcast_data_through_sfu(state, &encoded_msg).is_err() {
                warn!("Could not send leaving message through the SFU");
            } else {
                debug!("Send leaving message over RTP through SFU.");
            }
        } else {
            warn!("Could not encode leaving message")
        }

        let msg = DeviceToDevice {
            group_id: Some(state.group_id.clone()),
            leaving: Some(Leaving {
                demux_id: Some(local_demux_id),
            }),
            ..DeviceToDevice::default()
        };
        debug!(
            "Send leaving message to everyone over signaling (recipients: {:?}).",
            state.joined_members
        );
        for user_id in &state.joined_members {
            state
                .observer
                .send_signaling_message(user_id.clone(), msg.clone());
        }
    }

    fn broadcast_data_through_sfu(state: &mut State, message: &[u8]) -> Result<()> {
        debug!(
            "group_call::Client(inner)::broadcast_data_through_sfu(client_id: {}, message: {:?})",
            state.client_id, message,
        );
        if let JoinState::Joined(local_demux_id, _) = state.join_state {
            let message = Self::encrypt_data(state, message)?;
            let seqnum = state.rtp_data_through_sfu_next_seqnum;
            state.rtp_data_through_sfu_next_seqnum =
                state.rtp_data_through_sfu_next_seqnum.wrapping_add(1);

            let header = rtp::Header {
                pt:        RTP_DATA_PAYLOAD_TYPE,
                ssrc:      local_demux_id.saturating_add(RTP_DATA_THROUGH_SFU_SSRC_OFFSET),
                // This has to be incremented to make sure SRTP functions properly.
                seqnum:    seqnum as u16,
                // Just imagine the clock is the number of heartbeat ticks :).
                // Plus the above sequence number is too small to be useful.
                timestamp: seqnum,
            };
            state.peer_connection.send_rtp(header, &message)?;
        }
        Ok(())
    }

    fn send_data_to_sfu(state: &mut State, message: &[u8]) -> Result<()> {
        debug!(
            "group_call::Client(inner)::send_data_to_sfu(client_id: {}, message: {:?})",
            state.client_id, message,
        );
        if let JoinState::Joined(_local_demux_id, _) = state.join_state {
            let seqnum = state.rtp_data_to_sfu_next_seqnum;
            state.rtp_data_to_sfu_next_seqnum = state.rtp_data_to_sfu_next_seqnum.wrapping_add(1);

            let header = rtp::Header {
                pt:        RTP_DATA_PAYLOAD_TYPE,
                ssrc:      RTP_DATA_TO_SFU_SSRC,
                // This has to be incremented to make sure SRTP functions properly.
                seqnum:    seqnum as u16,
                // Just imagine the clock is the number of messages :),
                // Plus the above sequence number is too small to be useful.
                timestamp: seqnum,
            };
            state.peer_connection.send_rtp(header, &message)?;
        }
        Ok(())
    }

    fn handle_rtp_received(&self, header: rtp::Header, payload: &[u8]) {
        use protobuf::group_call::{
            sfu_to_device::DeviceJoinedOrLeft,
            sfu_to_device::Speaker,
            DeviceToDevice,
            SfuToDevice,
        };

        if header.pt == RTP_DATA_PAYLOAD_TYPE {
            if header.ssrc == RTP_DATA_TO_SFU_SSRC {
                if let Ok(msg) = SfuToDevice::decode(&payload[..]) {
                    let mut handled = false;
                    if let Some(Speaker {
                        long_device_id: Some(speaker_long_device_id),
                    }) = &msg.speaker
                    {
                        self.handle_speaker_received(
                            header.timestamp,
                            speaker_long_device_id.clone(),
                        );
                        handled = true;
                    };
                    if let Some(DeviceJoinedOrLeft { .. }) = msg.device_joined_or_left {
                        self.handle_remote_device_joined_or_left();
                    }
                    if !handled {
                        // TODO: Handle msg.devices to trigger a remote devices request.
                        // TODO: Handle msg.video_request to trigger a change to the resolution/bitrate we send.
                        // TODO: Handle msg.device_connection_status to add it to state.remote_devices so the UI can draw something
                        info!("Received message from SFU over RTP data: {:?}", msg);
                    }
                }
                debug!("Received RTP data from SFU: {:?}.", payload);
            } else {
                let demux_id = header.ssrc.saturating_sub(RTP_DATA_THROUGH_SFU_SSRC_OFFSET);
                if let Ok(payload) = self.decrypt_data(demux_id, payload) {
                    if let Ok(msg) = DeviceToDevice::decode(&payload[..]) {
                        if let Some(heartbeat) = msg.heartbeat {
                            self.handle_heartbeat_received(demux_id, header.timestamp, heartbeat);
                        }
                        if let Some(_leaving) = msg.leaving {
                            self.actor.send(move |state| {
                                Self::handle_leaving_received(state, demux_id);
                            });
                        }
                    } else {
                        warn!(
                            "Ignoring received RTP data because decoding failed. demux_id: {}",
                            demux_id,
                        );
                    }
                } else {
                    warn!(
                        "Ignoring received RTP data because decryption failed. demux_id: {}",
                        demux_id,
                    );
                }
                self.actor.send(move |state| {
                    let known = state
                        .remote_devices
                        .iter()
                        .any(|rd| rd.demux_id == demux_id);
                    if !known {
                        // It's likely this demux_id just joined.
                        debug!("Request devices because we just received a heartbeat from unknown demux_id = {}", demux_id);
                        Self::request_remote_devices_as_soon_as_possible(state);
                    }
                });
            }
        } else {
            warn!(
                "Ignoring received RTP data with unknown payload type: {}",
                header.pt
            );
        }
    }

    fn handle_speaker_received(&self, timestamp: rtp::Timestamp, speaker_long_device_id: String) {
        self.actor.send(move |state| {
            if let Some(speaker_rtp_timestamp) = state.speaker_rtp_timestamp {
                if timestamp <= speaker_rtp_timestamp {
                    // Ignored packets received out of order
                    debug!(
                        "Ignoring speaker change because the timestamp is old: {}",
                        timestamp
                    );
                    return;
                }
            }
            state.speaker_rtp_timestamp = Some(timestamp);

            if let Some(speaker_device) = state
                .remote_devices
                .iter_mut()
                .find(|device| device.long_device_id == speaker_long_device_id)
            {
                speaker_device.speaker_time = Some(SystemTime::now());
                debug!(
                    "Updated speaker time of {:?} to {:?}",
                    speaker_device.demux_id, speaker_device.speaker_time
                );
                state
                    .observer
                    .handle_remote_devices_changed(state.client_id, &state.remote_devices);
            } else {
                debug!(
                    "Ignoring speaker change because it isn't a known remote devices: {}",
                    speaker_long_device_id
                );
                // Unknown speaker device. It's probably the local device.
            }
        });
    }

    fn handle_remote_device_joined_or_left(&self) {
        self.actor.send(move |state| {
            info!("SFU notified that a remote device has joined or left, requesting update");
            Self::request_remote_devices_as_soon_as_possible(state);
        })
    }

    fn handle_heartbeat_received(
        &self,
        demux_id: DemuxId,
        timestamp: u32,
        heartbeat: protobuf::group_call::device_to_device::Heartbeat,
    ) {
        self.actor.send(move |state| {
            if let Some(remote_device) = state
                .remote_devices
                .iter_mut()
                .find(|device| device.demux_id == demux_id)
            {
                if timestamp > remote_device.muted_rtp_timestamp.unwrap_or(0) {
                    // Record this even if nothing changed.  Otherwise an old packet could override
                    // a new packet.
                    remote_device.muted_rtp_timestamp = Some(timestamp);
                    if remote_device.audio_muted != heartbeat.audio_muted
                        || remote_device.video_muted != heartbeat.video_muted
                    {
                        remote_device.audio_muted = heartbeat.audio_muted;
                        remote_device.video_muted = heartbeat.video_muted;
                        state
                            .observer
                            .handle_remote_devices_changed(state.client_id, &state.remote_devices);
                    }
                }
            } else {
                warn!(
                    "Ignoring received heartbeat for unknown demux_id {}",
                    demux_id
                );
            }
        });
    }

    fn handle_leaving_received(state: &mut State, demux_id: DemuxId) {
        // It's likely we haven't received an update from the SFU about this demux_id leaving.
        debug!(
            "Request devices because we just received a leaving message from demux_id = {}",
            demux_id
        );
        if let Some(device) = state
            .remote_devices
            .iter_mut()
            .find(|device| device.demux_id == demux_id)
        {
            if !device.leaving_received {
                device.leaving_received = true;
                Self::request_remote_devices_as_soon_as_possible(state);

                // It's also possible we have learned this before the SFU has, in which case the SFU may have stale data.
                // So let's wait a little while and ask again.
                state
                    .actor
                    .send_delayed(Duration::from_secs(2), move |state| {
                        info!("Request devices because we received a leaving message from demux_id = {} a while ago", demux_id);
                        Self::request_remote_devices_as_soon_as_possible(state);
                    });
            }
        }
    }
}

fn encode_proto(msg: impl prost::Message) -> Result<BytesMut> {
    let mut bytes = BytesMut::with_capacity(msg.encoded_len());
    msg.encode(&mut bytes)?;
    Ok(bytes)
}

// We need to wrap a Call to implement PeerConnectionObserverTrait
// because we need to pass an impl into PeerConnectionObserver::new
// before we call PeerConnectionFactory::create_peer_connection.
// So we need to either have an Option<PeerConnection> inside of the
// State or have an Option<Call> instead of here.  This seemed
// more convenient (fewer "if let Some(x) = x" to do).
struct PeerConnectionObserverImpl {
    client: Option<Client>,
}

impl PeerConnectionObserverImpl {
    fn uninitialized() -> Result<(Box<Self>, PeerConnectionObserver<Self>)> {
        let mut boxed_observer_impl = Box::new(Self { client: None });
        let observer = PeerConnectionObserver::new(
            &mut *boxed_observer_impl,
            true, /* enable_frame_encryption */
        )?;
        Ok((boxed_observer_impl, observer))
    }

    fn initialize(&mut self, client: Client) {
        self.client = Some(client);
    }
}

impl PeerConnectionObserverTrait for PeerConnectionObserverImpl {
    fn log_id(&self) -> &dyn std::fmt::Display {
        if let Some(client) = &self.client {
            &client.client_id
        } else {
            &"Call that hasn't been setup yet."
        }
    }

    fn handle_ice_candidate_gathered(
        &mut self,
        _ice_candidate: signaling::IceCandidate,
    ) -> Result<()> {
        Ok(())
    }

    fn handle_ice_connection_state_changed(
        &mut self,
        ice_connection_state: IceConnectionState,
    ) -> Result<()> {
        debug!(
            "group_call::Client(outer)::handle_ice_connection_state_changed(client_id: {}, state: {:?})",
            self.log_id(),
            ice_connection_state
        );
        if let Some(client) = &self.client {
            client.actor.send(move |state| {
                debug!("group_call::Client(inner)::handle_ice_connection_state_changed(client_id: {}, state: {:?})", state.client_id, ice_connection_state);

                match (state.connection_state, ice_connection_state) {
                    (ConnectionState::Connecting, IceConnectionState::Disconnected) |
                    (ConnectionState::Connecting, IceConnectionState::Closed) |
                    (ConnectionState::Connecting, IceConnectionState::Failed) => {
                        // ICE or DTLS failed before we got connected :(
                        Client::end(state, EndReason::IceFailedWhileConnecting);
                    }
                    (ConnectionState::Connecting, IceConnectionState::Checking) => {
                        // Normal.  Not much to report.
                    }
                    (ConnectionState::Connecting, IceConnectionState::Connected) |
                    (ConnectionState::Connecting, IceConnectionState::Completed) => {
                        // ICE and DTLS Connected!
                        // (Despite the name, PeerConnection::OnIceStateChanged is for ICE and DTLS.
                        Client::set_connection_state_and_notify_observer(state, ConnectionState::Connected);
                    }
                    (ConnectionState::Connected, IceConnectionState::Checking) |
                    (ConnectionState::Connected, IceConnectionState::Disconnected) => {
                        // Some connectivity problems, hopefully temporary.
                        Client::set_connection_state_and_notify_observer(state, ConnectionState::Reconnecting);
                    }
                    (ConnectionState::Reconnecting, IceConnectionState::Connected) |
                    (ConnectionState::Reconnecting, IceConnectionState::Completed) => {
                        // The connectivity problems have gone away it seems.
                        Client::set_connection_state_and_notify_observer(state, ConnectionState::Connected);
                    }
                    (_, IceConnectionState::Failed) |
                    (_, IceConnectionState::Closed) => {
                        // The connectivity problems persisted.  ICE has failed.
                        Client::end(state, EndReason::IceFailedAfterConnected);
                    }
                    (_, _) => {
                        warn!("Could not process ICE connection state {:?} while in group call ConnectionState {:?}", ice_connection_state, state.connection_state);
                    }
                }
            });
        } else {
            warn!("Call isn't setup yet!");
        }
        Ok(())
    }

    fn handle_incoming_video_added(&mut self, incoming_video_track: VideoTrack) -> Result<()> {
        debug!(
            "group_call::Client(outer)::handle_incoming_video_track(client_id: {})",
            self.log_id()
        );
        if let Some(client) = &self.client {
            client.actor.send(move |state| {
                debug!(
                    "group_call::Client(inner)::handle_incoming_video_track(client_id: {})",
                    state.client_id
                );

                if let Some(remote_demux_id) = incoming_video_track.id() {
                    state.observer.handle_incoming_video_track(
                        state.client_id,
                        remote_demux_id,
                        incoming_video_track,
                    )
                } else {
                    warn!("Ignoring incoming video track with unparsable ID",);
                }
            });
        } else {
            warn!("Call isn't setup yet!");
        }
        Ok(())
    }

    fn handle_signaling_data_channel_connected(
        &mut self,
        _data_channel: DataChannel,
    ) -> Result<()> {
        Ok(())
    }

    fn handle_rtp_received(&mut self, header: rtp::Header, payload: &[u8]) {
        if let Some(client) = &self.client {
            client.handle_rtp_received(header, payload);
        } else {
            warn!(
                "Ignoring received RTP data with SSRC {} because the call isn't setup",
                header.ssrc
            );
        }
    }

    #[allow(clippy::collapsible_if)]
    fn handle_signaling_data_channel_message(&mut self, _bytes: Bytes) {
        info!(
            "group_call::Client(outer)::handle_data_channel_message(client_id: {})",
            self.log_id()
        );
    }

    fn get_media_ciphertext_buffer_size(
        &mut self,
        _is_audio: bool,
        plaintext_size: usize,
    ) -> usize {
        Client::get_ciphertext_buffer_size(plaintext_size)
    }

    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn encrypt_media(
        &mut self,
        is_audio: bool,
        plaintext: &[u8],
        ciphertext_buffer: &mut [u8],
    ) -> Result<usize> {
        if let Some(client) = &self.client {
            client.encrypt_media(is_audio, plaintext, ciphertext_buffer)
        } else {
            warn!("Call isn't setup yet!  Can't encrypt.");
            Err(RingRtcError::FailedToEncrypt.into())
        }
    }

    fn get_media_plaintext_buffer_size(
        &mut self,
        _track_id: u32,
        _is_audio: bool,
        ciphertext_size: usize,
    ) -> usize {
        Client::get_plaintext_buffer_size(ciphertext_size)
    }

    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn decrypt_media(
        &mut self,
        track_id: u32,
        is_audio: bool,
        ciphertext: &[u8],
        plaintext_buffer: &mut [u8],
    ) -> Result<usize> {
        if let Some(client) = &self.client {
            let remote_demux_id = track_id;
            client.decrypt_media(remote_demux_id, is_audio, ciphertext, plaintext_buffer)
        } else {
            warn!("Call isn't setup yet!  Can't decrypt");
            Err(RingRtcError::FailedToDecrypt.into())
        }
    }
}

fn random_alphanumeric(len: usize) -> String {
    std::iter::repeat(())
        .map(|()| rand::rngs::OsRng.sample(rand::distributions::Alphanumeric))
        .take(len)
        .collect()
}

// Should this go in some util class?
struct Writer<'buf> {
    buf:    &'buf mut [u8],
    offset: usize,
}

impl<'buf> Writer<'buf> {
    fn new(buf: &'buf mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    fn remaining_len(&self) -> usize {
        self.buf.len() - self.offset
    }

    fn write_u8(&mut self, input: u8) -> Result<()> {
        if self.remaining_len() < 1 {
            return Err(RingRtcError::BufferTooSmall.into());
        }
        self.buf[self.offset] = input;
        self.offset += 1;
        Ok(())
    }

    fn write_u32(&mut self, input: u32) -> Result<()> {
        self.write_slice(&input.to_be_bytes())?;
        Ok(())
    }

    fn write_slice(&mut self, input: &[u8]) -> Result<&mut [u8]> {
        if self.remaining_len() < input.len() {
            return Err(RingRtcError::BufferTooSmall.into());
        }
        let start = self.offset;
        let end = start + input.len();
        let output = &mut self.buf[start..end];
        output.copy_from_slice(input);
        self.offset = end;
        Ok(output)
    }
}

struct Reader<'data> {
    data: &'data [u8],
}

impl<'data> Reader<'data> {
    fn new(data: &'data [u8]) -> Self {
        Self { data }
    }

    fn remaining(&self) -> &[u8] {
        self.data
    }

    fn read_u8_from_end(&mut self) -> Result<u8> {
        let (last, rest) = self.data.split_last().ok_or(RingRtcError::BufferTooSmall)?;
        self.data = rest;
        Ok(*last)
    }

    fn read_u32_from_end(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(
            self.read_slice_from_end(size_of::<u32>())?.try_into()?,
        ))
    }

    fn read_slice(&mut self, len: usize) -> Result<&'data [u8]> {
        if len > self.data.len() {
            return Err(RingRtcError::BufferTooSmall.into());
        }
        let (read, rest) = self.data.split_at(len);
        self.data = rest;
        Ok(read)
    }

    fn read_slice_from_end(&mut self, len: usize) -> Result<&'data [u8]> {
        if len > self.data.len() {
            return Err(RingRtcError::BufferTooSmall.into());
        }
        let (rest, read) = self.data.split_at(self.data.len() - len);
        self.data = rest;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webrtc::sim::media::FAKE_AUDIO_TRACK;
    use std::sync::{
        atomic::{self, AtomicU64},
        mpsc,
        Arc,
        Condvar,
        Mutex,
    };

    #[derive(Clone)]
    struct FakeSfuClient {
        sfu_info:       SfuInfo,
        local_demux_id: DemuxId,
        request_count:  Arc<AtomicU64>,
    }

    impl FakeSfuClient {
        fn new(sfu_info: SfuInfo, local_demux_id: DemuxId) -> Self {
            Self {
                sfu_info,
                local_demux_id,
                request_count: Arc::new(AtomicU64::new(0)),
            }
        }
    }

    impl FakeSfuClient {
        pub fn request_count(&self) -> u64 {
            self.request_count.load(atomic::Ordering::SeqCst)
        }
    }

    impl SfuClient for FakeSfuClient {
        fn join(
            &mut self,
            _ice_ufrag: &str,
            _ice_pwd: &str,
            _dtls_fingerprint: &DtlsFingerprint,
            client: Client,
        ) {
            client.on_sfu_client_joined(Ok((
                self.sfu_info.clone(),
                self.local_demux_id,
                "token".to_string(),
            )));
        }
        fn peek(&mut self, _handle_remote_devices: BoxedPeekInfoHandler) {
            self.request_count.fetch_add(1, atomic::Ordering::SeqCst);
        }
        fn set_group_members(&mut self, _members: Vec<GroupMemberInfo>) {}
        fn set_membership_proof(&mut self, _proof: MembershipProof) {}
        fn leave(&mut self, _long_device_id: String) {}
    }

    // TODO: Put this in common util area?
    #[derive(Clone)]
    struct Waitable<T> {
        val:  Arc<Mutex<Option<T>>>,
        cvar: Arc<Condvar>,
    }

    impl<T> Default for Waitable<T> {
        fn default() -> Self {
            Self {
                val:  Arc::default(),
                cvar: Arc::default(),
            }
        }
    }

    impl<T: Clone> Waitable<T> {
        fn set(&self, val: T) {
            let mut val_guard = self.val.lock().unwrap();
            *val_guard = Some(val);
            self.cvar.notify_all();
        }

        fn wait(&self) -> T {
            let mut val = self.val.lock().unwrap();
            while val.is_none() {
                val = self.cvar.wait(val).unwrap();
            }
            val.clone().unwrap()
        }
    }

    #[derive(Clone, Default)]
    struct Event {
        waitable: Waitable<()>,
    }

    impl Event {
        fn set(&self) {
            self.waitable.set(());
        }

        fn wait(&self) {
            self.waitable.wait();
        }
    }

    #[derive(Clone, Default)]
    struct FakeObserverPeekState {
        joined_members: Vec<UserId>,
        creator:        Option<UserId>,
        era_id:         Option<String>,
        max_devices:    Option<u32>,
        device_count:   u32,
    }

    #[derive(Clone)]
    struct FakeObserver {
        // For sending messages
        user_id:                    UserId,
        recipients:                 Arc<CallMutex<Vec<TestClient>>>,
        outgoing_signaling_blocked: Arc<CallMutex<bool>>,

        joined:                      Event,
        remote_devices:              Arc<CallMutex<Vec<RemoteDeviceState>>>,
        remote_devices_at_join_time: Arc<CallMutex<Vec<RemoteDeviceState>>>,
        peek_state:                  Arc<CallMutex<FakeObserverPeekState>>,
        max_send_bitrate:            Arc<CallMutex<Option<DataRate>>>,
        ended:                       Waitable<EndReason>,
        era_id:                      Option<String>,
    }

    impl FakeObserver {
        fn new(user_id: UserId) -> Self {
            Self {
                user_id,
                recipients: Arc::new(CallMutex::new(Vec::new(), "FakeObserver recipients")),
                outgoing_signaling_blocked: Arc::new(CallMutex::new(
                    false,
                    "FakeObserver outgoing_signaling_blocked",
                )),
                joined: Event::default(),
                remote_devices: Arc::new(CallMutex::new(Vec::new(), "FakeObserver remote devices")),
                remote_devices_at_join_time: Arc::new(CallMutex::new(
                    Vec::new(),
                    "FakeObserver remote devices",
                )),
                peek_state: Arc::new(CallMutex::new(
                    FakeObserverPeekState::default(),
                    "FakeObserver peek state",
                )),
                max_send_bitrate: Arc::new(CallMutex::new(None, "FakeObserver max send bitrate")),
                ended: Waitable::default(),
                era_id: None,
            }
        }

        fn set_outgoing_signaling_blocked(&self, blocked: bool) {
            let mut outgoing_signaling_blocked = self
                .outgoing_signaling_blocked
                .lock()
                .expect("Lock outgoing_signaling_blocked to set it");
            *outgoing_signaling_blocked = blocked;
        }

        fn outgoing_signaling_blocked(&self) -> bool {
            let outgoing_signaling_blocked = self
                .outgoing_signaling_blocked
                .lock()
                .expect("Lock outgoing_signaling_blocked to get it");
            *outgoing_signaling_blocked
        }

        fn set_recipients(&self, recipients: Vec<TestClient>) {
            let mut owned_recipients = self
                .recipients
                .lock()
                .expect("Lock recipients to add recipient");
            *owned_recipients = recipients;
        }

        fn remote_devices(&self) -> Vec<RemoteDeviceState> {
            let remote_devices = self
                .remote_devices
                .lock()
                .expect("Lock remote devices to read them");
            remote_devices.iter().cloned().collect()
        }

        fn remote_devices_at_join_time(&self) -> Vec<RemoteDeviceState> {
            let remote_devices_at_join_time = self
                .remote_devices_at_join_time
                .lock()
                .expect("Lock remote devices at join time to read them");
            remote_devices_at_join_time.iter().cloned().collect()
        }

        fn joined_members(&self) -> Vec<UserId> {
            let peek_state = self.peek_state.lock().expect("Lock peek state to read it");
            peek_state.joined_members.iter().cloned().collect()
        }

        fn peek_state(&self) -> FakeObserverPeekState {
            let peek_state = self.peek_state.lock().expect("Lock peek state to read it");
            peek_state.clone()
        }

        fn max_send_bitrate(&self) -> Option<DataRate> {
            let max_send_bitrate = self
                .max_send_bitrate
                .lock()
                .expect("Lock max send bitrate to read it");
            *max_send_bitrate
        }
    }

    impl Observer for FakeObserver {
        fn request_membership_proof(&self, _client_id: ClientId) {}
        fn request_group_members(&self, _client_id: ClientId) {}
        fn handle_connection_state_changed(
            &self,
            _client_id: ClientId,
            _connection_state: ConnectionState,
        ) {
        }
        fn handle_join_state_changed(&self, _client_id: ClientId, join_state: JoinState) {
            if let JoinState::Joined(_, _) = join_state {
                let mut owned_remote_devices_at_join_time = self
                    .remote_devices_at_join_time
                    .lock()
                    .expect("Lock joined members at join time to handle update");
                *owned_remote_devices_at_join_time = self.remote_devices();
                self.joined.set();
            }
        }
        fn handle_remote_devices_changed(
            &self,
            _client_id: ClientId,
            remote_devices: &[RemoteDeviceState],
        ) {
            let mut owned_remote_devices = self
                .remote_devices
                .lock()
                .expect("Lock recipients to set remote devices");
            *owned_remote_devices = remote_devices.iter().cloned().collect();
        }
        fn handle_peek_changed(
            &self,
            _client_id: ClientId,
            joined_members: &[UserId],
            creator: Option<UserId>,
            era_id: Option<&str>,
            max_devices: Option<u32>,
            device_count: u32,
        ) {
            let mut owned_state = self
                .peek_state
                .lock()
                .expect("Lock peek state to handle update");
            owned_state.joined_members = joined_members.iter().cloned().collect();
            owned_state.creator = creator.clone();
            owned_state.era_id = era_id.map(String::from);
            owned_state.max_devices = max_devices;
            owned_state.device_count = device_count;
        }
        fn handle_max_send_bitrate_changed(&self, _client_id: ClientId, rate: DataRate) {
            let mut max_send_bitrate = self
                .max_send_bitrate
                .lock()
                .expect("Lock max_send_bitrate to handle update");
            *max_send_bitrate = Some(rate);
        }

        fn send_signaling_message(
            &mut self,
            recipient_id: UserId,
            message: protobuf::group_call::DeviceToDevice,
        ) {
            if self.outgoing_signaling_blocked() {
                info!(
                    "Dropping message from {:?} to {:?} because we blocked signaling.",
                    self.user_id, recipient_id
                );
                return;
            }
            let recipients = self
                .recipients
                .lock()
                .expect("Lock recipients to add recipient");
            let mut sent = false;
            for recipient in recipients.iter() {
                if recipient.user_id == recipient_id {
                    recipient
                        .client
                        .on_signaling_message_received(self.user_id.clone(), message.clone());
                    sent = true;
                }
            }
            if sent {
                info!(
                    "Sent message from {:?} to {:?}.",
                    self.user_id, recipient_id
                );
            } else {
                info!(
                    "Did not sent message from {:?} to {:?} becuase it's not a known recipient.",
                    self.user_id, recipient_id
                );
            }
        }
        fn handle_incoming_video_track(
            &mut self,
            _client_id: ClientId,
            _remote_demux_id: DemuxId,
            _incoming_video_track: VideoTrack,
        ) {
        }
        fn handle_ended(&self, _client_id: ClientId, reason: EndReason) {
            self.ended.set(reason);
        }
    }

    #[derive(Clone)]
    struct TestClient {
        user_id:               UserId,
        demux_id:              DemuxId,
        sfu_client:            FakeSfuClient,
        observer:              FakeObserver,
        client:                Client,
        sfu_rtp_packet_sender: Option<mpsc::Sender<(rtp::Header, Vec<u8>)>>,
        default_peek_info:     PeekInfo,
    }

    // Just so it's something different
    fn demux_id_to_short_device_id(demux_id: DemuxId) -> u64 {
        (demux_id + 1000) as u64
    }

    // Just so it's something different
    fn demux_id_to_long_device_id(demux_id: DemuxId) -> String {
        format!("long-{}", demux_id).to_string()
    }

    impl TestClient {
        fn new(user_id: UserId, demux_id: DemuxId, forged_demux_id: Option<DemuxId>) -> Self {
            let sfu_client = FakeSfuClient::new(
                SfuInfo {
                    udp_addresses:    Vec::new(),
                    ice_ufrag:        "fake ICE ufrag".to_string(),
                    ice_pwd:          "fake ICE pwd".to_string(),
                    dtls_fingerprint: DtlsFingerprint::default(),
                },
                forged_demux_id.unwrap_or(demux_id),
            );
            let observer = FakeObserver::new(user_id.clone());
            let fake_busy = Arc::new(CallMutex::new(false, "fake_busy"));
            let fake_audio_track = AudioTrack::owned(FAKE_AUDIO_TRACK as *const u32);
            let client = Client::start(
                b"fake group ID".to_vec(),
                demux_id,
                Box::new(sfu_client.clone()),
                Box::new(observer.clone()),
                fake_busy,
                None,
                fake_audio_track,
                None,
            )
            .expect("Start Client");
            Self {
                user_id,
                demux_id,
                sfu_client,
                observer,
                client,
                sfu_rtp_packet_sender: None,
                default_peek_info: PeekInfo::default(),
            }
        }

        fn connect_join_and_wait_until_joined(&self) {
            self.client.connect();
            self.client.join();
            self.observer.joined.wait();
        }

        fn set_remotes_and_wait_until_applied(&self, clients: &[&TestClient]) {
            let remote_devices = clients
                .iter()
                .map(|client| PeekDeviceInfo {
                    demux_id:        client.demux_id,
                    user_id:         Some(client.user_id.clone()),
                    short_device_id: demux_id_to_short_device_id(client.demux_id),
                    long_device_id:  demux_id_to_long_device_id(client.demux_id),
                })
                .collect();
            // Need to clone to pass over to the actor and set in observer.
            let clients: Vec<TestClient> = clients.into_iter().copied().cloned().collect();
            self.observer.set_recipients(clients.clone());
            let peek_info = PeekInfo {
                devices: remote_devices,
                ..self.default_peek_info.clone()
            };
            self.client.set_peek_info(Ok(peek_info));
            let local_demux_id = self.demux_id;
            let sfu_rtp_packet_sender = self.sfu_rtp_packet_sender.clone();
            self.client.actor.send(move |state| {
                state
                    .peer_connection
                    .set_rtp_packet_sink(Box::new(move |header, payload| {
                        debug!(
                            "Test is going to deliver RTP packet with {:?} and {:?}",
                            header, payload
                        );
                        if header.ssrc == 1 {
                            if let Some(sender) = &sfu_rtp_packet_sender {
                                sender
                                    .send((header, payload.to_vec()))
                                    .expect("Send RTP packet to SFU");
                            }
                        } else {
                            for client in &clients {
                                if client.demux_id != local_demux_id {
                                    client.client.handle_rtp_received(header.clone(), payload)
                                }
                            }
                        }
                    }));
            });
            self.wait_for_client_to_process();
        }

        fn wait_for_client_to_process(&self) {
            let event = Event::default();
            let cloned = event.clone();
            self.client.actor.send(move |_state| {
                cloned.set();
            });
            event.wait();
        }

        fn encrypt_media(&mut self, is_audio: bool, plaintext: &[u8]) -> Result<Vec<u8>> {
            let mut ciphertext = vec![0; plaintext.len() + Client::FRAME_ENCRYPTION_FOOTER_LEN];
            assert_eq!(
                ciphertext.len(),
                Client::get_ciphertext_buffer_size(plaintext.len())
            );
            assert_eq!(
                ciphertext.len(),
                self.client
                    .encrypt_media(is_audio, &plaintext, &mut ciphertext)?
            );
            Ok(ciphertext)
        }

        fn decrypt_media(
            &mut self,
            remote_demux_id: DemuxId,
            is_audio: bool,
            ciphertext: &[u8],
        ) -> Result<Vec<u8>> {
            let mut plaintext = vec![
                0;
                ciphertext
                    .len()
                    .saturating_sub(Client::FRAME_ENCRYPTION_FOOTER_LEN)
            ];
            assert_eq!(
                plaintext.len(),
                Client::get_plaintext_buffer_size(ciphertext.len())
            );
            assert_eq!(
                plaintext.len(),
                self.client.decrypt_media(
                    remote_demux_id,
                    is_audio,
                    &ciphertext,
                    &mut plaintext
                )?
            );
            Ok(plaintext)
        }

        fn receive_speaker(&self, timestamp: u32, speaker_demux_id: DemuxId) {
            self.client
                .handle_speaker_received(timestamp, demux_id_to_long_device_id(speaker_demux_id));
            self.wait_for_client_to_process();
        }

        // DemuxIds sorted by speaker_time, then added_time, then demux_id.
        fn speakers(&self) -> Vec<DemuxId> {
            let mut devices = self.observer.remote_devices().clone();
            devices.sort_by_key(|device| {
                (
                    std::cmp::Reverse(device.speaker_time_as_unix_millis()),
                    device.added_time_as_unix_millis(),
                    device.demux_id,
                )
            });
            devices.iter().map(|device| device.demux_id).collect()
        }

        fn disconnect_and_wait_until_ended(&self) {
            self.client.disconnect();
            self.observer.ended.wait();
        }
    }

    #[allow(dead_code)]
    fn init_logging() {
        env_logger::builder()
            .is_test(true)
            .filter(None, log::LevelFilter::Debug)
            .init();
    }

    fn set_group_and_wait_until_applied(clients: &[&TestClient]) {
        for client in clients {
            // We're going to be lazy and not remove ourselves.  It shouldn't matter.
            client.set_remotes_and_wait_until_applied(clients);
        }
        for client in clients {
            client.wait_for_client_to_process();
        }
    }

    #[test]
    fn frame_encryption_normal() {
        let mut client1 = TestClient::new(vec![1], 1, None);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2, None);
        client2.connect_join_and_wait_until_joined();

        client2.set_remotes_and_wait_until_applied(&[&client1]);

        // At this point, client2 knows about client1, so can receive encrypted media.
        // But client1 does not know about client1, so has not yet shared its encryption key
        // with it, so client2 cannot decrypt media from client1.
        // And while client2 has shared the key with client1, client1 has not yet learned
        // about client2 so can't decrypt either.

        let is_audio = true;
        let plaintext = &b"Fake Audio"[..];
        let ciphertext1 = client1.encrypt_media(is_audio, plaintext).unwrap();
        let ciphertext2 = client2.encrypt_media(is_audio, plaintext).unwrap();

        // Check that the first byte for audio is left unencrypted
        // and the rest has changed
        assert_eq!(plaintext[0], ciphertext1[0]);
        assert_ne!(plaintext, &ciphertext1[..plaintext.len()]);

        assert!(client1
            .decrypt_media(client2.demux_id, is_audio, &ciphertext2)
            .is_err());
        assert!(client2
            .decrypt_media(client1.demux_id, is_audio, &ciphertext1)
            .is_err());

        client1.set_remotes_and_wait_until_applied(&[&client2]);
        // We wait until client2 has processed the key from client1
        client2.wait_for_client_to_process();

        // At this point, both clients know about each other and have shared keys
        // and should be able to decrypt.

        // Because client1 just learned about client2, it advanced its key
        // and so we need to re-encrypt with that key.
        let mut ciphertext1 = client1.encrypt_media(is_audio, plaintext).unwrap();

        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext1)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client1
                .decrypt_media(client2.demux_id, is_audio, &ciphertext2)
                .unwrap()
        );

        // But if the footer is too small, decryption should fail
        assert!(client1
            .decrypt_media(client2.demux_id, is_audio, b"small")
            .is_err());

        // And if the unencrypted media header has been modified, it should fail (bad mac)
        ciphertext1[0] = ciphertext1[0].wrapping_add(1);
        assert!(client2
            .decrypt_media(client1.demux_id, is_audio, &ciphertext1)
            .is_err());

        // Finally, let's make sure video works as well

        let is_audio = false;
        let plaintext = &b"Fake Video Needs To Be Bigger"[..];
        let ciphertext1 = client1.encrypt_media(is_audio, plaintext).unwrap();

        // Check that the first 10 bytes of video is left unencrypted
        // and the rest has changed
        assert_eq!(plaintext[..10], ciphertext1[..10]);
        assert_ne!(plaintext, &ciphertext1[..plaintext.len()]);

        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext1)
                .unwrap()
        );

        client1.disconnect_and_wait_until_ended();
        client2.disconnect_and_wait_until_ended();
    }

    #[test]
    #[ignore] // Because it's too slow
    fn frame_encryption_rotation_is_delayed() {
        let mut client1 = TestClient::new(vec![1], 1, None);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2, None);
        client2.connect_join_and_wait_until_joined();

        let mut client3 = TestClient::new(vec![3], 3, None);
        client3.connect_join_and_wait_until_joined();

        let mut client4 = TestClient::new(vec![4], 4, None);
        client4.connect_join_and_wait_until_joined();

        let mut client5 = TestClient::new(vec![5], 5, None);
        client5.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2, &client3]);

        // client2 and client3 can decrypt client1
        // client4 can't yet
        let is_audio = true;
        let plaintext = &b"Fake Audio"[..];
        let ciphertext = client1.encrypt_media(is_audio, plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client3
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert!(client4
            .decrypt_media(client1.demux_id, is_audio, &ciphertext)
            .is_err());

        // Add client4 and remove client3
        set_group_and_wait_until_applied(&[&client1, &client2, &client4]);

        // client2 and client4 can decrypt client1
        // client3 can as well, at least for a little while
        let ciphertext = client1.encrypt_media(is_audio, plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client3
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );

        // TODO: Make Actors use tokio so we can use fake time
        std::thread::sleep(std::time::Duration::from_millis(2000));

        // client5 joins during the period between when the new key is generated
        // and when it is applied.  client 5 should receive this key and decrypt
        // both before and after the key is applied.
        // meanwhile, client2 leaves, which will cause another rotation after this
        // one.
        set_group_and_wait_until_applied(&[&client1, &client4, &client5]);

        let ciphertext = client1.encrypt_media(is_audio, plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client3
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client5
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );

        std::thread::sleep(std::time::Duration::from_millis(2000));

        // client4 and client5 can still decrypt from client1
        // but client3 no longer can
        let ciphertext = client1.encrypt_media(is_audio, plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert!(client3
            .decrypt_media(client1.demux_id, is_audio, &ciphertext)
            .is_err());
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client5
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );

        std::thread::sleep(std::time::Duration::from_millis(3000));

        // After the next key rotation is applied, now client2 cannot decrypt,
        // but client4 and client5 can.
        let ciphertext = client1.encrypt_media(is_audio, plaintext).unwrap();
        assert!(client2
            .decrypt_media(client1.demux_id, is_audio, &ciphertext)
            .is_err());
        assert!(client3
            .decrypt_media(client1.demux_id, is_audio, &ciphertext)
            .is_err());
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client5
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );

        client1.disconnect_and_wait_until_ended();
        client2.disconnect_and_wait_until_ended();
        client3.disconnect_and_wait_until_ended();
        client4.disconnect_and_wait_until_ended();
        client5.disconnect_and_wait_until_ended();
    }

    #[test]
    fn frame_encryption_resend_keys() {
        let mut client1 = TestClient::new(vec![1], 1, None);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2, None);
        client2.connect_join_and_wait_until_joined();

        // Prevent client1 from sharing keys with client2
        client1.observer.set_outgoing_signaling_blocked(true);
        set_group_and_wait_until_applied(&[&client1, &client2]);

        let remote_devices = client2.observer.remote_devices();
        assert_eq!(1, remote_devices.len());
        assert_eq!(false, remote_devices[0].media_keys_received);

        let is_audio = false;
        let plaintext = &b"Fake Video is big"[..];
        let ciphertext = client1.encrypt_media(is_audio, plaintext).unwrap();
        // We can't decrypt because the keys got dropped
        assert!(client2
            .decrypt_media(client1.demux_id, is_audio, &ciphertext)
            .is_err());

        client1.observer.set_outgoing_signaling_blocked(false);
        client1.client.resend_media_keys();
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices = client2.observer.remote_devices();
        assert_eq!(1, remote_devices.len());
        assert_eq!(true, remote_devices[0].media_keys_received);

        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext)
                .unwrap()
        );
    }

    #[test]
    fn frame_encryption_send_advanced_key_to_same_user() {
        let mut client1a = TestClient::new(vec![1], 11, None);
        let mut client2a = TestClient::new(vec![2], 21, None);
        let mut client2b = TestClient::new(vec![2], 22, None);

        client1a.connect_join_and_wait_until_joined();
        client2a.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1a, &client2a]);

        let is_audio = true;
        let plaintext = &b"Fake Audio"[..];
        let ciphertext1a = client1a.encrypt_media(is_audio, plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2a
                .decrypt_media(client1a.demux_id, is_audio, &ciphertext1a)
                .unwrap()
        );

        // Make sure the advanced key gets sent to client2b even though it's the same user as 2a.
        client2b.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1a, &client2a, &client2b]);
        let ciphertext1a = client1a.encrypt_media(is_audio, plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2b
                .decrypt_media(client1a.demux_id, is_audio, &ciphertext1a)
                .unwrap()
        );
    }

    #[test]
    fn frame_encryption_someone_forging_demux_id() {
        let mut client1 = TestClient::new(vec![1], 1, None);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2, None);
        client2.connect_join_and_wait_until_joined();

        // Client3 is pretending to have demux ID 1 when sending media keys
        let mut client3 = TestClient::new(vec![3], 3, Some(1));
        client3.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2, &client3]);

        let is_audio = true;
        let plaintext = &b"Fake Audio"[..];
        let ciphertext1 = client1.encrypt_media(is_audio, plaintext).unwrap();
        let ciphertext3 = client3.encrypt_media(is_audio, plaintext).unwrap();
        // The forger doesn't mess anything up for the others
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, is_audio, &ciphertext1)
                .unwrap()
        );
        // And you can't decrypt from the forger.
        assert!(client2
            .decrypt_media(client3.demux_id, is_audio, &ciphertext3)
            .is_err());

        client1.disconnect_and_wait_until_ended();
        client2.disconnect_and_wait_until_ended();
        client3.disconnect_and_wait_until_ended();
    }

    #[test]
    fn remote_mute_states() {
        let client1 = TestClient::new(vec![1], 1, None);
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2, None);
        client2.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2]);

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(None, remote_devices2[0].audio_muted);
        assert_eq!(None, remote_devices2[0].video_muted);

        client1.client.set_outgoing_audio_muted(true);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(Some(true), remote_devices2[0].audio_muted);
        assert_eq!(None, remote_devices2[0].video_muted);

        client1.client.set_outgoing_video_muted(false);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(Some(true), remote_devices2[0].audio_muted);
        assert_eq!(Some(false), remote_devices2[0].video_muted);
    }

    fn hash_set<T: std::hash::Hash + Eq + Clone>(vals: impl IntoIterator<Item = T>) -> HashSet<T> {
        vals.into_iter().collect()
    }

    #[test]
    fn ignore_devices_that_arent_members() {
        let client = TestClient::new(vec![1], 1, None);
        client.connect_join_and_wait_until_joined();

        assert!(client.observer.remote_devices().is_empty());

        let peek_info = PeekInfo {
            devices:      vec![
                PeekDeviceInfo {
                    demux_id:        2,
                    user_id:         Some(b"2".to_vec()),
                    short_device_id: demux_id_to_short_device_id(2),
                    long_device_id:  demux_id_to_long_device_id(2),
                },
                PeekDeviceInfo {
                    demux_id:        3,
                    user_id:         None,
                    short_device_id: demux_id_to_short_device_id(3),
                    long_device_id:  demux_id_to_long_device_id(3),
                },
            ],
            creator:      None,
            era_id:       None,
            max_devices:  None,
            device_count: 3,
        };
        client.client.set_peek_info(Ok(peek_info));
        client.wait_for_client_to_process();

        let remote_devices = client.observer.remote_devices();
        assert_eq!(1, remote_devices.len());
        assert_eq!(2, remote_devices[0].demux_id);

        assert_eq!(vec![b"2".to_vec()], client.observer.joined_members());
    }

    #[test]
    fn joined_members() {
        // The peeker doesn't join
        let peeker = TestClient::new(vec![42], 42, None);
        peeker.client.connect();
        peeker.wait_for_client_to_process();

        assert_eq!(0, peeker.observer.joined_members().len());

        let joiner1 = TestClient::new(vec![1], 1, None);
        let joiner2 = TestClient::new(vec![2], 2, None);

        // The peeker sees updates to the joined members before joining
        peeker.set_remotes_and_wait_until_applied(&[&joiner1]);
        assert_eq!(
            vec![joiner1.user_id.clone()],
            peeker.observer.joined_members()
        );

        peeker.set_remotes_and_wait_until_applied(&[&joiner2]);
        assert_eq!(
            vec![joiner2.user_id.clone()],
            peeker.observer.joined_members()
        );

        peeker.set_remotes_and_wait_until_applied(&[&joiner1, &joiner2]);
        assert_eq!(
            hash_set(&[joiner1.user_id.clone(), joiner2.user_id.clone()]),
            hash_set(&peeker.observer.joined_members())
        );

        // Temporary clear the observer state so we can verify we don't get a
        // callback when nothing changes.
        peeker
            .observer
            .handle_peek_changed(0, &[], None, None, None, 0);
        assert_eq!(0, peeker.observer.joined_members().len());
        peeker.set_remotes_and_wait_until_applied(&[&joiner1, &joiner2]);
        assert_eq!(0, peeker.observer.joined_members().len());
        peeker.observer.handle_peek_changed(
            0,
            &[joiner1.user_id.clone(), joiner2.user_id.clone()],
            None,
            None,
            None,
            3,
        );

        peeker.set_remotes_and_wait_until_applied(&[]);
        assert_eq!(0, peeker.observer.joined_members().len());

        // And the peeker sees updates to the joined members before joining
        peeker.connect_join_and_wait_until_joined();

        peeker.set_remotes_and_wait_until_applied(&[&joiner2]);
        assert_eq!(
            vec![joiner2.user_id.clone()],
            peeker.observer.joined_members()
        );

        peeker.set_remotes_and_wait_until_applied(&[&joiner1, &joiner2]);
        assert_eq!(
            hash_set(&[joiner1.user_id.clone(), joiner2.user_id.clone()]),
            hash_set(&peeker.observer.joined_members())
        );

        peeker.set_remotes_and_wait_until_applied(&[]);
        assert_eq!(0, peeker.observer.joined_members().len());

        peeker.disconnect_and_wait_until_ended();
    }

    #[test]
    #[ignore] // Because it's too slow
    fn smart_polling() {
        let client1 = TestClient::new(vec![1], 1, None);
        let client2 = TestClient::new(vec![2], 2, None);

        assert_eq!(0, client1.sfu_client.request_count());

        // We don't query until we get a membership proof
        client1.client.connect();
        client1.wait_for_client_to_process();
        assert_eq!(0, client1.sfu_client.request_count());

        // Once we get a proof, we query immediately
        client1.client.set_membership_proof(b"proof".to_vec());
        client1.wait_for_client_to_process();

        // And when we join(), but only if it's been a while.
        // since we asked before.
        client1.client.join();
        client1.observer.joined.wait();
        assert_eq!(1, client1.sfu_client.request_count());
        client1.client.leave();
        std::thread::sleep(std::time::Duration::from_millis(1200));
        client1.client.join();
        // TODO: figure out a way to wait for a second join instead of sleeping.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(2, client1.sfu_client.request_count());
        client1.set_remotes_and_wait_until_applied(&[]);

        // Client2 learns about client1 and sends client crypto keys,
        // which causes client1 to request again.
        client2.connect_join_and_wait_until_joined();
        client2.set_remotes_and_wait_until_applied(&[&client1]);
        client1.wait_for_client_to_process();
        assert_eq!(3, client1.sfu_client.request_count());
        client1.set_remotes_and_wait_until_applied(&[]);

        // Client2 sends a heartbeat to client1
        // which causes client1 to request again.
        std::thread::sleep(std::time::Duration::from_millis(1000));
        assert_eq!(4, client1.sfu_client.request_count());
        client1.set_remotes_and_wait_until_applied(&[&client2]);

        // Client2 sends a leave message to client1
        // which causes client1 to request again.
        // But the SFU hasn't been update yet.
        client2.disconnect_and_wait_until_ended();
        assert_eq!(5, client1.sfu_client.request_count());
        client1.set_remotes_and_wait_until_applied(&[]);

        // Just in case the SFU was old, we request again around 2 seconds
        // after the leave message.
        std::thread::sleep(std::time::Duration::from_millis(2500));
        assert_eq!(6, client1.sfu_client.request_count());
        client1.set_remotes_and_wait_until_applied(&[]);

        // Make sure getting an updated membership proof doesn't mess anything up
        client1.client.set_membership_proof(b"proof".to_vec());
        std::thread::sleep(std::time::Duration::from_millis(5000));
        assert_eq!(6, client1.sfu_client.request_count());

        // And again after around 10 more seconds (infrequent polling).
        std::thread::sleep(std::time::Duration::from_millis(6000));
        assert_eq!(7, client1.sfu_client.request_count());
        client1.set_remotes_and_wait_until_applied(&[]);

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    #[ignore]
    fn polling_error_handling() {
        init_logging();
        let client = TestClient::new(vec![1], 1, None);
        client.client.set_membership_proof(b"proof".to_vec());
        client.connect_join_and_wait_until_joined();

        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(1, client.sfu_client.request_count());

        std::thread::sleep(std::time::Duration::from_millis(1000));
        assert_eq!(1, client.sfu_client.request_count());

        std::thread::sleep(std::time::Duration::from_millis(1000));
        assert_eq!(1, client.sfu_client.request_count());

        std::thread::sleep(std::time::Duration::from_millis(1000));
        assert_eq!(1, client.sfu_client.request_count());

        // Eventually, we give up on the lack of a response and ask again.
        std::thread::sleep(std::time::Duration::from_millis(1000));
        assert_eq!(2, client.sfu_client.request_count());

        client.disconnect_and_wait_until_ended();
    }

    #[test]
    #[ignore]
    fn request_video() {
        use protobuf::group_call::{
            device_to_sfu::{
                video_request_message::VideoRequest as VideoRequestProto,
                VideoRequestMessage,
            },
            DeviceToSfu,
        };

        let mut client1 = TestClient::new(vec![1], 1, None);
        let client2 = TestClient::new(vec![2], 2, None);
        let client3 = TestClient::new(vec![3], 3, None);
        let client4 = TestClient::new(vec![4], 4, None);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[&client2, &client3, &client4]);

        let requests = vec![
            VideoRequest {
                demux_id:  2,
                width:     1920,
                height:    1080,
                framerate: None,
            },
            VideoRequest {
                demux_id:  3,
                // Rotated!
                width:     80,
                height:    120,
                framerate: Some(5),
            },
            VideoRequest {
                demux_id:  4,
                width:     0,
                height:    0,
                framerate: None,
            },
            // This should be filtered out
            VideoRequest {
                demux_id:  5,
                width:     1000,
                height:    1000,
                framerate: None,
            },
        ];
        client1.client.request_video(requests.clone());
        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                video_request: Some(VideoRequestMessage {
                    requests: vec![
                        VideoRequestProto {
                            short_device_id: Some(demux_id_to_short_device_id(2)),
                            height:          Some(1080),
                        },
                        VideoRequestProto {
                            short_device_id: Some(demux_id_to_short_device_id(2)),
                            height:          Some(80),
                        },
                        VideoRequestProto {
                            short_device_id: Some(demux_id_to_short_device_id(2)),
                            height:          Some(0),
                        },
                    ],
                    max:      Some(2),
                }),
                ..DeviceToSfu::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.client.request_video(requests.clone());
        client1.client.request_video(requests.clone());
        client1.client.request_video(requests.clone());
        client1.client.request_video(requests.clone());

        let before = Instant::now();
        let _ = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("Get RTP packet to SFU");
        let elapsed = Instant::now() - before;
        assert!(elapsed > Duration::from_millis(980));
        assert!(elapsed < Duration::from_millis(1020));

        client1.client.request_video(requests.clone());
        client1.client.request_video(requests.clone());
        client1.client.request_video(requests.clone());
        client1.client.request_video(requests.clone());

        let before = Instant::now();
        let _ = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("Get RTP packet to SFU");
        let elapsed = Instant::now() - before;
        assert!(elapsed < Duration::from_millis(100));

        let before = Instant::now();
        let _ = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("Get RTP packet to SFU");
        let elapsed = Instant::now() - before;
        assert!(elapsed > Duration::from_millis(1000));

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn carry_over_devices_from_peeking_to_joined() {
        let client1 = TestClient::new(vec![1], 1, None);
        let client2 = TestClient::new(vec![2], 2, None);
        let client3 = TestClient::new(vec![3], 3, None);

        client1.client.set_membership_proof(b"proof".to_vec());
        client1.client.connect();
        client1.wait_for_client_to_process();

        client1.set_remotes_and_wait_until_applied(&[&client2, &client3]);
        assert_eq!(
            hash_set(vec![client2.user_id.clone(), client3.user_id.clone()]),
            hash_set(client1.observer.joined_members())
        );

        client1.client.join();
        client1.observer.joined.wait();
        client1.wait_for_client_to_process();
        let remote_devices = client1.observer.remote_devices();
        assert_eq!(2, remote_devices.len());
        assert_eq!(2, remote_devices[0].demux_id);
        assert_eq!(3, remote_devices[1].demux_id);
        assert_eq!(
            client1.observer.remote_devices(),
            client1.observer.remote_devices_at_join_time(),
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn era_id_populated_after_join() {
        let mut client1 = TestClient::new(vec![1], 1, None);

        client1.client.set_membership_proof(b"proof".to_vec());
        client1.client.connect();
        client1.wait_for_client_to_process();
        assert_eq!(None, client1.observer.peek_state().era_id);

        client1.default_peek_info = PeekInfo {
            era_id: Some("update me".to_string()),
            ..PeekInfo::default()
        };
        client1.set_remotes_and_wait_until_applied(&[]);
        assert_eq!(
            Some("update me"),
            client1.observer.peek_state().era_id.as_deref()
        );
        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn changing_group_members_triggers_poll() {
        let client1 = TestClient::new(vec![1], 1, None);
        client1.client.set_membership_proof(b"proof".to_vec());
        client1.client.connect();
        client1.wait_for_client_to_process();
        let initial_count = client1.sfu_client.request_count();
        let user_a = GroupMemberInfo {
            user_id:            b"a".to_vec(),
            user_id_ciphertext: b"A".to_vec(),
        };
        let user_b = GroupMemberInfo {
            user_id:            b"b".to_vec(),
            user_id_ciphertext: b"B".to_vec(),
        };
        client1.set_remotes_and_wait_until_applied(&[]);

        // Changing the list of group members triggers a poll
        client1
            .client
            .set_group_members(vec![user_a.clone(), user_b.clone()]);
        client1.wait_for_client_to_process();
        assert_eq!(initial_count + 1, client1.sfu_client.request_count());
        client1.set_remotes_and_wait_until_applied(&[]);

        // Setting the same list again - even in a different order - does not trigger a poll
        client1
            .client
            .set_group_members(vec![user_b.clone(), user_a.clone()]);
        client1.wait_for_client_to_process();
        assert_eq!(initial_count + 1, client1.sfu_client.request_count());

        // Setting a different list triggers a poll
        client1.client.set_group_members(vec![user_a.clone()]);
        client1.wait_for_client_to_process();
        assert_eq!(initial_count + 2, client1.sfu_client.request_count());

        client1.set_remotes_and_wait_until_applied(&[]);

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn full_call() {
        let client1 = TestClient::new(vec![1], 1, None);
        client1.client.connect();
        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      vec![PeekDeviceInfo {
                demux_id:        2,
                user_id:         None,
                short_device_id: demux_id_to_short_device_id(2),
                long_device_id:  demux_id_to_long_device_id(2),
            }],
            device_count: 1,
            max_devices:  Some(1),
            creator:      None,
            era_id:       None,
        }));
        client1.client.join();
        assert_eq!(EndReason::HasMaxDevices, client1.observer.ended.wait());

        let client1 = TestClient::new(vec![1], 1, None);
        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      vec![PeekDeviceInfo {
                demux_id:        2,
                user_id:         None,
                short_device_id: demux_id_to_short_device_id(2),
                long_device_id:  demux_id_to_long_device_id(2),
            }],
            device_count: 1,
            max_devices:  Some(2),
            creator:      None,
            era_id:       None,
        }));
        client1.connect_join_and_wait_until_joined();
        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn speakers() {
        let client1 = TestClient::new(vec![1], 1, None);
        let client2 = TestClient::new(vec![2], 2, None);
        let client3 = TestClient::new(vec![3], 3, None);
        let client4 = TestClient::new(vec![4], 4, None);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[&client3, &client4]);
        assert_eq!(vec![3, 4], client1.speakers());

        // New people put at the end regardless of DemuxId
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.set_remotes_and_wait_until_applied(&[&client2, &client4, &client3]);
        assert_eq!(vec![3, 4, 2], client1.speakers());

        // Changed
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(1, 4);
        assert_eq!(vec![4, 3, 2], client1.speakers());

        // Didn't change
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(2, 4);
        assert_eq!(vec![4, 3, 2], client1.speakers());

        // Changed back
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(3, 3);
        assert_eq!(vec![3, 4, 2], client1.speakers());

        // Ignore unknown demux ID
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(4, 5);
        assert_eq!(vec![3, 4, 2], client1.speakers());

        // Didn't change
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(6, 3);
        assert_eq!(vec![3, 4, 2], client1.speakers());

        // Ignore old messages
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(5, 4);
        assert_eq!(vec![3, 4, 2], client1.speakers());

        // Ignore when the local device is the current speaker
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(7, 1);
        assert_eq!(vec![3, 4, 2], client1.speakers());

        // Finally give 2 a chance
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(8, 2);
        assert_eq!(vec![2, 3, 4], client1.speakers());

        // Swap only the top two; leave the third alone
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(9, 3);
        assert_eq!(vec![3, 2, 4], client1.speakers());

        // Unchanged
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(10, 3);
        assert_eq!(vec![3, 2, 4], client1.speakers());

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn send_bitrate() {
        init_logging();
        let client1 = TestClient::new(vec![1], 1, None);
        client1.connect_join_and_wait_until_joined();
        assert_eq!(
            Some(DataRate::from_kbps(1)),
            client1.observer.max_send_bitrate()
        );

        let devices: Vec<PeekDeviceInfo> = (1..=20)
            .map(|demux_id| {
                let user_id = format!("{}", demux_id);
                PeekDeviceInfo {
                    demux_id,
                    user_id: Some(user_id.as_bytes().to_vec()),
                    short_device_id: demux_id_to_short_device_id(demux_id),
                    long_device_id: demux_id_to_long_device_id(demux_id),
                }
            })
            .collect();
        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      vec![],
            device_count: 0,
            max_devices:  None,
            creator:      None,
            era_id:       None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(1)),
            client1.observer.max_send_bitrate()
        );

        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      (&devices[..1]).to_vec(),
            device_count: 1,
            max_devices:  None,
            creator:      None,
            era_id:       None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(1)),
            client1.observer.max_send_bitrate()
        );

        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      (&devices[..2]).to_vec(),
            device_count: 1,
            max_devices:  None,
            creator:      None,
            era_id:       None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(1000)),
            client1.observer.max_send_bitrate()
        );

        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      (&devices[..5]).to_vec(),
            device_count: 5,
            max_devices:  None,
            creator:      None,
            era_id:       None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(1000)),
            client1.observer.max_send_bitrate()
        );

        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      (&devices[..20]).to_vec(),
            device_count: 20,
            max_devices:  None,
            creator:      None,
            era_id:       None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(500)),
            client1.observer.max_send_bitrate()
        );

        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      (&devices[..1]).to_vec(),
            device_count: 1,
            max_devices:  None,
            creator:      None,
            era_id:       None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(1)),
            client1.observer.max_send_bitrate()
        );

        client1
            .client
            .set_max_send_bitrate(DataRate::from_kbps(2000));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(2000)),
            client1.observer.max_send_bitrate()
        );
        client1.client.set_peek_info(Ok(PeekInfo {
            devices:      (&devices[..1]).to_vec(),
            device_count: 1,
            max_devices:  None,
            creator:      None,
            era_id:       None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(DataRate::from_kbps(2000)),
            client1.observer.max_send_bitrate()
        );

        client1.disconnect_and_wait_until_ended();
    }
}
