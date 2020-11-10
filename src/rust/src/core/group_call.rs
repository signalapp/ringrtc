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
    time::{Duration, Instant},
};

use bytes::{Bytes, BytesMut};
use prost::Message;
use rand::Rng;

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

    // The following notify the observer of state changes to the remote devices.
    fn handle_remote_devices_changed(
        &self,
        client_id: ClientId,
        remote_devices: &[RemoteDeviceState],
    );

    // NOT IMPLEMENTED
    fn handle_joined_members_changed(&self, client_id: ClientId, joined_members: &[UserId]);

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
// But updates to members joined (via handle_joined_members_changed)
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JoinState {
    /// Join() has not yet been called
    /// or leave() has been called
    /// or join() was called but failed.
    NotJoined,

    /// Join() has been called but a response from the SFU is pending.
    Joining,

    /// Join() has been called and a response from the SFU has been received.
    /// and a DemuxId has been assigned.
    Joined(DemuxId),
}

// The info about SFU needed in order to connect to it.
#[derive(Clone, Debug)]
pub struct SfuInfo {
    pub udp_addresses:    Vec<SocketAddr>,
    pub ice_ufrag:        String,
    pub ice_pwd:          String,
    pub dtls_fingerprint: DtlsFingerprint,
}

#[repr(C)]
#[derive(Debug)]
pub enum EndReason {
    // Normal events
    DeviceExplicitlyDisconnected = 0,
    ServerExplicitlyDisconnected,

    // Things that can go wrong
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
}

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
    // This should call Client.set_remote_devices whenever the client has updates
    // (including even when it hasn't been requested)
    fn request_remote_devices(&mut self, client: Client);

    // Notifies the client of the new membership proof.
    fn set_membership_proof(&mut self, proof: MembershipProof);
    fn set_group_members(&mut self, members: Vec<GroupMemberInfo>);
    fn leave(&mut self);
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
    pub demux_id:           DemuxId,
    pub user_id:            UserId,
    pub audio_muted:        Option<bool>,
    pub video_muted:        Option<bool>,
    // The latest timestamp we received from an update to
    // audio_muted and video_muted.
    muted_rtp_timestamp:    Option<u32>,
    // NOT IMPLEMENTED
    pub speaker_index:      Option<u16>,
    // NOT IMPLEMENTED
    pub video_aspect_ratio: Option<f32>,
    // NOT IMPLEMENTED
    pub audio_level:        Option<u16>,
}

impl RemoteDeviceState {
    pub fn new(demux_id: DemuxId, user_id: UserId) -> Self {
        Self {
            demux_id,
            user_id,
            audio_muted: None,
            video_muted: None,
            muted_rtp_timestamp: None,
            video_aspect_ratio: None,
            audio_level: None,
            // Not implemented yet
            speaker_index: None,
        }
    }
}

/// These can be sent to the SFU to request different resolutions of
/// video for different remote dem
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
const RTP_DATA_SSRC_OFFSET: rtp::Ssrc = 0xD;

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
    client_id:                  ClientId,
    group_id:                   GroupId,
    sfu_client:                 Box<dyn SfuClient>,
    observer:                   Box<dyn Observer>,
    // State that changes regularly and is sent to the observer
    connection_state:           ConnectionState,
    join_state:                 JoinState,
    // These are unset until the app sets them.
    // But we err on the side of caution and don't send anything when they are unset.
    outgoing_audio_muted:       Option<bool>,
    outgoing_video_muted:       Option<bool>,
    remote_devices:             Vec<RemoteDeviceState>,
    remote_devices_update_time: Option<Instant>,

    // Things for controlling the PeerConnection
    local_ice_ufrag:               String,
    local_ice_pwd:                 String,
    local_dtls_fingerprint:        DtlsFingerprint,
    sfu_info:                      Option<SfuInfo>,
    peer_connection:               PeerConnection,
    peer_connection_observer_impl: Box<PeerConnectionObserverImpl>,
    rtp_data_next_seqnum:          u32,

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

                let peer_connection_factory = PeerConnectionFactory::new(
                    false, /* use_injectable network */
                )
                .map_err(|e| {
                    observer
                        .handle_ended(client_id, EndReason::FailedToCreatePeerConnectionFactory);
                    e
                })?;
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
                    local_ice_ufrag,
                    local_ice_pwd,

                    connection_state: ConnectionState::NotConnected,
                    join_state: JoinState::NotJoined,
                    outgoing_audio_muted: None,
                    outgoing_video_muted: None,
                    remote_devices: Vec::new(),
                    remote_devices_update_time: None,

                    local_dtls_fingerprint,
                    sfu_info: None,
                    peer_connection_observer_impl,
                    peer_connection,
                    rtp_data_next_seqnum: 1,

                    next_stats_time: None,
                    stats_observer: create_stats_observer(),

                    frame_crypto_context,
                    pending_media_receive_keys: Vec::new(),
                    media_send_key_rotation_state: KeyRotationState::Applied,

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
    fn tick(state: &mut State, callback: Client) {
        let now = Instant::now();

        debug!(
            "group_call::Client(inner)::tick(group_id: {})",
            state.client_id
        );

        let remote_devices_need_update = match state.remote_devices_update_time {
            None => true,
            Some(remote_devices_update_time) => {
                let age = now - remote_devices_update_time;
                age > Duration::from_secs(2)
            }
        };
        if remote_devices_need_update {
            state.sfu_client.request_remote_devices(callback.clone());
        }

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

        state
            .actor
            .send_delayed(Duration::from_secs(TICK_INTERVAL_SECS), move |state| {
                Self::tick(state, callback)
            });
    }

    pub fn connect(&self) {
        debug!(
            "group_call::Client(outer)::connect(client_id: {})",
            self.client_id
        );
        let tick_callback = self.clone();
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

                    Self::tick(state, tick_callback);
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
                JoinState::Joined(_) => {
                    warn!("Can't join when already joined.");
                }
                JoinState::Joining => {
                    warn!("Can't join when already joining.");
                }
                JoinState::NotJoined => {
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
                }
            }
        });
    }

    // Pulled into a named private method because it might be called by leave_inner().
    fn set_join_state_and_notify_observer(state: &mut State, join_state: JoinState) {
        debug!(
            "group_call::Client(inner)::set_join_state_and_notify_observer(client_id: {})",
            state.client_id
        );
        state.join_state = join_state;
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
            "group_call::Client(inner)::leave(client_id: {})",
            state.client_id
        );

        match state.join_state {
            JoinState::NotJoined => {
                warn!("Can't leave when not joined.");
            }
            JoinState::Joining | JoinState::Joined(_) => {
                state.peer_connection.set_outgoing_media_enabled(false);
                state.peer_connection.set_incoming_media_enabled(false);

                Self::set_join_state_and_notify_observer(state, JoinState::NotJoined);
                state.next_stats_time = None;
                state.sfu_client.leave();
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

    pub fn set_max_send_bitrate(&self, max_send_rate: DataRate) {
        debug!(
            "group_call::Client(outer)::set_max_send_rate(client_id: {}, max_send_rate: {:?})",
            self.client_id, max_send_rate
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_max_send_rate(client_id: {}, max_send_rate: {:?})",
                state.client_id, max_send_rate
            );

            if state
                .peer_connection
                .set_max_send_bitrate(max_send_rate)
                .is_err()
            {
                Self::end(state, EndReason::FailedToSetMaxSendBitrate);
            }
        });
    }

    pub fn set_rendered_resolutions(&self, _requests: Vec<VideoRequest>) {
        debug!(
            "group_call::Client(outer)::set_rendered_resolutions(client_id: {})",
            self.client_id
        );
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
            state.sfu_client.set_group_members(group_members);
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
            JoinState::Joined(_) | JoinState::Joining => true,
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
    pub fn on_sfu_client_joined(&self, result: Result<(SfuInfo, DemuxId)>) {
        debug!(
            "group_call::Client(outer)::on_sfu_client_joined(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::on_sfu_client_joined(client_id: {})",
                state.client_id
            );

            if let Ok((sfu_info, local_demux_id)) = result {
                if state.sfu_info.is_some() {
                    warn!("The SFU completed joining more than once.");
                    return;
                }
                match state.join_state {
                    JoinState::NotJoined => {
                        warn!("The SFU completed joining before join() was requested.");
                        return;
                    }
                    JoinState::Joining => {
                        Self::set_join_state_and_notify_observer(
                            state,
                            JoinState::Joined(local_demux_id),
                        );
                        state.next_stats_time =
                            Some(Instant::now() + Duration::from_secs(STATS_INTERVAL_SECS));
                    }
                    JoinState::Joined(_) => {
                        warn!("The SFU completed joining more than once.");
                        return;
                    }
                }
                match state.connection_state {
                    ConnectionState::NotConnected => {
                        warn!("The SFU completed joining before connect() was requested.");
                    }
                    ConnectionState::Connecting => {
                        if Self::start_peer_connection(state, &sfu_info, local_demux_id).is_err() {
                            Self::end(state, EndReason::FailedToStartPeerConnection);
                        };

                        state.sfu_info = Some(sfu_info);
                    }
                    ConnectionState::Connected | ConnectionState::Reconnecting => {
                        warn!("The SFU completed joining after already being connected.");
                    }
                }
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

    pub fn update_remote_devices(&self, id_pairs: Vec<(DemuxId, UserId)>) {
        self.actor.send(move |state| {
            if let JoinState::Joined(local_demux_id) = state.join_state {
                // We remember these before changing state.remote_devices so we can calculate changes after.
                let old_user_ids: HashSet<UserId> = state
                    .remote_devices
                    .iter()
                    .map(|rd| rd.user_id.clone())
                    .collect();
                let old_demux_ids: HashSet<DemuxId> =
                    state.remote_devices.iter().map(|rd| rd.demux_id).collect();
                let old_update_time = state.remote_devices_update_time;

                // Then we update state.remote_devices by first building a map of id_pair => RemoteDeviceState
                // from the old values and then building a new Vec using either the old value (if there is one)
                // or creating a new one.
                let mut old_remote_devices_by_id_pair: HashMap<
                    (DemuxId, UserId),
                    RemoteDeviceState,
                > = std::mem::replace(&mut state.remote_devices, Vec::new())
                    .into_iter()
                    .map(|rd| ((rd.demux_id, rd.user_id.clone()), rd))
                    .collect();
                state.remote_devices = id_pairs
                    .into_iter()
                    .flat_map(|(demux_id, user_id)| {
                        if demux_id == local_demux_id {
                            // Don't add a remote device to represent the local device.
                            return None;
                        }
                        Some(
                            // Keep the old one, with its state, if there is one.
                            match old_remote_devices_by_id_pair.remove(&(demux_id, user_id.clone()))
                            {
                                Some(existing_remote_device) => existing_remote_device,
                                None => RemoteDeviceState::new(demux_id, user_id),
                            },
                        )
                    })
                    .collect();
                // Even if nothing changed, we remember that we have updated it so we don't get in a
                // hot loop polling it.
                state.remote_devices_update_time = Some(Instant::now());

                // Recalculate to see the differences
                let new_user_ids: HashSet<UserId> = state
                    .remote_devices
                    .iter()
                    .map(|rd| rd.user_id.clone())
                    .collect();
                let new_demux_ids: HashSet<DemuxId> =
                    state.remote_devices.iter().map(|rd| rd.demux_id).collect();

                let demux_ids_changed = old_demux_ids != new_demux_ids;
                // If demux IDs changed, let the PeerConnection know that related SSRCs changed as well
                if demux_ids_changed {
                    if let Some(sfu_info) = state.sfu_info.as_ref() {
                        let new_demux_ids: Vec<DemuxId> = new_demux_ids.into_iter().collect();
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

                // Note: if the first call to set_remote_devices is [], we still fire the
                // handle_remote_devices_changed to ensure the observer can tell the difference
                // between "we know we have no remote devices" and "we don't know what we have yet".
                if demux_ids_changed || old_update_time.is_none() {
                    state
                        .observer
                        .handle_remote_devices_changed(state.client_id, &state.remote_devices);
                }

                // If someone was added, we must advance the send media key
                // and send it to everyone that was added.
                let user_ids_added: Vec<&UserId> = new_user_ids.difference(&old_user_ids).collect();
                if !user_ids_added.is_empty() {
                    Self::advance_media_send_key_and_send_to_users_added(
                        state,
                        &user_ids_added[..],
                    );
                    Self::send_pending_media_send_key_to_users_added(state, &user_ids_added[..]);
                }

                // If someone was removed, we must reset the send media key and send it to everyone not removed.
                let user_ids_removed: Vec<&UserId> =
                    old_user_ids.difference(&new_user_ids).collect();
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
            }
        });
    }

    // Pulled into a named private method because it might be called by set_remote_devices
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
                info!("Waiting to generate a new media send key until after the pending one has been applied.  Client ID: {}", state.client_id);

                state.media_send_key_rotation_state = KeyRotationState::Pending {
                    secret,
                    needs_another_rotation: true,
                }
            }
            KeyRotationState::Applied => {
                info!("Generating a new random media send key because a user has been removed.  Client ID: {}", state.client_id);

                // First generate a new key, then wait some time, and then apply it.
                let ratchet_counter: frame_crypto::RatchetCounter = 0;
                let secret = frame_crypto::random_secret(&mut rand::rngs::OsRng);

                if let JoinState::Joined(local_demux_id) = state.join_state {
                    let user_ids: HashSet<UserId> = state
                        .remote_devices
                        .iter()
                        .map(|rd| rd.user_id.clone())
                        .collect();
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
                        info!("Applying the new send key.  Client ID: {}", state.client_id);
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

    fn advance_media_send_key_and_send_to_users_added(
        state: &mut State,
        user_ids_not_removed: &[&UserId],
    ) {
        info!(
            "Advancing current media send key because a user has been added.  Client ID: {}",
            state.client_id
        );

        let (ratchet_counter, secret) = {
            let mut frame_crypto_context = state
                .frame_crypto_context
                .lock()
                .expect("Get lock for frame encryption context to advance media send key");
            frame_crypto_context.advance_send_ratchet()
        };
        if let JoinState::Joined(local_demux_id) = state.join_state {
            for &user_id in user_ids_not_removed.iter() {
                Self::send_media_send_key_to_user_over_signaling(
                    state,
                    user_id.clone(),
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
            .iter()
            .find(|device| device.demux_id == demux_id)
        {
            if device.user_id == user_id {
                info!(
                    "Adding media receive key from {}. Client ID: {}",
                    device.demux_id, state.client_id
                );
                let mut frame_crypto_context = state
                    .frame_crypto_context
                    .lock()
                    .expect("Get lock for frame encryption context to add media receive key");
                frame_crypto_context.add_receive_secret(demux_id, ratchet_counter, secret);
            } else {
                warn!("Ignoring received media key from {:?} because the demux ID {} doesn't make sense.", user_id, demux_id);
            }
        } else {
            // We still don't know what this device is, so wait again until we do.
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
        info!(
            "send_media_send_key_to_user_over_signaling(): recipient user ID: {:?}",
            recipient_id
        );
        let mut message = protobuf::group_call::DeviceToDevice::default();
        let mut media_key = protobuf::group_call::device_to_device::MediaKey::default();
        media_key.demux_id = Some(local_demux_id);
        media_key.ratchet_counter = Some(ratchet_counter as u32);
        media_key.secret = Some(secret.to_vec());
        message.group_id = Some(state.group_id.clone());
        message.media_key = Some(media_key);

        state.observer.send_signaling_message(recipient_id, message);
    }

    fn send_pending_media_send_key_to_users_added(state: &mut State, user_ids_added: &[&UserId]) {
        if let JoinState::Joined(local_demux_id) = state.join_state {
            if let KeyRotationState::Pending { secret, .. } = state.media_send_key_rotation_state {
                for &user_id in user_ids_added {
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
        let heartbeat_msg = encode_proto({
            let mut msg = protobuf::group_call::DeviceToDevice::default();
            msg.heartbeat = {
                let mut heartbeat = protobuf::group_call::device_to_device::Heartbeat::default();
                heartbeat.audio_muted = state.outgoing_audio_muted;
                heartbeat.video_muted = state.outgoing_video_muted;
                Some(heartbeat)
            };
            msg
        })?;
        Self::broadcast_data_through_sfu(state, &heartbeat_msg)
    }

    fn broadcast_data_through_sfu(state: &mut State, message: &[u8]) -> Result<()> {
        if let JoinState::Joined(local_demux_id) = state.join_state {
            let message = Self::encrypt_data(state, message)?;
            let seqnum = state.rtp_data_next_seqnum;
            state.rtp_data_next_seqnum = state.rtp_data_next_seqnum.wrapping_add(1);

            let header = rtp::Header {
                pt:        RTP_DATA_PAYLOAD_TYPE,
                ssrc:      local_demux_id.saturating_add(RTP_DATA_SSRC_OFFSET),
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

    fn handle_rtp_received(&self, header: rtp::Header, payload: &[u8]) {
        if header.pt == RTP_DATA_PAYLOAD_TYPE {
            let demux_id = header.ssrc.saturating_sub(RTP_DATA_SSRC_OFFSET);
            if let Ok(payload) = self.decrypt_data(demux_id, payload) {
                if let Ok(msg) = protobuf::group_call::DeviceToDevice::decode(&payload[..]) {
                    if let Some(heartbeat) = msg.heartbeat {
                        self.handle_heartbeat_received(demux_id, header.timestamp, heartbeat);
                    }
                } else {
                    warn!(
                        "Ignoring received RTP data because decoding failed.  demux_id: {}",
                        demux_id,
                    );
                }
            } else {
                warn!(
                    "Ignoring received RTP data because decryption failed.  demux_id: {}",
                    demux_id,
                );
            }
        } else {
            warn!(
                "Ignoring received RTP data with unknown payload type: {}",
                header.pt
            );
        }
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
                    "Ignoring received heartbeat for unknown DemuxId {}",
                    demux_id
                );
            }
        });
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
    use std::sync::{Arc, Condvar, Mutex};

    struct FakeSfuClient {
        sfu_info:       SfuInfo,
        local_demux_id: DemuxId,
    }

    impl FakeSfuClient {
        fn new(sfu_info: SfuInfo, local_demux_id: DemuxId) -> Self {
            Self {
                sfu_info,
                local_demux_id,
            }
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
            client.on_sfu_client_joined(Ok((self.sfu_info.clone(), self.local_demux_id)));
        }
        fn request_remote_devices(&mut self, _client: Client) {}
        fn set_group_members(&mut self, _members: Vec<GroupMemberInfo>) {}
        fn set_membership_proof(&mut self, _proof: MembershipProof) {}
        fn leave(&mut self) {}
    }

    // TODO: Put this in common util area?
    #[derive(Clone, Default)]
    struct Waitable<T> {
        val:  Arc<Mutex<Option<T>>>,
        cvar: Arc<Condvar>,
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

    #[derive(Clone)]
    struct FakeObserver {
        // For sending messages
        user_id:    UserId,
        recipients: Arc<CallMutex<Vec<TestClient>>>,

        joined:         Event,
        remote_devices: Arc<CallMutex<Vec<RemoteDeviceState>>>,
        ended:          Event,
    }

    impl FakeObserver {
        fn new(user_id: UserId) -> Self {
            Self {
                user_id,
                recipients: Arc::new(CallMutex::new(Vec::new(), "FakeObserver recipients")),
                joined: Event::default(),
                remote_devices: Arc::new(CallMutex::new(Vec::new(), "FakeObserver remote devices")),
                ended: Event::default(),
            }
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
                .expect("Lock recipients to add recipient");
            remote_devices.iter().cloned().collect()
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
            if let JoinState::Joined(_) = join_state {
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
        fn handle_joined_members_changed(&self, _client_id: ClientId, _joined_members: &[UserId]) {}
        fn send_signaling_message(
            &mut self,
            recipient_id: UserId,
            message: protobuf::group_call::DeviceToDevice,
        ) {
            let recipients = self
                .recipients
                .lock()
                .expect("Lock recipients to add recipient");
            for recipient in recipients.iter() {
                if recipient.user_id == recipient_id {
                    recipient
                        .client
                        .on_signaling_message_received(self.user_id.clone(), message.clone());
                }
            }
        }
        fn handle_incoming_video_track(
            &mut self,
            _client_id: ClientId,
            _remote_demux_id: DemuxId,
            _incoming_video_track: VideoTrack,
        ) {
        }
        fn handle_ended(&self, _client_id: ClientId, _reason: EndReason) {
            self.ended.set();
        }
    }

    #[derive(Clone)]
    struct TestClient {
        user_id:  UserId,
        demux_id: DemuxId,
        observer: FakeObserver,
        client:   Client,
    }

    impl TestClient {
        fn new(user_id: UserId, demux_id: DemuxId, forged_demux_id: Option<DemuxId>) -> Self {
            let sfu_client = Box::new(FakeSfuClient::new(
                SfuInfo {
                    udp_addresses:    Vec::new(),
                    ice_ufrag:        "fake ICE ufrag".to_string(),
                    ice_pwd:          "fake ICE pwd".to_string(),
                    dtls_fingerprint: DtlsFingerprint::default(),
                },
                forged_demux_id.unwrap_or(demux_id),
            ));
            let observer = FakeObserver::new(user_id.clone());
            let fake_audio_track = AudioTrack::owned(FAKE_AUDIO_TRACK as *const u32);
            let client = Client::start(
                b"fake group ID".to_vec(),
                demux_id,
                sfu_client,
                Box::new(observer.clone()),
                fake_audio_track,
                None,
            )
            .expect("Start Client");
            Self {
                user_id,
                demux_id,
                observer,
                client,
            }
        }

        fn connect_join_and_wait_until_joined(&self) {
            self.client.connect();
            self.client.join();
            self.observer.joined.wait();
        }

        fn set_remotes_and_wait_until_applied(&self, clients: Vec<TestClient>) {
            let ids = clients
                .iter()
                .map(|client| (client.demux_id, client.user_id.clone()))
                .collect();
            self.observer
                .set_recipients(clients.iter().cloned().collect());
            self.client.update_remote_devices(ids);
            let local_demux_id = self.demux_id;
            self.client.actor.send(move |state| {
                state
                    .peer_connection
                    .set_rtp_packet_sink(Box::new(move |header, payload| {
                        for client in &clients {
                            if client.demux_id != local_demux_id {
                                client.client.handle_rtp_received(header.clone(), payload)
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

        fn disconnect_and_wait_until_ended(&self) {
            self.client.disconnect();
            self.observer.ended.wait();
        }
    }

    #[allow(dead_code)]
    fn init_logging() {
        env_logger::builder()
            .is_test(true)
            .filter(None, log::LevelFilter::Info)
            .init();
    }

    fn set_group_and_wait_until_applied(clients: &[&TestClient]) {
        for client in clients {
            // We're going to be lazy and not remove ourselves.  It shouldn't matter.
            let other_clients = clients.iter().cloned().cloned().collect();
            client.set_remotes_and_wait_until_applied(other_clients);
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

        client2.set_remotes_and_wait_until_applied(vec![client1.clone()]);

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

        client1.set_remotes_and_wait_until_applied(vec![client2.clone()]);
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
}
