//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    collections::{HashMap, HashSet},
    convert::TryInto,
    hash::{Hash, Hasher},
    iter::FromIterator,
    mem::size_of,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use anyhow;
use hkdf::Hkdf;
use mrp::{MrpReceiveError, MrpSendError, MrpStream};
use num_enum::TryFromPrimitive;
use prost::Message;
use rand::{Rng, rngs::OsRng};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey};
use zkgroup::{
    Timestamp,
    groups::{GroupSendEndorsementsResponse, UuidCiphertext},
};

use crate::{
    common::{
        CallEndReason, CallId, DataMode, Result,
        actor::{Actor, Stopper},
        units::DataRate,
    },
    core::{
        call_mutex::CallMutex,
        call_summary::{CallSummary, GroupCallSummary},
        crypto as frame_crypto,
        crypto::DecryptionErrorStats,
        endorsements::{EndorsementUpdateError, EndorsementUpdateResultRef, EndorsementsCache},
        signaling,
        util::uuid_to_string,
    },
    error::RingRtcError,
    lite::{
        call_links::CallLinkEpoch,
        http,
        sfu::{
            self, ClientStatus, DemuxId, GroupMember, MemberMap, MembershipProof,
            ObfuscatedResolver, PeekInfo, PeekResult, PeekResultCallback, UserId,
        },
    },
    protobuf::{
        self,
        group_call::{
            DeviceToSfu, SfuToDevice,
            sfu_to_device::{DeviceJoinedOrLeft, SendEndorsementsResponse},
        },
    },
    webrtc::{
        self,
        media::{
            AudioEncoderConfig, AudioTrack, VideoFrame, VideoFrameMetadata, VideoSink, VideoTrack,
        },
        peer_connection::{AudioLevel, PeerConnection, Protocol, ReceivedAudioLevel, SendRates},
        peer_connection_factory::{self as pcf, AudioJitterBufferConfig, PeerConnectionFactory},
        peer_connection_observer::{
            IceConnectionState, NetworkRoute, PeerConnectionObserver, PeerConnectionObserverTrait,
        },
        rtp,
        rtp_observer::{RffiRtpObserver, RtpObserver, RtpObserverTrait},
        sdp_observer::{
            SessionDescription, SrtpCryptoSuite, SrtpKey, create_csd_observer, create_ssd_observer,
        },
        stats_observer::{StatsObserver, create_stats_observer},
    },
};

// Each instance of a group_call::Client has an ID for logging and passing events
// around (such as callbacks to the Observer).  It's just very convenient to have.
pub type ClientId = u32;
// Group UUID
pub type GroupId = Vec<u8>;
pub type GroupIdRef<'a> = &'a [u8];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingId(i64);

impl RingId {
    pub fn from_era_id(era_id: &str) -> Self {
        // Happy path: 16 hex digits
        if era_id.len() == 16
            && let Ok(i) = u64::from_str_radix(era_id, 16)
        {
            // We reserve 0 as an invalid ring ID; treat it as the equally-unlikely -1.
            // This does make -1 twice as likely! Out of 2^64 - 1 possibilities.
            if i == 0 {
                return Self(-1);
            }
            return Self(i as i64);
        }
        // Sad path: arbitrary strings get a truncated hash as their ring ID.
        // We have no current plans to change era IDs from being 16 hex digits,
        // but nothing enforces this today, and we may want to change them in the future.
        let truncated_hash: [u8; 8] = Sha256::digest(era_id.as_bytes()).as_slice()[..8]
            .try_into()
            .unwrap();
        Self(i64::from_le_bytes(truncated_hash))
    }
}

impl From<i64> for RingId {
    fn from(raw_id: i64) -> Self {
        Self(raw_id)
    }
}

impl From<RingId> for i64 {
    fn from(id: RingId) -> Self {
        id.0
    }
}

impl std::fmt::Display for RingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingUpdate {
    /// The sender is trying to ring this user.
    Requested = 0,
    /// The sender tried to ring this user, but it's been too long.
    ExpiredRequest,
    /// Call was accepted elsewhere by a different device.
    AcceptedOnAnotherDevice,
    /// Call was declined elsewhere by a different device.
    DeclinedOnAnotherDevice,
    /// This device is currently on a different call.
    BusyLocally,
    /// A different device is currently on a different call.
    BusyOnAnotherDevice,
    /// The sender cancelled the ring request.
    CancelledByRinger,
}

/// Describes why a ring was cancelled.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
pub enum RingCancelReason {
    /// The user explicitly clicked "Decline".
    DeclinedByUser = 0,
    /// The device is busy with another call.
    Busy,
}

/// Indicates whether a signaling message should be marked for immediate processing
/// even if the receiving app isn't running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalingMessageUrgency {
    Droppable,
    HandleImmediately,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SrtpKeys {
    client: SrtpKey,
    server: SrtpKey,
}

impl SrtpKeys {
    const SUITE: SrtpCryptoSuite = SrtpCryptoSuite::AeadAes128Gcm;
    const KEY_LEN: usize = Self::SUITE.key_size();
    const SALT_LEN: usize = Self::SUITE.salt_size();
    const MASTER_KEY_MATERIAL_LEN: usize =
        Self::KEY_LEN + Self::SALT_LEN + Self::KEY_LEN + Self::SALT_LEN;

    fn from_master_key_material(master_key_material: &[u8; Self::MASTER_KEY_MATERIAL_LEN]) -> Self {
        Self {
            client: SrtpKey {
                suite: Self::SUITE,
                key: master_key_material[..Self::KEY_LEN].to_vec(),
                salt: master_key_material[Self::KEY_LEN..][..Self::SALT_LEN].to_vec(),
            },
            server: SrtpKey {
                suite: SrtpCryptoSuite::AeadAes128Gcm,
                key: master_key_material[Self::KEY_LEN..][Self::SALT_LEN..][..Self::KEY_LEN]
                    .to_vec(),
                salt: master_key_material[Self::KEY_LEN..][Self::SALT_LEN..][Self::KEY_LEN..]
                    [..Self::SALT_LEN]
                    .to_vec(),
            },
        }
    }
}

pub const INVALID_CLIENT_ID: ClientId = 0;

// The minimum level of sound to detect as "likely speaking" if we get consistently above this level
// for a minimum amount of time.
// AudioLevel can go up to ~32k, and even quiet sounds (e.g. a mouse click) can empirically cause
// audio levels up to ~400.
// In an unscientific test, even soft speaking with a distant microphone easily gets levels of 2000.
// So, use 1000 as a cutoff for "silence".
const MIN_NON_SILENT_LEVEL: AudioLevel = 1000;
// How often to poll for speaking/silence.
const SPEAKING_POLL_INTERVAL: Duration = Duration::from_millis(200);
// The amount of time with audio at or below `MIN_NON_SILENT_LEVEL` before we consider the
// user as having stopped speaking, rather than pausing.
// This should be less than MIN_SPEAKING_HAND_LOWER, or it won't be effective.
const STOPPED_SPEAKING_DURATION: Duration = Duration::from_secs(3);
// Amount of "continuous" speech (i.e., with gaps no longer than `STOPPED_SPEAKING_DURATION`)
// after which we suggest lowering a raised hand.
const MIN_SPEAKING_HAND_LOWER: Duration = Duration::from_secs(5);

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SpeechEvent {
    StoppedSpeaking = 0,
    LowerHandSuggestion,
}

impl SpeechEvent {
    pub fn ordinal(&self) -> i32 {
        // Must be kept in sync with the Java, Swift, and TypeScript enums.
        match self {
            SpeechEvent::StoppedSpeaking => 0,
            SpeechEvent::LowerHandSuggestion => 1,
        }
    }
}

#[derive(Debug)]
pub enum RemoteDevicesChangedReason {
    DemuxIdsChanged,
    MediaKeyReceived(DemuxId),
    SpeakerTimeChanged(DemuxId),
    HeartbeatStateChanged(DemuxId),
    ForwardedVideosChanged,
    HigherResolutionPendingChanged,
}

// The callbacks from the Call to the Observer of the call.
// Some of these are more than an "observer" in that a response is needed,
// which is provided asynchronously.
pub trait Observer {
    // A response should be provided via Call.update_membership_proof.
    fn request_membership_proof(&self, client_id: ClientId);
    // A response should be provided via Call.update_group_members.
    fn request_group_members(&self, client_id: ClientId);
    // Send a signaling message to the given remote user.
    fn send_signaling_message(
        &mut self,
        recipient_id: UserId,
        call_message: protobuf::signaling::CallMessage,
        urgency: SignalingMessageUrgency,
    );
    // Send a generic call message to a group. Send to all members of the group
    // or, if recipients_override is not empty, send to the given subset of members
    // using multi-recipient sealed sender.
    fn send_signaling_message_to_group(
        &mut self,
        group_id: GroupId,
        call_message: protobuf::signaling::CallMessage,
        urgency: SignalingMessageUrgency,
        // Use `Default::default()` to send to all group members.
        recipients_override: HashSet<UserId>,
    );
    // Send a generic call message to the specified recipients. Provides endorsements
    // that can be used to create a send token. Endorsements provided in the same order as
    // recipients.
    fn send_signaling_message_to_adhoc_group(
        &mut self,
        call_message: protobuf::signaling::CallMessage,
        urgency: SignalingMessageUrgency,
        expiration: u64,
        recipients_to_endorsements: HashMap<UserId, Vec<u8>>,
    );

    // The following notify the observer of state changes to the local device.
    fn handle_connection_state_changed(
        &self,
        client_id: ClientId,
        connection_state: ConnectionState,
    );
    fn handle_network_route_changed(&self, client_id: ClientId, network_route: NetworkRoute);
    fn handle_join_state_changed(&self, client_id: ClientId, join_state: JoinState);
    fn handle_send_rates_changed(&self, _client_id: ClientId, _send_rates: SendRates) {}

    // The following notify the observer of state changes to the remote devices.
    fn handle_remote_devices_changed(
        &self,
        client_id: ClientId,
        remote_devices: &[RemoteDeviceState],
        reason: RemoteDevicesChangedReason,
    );

    // Notifies the observer of changes to the list of call participants.
    fn handle_peek_changed(
        &self,
        client_id: ClientId,
        peek_info: &PeekInfo,
        // We use a HashSet because the client expects a unique list of users,
        // and there can be multiple devices from the same user.
        joined_members: &HashSet<UserId>,
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

    fn handle_speaking_notification(&mut self, client_id: ClientId, speech_event: SpeechEvent);

    fn handle_audio_levels(
        &self,
        client_id: ClientId,
        captured_level: AudioLevel,
        received_levels: Vec<ReceivedAudioLevel>,
    );

    fn handle_low_bandwidth_for_video(&self, client_id: ClientId, recovered: bool);

    fn handle_reactions(&self, client_id: ClientId, reactions: Vec<Reaction>);

    fn handle_raised_hands(&self, client_id: ClientId, raised_hands: Vec<DemuxId>);

    fn handle_remote_mute_request(&self, client_id: ClientId, mute_source: DemuxId);

    fn handle_observed_remote_mute(
        &self,
        client_id: ClientId,
        mute_source: DemuxId,
        mute_target: DemuxId,
    );

    fn handle_rtc_stats_report(&self, report_json: String);

    // This will be the last callback.
    // The observer can assume the Call is completely shut down and can be deleted.
    fn handle_ended(&self, client_id: ClientId, reason: CallEndReason, call_summary: CallSummary);

    fn handle_endorsements_update(&self, client_id: ClientId, update: EndorsementUpdateResultRef);
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

impl ConnectionState {
    pub fn ordinal(&self) -> i32 {
        // Must be kept in sync with the Java, Swift, and TypeScript enums.
        match self {
            ConnectionState::NotConnected => 0,
            ConnectionState::Connecting => 1,
            ConnectionState::Connected => 2,
            ConnectionState::Reconnecting => 3,
        }
    }
}

// The join states of a device joining a group call.
// Has a state diagram like this:
//        |
//        | start()
//        V
//    NotJoined
//        |             ^
//        | join()      |
//        V             |
//     Joining       -->|  leave() or
//        |             |  failed to join
//        | response    |
//        V             |
// Joined || Pending -->|
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinState {
    /// Join() has not yet been called
    /// or leave() has been called
    /// or join() was called but failed.
    ///
    /// If the ring ID is present,
    /// joining will sent an "accepted" message to your other devices.
    NotJoined(Option<RingId>),

    /// Join() has been called but a response from the SFU is pending.
    Joining,

    /// Join() has been called, a response from the SFU has been received,
    /// and a DemuxId has been assigned...
    /// The SFU notified us our join is pending, and we are waiting to
    /// appear in the list of active participants.
    Pending(DemuxId),

    /// Join() has been called, a response from the SFU has been received,
    /// a DemuxId has been assigned. Either the SFU notified us we joined
    /// or we've appeared in the list of active participants.
    Joined(DemuxId),
}

impl JoinState {
    pub fn ordinal(&self) -> i32 {
        // Must be kept in sync with the Java, Swift, and TypeScript enums.
        match self {
            JoinState::NotJoined(_) => 0,
            JoinState::Joining => 1,
            JoinState::Pending(_) => 2,
            JoinState::Joined(_) => 3,
        }
    }
}

// This really should go in JoinState and/or ConnectionState,
// but an EphemeralSecret isn't Clone or Debug, so it's inconvenient
// to put them in there.  Plus, because of the weird relationship
// between the ConnectionState and JoinState due to limitations of
// the SFU (not being able to connect until after joined), it's
// also more convenient to call GroupCall::start_peer_connection
// with a state separate from those 2.
#[derive(Default)]
enum DheState {
    #[default]
    NotYetStarted,
    WaitingForServerPublicKey {
        client_secret: EphemeralSecret,
    },
    Negotiated {
        srtp_keys: SrtpKeys,
    },
}

impl DheState {
    fn start(client_secret: EphemeralSecret) -> Self {
        DheState::WaitingForServerPublicKey { client_secret }
    }

    fn negotiate_in_place(&mut self, server_pub_key: &PublicKey, hkdf_extra_info: &[u8]) {
        *self = std::mem::take(self).negotiate(server_pub_key, hkdf_extra_info)
    }

    fn negotiate(self, server_pub_key: &PublicKey, hkdf_extra_info: &[u8]) -> Self {
        match self {
            DheState::NotYetStarted => {
                error!("Attempting to negotiated SRTP keys before starting DHE.");
                self
            }
            DheState::WaitingForServerPublicKey { client_secret } => {
                let shared_secret = client_secret.diffie_hellman(server_pub_key);
                let mut master_key_material = [0u8; SrtpKeys::MASTER_KEY_MATERIAL_LEN];
                Hkdf::<Sha256>::new(Some(&[0u8; 32]), shared_secret.as_bytes())
                    .expand_multi_info(
                        &[
                            b"Signal_Group_Call_20211105_SignallingDH_SRTPKey_KDF",
                            hkdf_extra_info,
                        ],
                        &mut master_key_material,
                    )
                    .expect("SRTP master key material expansion");
                DheState::Negotiated {
                    srtp_keys: SrtpKeys::from_master_key_material(&master_key_material),
                }
            }
            DheState::Negotiated { .. } => {
                warn!("Attempting to negotiated SRTP keys a second time.");
                self
            }
        }
    }
}

// The info about SFU needed in order to connect to it.
#[derive(Clone, Debug)]
pub struct SfuInfo {
    pub udp_addresses: Vec<SocketAddr>,
    pub tcp_addresses: Vec<SocketAddr>,
    pub tls_addresses: Vec<SocketAddr>,
    pub hostname: Option<String>,
    pub ice_ufrag: String,
    pub ice_pwd: String,
}

const ADMIN_LOG_TAG: &str = "AdminAction";

#[repr(C)]
#[derive(Clone, Debug)]
pub struct Reaction {
    pub demux_id: DemuxId,
    pub value: String,
}

// The callbacks from the Client to the "SFU client" for the group call.
pub trait SfuClient {
    // This should call Client.on_sfu_client_joined when the SfuClient has joined.
    fn join(&mut self, ice_ufrag: &str, ice_pwd: &str, dhe_pub_key: [u8; 32], client: Client);
    fn peek(&mut self, result_callback: PeekResultCallback);

    // Notifies the client of the new membership proof.
    fn set_membership_proof(&mut self, proof: MembershipProof);
    fn set_group_members(&mut self, members: Vec<GroupMember>);
}

pub struct Joined {
    pub sfu_info: SfuInfo,
    pub local_demux_id: DemuxId,
    pub server_dhe_pub_key: [u8; 32],
    pub hkdf_extra_info: Vec<u8>,
    pub creator: Option<UserId>,
    pub era_id: String,
    pub join_state: JoinState,
}

/// Communicates with the SFU using HTTP.
pub struct HttpSfuClient {
    sfu_url: String,
    room_id_header: Option<String>,
    epoch_header: Option<String>,
    admin_passkey: Option<Vec<u8>>,
    // For use post-DHE
    hkdf_extra_info: Vec<u8>,
    http_client: Box<dyn http::Client + Send>,
    auth_header: Option<String>,
    member_resolver: Arc<dyn sfu::MemberResolver + Send + Sync>,
    deferred_join: Option<(String, String, [u8; 32], Client)>,
}

impl HttpSfuClient {
    pub fn new(
        http_client: Box<dyn http::Client + Send>,
        url: String,
        room_id_for_header: Option<&[u8]>,
        epoch_for_header: Option<CallLinkEpoch>,
        admin_passkey: Option<Vec<u8>>,
        hkdf_extra_info: Vec<u8>,
    ) -> Self {
        Self {
            sfu_url: url,
            room_id_header: room_id_for_header.map(hex::encode),
            epoch_header: epoch_for_header.map(|epoch| epoch.to_string()),
            admin_passkey,
            hkdf_extra_info,
            http_client,
            auth_header: None,
            member_resolver: Arc::new(sfu::MemberMap::default()),
            deferred_join: None,
        }
    }

    pub fn set_auth_header(&mut self, auth_header: String) {
        self.auth_header = Some(auth_header)
    }

    pub fn set_member_resolver(
        &mut self,
        member_resolver: Arc<dyn sfu::MemberResolver + Send + Sync>,
    ) {
        self.member_resolver = member_resolver;
    }

    fn join_with_header(
        &self,
        auth_header: String,
        ice_ufrag: &str,
        ice_pwd: &str,
        dhe_pub_key: &[u8],
        client: Client,
    ) {
        let hkdf_extra_info = self.hkdf_extra_info.clone();
        sfu::join(
            self.http_client.as_ref(),
            &self.sfu_url,
            self.room_id_header.clone(),
            self.epoch_header.clone(),
            auth_header,
            self.admin_passkey.as_deref(),
            ice_ufrag,
            ice_pwd,
            dhe_pub_key,
            &self.hkdf_extra_info,
            self.member_resolver.clone(),
            Box::new(move |join_response| {
                let join_result: Result<Joined> = match join_response {
                    Ok(join_response) => Ok(Joined {
                        sfu_info: SfuInfo {
                            udp_addresses: join_response.server_udp_addresses,
                            tcp_addresses: join_response.server_tcp_addresses,
                            tls_addresses: join_response.server_tls_addresses,
                            hostname: join_response.server_hostname,
                            ice_ufrag: join_response.server_ice_ufrag,
                            ice_pwd: join_response.server_ice_pwd,
                        },
                        local_demux_id: join_response.client_demux_id,
                        server_dhe_pub_key: join_response.server_dhe_pub_key,
                        creator: join_response.call_creator,
                        era_id: join_response.era_id,
                        hkdf_extra_info,
                        join_state: match join_response.client_status {
                            ClientStatus::Active => {
                                JoinState::Joined(join_response.client_demux_id)
                            }
                            // swallow the blocked status until we flesh out the UX
                            ClientStatus::Pending | ClientStatus::Blocked => {
                                JoinState::Pending(join_response.client_demux_id)
                            }
                        },
                    }),
                    Err(http_status) if http_status == http::ResponseStatus::REQUEST_FAILED => {
                        Err(RingRtcError::SfuClientRequestFailed.into())
                    }
                    Err(http_status) if http_status == http::ResponseStatus::GROUP_CALL_FULL => {
                        Err(RingRtcError::GroupCallFull.into())
                    }
                    Err(http_status) => {
                        Err(RingRtcError::UnexpectedResponseCodeFromSFu(http_status.code).into())
                    }
                };
                client.on_sfu_client_join_attempt_completed(join_result);
            }),
        );
    }
}

impl SfuClient for HttpSfuClient {
    fn set_membership_proof(&mut self, proof: MembershipProof) {
        if let Some(auth_header) = sfu::auth_header_from_membership_proof(&proof) {
            self.auth_header = Some(auth_header.clone());
            // Release any tasks that were blocked on getting the token.
            if let Some((ice_ufrag, ice_pwd, dhe_pub_key, client)) = self.deferred_join.take() {
                info!("membership token received, proceeding with deferred join");
                self.join_with_header(auth_header, &ice_ufrag, &ice_pwd, &dhe_pub_key[..], client);
            }
        }
    }

    fn join(&mut self, ice_ufrag: &str, ice_pwd: &str, dhe_pub_key: [u8; 32], client: Client) {
        match self.auth_header.as_ref() {
            Some(h) => {
                self.join_with_header(h.clone(), ice_ufrag, ice_pwd, &dhe_pub_key[..], client)
            }
            None => {
                info!("join requested without membership token - deferring");
                let ice_ufrag = ice_ufrag.to_string();
                let ice_pwd = ice_pwd.to_string();
                self.deferred_join = Some((ice_ufrag, ice_pwd, dhe_pub_key, client));
            }
        }
    }

    fn peek(&mut self, result_callback: PeekResultCallback) {
        match self.auth_header.clone() {
            Some(auth_header) => sfu::peek(
                self.http_client.as_ref(),
                &self.sfu_url,
                self.room_id_header.clone(),
                self.epoch_header.clone(),
                auth_header,
                self.member_resolver.clone(),
                None,
                result_callback,
            ),
            None => {
                result_callback(Err(http::ResponseStatus::INVALID_CLIENT_AUTH));
            }
        }
    }

    fn set_group_members(&mut self, members: Vec<GroupMember>) {
        info!("SfuClient set_group_members: {} members", members.len());
        self.set_member_resolver(Arc::new(sfu::MemberMap::new(&members)));
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HeartbeatState {
    pub audio_muted: Option<bool>,
    pub video_muted: Option<bool>,
    pub presenting: Option<bool>,
    pub sharing_screen: Option<bool>,
    pub muted_by_demux_id: Option<u32>,
}

impl From<protobuf::group_call::device_to_device::Heartbeat> for HeartbeatState {
    fn from(proto: protobuf::group_call::device_to_device::Heartbeat) -> Self {
        Self {
            audio_muted: proto.audio_muted,
            video_muted: proto.video_muted,
            presenting: proto.presenting,
            sharing_screen: proto.sharing_screen,
            muted_by_demux_id: proto.muted_by_demux_id,
        }
    }
}

// The info about remote devices received from the SFU
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteDeviceState {
    pub demux_id: DemuxId,
    pub user_id: UserId,
    pub media_keys_received: bool,
    pub heartbeat_state: HeartbeatState,
    // The latest timestamp we received from an update to
    // heartbeat_state.
    heartbeat_rtp_timestamp: Option<rtp::Timestamp>,
    // The time at which this device was added to the list of devices.
    // A combination of (added_timestamp, demux_id) can be used for a stable
    // sort of remote devices for a grid layout.
    pub added_time: SystemTime,
    // The most recent time at which this device became the primary speaker
    // Sorting using this value will give a history of who spoke.
    pub speaker_time: Option<SystemTime>,
    pub leaving_received: bool,
    pub forwarding_video: Option<bool>,
    pub server_allocated_height: u16,
    pub client_decoded_height: Option<u32>,
    pub is_higher_resolution_pending: bool,
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
    fn new(demux_id: DemuxId, user_id: UserId, added_time: SystemTime) -> Self {
        Self {
            demux_id,
            user_id,
            media_keys_received: false,
            heartbeat_state: Default::default(),
            heartbeat_rtp_timestamp: None,

            added_time,
            speaker_time: None,
            leaving_received: false,
            forwarding_video: None,
            server_allocated_height: 0,
            client_decoded_height: None,
            is_higher_resolution_pending: false,
        }
    }

    pub fn speaker_time_as_unix_millis(&self) -> u64 {
        as_unix_millis(self.speaker_time)
    }

    pub fn added_time_as_unix_millis(&self) -> u64 {
        as_unix_millis(Some(self.added_time))
    }

    fn recalculate_higher_resolution_pending(&mut self) {
        let was_pending = self.is_higher_resolution_pending;
        self.is_higher_resolution_pending =
            self.server_allocated_height as u32 > self.client_decoded_height.unwrap_or(0);

        if !was_pending && self.is_higher_resolution_pending {
            info!(
                "Higher resolution video (height={}) now pending for {}. Current height is {:?}",
                self.server_allocated_height, self.demux_id, self.client_decoded_height
            );
        }
    }
}

/// These can be sent to the SFU to request different resolutions of
/// video for different remote dem
#[derive(Clone, Debug)]
pub struct VideoRequest {
    pub demux_id: DemuxId,
    pub width: u16,
    pub height: u16,
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
const ALL_ALONE_MAX_SEND_RATE: DataRate = DataRate::from_kbps(1);

const SMALL_CALL_MAX_SEND_RATE: DataRate = DataRate::from_kbps(1000);

// This is the smallest rate at which WebRTC seems to still send VGA.
const LARGE_CALL_MAX_SEND_RATE: DataRate = DataRate::from_kbps(671);

const SCREENSHARE_MIN_SEND_RATE: DataRate = DataRate::from_kbps(500);
const SCREENSHARE_START_SEND_RATE: DataRate = DataRate::from_mbps(1);
const SCREENSHARE_MAX_SEND_RATE: DataRate = DataRate::from_mbps(2);

const LOW_MAX_RECEIVE_RATE: DataRate = DataRate::from_kbps(500);

const NORMAL_MAX_RECEIVE_RATE: DataRate = DataRate::from_mbps(20);

const BWE_THRESHOLD_FOR_LOW_NOTIFICATION: DataRate = DataRate::from_kbps(70);
const BWE_THRESHOLD_FOR_RECOVERED_NOTIFICATION: DataRate = DataRate::from_kbps(80);

const DELAY_FOR_RECOVERED_BWE_CALLBACK: Duration = Duration::from_secs(6);

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
        secret: frame_crypto::Secret,
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
        // While waiting, something happened that makes us think we should ask again.
        should_request_again: bool,
        at: Instant,
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
    client_id: ClientId,
    pub group_id: GroupId,
    // We have to leave this outside of the actor state
    // because WebRTC calls back to the PeerConnectionObserver
    // synchronously.
    frame_crypto_context: Arc<CallMutex<frame_crypto::Context>>,
    actor: Actor<State>,
}

#[derive(Default)]
struct RemoteDevices(Vec<RemoteDeviceState>);

impl Deref for RemoteDevices {
    type Target = Vec<RemoteDeviceState>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RemoteDevices {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromIterator<RemoteDeviceState> for RemoteDevices {
    fn from_iter<T: IntoIterator<Item = RemoteDeviceState>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for RemoteDevices {
    type Item = RemoteDeviceState;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Debug)]
enum OutgoingRingState {
    /// The initial state
    Unknown,
    /// The local client is permitted to send a ring if they choose, but has not requested one.
    PermittedToRing { ring_id: RingId },
    /// The local client has requested to ring, but it is unknown whether it is permitted.
    WantsToRing { recipient: Option<UserId> },
    /// The local client has, in fact, sent a ring (and may still cancel it).
    HasSentRing { ring_id: RingId },
    /// The local client is not permitted to send rings at this time.
    ///
    /// They may not be the creator of the call, or they may have already sent a ring and had other
    /// people join.
    NotPermittedToRing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupCallKind {
    SignalGroup,
    CallLink,
}

/// The next time to check WebRTC's bandwidth estimate (BWE).
///
/// The initial state is Disabled. Possible state transitions:
///
///   Disabled -> At       (when video is enabled)
///   At -> Disabled       (when video is disabled)
///   At -> RecoveredAt    (after callback is made for low bandwidth)
///   RecoveredAt -> None  (after callback is made for bandwidth recovered)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BweCheckState {
    /// Check again after the `Instant` has passed.
    At(Instant),
    /// Check to see whether the BWE has recovered after `check_at` has passed.
    RecoveredAt {
        next_bwe_time: Instant,
        last_callback: Instant,
    },
    /// Don't check now, but there might be another check later in the call.
    Disabled,
    /// Don't check again for the remainder of the call.
    None,
}

#[derive(Default)]
struct RaiseHandState {
    pub seqnum: u32,
    pub raise: bool,
    pub outstanding: bool,
}

/// The state inside the Actor
struct State {
    // Things passed in that never change
    client_id: ClientId,
    group_id: GroupId,
    kind: GroupCallKind,
    sfu_client: Box<dyn SfuClient>,
    observer: Box<dyn Observer>,

    call_summary: GroupCallSummary,

    // Shared state with the CallManager that might change
    busy: Arc<CallMutex<bool>>,
    self_uuid: Arc<CallMutex<Option<UserId>>>,

    // State that changes regularly and is sent to the observer
    connection_state: ConnectionState,
    join_state: JoinState,
    remote_devices: RemoteDevices,

    // State that changes infrequently and is not sent to the observer.
    dhe_state: DheState,

    // Things to control peeking
    remote_devices_request_state: RemoteDevicesRequestState,
    last_peek_info: Option<PeekInfo>,
    known_members: HashSet<UserId>,
    obfuscated_resolver: ObfuscatedResolver,

    // Derived from remote_devices but stored so we can fire
    // Observer::handle_peek_changed only when it changes
    joined_members: HashSet<UserId>,
    pending_users_signature: u64,

    // Things we send to other clients via heartbeats
    // These are unset until the app sets them.
    // But we err on the side of caution and don't send anything when they are unset.
    outgoing_heartbeat_state: HeartbeatState,

    // Things for controlling the PeerConnection
    local_ice_ufrag: String,
    local_ice_pwd: String,
    sfu_info: Option<SfuInfo>,
    peer_connection: PeerConnection,
    peer_connection_observer_impl: Box<PeerConnectionObserverImpl>,
    rtp_observer_impl: Option<Box<RtpObserverImpl>>,
    rtp_observer_ptr: Option<webrtc::ptr::Unique<RffiRtpObserver>>,
    rtp_data_to_sfu_next_seqnum: u32,
    rtp_data_through_sfu_next_seqnum: u32,
    next_heartbeat_time: Option<Instant>,
    /// The remote demux IDs are in the order of the corresponding transceivers
    /// in peer_connection. Each demux ID is associated with two transceivers
    /// (audio and video). None represents an unused transceiver pair.
    remote_transceiver_demux_ids: Vec<Option<DemuxId>>,

    // Things for getting statistics from the PeerConnection
    // Stats gathering happens only when joined
    next_stats_time: Option<Instant>,
    get_stats_interval: Duration,
    stats_observer: Box<StatsObserver>,
    next_decryption_error_time: Option<Instant>,

    // Things for getting audio levels from the PeerConnection
    audio_levels_interval: Option<Duration>,
    next_audio_levels_time: Option<Instant>,
    // Variables to track the start of the current utterance, and how frequently
    // to poll for "is the user speaking?"
    speaking_interval: Duration,
    next_speaking_audio_levels_time: Option<Instant>,
    // Track the time the current speech began, if the user is not silent.
    started_speaking: Option<Instant>,
    // Track the time the current silence started, if the user is not speaking.
    silence_started: Option<Instant>,
    // Tracker for the last time speech-related notification sent to the client.
    last_speaking_notification: Option<SpeechEvent>,

    next_membership_proof_request_time: Option<Instant>,

    next_raise_hand_time: Option<Instant>,

    bwe_check_state: BweCheckState,

    // We serve endorsements with messages from the cached endorsements
    group_send_endorsement_cache: Option<EndorsementsCache>,

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
    video_requests: Option<Vec<VideoRequest>>,
    active_speaker_height: Option<u16>,
    on_demand_video_request_sent_since_last_heartbeat: bool,
    speaker_rtp_timestamp: Option<rtp::Timestamp>,

    send_rates: SendRates,
    // If set, will always override the send_rates.  Intended for testing.
    send_rates_override: Option<SendRates>,
    max_receive_rate: Option<DataRate>,
    data_mode: DataMode,
    // Demux IDs where video is being forward from, mapped to the server allocated height.
    forwarding_videos: HashMap<DemuxId, u16>,

    outgoing_ring_state: OutgoingRingState,

    reactions: Vec<Reaction>,
    raised_hands: Vec<DemuxId>,
    raise_hand_state: RaiseHandState,
    mute_request: Option<DemuxId>,

    sfu_reliable_stream: MrpStream<Vec<u8>, (rtp::Header, SfuToDevice)>,
    actor: Actor<State>,
}

const RELIABLE_RTP_BUFFER_SIZE: usize = 64;
const DEVICE_TO_SFU_TIMEOUT: Duration = Duration::from_millis(1000);

impl From<&protobuf::group_call::MrpHeader> for mrp::MrpHeader {
    fn from(value: &protobuf::group_call::MrpHeader) -> Self {
        Self {
            seqnum: value.seqnum,
            ack_num: value.ack_num,
            num_packets: value.num_packets,
        }
    }
}

impl From<mrp::MrpHeader> for protobuf::group_call::MrpHeader {
    fn from(value: mrp::MrpHeader) -> Self {
        Self {
            seqnum: value.seqnum,
            ack_num: value.ack_num,
            num_packets: value.num_packets,
        }
    }
}

impl RemoteDevices {
    /// Find the latest speaker
    fn latest_speaker_demux_id(&self) -> Option<DemuxId> {
        let latest_speaker = self.iter().max_by_key(|a| a.speaker_time);
        if latest_speaker?.speaker_time.is_none() {
            None
        } else {
            latest_speaker.map(|speaker| speaker.demux_id)
        }
    }

    /// Find remote device state by demux id
    fn find_by_demux_id(&self, demux_id: DemuxId) -> Option<&RemoteDeviceState> {
        self.iter().find(|device| device.demux_id == demux_id)
    }

    /// Find remote device state by demux id
    fn find_by_demux_id_mut(&mut self, demux_id: DemuxId) -> Option<&mut RemoteDeviceState> {
        self.0.iter_mut().find(|device| device.demux_id == demux_id)
    }

    /// Returns a set containing all the demux ids in the collection
    fn demux_id_set(&self) -> HashSet<DemuxId> {
        self.iter().map(|device| device.demux_id).collect()
    }
}

// The time between ticks to do periodic things like request updated
// membership list from the SfuClient
const TICK_INTERVAL: Duration = Duration::from_millis(200);

// How often to send RTP data messages and video requests.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

// Call summary time limit.
const DEFAULT_CALL_SUMMARY_TIME_LIMIT: Duration = Duration::from_secs(300);

// How often to get and log stats.
const DEFAULT_STATS_INTERVAL: Duration = Duration::from_secs(10);
const STATS_INITIAL_OFFSET: Duration = Duration::from_secs(2);
const DECRYPTION_ERROR_INTERVAL: Duration = Duration::from_millis(500);

// How often to request an updated membership proof (24 hours).
const MEMBERSHIP_PROOF_REQUEST_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

const RAISE_HAND_INTERVAL: Duration = Duration::from_millis(500);

// How often to check the latest bandwidth estimate from WebRTC
const BWE_INTERVAL: Duration = Duration::from_secs(1);
/// How much to delay the check for the bandwidth estimate to allow for the estimator to update
/// when connecting.
const DELAYED_BWE_CHECK: Duration = Duration::from_secs(10);

const REACTION_STRING_MAX_SIZE: usize = 256;

/// How long to wait before ending the client and cleaning up.
const CLIENT_END_DELAY: Duration = Duration::from_millis(20);

/// The max byte size of Rtp Packet serialized size before needing to be fragmented
const MAX_PACKET_SERIALIZED_BYTE_SIZE: usize = 1200;
/// The non-content byte size overhead of an MRP fragment
/// With an MRP header with seqnum, num_packets, and content specified, the overhead is 22. We add
/// a safety margin in case of unexpected overhead increases.
const MRP_FRAGMENT_OVERHEAD: usize = 60;
/// Max byte size for content in an MRP fragment
const MAX_MRP_FRAGMENT_BYTE_SIZE: usize = MAX_PACKET_SERIALIZED_BYTE_SIZE - MRP_FRAGMENT_OVERHEAD;

pub struct ClientStartParams {
    pub group_id: GroupId,
    pub client_id: ClientId,
    pub kind: GroupCallKind,
    pub sfu_client: Box<dyn SfuClient + Send>,
    pub observer: Box<dyn Observer + Send>,
    pub busy: Arc<CallMutex<bool>>,
    pub self_uuid: Arc<CallMutex<Option<UserId>>>,
    pub peer_connection_factory: Option<PeerConnectionFactory>,
    pub outgoing_audio_track: AudioTrack,
    pub outgoing_video_track: Option<VideoTrack>,
    pub incoming_video_sink: Option<Box<dyn VideoSink>>,
    pub ring_id: Option<RingId>,
    pub audio_levels_interval: Option<Duration>,
    pub obfuscated_resolver: ObfuscatedResolver,
    pub group_send_endorsement_cache: Option<EndorsementsCache>,
}

impl Client {
    pub fn start(params: ClientStartParams) -> Result<Self> {
        let ClientStartParams {
            group_id,
            client_id,
            kind,
            sfu_client,
            observer,
            busy,
            self_uuid,
            peer_connection_factory,
            outgoing_audio_track,
            outgoing_video_track,
            incoming_video_sink,
            ring_id,
            audio_levels_interval,
            obfuscated_resolver,
            group_send_endorsement_cache,
        } = params;

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
            actor: Actor::start("group-call-client", stopper, move |actor| {
                debug!("group_call::Client(inner)::new(client_id: {})", client_id);

                let peer_connection_factory = match peer_connection_factory {
                    None => {
                        match PeerConnectionFactory::new(
                            &pcf::AudioConfig::default(),
                            false,
                            "",
                            None,
                        ) {
                            Ok(v) => v,
                            Err(err) => {
                                observer.handle_ended(
                                    client_id,
                                    CallEndReason::FailedToCreatePeerConnectionFactory,
                                    CallSummary::default(),
                                );
                                return Err(err);
                            }
                        }
                    }
                    Some(v) => v,
                };

                let (peer_connection_observer_impl, peer_connection_observer) =
                    PeerConnectionObserverImpl::uninitialized(incoming_video_sink)?;
                // WebRTC uses alphanumeric plus + and /, which is just barely a superset of this,
                // but we can't uses dashes due to the sfu.
                let local_ice_ufrag = random_alphanumeric(4);
                let local_ice_pwd = random_alphanumeric(22);
                let audio_rtcp_report_interval_ms = 5000;
                let ice_servers = vec![];
                let peer_connection = peer_connection_factory
                    .create_peer_connection(
                        peer_connection_observer,
                        pcf::RffiPeerConnectionKind::GroupCall,
                        &AudioJitterBufferConfig::default(),
                        audio_rtcp_report_interval_ms,
                        &ice_servers,
                        outgoing_audio_track,
                        outgoing_video_track,
                    )
                    .inspect_err(|_| {
                        observer.handle_ended(
                            client_id,
                            CallEndReason::FailedToCreatePeerConnection,
                            CallSummary::default(),
                        );
                    })?;
                let call_id_for_stats = CallId::from(client_id as u64);
                let mut stats_observer =
                    create_stats_observer(call_id_for_stats, DEFAULT_STATS_INTERVAL);
                let call_summary =
                    GroupCallSummary::new(DEFAULT_CALL_SUMMARY_TIME_LIMIT, DEFAULT_STATS_INTERVAL)?;
                stats_observer.set_stats_snapshot_consumer(call_summary.as_stats_consumer());
                Ok(State {
                    client_id,
                    group_id,
                    kind,
                    sfu_client,
                    observer,
                    busy,
                    self_uuid,
                    local_ice_ufrag,
                    local_ice_pwd,

                    call_summary,

                    connection_state: ConnectionState::NotConnected,
                    join_state: JoinState::NotJoined(ring_id),
                    dhe_state: DheState::default(),
                    remote_transceiver_demux_ids: Default::default(),
                    remote_devices: Default::default(),

                    remote_devices_request_state: match kind {
                        GroupCallKind::SignalGroup => {
                            RemoteDevicesRequestState::WaitingForMembershipProof
                        }
                        GroupCallKind::CallLink => RemoteDevicesRequestState::NeverRequested,
                    },
                    last_peek_info: None,

                    known_members: HashSet::new(),
                    obfuscated_resolver,

                    joined_members: HashSet::new(),
                    pending_users_signature: 0,

                    outgoing_heartbeat_state: Default::default(),

                    sfu_info: None,
                    peer_connection_observer_impl,
                    rtp_observer_impl: None,
                    rtp_observer_ptr: None,
                    peer_connection,
                    rtp_data_to_sfu_next_seqnum: 1,
                    rtp_data_through_sfu_next_seqnum: 1,

                    next_heartbeat_time: None,

                    next_stats_time: None,
                    get_stats_interval: DEFAULT_STATS_INTERVAL,

                    stats_observer,

                    next_decryption_error_time: None,

                    audio_levels_interval,
                    next_audio_levels_time: None,

                    speaking_interval: SPEAKING_POLL_INTERVAL,
                    next_speaking_audio_levels_time: None,
                    started_speaking: None,
                    silence_started: None,
                    last_speaking_notification: None,

                    next_membership_proof_request_time: None,

                    next_raise_hand_time: None,

                    bwe_check_state: BweCheckState::Disabled,

                    group_send_endorsement_cache,

                    frame_crypto_context,
                    pending_media_receive_keys: Vec::new(),
                    media_send_key_rotation_state: KeyRotationState::Applied,

                    video_requests: None,
                    active_speaker_height: None,
                    on_demand_video_request_sent_since_last_heartbeat: false,
                    speaker_rtp_timestamp: None,

                    send_rates: SendRates::default(),
                    send_rates_override: None,
                    // If the client never calls set_data_mode, use the normal max receive rate.
                    max_receive_rate: Some(NORMAL_MAX_RECEIVE_RATE),
                    data_mode: DataMode::Normal,
                    forwarding_videos: HashMap::default(),

                    outgoing_ring_state: OutgoingRingState::Unknown,

                    reactions: Vec::new(),
                    raised_hands: Vec::new(),
                    raise_hand_state: RaiseHandState::default(),
                    mute_request: None,

                    sfu_reliable_stream: MrpStream::with_capacity_limit(RELIABLE_RTP_BUFFER_SIZE),

                    actor,
                })
            })?,
            frame_crypto_context: frame_crypto_context_for_outside_actor,
        };

        // After we have the actor, we can initialize the observer implementations,
        // create and set the RTP observer, and kick off ticking.
        let client_clone_to_init_peer_connection_observer_impl = client.clone();

        let rtp_observer_impl = Box::new(RtpObserverImpl {
            client: client.clone(),
        });

        client.actor.send(move |state| {
            state
                .peer_connection_observer_impl
                .initialize(client_clone_to_init_peer_connection_observer_impl);

            let rtp_observer =
                RtpObserver::new(webrtc::ptr::Borrowed::from_ptr(&*rtp_observer_impl))
                    .expect("Failed to create RtpObserver");
            let rtp_observer_ptr = rtp_observer.into_rffi();
            state
                .peer_connection
                .set_rtp_packet_observer(rtp_observer_ptr.borrow());
            state.rtp_observer_impl = Some(rtp_observer_impl);
            state.rtp_observer_ptr = Some(rtp_observer_ptr);

            Self::request_remote_devices_as_soon_as_possible(state);
        });
        Ok(client)
    }

    pub fn provide_ring_id_if_absent(&self, ring_id: RingId) {
        self.actor.send(move |state| match &mut state.join_state {
            JoinState::NotJoined(Some(existing_ring_id)) => {
                // Note that we prefer older rings to newer, unlike when processing incoming rings.
                // This is because we expect the call to already be handling the existing ring
                // (maybe that's what's actively ringing in the app).
                warn!(
                    "discarding ring {}; already have a ring for the same group ({})",
                    ring_id, existing_ring_id
                );
            }
            JoinState::NotJoined(saved_ring_id @ None) => {
                *saved_ring_id = Some(ring_id);
            }
            JoinState::Joining | JoinState::Pending(_) | JoinState::Joined(_) => {
                warn!(
                    "ignoring ring {} for a call we have already joined or are currently joining",
                    ring_id
                );
            }
        });
    }

    // Should only be used for testing
    pub fn override_send_rates(&self, send_rates_override: SendRates) {
        self.actor.send(move |state| {
            state.send_rates_override = Some(send_rates_override.clone());
            Self::set_send_rates_inner(state, send_rates_override);
        });
    }

    // Pulled into a named private method so we can call it recursively.
    fn tick(state: &mut State) {
        let now = Instant::now();

        trace!(
            "group_call::Client(inner)::tick(group_id: {})",
            state.client_id
        );

        Self::request_remote_devices_from_sfu_if_older_than(state, Duration::from_secs(10));

        if let Some(next_heartbeat_time) = state.next_heartbeat_time
            && now >= next_heartbeat_time
        {
            if let Err(err) = Self::send_heartbeat(state) {
                warn!("Failed to send regular heartbeat: {:?}", err);
            }
            // Also send video requests at the same rate as the heartbeat.
            Self::send_video_requests_to_sfu(state);
            state.on_demand_video_request_sent_since_last_heartbeat = false;
            state.next_heartbeat_time = Some(now + HEARTBEAT_INTERVAL)
        }

        if let Some(next_stats_time) = state.next_stats_time {
            if now >= next_stats_time {
                let _ = state
                    .peer_connection
                    .get_stats(state.stats_observer.as_ref());
                state.next_stats_time = Some(now + state.get_stats_interval);
            }
            if let Some(report_json) = state.stats_observer.take_stats_report() {
                state.observer.handle_rtc_stats_report(report_json)
            }
        }

        if let Some(next_decryption_error_time) = state.next_decryption_error_time
            && now >= next_decryption_error_time
        {
            let decryption_errors = {
                state
                    .frame_crypto_context
                    .lock()
                    .ok()
                    .and_then(|mut context| context.get_error_report())
            };
            if let Some(decryption_errors) = decryption_errors {
                Self::send_decryption_stats_inner(state, decryption_errors);
            }
            state.next_decryption_error_time = Some(now + DECRYPTION_ERROR_INTERVAL);
        }

        if let Some(next_speaking_audio_levels_time) = state.next_speaking_audio_levels_time
            && now >= next_speaking_audio_levels_time
        {
            let (captured_level, _) = state.peer_connection.get_audio_levels();
            let mut time_silent = Duration::from_secs(0);
            state.started_speaking = if captured_level > MIN_NON_SILENT_LEVEL
                && !state.outgoing_heartbeat_state.audio_muted.unwrap_or(true)
            {
                state.silence_started = None;
                state.started_speaking.or(Some(now))
            } else {
                state.silence_started = state.silence_started.or(Some(now));
                time_silent = state
                    .silence_started
                    .map_or(Duration::from_secs(0), |start| now.duration_since(start));
                if time_silent >= STOPPED_SPEAKING_DURATION {
                    None
                } else {
                    state.started_speaking
                }
            };

            let time_speaking = now
                .duration_since(state.started_speaking.unwrap_or(now))
                .saturating_sub(time_silent);

            let event = if time_speaking >= MIN_SPEAKING_HAND_LOWER {
                Some(SpeechEvent::LowerHandSuggestion)
            } else if time_speaking.is_zero() && state.last_speaking_notification.is_some() {
                Some(SpeechEvent::StoppedSpeaking)
            } else {
                None
            };
            if state.last_speaking_notification != event
                && let Some(event) = event
            {
                state
                    .observer
                    .handle_speaking_notification(state.client_id, event);
                state.last_speaking_notification = Some(event);
            }

            state.next_speaking_audio_levels_time = Some(now + state.speaking_interval);
        }

        if let (Some(audio_levels_interval), Some(next_audio_levels_time)) =
            (state.audio_levels_interval, state.next_audio_levels_time)
            && now >= next_audio_levels_time
        {
            let (captured_level, received_levels) = state.peer_connection.get_audio_levels();
            state
                .observer
                .handle_audio_levels(state.client_id, captured_level, received_levels);
            state.next_audio_levels_time = Some(now + audio_levels_interval);
        }

        if state.kind == GroupCallKind::SignalGroup
            && let Some(next_membership_proof_request_time) =
                state.next_membership_proof_request_time
            && now >= next_membership_proof_request_time
        {
            state.observer.request_membership_proof(state.client_id);
            state.next_membership_proof_request_time =
                Some(now + MEMBERSHIP_PROOF_REQUEST_INTERVAL);
        }

        match state.bwe_check_state {
            BweCheckState::At(next_bwe_time) => {
                if state.connection_state == ConnectionState::Connected {
                    if now >= next_bwe_time {
                        let bwe = state.peer_connection.get_last_bandwidth_estimate();
                        if bwe < BWE_THRESHOLD_FOR_LOW_NOTIFICATION {
                            state
                                .observer
                                .handle_low_bandwidth_for_video(state.client_id, false);
                            state.bwe_check_state = BweCheckState::RecoveredAt {
                                next_bwe_time: now + BWE_INTERVAL,
                                last_callback: now,
                            };
                        } else {
                            state.bwe_check_state = BweCheckState::At(now + BWE_INTERVAL);
                        }
                    }
                } else {
                    state.bwe_check_state = BweCheckState::At(now + DELAYED_BWE_CHECK);
                }
            }
            BweCheckState::RecoveredAt {
                next_bwe_time,
                last_callback,
            } => {
                if now >= next_bwe_time && now >= last_callback + DELAY_FOR_RECOVERED_BWE_CALLBACK {
                    let bwe = state.peer_connection.get_last_bandwidth_estimate();
                    if bwe > BWE_THRESHOLD_FOR_RECOVERED_NOTIFICATION {
                        state
                            .observer
                            .handle_low_bandwidth_for_video(state.client_id, true);
                        state.bwe_check_state = BweCheckState::None;
                    } else {
                        state.bwe_check_state = BweCheckState::RecoveredAt {
                            next_bwe_time: now + BWE_INTERVAL,
                            last_callback,
                        };
                    }
                }
            }
            BweCheckState::Disabled => {}
            BweCheckState::None => {}
        }

        if !state.reactions.is_empty() {
            state
                .observer
                .handle_reactions(state.client_id, std::mem::take(&mut state.reactions));
        }

        if let Some(next_raise_hand_time) = state.next_raise_hand_time
            && now >= next_raise_hand_time
            && state.raise_hand_state.outstanding
        {
            state.next_raise_hand_time = Some(now + RAISE_HAND_INTERVAL);
            Self::send_raise_hand(state);
        }

        if let Some(source) = state.mute_request {
            state
                .observer
                .handle_remote_mute_request(state.client_id, source);
            state.mute_request = None;
        }

        let State {
            join_state,
            client_id,
            rtp_data_to_sfu_next_seqnum,
            peer_connection,
            ..
        } = state;
        if let Err(err) = state.sfu_reliable_stream.try_send_ack(|header| {
            let ack = DeviceToSfu {
                mrp_header: Some(header.into()),
                ..Default::default()
            };
            *rtp_data_to_sfu_next_seqnum = Self::unreliable_send_data_inner(
                *join_state,
                *client_id,
                RTP_DATA_TO_SFU_SSRC,
                *rtp_data_to_sfu_next_seqnum,
                peer_connection,
                &ack.encode_to_vec(),
            )?;
            Ok(())
        }) {
            warn!("Failed to send reliable ack to SFU: {:?}", err);
        }

        if let Err(err) = state.sfu_reliable_stream.try_resend(now, |payload| {
            info!("Attempting resend over mrp stream");
            *rtp_data_to_sfu_next_seqnum = Self::unreliable_send_data_inner(
                *join_state,
                *client_id,
                RTP_DATA_TO_SFU_SSRC,
                *rtp_data_to_sfu_next_seqnum,
                peer_connection,
                payload,
            )?;
            Ok(Instant::now() + DEVICE_TO_SFU_TIMEOUT)
        }) {
            warn!("Failed to resend reliable data to SFU: {:?}", err);
        }

        state.actor.send_delayed(TICK_INTERVAL, Self::tick);
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
                now > failure_time + Duration::from_secs(5)
            }
        };
        if should_request_now {
            // We've already requested, so just wait until the next update and then request again.
            debug!("Request remote devices now.");
            let actor = state.actor.clone();
            state.sfu_client.peek(Box::new(move |peek_info| {
                actor.send(move |state| {
                    Self::set_peek_result_inner(state, peek_info, None);
                });
            }));
            state.remote_devices_request_state = RemoteDevicesRequestState::Requested {
                should_request_again: false,
                at: Instant::now(),
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

                    let now = Instant::now();

                    // Start heartbeats, audio levels, and raise hand right away.
                    state.next_heartbeat_time = Some(now);
                    state.next_audio_levels_time = Some(now);
                    state.next_speaking_audio_levels_time = Some(now);
                    state.next_raise_hand_time = Some(now);

                    // Request group membership refresh as we start polling the participant list.
                    if state.kind == GroupCallKind::SignalGroup {
                        state.observer.request_membership_proof(state.client_id);
                        state.next_membership_proof_request_time =
                            Some(now + MEMBERSHIP_PROOF_REQUEST_INTERVAL);

                        // Request the list of all group members
                        state.observer.request_group_members(state.client_id);
                    }

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
        state
            .call_summary
            .on_connection_state_changed(connection_state);
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
                JoinState::Joining | JoinState::Pending(_) | JoinState::Joined(_) => {
                    warn!("Already attempted to join.");
                }
                JoinState::NotJoined(ring_id) => {
                    if Self::take_busy(state) {
                        info!(
                            "ringrtc_stats!,\
                                sfu,\
                                recv,\
                                target_send_rate,\
                                ideal_send_rate,\
                                allocated_send_rate"
                        );
                        StatsObserver::print_headers();

                        Self::set_join_state_and_notify_observer(state, JoinState::Joining);
                        Self::accept_ring_if_needed(state, ring_id);

                        if state.kind == GroupCallKind::SignalGroup {
                            // Request group membership refresh before joining.
                            // The Join request will then proceed once SfuClient has the token.
                            state.observer.request_membership_proof(state.client_id);
                            state.next_membership_proof_request_time =
                                Some(Instant::now() + MEMBERSHIP_PROOF_REQUEST_INTERVAL);
                        }

                        let client_secret = EphemeralSecret::random_from_rng(OsRng);
                        let client_pub_key = PublicKey::from(&client_secret);
                        state.dhe_state = DheState::start(client_secret);
                        state.sfu_client.join(
                            &state.local_ice_ufrag,
                            &state.local_ice_pwd,
                            *client_pub_key.as_bytes(),
                            callback,
                        );
                    } else {
                        Self::end(state, CallEndReason::CallManagerIsBusy);
                    }
                }
            }
        });
    }

    fn accept_ring_if_needed(state: &mut State, ring_id: Option<RingId>) {
        if let Some(ring_id) = ring_id {
            if let Some(self_uuid) = state.self_uuid.lock().expect("can read UUID").clone() {
                let accept_message = protobuf::signaling::CallMessage {
                    ring_response: Some(protobuf::signaling::call_message::RingResponse {
                        group_id: Some(state.group_id.clone()),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Accepted.into(),
                        ),
                    }),
                    ..Default::default()
                };

                state.observer.send_signaling_message(
                    self_uuid,
                    accept_message,
                    SignalingMessageUrgency::HandleImmediately,
                );
            } else {
                error!("self UUID unknown; cannot notify other devices of accept");
            }
        }
    }

    // Pulled into a named private method because it might be called by leave_inner().
    fn set_join_state_and_notify_observer(state: &mut State, join_state: JoinState) {
        debug!(
            "group_call::Client(inner)::set_join_state_and_notify_observer(client_id: {}, join_state: {:?})",
            state.client_id, join_state
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
            "group_call::Client(inner)::leave(client_id: {}, join_state: {:?})",
            state.client_id, state.join_state
        );

        Self::cancel_full_group_ring_if_needed(state);

        let decryption_errors = {
            state
                .frame_crypto_context
                .lock()
                .ok()
                .map(|mut context| context.get_error_stats().clone())
                .unwrap_or_default()
        };
        if !decryption_errors.is_empty() {
            Self::send_decryption_stats_inner(state, decryption_errors);
        }

        match state.join_state {
            JoinState::NotJoined(_) => {
                warn!("Can't leave when not joined.");
                return;
            }
            JoinState::Joining => {
                state.peer_connection.set_outgoing_media_enabled(false);
                state.peer_connection.set_incoming_media_enabled(false);
            }
            JoinState::Pending(local_demux_id) | JoinState::Joined(local_demux_id) => {
                state.peer_connection.set_outgoing_media_enabled(false);
                state.peer_connection.set_incoming_media_enabled(false);
                Self::send_leaving_through_sfu_and_over_signaling(state, local_demux_id);
                Self::send_leave_to_sfu(state);
            }
        }

        Self::release_busy(state);
        Self::set_join_state_and_notify_observer(state, JoinState::NotJoined(None));
        state.next_heartbeat_time = None;
        state.next_stats_time = None;
        state.next_audio_levels_time = None;
        state.next_speaking_audio_levels_time = None;
        state.next_membership_proof_request_time = None;
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
            Self::end(state, CallEndReason::DeviceExplicitlyDisconnected);
        });
    }

    pub fn ring(&self, recipient: Option<UserId>) {
        debug!(
            "group_call::Client(outer)::ring(client_id: {}, recipient: {:?})",
            self.client_id, recipient,
        );
        self.actor
            .send(move |state| Self::ring_inner(state, recipient));
    }

    fn ring_inner(state: &mut State, recipient: Option<UserId>) {
        debug!(
            "group_call::Client(inner)::ring(client_id: {}, recipient: {:?})",
            state.client_id, recipient
        );

        match state.outgoing_ring_state {
            OutgoingRingState::PermittedToRing { ring_id } => {
                let message = protobuf::signaling::CallMessage {
                    ring_intention: Some(protobuf::signaling::call_message::RingIntention {
                        group_id: Some(state.group_id.clone()),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_intention::Type::Ring.into(),
                        ),
                    }),
                    ..Default::default()
                };

                if recipient.is_some() {
                    unimplemented!("cannot ring just one person yet");
                } else {
                    state.observer.send_signaling_message_to_group(
                        state.group_id.clone(),
                        message,
                        SignalingMessageUrgency::HandleImmediately,
                        Default::default(),
                    );

                    if state.remote_devices.is_empty() {
                        // If you're the only one in the call at the time of the ring,
                        // and then you leave before anyone joins, the ring is auto-cancelled.
                        state.outgoing_ring_state = OutgoingRingState::HasSentRing { ring_id };
                    } else {
                        // Otherwise, the ring is sent-and-forgotten.
                        state.outgoing_ring_state = OutgoingRingState::NotPermittedToRing;
                    }
                }
            }
            OutgoingRingState::WantsToRing { .. } => {
                warn!(
                    "repeat ring request not supported (client_id: {}, ring not yet sent)",
                    state.client_id
                );
            }
            OutgoingRingState::HasSentRing { ring_id, .. } => {
                warn!(
                    "repeat ring request not supported (client_id: {}, previous ring id: {})",
                    state.client_id, ring_id
                );
            }
            OutgoingRingState::Unknown => {
                // Need to wait until joining
                state.outgoing_ring_state = OutgoingRingState::WantsToRing { recipient };
            }
            OutgoingRingState::NotPermittedToRing => {
                info!(
                    "ringing is not permitted (client_id: {}); most likely someone else started the call first",
                    state.client_id
                );
            }
        }
    }

    fn set_outgoing_audio_muted_inner(
        state: &mut State,
        muted: bool,
        muted_by_demux_id: Option<DemuxId>,
    ) {
        debug!(
            "group_call::Client(inner)::set_audio_muted(client_id: {}, muted: {})",
            state.client_id, muted
        );
        match (state.outgoing_heartbeat_state.audio_muted, muted) {
            (Some(false) | None, true) => {
                // Only pay attention to the |muted_by_demux_id| if we're moving from an unmuted
                // state to a muted state
                state.outgoing_heartbeat_state.muted_by_demux_id = muted_by_demux_id;
            }
            // Do nothing if transitioning from muted -> muted -- keep the attribution.
            (Some(true), true) => {}
            // If unmuting, clear the "muted by" attribution
            (_, false) => {
                state.outgoing_heartbeat_state.muted_by_demux_id = None;
            }
        }
        // We don't modify the outgoing audio track.  We expect the app to handle that.
        state.outgoing_heartbeat_state.audio_muted = Some(muted);
        if let Err(err) = Self::send_heartbeat(state) {
            warn!(
                "Failed to send heartbeat after updating audio mute state: {:?}",
                err
            );
        }
    }

    pub fn set_outgoing_audio_muted(&self, muted: bool) {
        debug!(
            "group_call::Client(outer)::set_audio_muted(client_id: {}, muted: {})",
            self.client_id, muted
        );
        self.actor.send(move |state| {
            Self::set_outgoing_audio_muted_inner(state, muted, None);
        });
    }

    pub fn set_outgoing_audio_muted_remotely(&self, source: DemuxId) {
        debug!(
            "group_call::Client(outer)::set_audio_muted_remotely(client_id: {}, source: {})",
            self.client_id, source
        );
        self.actor.send(move |state| {
            Self::set_outgoing_audio_muted_inner(state, true, Some(source));
        });
    }

    pub fn send_remote_mute_request(&self, target: DemuxId) {
        debug!(
            "group_call::Client(outer)::send_remote_mute_request(client_id: {}, target: {})",
            self.client_id, target
        );
        use crate::protobuf::group_call::{DeviceToDevice, RemoteMuteRequest};
        let msg = DeviceToDevice {
            remote_mute_request: Some(RemoteMuteRequest {
                target_demux_id: Some(target),
            }),
            ..Default::default()
        };
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::send_remote_mute_request(client_id: {}, target:{}",
                state.client_id, target
            );
            match state.join_state {
                JoinState::Pending(our_demux_id) | JoinState::Joined(our_demux_id) => {
                    if our_demux_id == target {
                        error!("Refusing to send remote mute request to self");
                        return;
                    }
                }
                _ => {}
            }
            if let Err(err) = Self::broadcast_data_through_sfu(state, &msg.encode_to_vec()) {
                warn!("Failed to send remote mute request: {:?}", err);
            }
        })
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
            state.outgoing_heartbeat_state.video_muted = Some(muted);
            if let Err(err) = Self::send_heartbeat(state) {
                warn!(
                    "Failed to send heartbeat after updating video mute state: {:?}",
                    err
                );
            }
        });
    }

    pub fn set_presenting(&self, presenting: bool) {
        debug!(
            "group_call::Client(outer)::set_presenting(client_id: {}, presenting: {})",
            self.client_id, presenting
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_presenting(client_id: {}, presenting: {})",
                state.client_id, presenting
            );
            state.outgoing_heartbeat_state.presenting = Some(presenting);
            if let Err(err) = Self::send_heartbeat(state) {
                warn!(
                    "Failed to send heartbeat after updating presenting state: {:?}",
                    err
                );
            }
        });
    }

    pub fn set_sharing_screen(&self, sharing_screen: bool) {
        debug!(
            "group_call::Client(outer)::set_sharing_screen(client_id: {}, sharing_screen: {})",
            self.client_id, sharing_screen
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_sharing_screen(client_id: {}, sharing_screen: {})",
                state.client_id, sharing_screen
            );
            state.outgoing_heartbeat_state.sharing_screen = Some(sharing_screen);
            if let Err(err) = Self::send_heartbeat(state) {
                warn!(
                    "Failed to send heartbeat after updating sharing screen state: {:?}",
                    err
                );
            }
            let send_rates = Self::compute_send_rates(state.remote_devices.len(), sharing_screen);
            Self::set_send_rates_inner(state, send_rates);
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

            match state.join_state {
                JoinState::NotJoined(_) | JoinState::Joining => {
                    // Wait until we've at least completed our join request to send media keys.
                }
                JoinState::Pending(local_demux_id) | JoinState::Joined(local_demux_id) => {
                    let user_ids: HashSet<UserId> = state
                        .remote_devices
                        .iter()
                        .map(|rd| rd.user_id.clone())
                        .collect();

                    let (ratchet_counter, secret) = {
                        let frame_crypto_context = state.frame_crypto_context.lock().expect(
                            "Get lock for frame encryption context to advance media send key",
                        );
                        frame_crypto_context.send_state()
                    };

                    info!(
                        "Resending media keys to everyone (number of users: {})",
                        user_ids.len()
                    );
                    Self::send_media_send_key_to_users_over_signaling(
                        state,
                        user_ids,
                        local_demux_id,
                        ratchet_counter,
                        secret,
                        None,
                    );
                }
            }
        });
    }

    pub fn set_data_mode(&self, data_mode: DataMode) {
        debug!(
            "group_call::Client(outer)::set_data_mode(client_id: {}, data_mode: {:?})",
            self.client_id, data_mode
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::set_data_mode(client_id: {}), data_mode: {:?}",
                state.client_id, data_mode,
            );

            state.max_receive_rate = Some(match data_mode {
                DataMode::Low => LOW_MAX_RECEIVE_RATE,
                DataMode::Normal => NORMAL_MAX_RECEIVE_RATE,
                DataMode::Custom {
                    max_group_call_receive_rate,
                    ..
                } => max_group_call_receive_rate,
            });

            state.data_mode = data_mode;

            if !state.on_demand_video_request_sent_since_last_heartbeat {
                Self::send_video_requests_to_sfu(state);
                state.on_demand_video_request_sent_since_last_heartbeat = true;
            }
        });
    }

    fn set_send_rates_inner(state: &mut State, mut send_rates: SendRates) {
        if let Some(send_rates_override) = &state.send_rates_override {
            send_rates = send_rates_override.clone();
        }
        if state.send_rates != send_rates {
            if send_rates.max == Some(ALL_ALONE_MAX_SEND_RATE) {
                info!("Disable audio and outgoing media because there are no other devices.");
                state.peer_connection.set_audio_recording_enabled(false);
                state.peer_connection.set_outgoing_media_enabled(false);
                state.peer_connection.set_audio_playout_enabled(false);
                if let BweCheckState::At(_) = state.bwe_check_state {
                    state.bwe_check_state = BweCheckState::Disabled;
                }
            } else {
                info!("Enable audio and outgoing media because there are other devices.");
                state.peer_connection.set_audio_playout_enabled(true);
                state.peer_connection.set_outgoing_media_enabled(true);
                state.peer_connection.set_audio_recording_enabled(true);
                if state.bwe_check_state == BweCheckState::Disabled {
                    state.bwe_check_state = BweCheckState::At(Instant::now() + BWE_INTERVAL);
                }
            }
            if let Err(e) = state.peer_connection.set_send_rates(send_rates.clone()) {
                warn!("Could not set send rates to {:?}: {}", send_rates, e);
            } else {
                info!("Setting send rates to {:?}", send_rates);
                state
                    .observer
                    .handle_send_rates_changed(state.client_id, send_rates.clone());
                state.send_rates = send_rates;
            }
        }
    }

    pub fn request_video(&self, requests: Vec<VideoRequest>, active_speaker_height: u16) {
        debug!(
            "group_call::Client(outer)::request_video(client_id: {}, requests: {:?}, active_speaker_height: {})",
            self.client_id, requests, active_speaker_height,
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::request_video(client_id: {})",
                state.client_id
            );
            state.video_requests = Some(requests);
            state.active_speaker_height = Some(active_speaker_height);
            if !state.on_demand_video_request_sent_since_last_heartbeat {
                Self::send_video_requests_to_sfu(state);
                state.on_demand_video_request_sent_since_last_heartbeat = true;
            }
        });
    }

    fn send_video_requests_to_sfu(state: &mut State) {
        use std::cmp::min;

        use protobuf::group_call::device_to_sfu::{
            VideoRequestMessage, video_request_message::VideoRequest as VideoRequestProto,
        };

        if let Some(video_requests) = &state.video_requests {
            let requests: Vec<_> = video_requests
                .iter()
                .filter_map(|request| {
                    state
                        .remote_devices
                        .find_by_demux_id(request.demux_id)
                        .map(|device| {
                            VideoRequestProto {
                                demux_id: Some(device.demux_id),
                                // We use the min because the SFU does not understand the concept of video rotation
                                // so all requests must be in terms of non-rotated video even though the apps
                                // will request in terms of rotated video.  We assume that all video is sent over the
                                // wire in landscape format with rotation metadata.
                                // If it's not, we'll have a problem.
                                height: Some(min(request.height, request.width) as u32),
                            }
                        })
                })
                .collect();
            let msg = DeviceToSfu {
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
                    max_kbps: state.max_receive_rate.map(|rate| rate.as_kbps() as u32),
                    requests,
                    active_speaker_height: state.active_speaker_height.map(|height| height.into()),
                }),
                ..Default::default()
            };

            if let Err(e) = Self::unreliable_send_data_to_sfu(state, &msg.encode_to_vec()) {
                warn!("Failed to send video request: {:?}", e);
            }
        }
    }

    fn approve_or_deny_user(state: &mut State, user_id: UserId, approved: bool) {
        use protobuf::group_call::device_to_sfu::{AdminAction, GenericAdminAction};

        // Approval is implemented by demux ID (because we don't put user IDs in RTP messages).
        // So we have to find a corresponding demux ID in the pending users list.
        let Some(peek_info) = state.last_peek_info.as_ref() else {
            error!("{ADMIN_LOG_TAG}: Cannot approve users without peek info");
            return;
        };

        let action_to_log = if approved { "approval" } else { "denial" };

        if let Some(demux_id) = peek_info
            .pending_devices
            .iter()
            .find(|device| device.user_id.as_ref() == Some(&user_id))
            .map(|device| device.demux_id)
        {
            let action = if approved {
                AdminAction::Approve
            } else {
                AdminAction::Deny
            };
            let msg = DeviceToSfu {
                admin_action: Some((action)(GenericAdminAction {
                    target_demux_id: Some(demux_id),
                })),
                ..Default::default()
            };

            if let Err(e) = Self::reliable_send_device_to_sfu(state, msg) {
                warn!(
                    "{ADMIN_LOG_TAG}: Failed to send {action_to_log} for demux {demux_id}: {e:?}"
                );
            } else {
                info!("{ADMIN_LOG_TAG}: Sent {action_to_log} for {demux_id}");
            }
        } else if let Some(demux_id) = peek_info
            .devices
            .iter()
            .find(|device| device.user_id.as_ref() == Some(&user_id))
            .map(|device| device.demux_id)
        {
            info!("{ADMIN_LOG_TAG}: User has already been added to call with demux ID {demux_id}");
        } else {
            warn!(
                "{ADMIN_LOG_TAG}: Failed to find user for {action_to_log}. They may have left or been denied by another admin."
            );
        }
    }

    pub fn approve_user(&self, user_id: UserId) {
        debug!(
            "group_call::Client(outer)::approve_user(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::approve_user(client_id: {})",
                state.client_id
            );
            Self::approve_or_deny_user(state, user_id, true);
        });
    }

    pub fn deny_user(&self, user_id: UserId) {
        debug!(
            "group_call::Client(outer)::deny_user(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::deny_user(client_id: {})",
                state.client_id
            );
            Self::approve_or_deny_user(state, user_id, false);
        });
    }

    pub fn remove_client(&self, other_client: DemuxId) {
        use protobuf::group_call::device_to_sfu::{AdminAction, GenericAdminAction};
        debug!(
            "group_call::Client(outer)::remove_client(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::remove_client(client_id: {})",
                state.client_id
            );

            // We could check that other_client is a valid demux ID according to our current peek
            // info, but that's a racy check anyway. Just let the calling server do it.
            let msg = DeviceToSfu {
                admin_action: Some(AdminAction::Remove(GenericAdminAction {
                    target_demux_id: Some(other_client),
                })),
                ..Default::default()
            };

            if let Err(e) = Self::reliable_send_device_to_sfu(state, msg) {
                warn!("{ADMIN_LOG_TAG}: Failed to send removal for {other_client}: {e:?}");
            } else {
                info!("{ADMIN_LOG_TAG}: Sent removal for {other_client}.");
            }
        });
    }

    // Blocks are performed on a particular client, but end up affecting all of the user's devices.
    // Still, we define it as a demux-ID-based operation for more flexibility later.
    pub fn block_client(&self, other_client: DemuxId) {
        use protobuf::group_call::device_to_sfu::{AdminAction, GenericAdminAction};
        debug!(
            "group_call::Client(outer)::block_client(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::block_client(client_id: {})",
                state.client_id
            );

            // We could check that other_client is a valid demux ID according to our current peek
            // info, but that's a racy check anyway. Just let the calling server do it.
            let msg = DeviceToSfu {
                admin_action: Some(AdminAction::Block(GenericAdminAction {
                    target_demux_id: Some(other_client),
                })),
                ..Default::default()
            };

            if let Err(e) = Self::reliable_send_device_to_sfu(state, msg) {
                warn!("{ADMIN_LOG_TAG}: Failed to send block for {other_client}: {e:?}");
            } else {
                info!("{ADMIN_LOG_TAG}: Sent block for {other_client}");
            }
        });
    }

    pub fn set_group_members(&self, group_members: Vec<GroupMember>) {
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
                state
                    .obfuscated_resolver
                    .set_member_resolver(Arc::new(MemberMap::new(&group_members)));
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

    pub fn react(&self, value: String) {
        debug!(
            "group_call::Client(outer)::react(client_id: {} value: {})",
            self.client_id, value
        );

        if value.is_empty() {
            warn!("group_call::Client(outer)::react value is empty");
        } else if value.len() > REACTION_STRING_MAX_SIZE {
            warn!(
                "group_call::Client(outer)::react reaction value size of {} exceeded allowed size of {}",
                value.len(),
                REACTION_STRING_MAX_SIZE
            );
        } else {
            self.actor.send(move |state| {
                debug!(
                    "group_call::Client(inner)::react(client_id: {}, value: {})",
                    state.client_id, value
                );
                if let Err(err) = Self::send_reaction(state, value) {
                    warn!("Failed to send reaction: {:?}", err);
                }
            });
        }
    }

    pub fn raise_hand(&self, raise: bool) {
        debug!(
            "group_call::Client(outer)::raise_hand(client_id: {} raise: {})",
            self.client_id, raise
        );

        self.actor.send(move |state| {
            state.raise_hand_state.seqnum += 1;
            state.raise_hand_state.raise = raise;
            state.raise_hand_state.outstanding = true;

            info!(
                "group_call::Client(inner)::raise_hand(client_id: {}, raise: {} seqnum: {})",
                state.client_id, state.raise_hand_state.raise, state.raise_hand_state.seqnum
            );

            Self::send_raise_hand(state);
        });
    }

    // Pulled into a named private method because it can be called in many places.
    fn end(state: &mut State, reason: CallEndReason) {
        debug!(
            "group_call::Client(inner)::end(client_id: {})",
            state.client_id
        );

        match state.join_state {
            JoinState::NotJoined(_) => {
                // Nothing to do.
            }
            JoinState::Joining | JoinState::Pending(_) | JoinState::Joined(_) => {
                // This will send an update after changing the join state.
                Self::leave_inner(state);
            }
        };

        match state.connection_state {
            ConnectionState::NotConnected => {
                warn!("Can't disconnect when not connected.");
            }
            ConnectionState::Connecting
            | ConnectionState::Connected
            | ConnectionState::Reconnecting => {
                // We need to finish the disconnection, but we might have sent out RTP
                // packets for the leave signal. Wait for a short delay before closing
                // the peer connection and cleaning up.
                let actor = state.actor.clone();
                actor.send_delayed(CLIENT_END_DELAY, move |state| {
                    state.peer_connection.close();
                    Self::set_connection_state_and_notify_observer(
                        state,
                        ConnectionState::NotConnected,
                    );
                    let _join_handles = state.actor.stopper().stop_all_without_joining();
                    state.observer.handle_ended(
                        state.client_id,
                        reason,
                        state.call_summary.build_call_summary(reason),
                    );
                });
            }
        }
    }

    fn on_sfu_client_join_success(state: &mut State, joined: Joined) {
        match state.connection_state {
            ConnectionState::NotConnected => {
                warn!("The SFU completed joining before connect() was requested.");
            }
            ConnectionState::Connecting => {
                state.dhe_state.negotiate_in_place(
                    &PublicKey::from(joined.server_dhe_pub_key),
                    &joined.hkdf_extra_info,
                );
                let srtp_keys = match &state.dhe_state {
                    DheState::Negotiated { srtp_keys } => srtp_keys,
                    _ => {
                        Self::end(state, CallEndReason::FailedToNegotiatedSrtpKeys);
                        return;
                    }
                };

                if Self::start_peer_connection(
                    state,
                    &joined.sfu_info,
                    joined.local_demux_id,
                    srtp_keys,
                )
                .is_err()
                {
                    Self::end(state, CallEndReason::FailedToStartPeerConnection);
                    return;
                };

                // Set a low bitrate until we learn someone else is in the call.
                Self::set_send_rates_inner(
                    state,
                    SendRates {
                        max: Some(ALL_ALONE_MAX_SEND_RATE),
                        ..SendRates::default()
                    },
                );

                state.sfu_info = Some(joined.sfu_info);
            }
            ConnectionState::Connected | ConnectionState::Reconnecting => {
                warn!("The SFU completed joining after already being connected.");
            }
        };
        match state.join_state {
            JoinState::NotJoined(_) => {
                warn!("The SFU completed joining before join() was requested.");
            }
            JoinState::Joining => {
                // We just now appeared in the participants list (unless we're pending
                // approval) and possibly even updated the eraId. Request this before doing
                // anything else because it'll take a while for the app to get back to us.
                Self::request_remote_devices_as_soon_as_possible(state);

                // The call to set_peek_result_inner needs the demux ID to be set in the
                // join state. But make sure to fire observer.handle_join_state_changed
                // after set_peek_result_inner so that state.remote_devices are filled in.
                state.join_state = joined.join_state;
                if let Some(peek_info) = &state.last_peek_info {
                    // TODO: Do the same processing without making it look like we just
                    // got an update from the server even though the update actually came
                    // from earlier.  For now, it's close enough.
                    let peek_info = peek_info.clone();
                    Self::set_peek_result_inner(state, Ok(peek_info), None);
                    if state.remote_devices.is_empty() {
                        // If there are no remote devices, then Self::set_peek_result_inner
                        // will not fire handle_remote_devices_changed and the observer can't tell the difference
                        // between "we know we have no remote devices" and "we don't know what we have yet".
                        // This way, the observer can.
                        state.observer.handle_remote_devices_changed(
                            state.client_id,
                            &state.remote_devices,
                            RemoteDevicesChangedReason::DemuxIdsChanged,
                        );
                    }
                }

                // Just in case, check if the cached peek info happened to have the local
                // device in it already (possible if the peek raced with the join request).
                // In that case, set_peek_info_inner will have notified the observer about
                // the join state change already.
                state
                    .observer
                    .handle_join_state_changed(state.client_id, state.join_state);

                // Check state.join_state to make sure we didn't process an `end()` since receiving the response.
                // We need to check the response's `join_state` since `peek_result_inner` can transition
                // the call to joined and have already called `on_client_joined`
                if matches!(joined.join_state, JoinState::Joined(_))
                    && matches!(state.join_state, JoinState::Joined(_))
                {
                    Self::on_client_joined(state);
                }

                if joined.creator.is_some() {
                    // Check if we're permitted to ring
                    let creator_is_self = {
                        let self_uuid_guard = state.self_uuid.lock();
                        self_uuid_guard
                            .map(|guarded_uuid| joined.creator == *guarded_uuid)
                            .unwrap_or(false)
                    };
                    let new_ring_state = if creator_is_self {
                        OutgoingRingState::PermittedToRing {
                            ring_id: RingId::from_era_id(&joined.era_id),
                        }
                    } else {
                        OutgoingRingState::NotPermittedToRing
                    };
                    debug!("updating ring state to {:?}", new_ring_state);
                    let previous_ring_state =
                        std::mem::replace(&mut state.outgoing_ring_state, new_ring_state);
                    if let OutgoingRingState::WantsToRing { recipient } = previous_ring_state {
                        Self::ring_inner(state, recipient)
                    }
                }

                state.next_stats_time = Some(Instant::now() + STATS_INITIAL_OFFSET);
                state.next_decryption_error_time = Some(Instant::now() + DECRYPTION_ERROR_INTERVAL);
            }
            JoinState::Pending(_) | JoinState::Joined(_) => {
                warn!("The SFU completed joining more than once.");
            }
        };
    }

    fn on_sfu_client_join_failure(state: &mut State, err: anyhow::Error) {
        // Map the error to an appropriate end reason.
        let end_reason = err.downcast_ref::<RingRtcError>().map_or_else(
            || {
                error!("Unexpected error: {}", err);
                CallEndReason::SfuClientFailedToJoin
            },
            |err| match err {
                RingRtcError::GroupCallFull => CallEndReason::HasMaxDevices,
                _ => CallEndReason::SfuClientFailedToJoin,
            },
        );
        Self::end(state, end_reason);
    }

    // Called by the SfuClient after a join attempt completes.
    pub fn on_sfu_client_join_attempt_completed(&self, join_result: Result<Joined>) {
        debug!(
            "group_call::Client(outer)::on_sfu_client_join_attempt_completed(client_id: {})",
            self.client_id
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::on_sfu_client_join_attempt_completed(client_id: {})",
                state.client_id
            );
            match join_result {
                Ok(joined) => {
                    Self::on_sfu_client_join_success(state, joined);
                }
                Err(err) => {
                    warn!("Failed to join group call: {}", err);
                    Self::on_sfu_client_join_failure(state, err);
                }
            }
        });
    }

    // Called once per call, when the client transitions to JoinState::Joined.
    // Currently, this occurs via on_sfu_client_joined (Joining -> Joined) or
    // or via peek_result_inner (Joining -> Pending -> Joined)
    fn on_client_joined(state: &mut State) {
        state
            .peer_connection
            .configure_audio_encoders(&AudioEncoderConfig::default());
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
        srtp_keys: &SrtpKeys,
    ) -> Result<()> {
        debug!(
            "group_call::Client(inner)::start_peer_connection(client_id: {})",
            state.client_id
        );

        Self::set_peer_connection_descriptions(state, sfu_info, local_demux_id, srtp_keys)?;

        for addr in &sfu_info.udp_addresses {
            // We use the octets instead of to_string() to bypass the IP address logging filter.
            info!(
                "Connecting to group call SFU via UDP with ip={:?} port={}",
                match addr.ip() {
                    std::net::IpAddr::V4(v4) => v4.octets().to_vec(),
                    std::net::IpAddr::V6(v6) => v6.octets().to_vec(),
                },
                addr.port()
            );
            state.peer_connection.add_ice_candidate_from_server(
                addr.ip(),
                addr.port(),
                Protocol::Udp,
            )?;
        }

        for addr in &sfu_info.tcp_addresses {
            // We use the octets instead of to_string() to bypass the IP address logging filter.
            info!(
                "Connecting to group call SFU via TCP with ip={:?} port={}",
                match addr.ip() {
                    std::net::IpAddr::V4(v4) => v4.octets().to_vec(),
                    std::net::IpAddr::V6(v6) => v6.octets().to_vec(),
                },
                addr.port()
            );
            state.peer_connection.add_ice_candidate_from_server(
                addr.ip(),
                addr.port(),
                Protocol::Tcp,
            )?;
        }

        for addr in &sfu_info.tls_addresses {
            if let Some(hostname) = &sfu_info.hostname {
                // We use the octets instead of to_string() to bypass the IP address logging filter.
                info!(
                    "Connecting to group call SFU via TLS with ip={:?} port={} hostname={}",
                    match addr.ip() {
                        std::net::IpAddr::V4(v4) => v4.octets().to_vec(),
                        std::net::IpAddr::V6(v6) => v6.octets().to_vec(),
                    },
                    addr.port(),
                    &hostname
                );
                state.peer_connection.add_ice_candidate_from_server(
                    addr.ip(),
                    addr.port(),
                    Protocol::Tls(hostname),
                )?;
            }
        }

        if state
            .peer_connection
            .receive_rtp(RTP_DATA_PAYLOAD_TYPE, true)
            .is_err()
        {
            warn!("Could not tell PeerConnection to receive RTP");
        }

        Ok(())
    }

    #[cfg(test)]
    pub fn set_peek_result(&self, result: PeekResult) {
        debug!(
            "group_call::Client(outer)::set_peek_result: {}, result: {:?})",
            self.client_id, result
        );

        self.actor.send(move |state| {
            Self::set_peek_result_inner(state, result, None);
        });
    }

    pub fn set_rtc_stats_interval(&self, interval: Duration) {
        info!(
            "group_call::Client(outer)::set_rtc_stats_interval: {}, interval: {:?})",
            self.client_id, interval
        );

        self.actor.send(move |state| {
            let old_stats_interval = state.get_stats_interval;
            state.get_stats_interval = if interval.is_zero() {
                state.stats_observer.set_collect_raw_stats_report(false);
                DEFAULT_STATS_INTERVAL
            } else {
                state.stats_observer.set_collect_raw_stats_report(true);
                interval
            };

            state.next_stats_time = state
                .next_stats_time
                .map(|stats_time| stats_time - old_stats_interval + state.get_stats_interval);
        });
    }

    // Most of the logic moved to inner method so this can be called by both
    // set_peek_result() and as a callback to SfuClient::request_remote_devices.
    fn set_peek_result_inner(
        state: &mut State,
        result: PeekResult,
        endorsements_expiration: Option<Timestamp>,
    ) {
        debug!(
            "group_call::Client(inner)::set_peek_result_inner(client_id: {}, result: {:?} state: {:?})",
            state.client_id, result, state.remote_devices_request_state
        );

        if let Err(e) = result {
            warn!("Failed to request remote devices from SFU: {:?}", e);
            state.remote_devices_request_state =
                RemoteDevicesRequestState::Failed { at: Instant::now() };
            return;
        }
        let peek_info = result.unwrap();

        let is_first_peek_info = state.last_peek_info.is_none();
        let should_request_again = matches!(
            state.remote_devices_request_state,
            RemoteDevicesRequestState::Requested {
                should_request_again: true,
                ..
            }
        );
        state.remote_devices_request_state =
            RemoteDevicesRequestState::Updated { at: Instant::now() };

        let old_user_ids: HashSet<UserId> = std::mem::take(&mut state.joined_members);
        let new_user_ids: HashSet<UserId> = peek_info
            .devices
            .iter()
            // Note: this ignores users that aren't in the group, but does include ourselves.
            // This is relevant because we may have multiple devices in the call.
            .filter_map(|device| device.user_id.clone())
            .collect();

        // When would this combined hash falsely claim that the set of pending users hasn't changed?
        // If the combined hash of the user IDs that have been added and removed since the last peek
        // comes out to the exact bit-pattern needed to match the change in `pending_devices.len()`.
        // For example, if one person left and one person joined the pending list, their user IDs
        // would have to have hashes of `x` and `-x`, so that combined they equal 0. This is
        // extremely unlikely.
        let new_pending_users_signature = peek_info
            .unique_pending_users()
            .into_iter()
            .map(|user_id| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                user_id.hash(&mut hasher);
                hasher.finish()
            })
            .fold(peek_info.pending_devices.len() as u64, |a, b| {
                // Note that this is an order-independent fold, so that two differently-ordered
                // HashSets produce the same signature.
                a.wrapping_add(b)
            });

        let pending_users_changed = state.pending_users_signature != new_pending_users_signature;

        let old_era_id = state
            .last_peek_info
            .as_ref()
            .and_then(|peek_info| peek_info.era_id.as_ref());

        if is_first_peek_info
            || old_user_ids != new_user_ids
            || old_era_id != peek_info.era_id.as_ref()
            || pending_users_changed
        {
            state
                .observer
                .handle_peek_changed(state.client_id, &peek_info, &new_user_ids);
        }

        if let (
            JoinState::Pending(local_demux_id) | JoinState::Joined(local_demux_id),
            DheState::Negotiated { srtp_keys },
        ) = (&state.join_state, &state.dhe_state)
        {
            let local_demux_id = *local_demux_id;
            // We remember these before changing state.remote_devices so we can calculate changes after.
            let old_demux_ids: HashSet<DemuxId> = state.remote_devices.demux_id_set();

            // Then we update state.remote_devices by first building a map of demux_id => RemoteDeviceState
            // from the old values and then building a new Vec using either the old value (if there is one)
            // or creating a new one.
            let mut old_remote_devices_by_demux_id: HashMap<DemuxId, RemoteDeviceState> =
                std::mem::take(&mut state.remote_devices)
                    .into_iter()
                    .map(|rd| (rd.demux_id, rd))
                    .collect();
            let added_time = SystemTime::now();
            let mut local_device_is_participant = false;
            state.remote_devices = peek_info
                .devices
                .iter()
                .filter_map(|device| {
                    if device.demux_id == local_demux_id {
                        local_device_is_participant = true;
                        // Don't add a remote device to represent the local device.
                        return None;
                    }
                    device.user_id.as_ref().map(|user_id| {
                        // Keep the old one, with its state, if there is one and the user ID
                        // matches.
                        if let Some(existing_remote_device) =
                            old_remote_devices_by_demux_id.remove(&device.demux_id)
                            && &existing_remote_device.user_id == user_id
                        {
                            return existing_remote_device;
                        }
                        RemoteDeviceState::new(device.demux_id, user_id.clone(), added_time)
                    })
                })
                .collect();

            // Recalculate to see the differences
            let new_demux_ids: HashSet<DemuxId> = state.remote_devices.demux_id_set();

            let added_demux_ids: HashSet<DemuxId> =
                new_demux_ids.difference(&old_demux_ids).copied().collect();

            let demux_ids_changed = old_demux_ids != new_demux_ids;
            // If demux IDs changed, let the PeerConnection know that related SSRCs changed as well
            if demux_ids_changed {
                info!(
                    "New set of demux IDs to be pushed down to PeerConnection: {:?}",
                    new_demux_ids
                );
                if let Some(sfu_info) = state.sfu_info.as_ref() {
                    let mut removed_demux_id = false;
                    for demux_id in &mut state.remote_transceiver_demux_ids {
                        if let Some(id) = demux_id
                            && !new_demux_ids.contains(id)
                        {
                            *demux_id = None;
                            removed_demux_id = true;
                        }
                    }

                    if removed_demux_id {
                        // Apply demux ID removals separately from additions. This ensures that
                        // transceivers are transitioned to the inactive direction before trying to
                        // reuse them.
                        //
                        // Without this, a transceiver could persist the receiving direction across
                        // a change in the associated demux ID. When that happens,
                        // PeerConnectionObserver::OnTrack won't be called for the new demux ID
                        // when the remote description is applied.
                        let result = Self::set_peer_connection_descriptions(
                            state,
                            sfu_info,
                            local_demux_id,
                            srtp_keys,
                        );
                        if result.is_err() {
                            Self::end(state, CallEndReason::FailedToUpdatePeerConnection);
                            return;
                        }
                    }

                    if !added_demux_ids.is_empty() {
                        let mut added_demux_ids_iter = added_demux_ids.iter().copied();
                        for demux_id in &mut state.remote_transceiver_demux_ids {
                            // If demux_id is None, that means that there's an empty space (from a
                            // previously removed demux ID) in remote_transceiver_demux_ids that can be
                            // used. If demux_id is Some, only replace it with a newly added demux ID
                            // if it is being removed now (it's not in new_demux_ids).
                            if demux_id.is_none_or(|id| !new_demux_ids.contains(&id)) {
                                *demux_id = added_demux_ids_iter.next();
                            }
                        }

                        // Add any remaining new demux IDs to remote_transceiver_demux_ids.
                        state
                            .remote_transceiver_demux_ids
                            .extend(added_demux_ids_iter.map(Some));

                        let result = Self::set_peer_connection_descriptions(
                            state,
                            sfu_info,
                            local_demux_id,
                            srtp_keys,
                        );
                        if result.is_err() {
                            Self::end(state, CallEndReason::FailedToUpdatePeerConnection);
                            return;
                        }
                    }
                }
            }

            if pending_users_changed {
                let demux_ids: Vec<String> = peek_info
                    .pending_devices
                    .iter()
                    .map(|pd| pd.demux_id.to_string())
                    .collect();
                info!(
                    "Pending users changed ({} total): {:?}",
                    demux_ids.len(),
                    demux_ids
                );
            }

            if demux_ids_changed {
                state.observer.handle_remote_devices_changed(
                    state.client_id,
                    &state.remote_devices,
                    RemoteDevicesChangedReason::DemuxIdsChanged,
                );
                state
                    .call_summary
                    .on_remote_devices_changed(&state.remote_devices);
            }
            // Make sure not to notify for the updated join state until the remote devices have been
            // updated.
            if local_device_is_participant && matches!(state.join_state, JoinState::Pending(_)) {
                Self::set_join_state_and_notify_observer(state, JoinState::Joined(local_demux_id));
                Self::on_client_joined(state);
            }

            // If someone was added, we must advance the send media key
            // and send it to everyone that was added.
            let users_with_added_devices: HashSet<UserId> = state
                .remote_devices
                .iter()
                .filter(|device| added_demux_ids.contains(&device.demux_id))
                .map(|device| device.user_id.clone())
                .collect();
            if !users_with_added_devices.is_empty() {
                Self::advance_media_send_key_and_send_to_users_with_added_devices(
                    state,
                    users_with_added_devices.clone(),
                    endorsements_expiration,
                );
                Self::send_pending_media_send_key_to_users_with_added_devices(
                    state,
                    users_with_added_devices,
                    endorsements_expiration,
                );
            }

            // If someone was removed, we must reset the send media key and send it to everyone not removed.
            if old_user_ids.difference(&new_user_ids).next().is_some() {
                Self::rotate_media_send_key_and_send_to_users_not_removed(
                    state,
                    endorsements_expiration,
                );
            }

            // We can't gate this behind the demux IDs changing because a forged demux ID might
            // be in there already when the non-forged one comes in.
            let pending_receive_keys = std::mem::take(&mut state.pending_media_receive_keys);
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

            if local_device_is_participant {
                let send_rates = Self::compute_send_rates(
                    new_demux_ids.len(),
                    state
                        .outgoing_heartbeat_state
                        .sharing_screen
                        .unwrap_or(false),
                );
                Self::set_send_rates_inner(state, send_rates);
            }

            // If anyone has joined besides us, we won't cancel the ring on leave.
            if !new_demux_ids.is_empty()
                && matches!(
                    state.outgoing_ring_state,
                    OutgoingRingState::HasSentRing { .. }
                )
            {
                state.outgoing_ring_state = OutgoingRingState::NotPermittedToRing;
            }
        }
        state.last_peek_info = Some(peek_info);

        // Do this later so that we can use new_user_ids above without running into
        // referencing issues
        state.joined_members = new_user_ids;
        state.pending_users_signature = new_pending_users_signature;

        if should_request_again {
            // Something occurred while we were waiting for this update.
            // We should request again.
            debug!("Request devices because we previously requested while a request was pending");
            Self::request_remote_devices_as_soon_as_possible(state);
        }
    }

    // Returns (min, start, max)
    fn compute_send_rates(joined_member_count: usize, sharing_screen: bool) -> SendRates {
        match (joined_member_count, sharing_screen) {
            (0, _) => SendRates {
                max: Some(ALL_ALONE_MAX_SEND_RATE),
                ..SendRates::default()
            },
            (_, true) => SendRates {
                min: Some(SCREENSHARE_MIN_SEND_RATE),
                start: Some(SCREENSHARE_START_SEND_RATE),
                max: Some(SCREENSHARE_MAX_SEND_RATE),
            },
            (1..=7, _) => SendRates {
                max: Some(SMALL_CALL_MAX_SEND_RATE),
                ..SendRates::default()
            },
            _ => SendRates {
                max: Some(LARGE_CALL_MAX_SEND_RATE),
                ..SendRates::default()
            },
        }
    }

    // Pulled into a named private method because it might be called by set_peek_result
    fn set_peer_connection_descriptions(
        state: &State,
        sfu_info: &SfuInfo,
        local_demux_id: DemuxId,
        srtp_keys: &SrtpKeys,
    ) -> Result<()> {
        let remote_demux_ids = state
            .remote_transceiver_demux_ids
            .iter()
            .map(|id| id.unwrap_or(0))
            .collect::<Vec<_>>();

        state
            .peer_connection
            .update_transceivers(&remote_demux_ids)?;

        // Call create_offer for the side effect of setting up the state of the RtpTransceivers
        // potentially created above.
        let observer = create_csd_observer();
        state.peer_connection.create_offer(observer.as_ref());
        let _ = observer.get_result()?;

        let local_description = SessionDescription::local_for_group_call(
            &state.local_ice_ufrag,
            &state.local_ice_pwd,
            &srtp_keys.client,
            Some(local_demux_id),
            &remote_demux_ids,
        )?;
        let observer = create_ssd_observer();
        state
            .peer_connection
            .set_local_description(observer.as_ref(), local_description);
        observer.get_result()?;

        let remote_description = SessionDescription::remote_for_group_call(
            &sfu_info.ice_ufrag,
            &sfu_info.ice_pwd,
            &srtp_keys.server,
            local_demux_id,
            &remote_demux_ids,
        )?;
        let observer = create_ssd_observer();
        state
            .peer_connection
            .set_remote_description(observer.as_ref(), remote_description);
        observer.get_result()?;
        Ok(())
    }

    fn rotate_media_send_key_and_send_to_users_not_removed(
        state: &mut State,
        endorsements_expiration: Option<Timestamp>,
    ) {
        match state.media_send_key_rotation_state {
            KeyRotationState::Pending { secret, .. } => {
                info!(
                    "Waiting to generate a new media send key until after the pending one has been applied. client_id: {}",
                    state.client_id
                );

                state.media_send_key_rotation_state = KeyRotationState::Pending {
                    secret,
                    needs_another_rotation: true,
                }
            }
            KeyRotationState::Applied => {
                info!(
                    "Generating a new random media send key because a user has been removed. client_id: {}",
                    state.client_id
                );

                // First generate a new key, then wait some time, and then apply it.
                let ratchet_counter: frame_crypto::RatchetCounter = 0;
                let secret = frame_crypto::random_secret(&mut rand::rngs::OsRng);

                if let JoinState::Pending(local_demux_id) | JoinState::Joined(local_demux_id) =
                    state.join_state
                {
                    let user_ids: HashSet<UserId> = state
                        .remote_devices
                        .iter()
                        .map(|rd| rd.user_id.clone())
                        .collect();
                    info!(
                        "Sending newly rotated key to everyone (number of users: {})",
                        user_ids.len()
                    );
                    Self::send_media_send_key_to_users_over_signaling(
                        state,
                        user_ids,
                        local_demux_id,
                        ratchet_counter,
                        secret,
                        endorsements_expiration,
                    );
                }

                state.media_send_key_rotation_state = KeyRotationState::Pending {
                    secret,
                    needs_another_rotation: false,
                };
                state.actor.send_delayed(
                    Duration::from_secs(MEDIA_SEND_KEY_ROTATION_DELAY_SECS),
                    move |state| {
                        info!("Applying the new send key. client_id: {}", state.client_id);
                        {
                            let mut frame_crypto_context =
                                state.frame_crypto_context.lock().expect(
                                    "Get lock for frame encryption context to reset media send key",
                                );
                            frame_crypto_context.reset_send_ratchet(secret);
                        }

                        let needs_another_rotation = matches!(
                            state.media_send_key_rotation_state,
                            KeyRotationState::Pending {
                                needs_another_rotation: true,
                                ..
                            }
                        );
                        state.media_send_key_rotation_state = KeyRotationState::Applied;
                        if needs_another_rotation {
                            Self::rotate_media_send_key_and_send_to_users_not_removed(
                                state,
                                endorsements_expiration,
                            );
                        }
                    },
                )
            }
        }
    }

    fn advance_media_send_key_and_send_to_users_with_added_devices(
        state: &mut State,
        users_with_added_devices: HashSet<UserId>,
        endorsements_expiration: Option<Timestamp>,
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
        if let JoinState::Pending(local_demux_id) | JoinState::Joined(local_demux_id) =
            state.join_state
        {
            info!(
                "Sending newly advanced key to users with added devices (number of users: {})",
                users_with_added_devices.len()
            );
            Self::send_media_send_key_to_users_over_signaling(
                state,
                users_with_added_devices,
                local_demux_id,
                ratchet_counter,
                secret,
                endorsements_expiration,
            );
        }
    }

    fn add_media_receive_key_or_store_for_later(
        state: &mut State,
        user_id: UserId,
        demux_id: DemuxId,
        ratchet_counter: frame_crypto::RatchetCounter,
        secret: frame_crypto::Secret,
    ) {
        if let Some(device) = state.remote_devices.find_by_demux_id_mut(demux_id) {
            if device.user_id == user_id {
                info!(
                    "Adding media receive key from {}. client_id: {}",
                    device.demux_id, state.client_id
                );
                {
                    let mut frame_crypto_context = state
                        .frame_crypto_context
                        .lock()
                        .expect("Get lock for frame encryption context to add media receive key");
                    frame_crypto_context.add_receive_secret(demux_id, ratchet_counter, secret);
                }
                let had_media_keys = std::mem::replace(&mut device.media_keys_received, true);
                if !had_media_keys {
                    state.observer.handle_remote_devices_changed(
                        state.client_id,
                        &state.remote_devices,
                        RemoteDevicesChangedReason::MediaKeyReceived(demux_id),
                    )
                }
            } else {
                warn!(
                    "Ignoring received media key from user because the demux ID {} doesn't make sense",
                    demux_id
                );
                debug!("  user_id: {}", uuid_to_string(&user_id));
            }
        } else {
            info!(
                "Storing media receive key from {} because we don't know who they are yet.",
                demux_id
            );
            if state.pending_media_receive_keys.is_empty()
                && state.kind == GroupCallKind::SignalGroup
            {
                // Proactively ask for the group members again.
                // Since pending_media_receive_keys is re-processed every time we get a device
                // update, this will effectively be requested once per peek as long as there's an
                // unknown device in the call.
                state.observer.request_group_members(state.client_id);
            }
            state
                .pending_media_receive_keys
                .push((user_id, demux_id, ratchet_counter, secret));
        }
    }

    fn send_media_send_key_to_users_over_signaling(
        state: &mut State,
        mut recipients: HashSet<UserId>,
        local_demux_id: DemuxId,
        ratchet_counter: frame_crypto::RatchetCounter,
        secret: frame_crypto::Secret,
        endorsements_expiration: Option<Timestamp>,
    ) {
        if recipients.is_empty() {
            return;
        }

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
        let call_message = protobuf::signaling::CallMessage {
            group_call_message: Some(message),
            ..Default::default()
        };
        // The multi-recipient API should not be used to send to a user's own UUID. If it
        // is in the set, remove it and send separately with a normal message.
        let self_uuid_to_send_to =
            if let Some(self_uuid) = state.self_uuid.lock().expect("can read UUID").deref() {
                recipients.take(self_uuid)
            } else {
                None
            };

        match (state.kind, state.group_send_endorsement_cache.as_ref()) {
            (GroupCallKind::SignalGroup, _) if recipients.len() > 1 => {
                state.observer.send_signaling_message_to_group(
                    state.group_id.clone(),
                    call_message.clone(),
                    SignalingMessageUrgency::Droppable,
                    recipients,
                );
            }
            (GroupCallKind::CallLink, Some(endorsement_cache)) => {
                let recipients: Vec<UserId> = recipients.into_iter().collect();
                if let Some((expiration, recipients_to_endorsements)) = endorsement_cache
                    .get_endorsements_for_users(endorsements_expiration, recipients.iter())
                {
                    let recipients_to_endorsements = recipients_to_endorsements
                        .into_iter()
                        .map(|(id, endorsement)| (id.clone(), zkgroup::serialize(endorsement)))
                        .collect();

                    state.observer.send_signaling_message_to_adhoc_group(
                        call_message.clone(),
                        SignalingMessageUrgency::Droppable,
                        expiration.epoch_seconds(),
                        recipients_to_endorsements,
                    );
                } else {
                    for recipient_id in recipients {
                        debug!("  recipient_id: {}", uuid_to_string(&recipient_id));
                        state.observer.send_signaling_message(
                            recipient_id.to_vec(),
                            call_message.clone(),
                            SignalingMessageUrgency::Droppable,
                        );
                    }
                }
            }
            _ => {
                for recipient_id in recipients {
                    debug!("  recipient_id: {}", uuid_to_string(&recipient_id));
                    state.observer.send_signaling_message(
                        recipient_id.to_vec(),
                        call_message.clone(),
                        SignalingMessageUrgency::Droppable,
                    );
                }
            }
        };

        if let Some(self_uuid) = self_uuid_to_send_to {
            debug!("  recipient_id: {}", uuid_to_string(&self_uuid));
            state.observer.send_signaling_message(
                self_uuid,
                call_message,
                SignalingMessageUrgency::Droppable,
            );
        }
    }

    fn send_pending_media_send_key_to_users_with_added_devices(
        state: &mut State,
        users_with_added_devices: HashSet<UserId>,
        endorsements_expiration: Option<Timestamp>,
    ) {
        if let JoinState::Pending(local_demux_id) | JoinState::Joined(local_demux_id) =
            state.join_state
            && let KeyRotationState::Pending { secret, .. } = state.media_send_key_rotation_state
        {
            info!(
                "Sending pending media key to users with added devices (number of users: {})",
                users_with_added_devices.len()
            );
            Self::send_media_send_key_to_users_over_signaling(
                state,
                users_with_added_devices,
                local_demux_id,
                0,
                secret,
                endorsements_expiration,
            );
        }
    }

    // The format for the ciphertext is:
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

    // Called by WebRTC through PeerConnectionObserver
    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn get_ciphertext_buffer_size(plaintext_size: usize) -> usize {
        // If we get asked to encrypt a message of size greater than (usize::MAX - FRAME_ENCRYPTION_FOOTER_LEN),
        // we'd fail to write the footer in encrypt_media and the frame would be dropped.
        plaintext_size.saturating_add(Self::FRAME_ENCRYPTION_FOOTER_LEN)
    }

    // Called by WebRTC through PeerConnectionObserver
    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn encrypt_media(&self, plaintext: &[u8], ciphertext_buffer: &mut [u8]) -> Result<usize> {
        let mut frame_crypto_context = self
            .frame_crypto_context
            .lock()
            .expect("Get e2ee context to encrypt media");

        Self::encrypt(&mut frame_crypto_context, plaintext, ciphertext_buffer)
    }

    fn encrypt_data(state: &mut State, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut frame_crypto_context = state
            .frame_crypto_context
            .lock()
            .expect("Get e2ee context to encrypt data");

        let mut ciphertext = vec![0; Self::get_ciphertext_buffer_size(plaintext.len())];
        Self::encrypt(&mut frame_crypto_context, plaintext, &mut ciphertext)?;
        Ok(ciphertext)
    }

    fn encrypt(
        frame_crypto_context: &mut frame_crypto::Context,
        plaintext: &[u8],
        ciphertext_buffer: &mut [u8],
    ) -> Result<usize> {
        let ciphertext_size = Self::get_ciphertext_buffer_size(plaintext.len());
        let mut ciphertext = Writer::new(ciphertext_buffer);

        let encrypted_payload = ciphertext.write_slice(plaintext)?;

        let mut mac = frame_crypto::Mac::default();
        let (ratchet_counter, frame_counter) =
            frame_crypto_context.encrypt(encrypted_payload, &mut mac)?;
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
        ciphertext: &[u8],
        plaintext_buffer: &mut [u8],
    ) -> Result<usize> {
        let mut frame_crypto_context = self
            .frame_crypto_context
            .lock()
            .expect("Get e2ee context to decrypt media");

        Self::decrypt(
            &mut frame_crypto_context,
            remote_demux_id,
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
            ciphertext,
            &mut plaintext,
        )?;
        Ok(plaintext)
    }

    fn decrypt(
        frame_crypto_context: &mut frame_crypto::Context,
        remote_demux_id: DemuxId,
        ciphertext: &[u8],
        plaintext_buffer: &mut [u8],
    ) -> Result<usize> {
        let mut ciphertext = Reader::new(ciphertext);
        let mut plaintext = Writer::new(plaintext_buffer);

        let mac: frame_crypto::Mac = ciphertext
            .read_slice_from_end(size_of::<frame_crypto::Mac>())?
            .try_into()?;
        let frame_counter = ciphertext.read_u32_from_end()?;
        let ratchet_counter = ciphertext.read_u8_from_end()?;

        // Allow for in-place decryption from ciphertext to plaintext_buffer by using
        // the write_slice that supports overlapping copies.
        let encrypted_payload = plaintext.write_slice_overlapping(ciphertext.remaining())?;

        frame_crypto_context.decrypt(
            remote_demux_id,
            ratchet_counter,
            frame_counter as u64,
            encrypted_payload,
            &mac,
        )?;
        Ok(encrypted_payload.len())
    }

    fn send_heartbeat(state: &mut State) -> Result<()> {
        let heartbeat_msg = protobuf::group_call::DeviceToDevice {
            heartbeat: {
                Some(protobuf::group_call::device_to_device::Heartbeat {
                    audio_muted: state.outgoing_heartbeat_state.audio_muted,
                    video_muted: state.outgoing_heartbeat_state.video_muted,
                    presenting: state.outgoing_heartbeat_state.presenting,
                    sharing_screen: state.outgoing_heartbeat_state.sharing_screen,
                    muted_by_demux_id: state.outgoing_heartbeat_state.muted_by_demux_id,
                })
            },
            ..Default::default()
        };
        Self::broadcast_data_through_sfu(state, &heartbeat_msg.encode_to_vec())
    }

    fn send_reaction(state: &mut State, value: String) -> Result<()> {
        let react_msg = protobuf::group_call::DeviceToDevice {
            reaction: {
                Some(protobuf::group_call::device_to_device::Reaction { value: Some(value) })
            },
            ..Default::default()
        };
        Self::broadcast_data_through_sfu(state, &react_msg.encode_to_vec())
    }

    fn send_raise_hand(state: &mut State) {
        use protobuf::group_call::device_to_sfu::RaiseHand;
        let msg = DeviceToSfu {
            raise_hand: {
                Some(RaiseHand {
                    raise: Some(state.raise_hand_state.raise),
                    seqnum: Some(state.raise_hand_state.seqnum),
                })
            },
            ..Default::default()
        }
        .encode_to_vec();

        if let Err(e) = Self::unreliable_send_data_to_sfu(state, &msg) {
            warn!("Failed to send raise hand: {:?}", e);
        }
    }

    fn send_leave_to_sfu(state: &mut State) {
        use protobuf::group_call::device_to_sfu::LeaveMessage;
        let msg = DeviceToSfu {
            leave: Some(LeaveMessage {}),
            ..Default::default()
        }
        .encode_to_vec();

        if let Err(e) = Self::unreliable_send_data_to_sfu(state, &msg) {
            warn!("Failed to send LeaveMessage: {:?}", e);
        }
        // Send it *again* to increase reliability just a little.
        if let Err(e) = Self::unreliable_send_data_to_sfu(state, &msg) {
            warn!("Failed to send extra redundancy LeaveMessage: {:?}", e);
        }
    }

    fn send_leaving_through_sfu_and_over_signaling(state: &mut State, local_demux_id: DemuxId) {
        use protobuf::group_call::{DeviceToDevice, device_to_device::Leaving};

        debug!(
            "group_call::Client(inner)::send_leaving_through_sfu_and_over_signaling(client_id: {}, local_demux_id: {})",
            state.client_id, local_demux_id,
        );

        let msg = DeviceToDevice {
            leaving: Some(Leaving::default()),
            ..DeviceToDevice::default()
        };
        if Self::broadcast_data_through_sfu(state, &msg.encode_to_vec()).is_err() {
            warn!("Could not send leaving message through the SFU");
        } else {
            debug!("Send leaving message over RTP through SFU.");
        }

        let call_message = protobuf::signaling::CallMessage {
            group_call_message: Some(DeviceToDevice {
                group_id: Some(state.group_id.clone()),
                leaving: Some(Leaving {
                    demux_id: Some(local_demux_id),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        debug!("leaving message recipients: {:?}", state.joined_members);

        let mut recipients = state.joined_members.clone();

        info!(
            "Sending leaving message to everyone (number of users: {})",
            recipients.len()
        );

        // The multi-recipient API should not be used to send to a user's own UUID. If it
        // is in the set, remove it and send separately with a normal message.
        let self_uuid_to_send_to =
            if let Some(self_uuid) = state.self_uuid.lock().expect("can read UUID").deref() {
                recipients.take(self_uuid)
            } else {
                None
            };

        if recipients.len() > 1 && state.kind == GroupCallKind::SignalGroup {
            state.observer.send_signaling_message_to_group(
                state.group_id.clone(),
                call_message.clone(),
                SignalingMessageUrgency::Droppable,
                recipients,
            );
        } else {
            for user_id in &recipients {
                state.observer.send_signaling_message(
                    user_id.clone(),
                    call_message.clone(),
                    SignalingMessageUrgency::Droppable,
                );
            }
        }

        if let Some(self_uuid) = self_uuid_to_send_to {
            state.observer.send_signaling_message(
                self_uuid,
                call_message,
                SignalingMessageUrgency::Droppable,
            );
        }
    }

    pub fn send_decryption_stats(&self, errors: HashMap<DemuxId, DecryptionErrorStats>) {
        debug!(
            "group_call::Client(outer)::send_decryption_stats(client_id: {}, errors: {:?})",
            self.client_id, errors,
        );
        self.actor.send(move |state| {
            debug!(
                "group_call::Client(inner)::send_decryption_stats(client_id: {})",
                state.client_id
            );
            Self::send_decryption_stats_inner(state, errors);
        });
    }

    fn send_decryption_stats_inner(
        state: &mut State,
        decryption_errors: HashMap<DemuxId, DecryptionErrorStats>,
    ) {
        use protobuf::group_call::{
            DeviceToSfu,
            device_to_sfu::{self, StatsReport, client_error},
        };

        use crate::common::time::saturating_epoch_time;

        let decryption_error_protos: Vec<_> = decryption_errors
            .into_iter()
            .map(|(demux_id, stats)| device_to_sfu::ClientError {
                error: Some(client_error::Error::Decryption(
                    client_error::DecryptionError {
                        sender_demux_id: Some(demux_id),
                        count: Some(stats.count),
                        start_ts: Some(saturating_epoch_time(stats.start_time).as_millis() as u64),
                        last_ts: Some(saturating_epoch_time(stats.last_time).as_millis() as u64),
                    },
                )),
            })
            .collect();

        let stats_msg = DeviceToSfu {
            stats: Some(StatsReport {
                client_errors: decryption_error_protos,
            }),
            ..Default::default()
        };
        if Self::send_data_to_sfu(state, &stats_msg.encode_to_vec()).is_err() {
            warn!("Failed to send stats report to SFU");
        }
    }

    fn cancel_full_group_ring_if_needed(state: &mut State) {
        debug!(
            "group_call::Client(inner)::cancel_full_group_ring_if_needed(client_id: {})",
            state.client_id,
        );

        if let OutgoingRingState::HasSentRing { ring_id } = state.outgoing_ring_state {
            let message = protobuf::signaling::CallMessage {
                ring_intention: Some(protobuf::signaling::call_message::RingIntention {
                    group_id: Some(state.group_id.clone()),
                    ring_id: Some(ring_id.into()),
                    r#type: Some(
                        protobuf::signaling::call_message::ring_intention::Type::Cancelled.into(),
                    ),
                }),
                ..Default::default()
            };

            state.observer.send_signaling_message_to_group(
                state.group_id.clone(),
                message,
                SignalingMessageUrgency::HandleImmediately,
                Default::default(),
            );
        }
    }

    fn broadcast_data_through_sfu(state: &mut State, message: &[u8]) -> Result<()> {
        debug!(
            "group_call::Client(inner)::broadcast_data_through_sfu(client_id: {}, message: {:?})",
            state.client_id, message,
        );
        if let JoinState::Joined(local_demux_id) = state.join_state {
            let message = Self::encrypt_data(state, message)?;
            let ssrc = local_demux_id.saturating_add(RTP_DATA_THROUGH_SFU_SSRC_OFFSET);
            state.rtp_data_through_sfu_next_seqnum = Self::unreliable_send_data_inner(
                state.join_state,
                state.client_id,
                ssrc,
                state.rtp_data_through_sfu_next_seqnum,
                &state.peer_connection,
                &message,
            )?;
        }
        Ok(())
    }

    // If data is too large for MTU, uses reliable send to support chunking messages
    // Use Client::unreliable_send_data_to_sfu directly if you are certain you do not want MRP semantics
    fn send_data_to_sfu(state: &mut State, message: &[u8]) -> Result<()> {
        debug!(
            "group_call::Client(inner)::send_data_to_sfu(client_id: {}, message: {:?})",
            state.client_id, message,
        );

        if message.len() > MAX_MRP_FRAGMENT_BYTE_SIZE {
            state.rtp_data_to_sfu_next_seqnum = Self::reliable_send_to_sfu_inner(
                &mut state.sfu_reliable_stream,
                state.join_state,
                state.client_id,
                state.rtp_data_to_sfu_next_seqnum,
                &state.peer_connection,
                message,
            )?;
        } else {
            Self::unreliable_send_data_to_sfu(state, message)?
        }
        Ok(())
    }

    fn unreliable_send_data_to_sfu(state: &mut State, message: &[u8]) -> Result<()> {
        debug!(
            "group_call::Client(inner)::unreliable_send_data_to_sfu(client_id: {}, message: {:?})",
            state.client_id, message,
        );
        state.rtp_data_to_sfu_next_seqnum = Self::unreliable_send_data_inner(
            state.join_state,
            state.client_id,
            RTP_DATA_TO_SFU_SSRC,
            state.rtp_data_to_sfu_next_seqnum,
            &state.peer_connection,
            message,
        )?;
        Ok(())
    }

    /// Reliably sends DeviceToSfu message over RTP. Will NOT chunk messages if too large for MTU
    /// Only sends when join_state == Pending or Joined
    fn reliable_send_device_to_sfu(
        state: &mut State,
        mut message: DeviceToSfu,
    ) -> std::result::Result<(), MrpSendError> {
        state.sfu_reliable_stream.try_send(|header| {
            message.mrp_header = Some(header.into());
            let payload = message.encode_to_vec();

            state.rtp_data_to_sfu_next_seqnum = Self::unreliable_send_data_inner(
                state.join_state,
                state.client_id,
                RTP_DATA_TO_SFU_SSRC,
                state.rtp_data_to_sfu_next_seqnum,
                &state.peer_connection,
                &payload,
            )?;
            Ok((payload, Instant::now() + DEVICE_TO_SFU_TIMEOUT))
        })
    }

    /// Should not be called from within MrpStream methods `try_resend` and `try_send_ack`
    /// Only sends when join_state == Pending or Joined
    /// Will chunk messages if they are too large
    fn reliable_send_to_sfu_inner(
        sfu_reliable_stream: &mut MrpStream<Vec<u8>, (rtp::Header, SfuToDevice)>,
        join_state: JoinState,
        client_id: ClientId,
        mut seqnum: u32,
        peer_connection: &PeerConnection,
        message: &[u8],
    ) -> Result<u32> {
        debug!(
            "group_call::Client(inner)::reliable_send_to_sfu_inner(client_id: {}, message: {:?})",
            client_id, message,
        );
        if let JoinState::Pending(_) | JoinState::Joined(_) = join_state {
            let fragments = message
                .chunks(MAX_MRP_FRAGMENT_BYTE_SIZE)
                .map(|b| b.to_vec())
                .collect();

            sfu_reliable_stream.try_send_fragmented(fragments, |_, mrp_header, fragment| {
                let rtp_header = rtp::Header {
                    pt: RTP_DATA_PAYLOAD_TYPE,
                    ssrc: RTP_DATA_TO_SFU_SSRC,
                    // This has to be incremented to make sure SRTP functions properly.
                    seqnum: seqnum as u16,
                    // Just imagine the clock is the number of messages :),
                    // Plus the above sequence number is too small to be useful.
                    timestamp: seqnum,
                };

                let payload = DeviceToSfu {
                    mrp_header: Some(mrp_header.into()),
                    content: Some(fragment),
                    ..Default::default()
                }
                .encode_to_vec();

                if let Err(e) = peer_connection.send_rtp(rtp_header, &payload) {
                    error!(
                        "Failed to send reliable message over rtp, queuing retry: {:?}",
                        e
                    );
                };
                seqnum = seqnum.wrapping_add(1);
                (payload, Instant::now() + DEVICE_TO_SFU_TIMEOUT)
            })?;
            Ok(seqnum)
        } else {
            Err(anyhow::anyhow!(
                "Can't perform reliable send, invalid JoinState: {:?}",
                join_state
            ))
        }
    }

    fn unreliable_send_data_inner(
        join_state: JoinState,
        client_id: ClientId,
        ssrc: rtp::Ssrc,
        seqnum: u32,
        peer_connection: &PeerConnection,
        message: &[u8],
    ) -> Result<u32> {
        debug!(
            "group_call::Client(inner)::unreliable_send_data_inner(client_id: {}, message: {:?})",
            client_id, message,
        );
        if let JoinState::Pending(_) | JoinState::Joined(_) = join_state {
            let header = rtp::Header {
                pt: RTP_DATA_PAYLOAD_TYPE,
                ssrc,
                // This has to be incremented to make sure SRTP functions properly.
                seqnum: seqnum as u16,
                // Just imagine the clock is the number of messages :),
                // Plus the above sequence number is too small to be useful.
                timestamp: seqnum,
            };
            peer_connection.send_rtp(header, message)?;
            Ok(seqnum.wrapping_add(1))
        } else {
            Err(anyhow::anyhow!(
                "Can't perform reliable send, invalid JoinState: {:?}",
                join_state
            ))
        }
    }

    /// Warning: this runs on the WebRTC network thread, so doing anything that
    /// would block is dangerous, especially taking a lock that is also taken
    /// while calling something that blocks on the network thread.
    fn handle_rtp_received(&self, header: rtp::Header, payload: &[u8]) {
        use protobuf::group_call::DeviceToDevice;

        if header.pt == RTP_DATA_PAYLOAD_TYPE {
            if header.ssrc == RTP_DATA_TO_SFU_SSRC {
                match SfuToDevice::decode(payload) {
                    Ok(msg) => Self::handle_sfu_to_device(&self.actor, header, msg),
                    Err(e) => warn!(
                        "Ignoring received RTP marked SfuToDevice because decoding failed: {:?}",
                        e
                    ),
                };
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
                        if let Some(reaction) = msg.reaction {
                            self.handle_reaction(demux_id, reaction);
                        }
                        if let Some(remote_mute_request) = msg.remote_mute_request {
                            self.handle_remote_mute_request(demux_id, remote_mute_request);
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

    fn handle_sfu_to_device(actor: &Actor<State>, header: rtp::Header, msg: SfuToDevice) {
        if let Some(mrp_header) = msg.mrp_header.as_ref() {
            let mrp_header = mrp_header.into();
            actor.send(move |state| {
                match state
                    .sfu_reliable_stream
                    .receive_and_merge(&mrp_header, (header, msg))
                {
                    Ok(ready_packets) => {
                        for (buffered_header, sfu_to_device) in ready_packets {
                            Self::handle_sfu_to_device_inner(
                                &state.actor,
                                buffered_header,
                                sfu_to_device,
                            )
                        }
                    }
                    err @ Err(MrpReceiveError::ReceiveWindowFull(_)) => {
                        warn!(
                            "Buffer full when receiving reliable SfuToDevice message, discarding. {:?}",
                            err
                        );
                    }

                    Err(err) => {
                        error!(
                            "Error when receiving reliable SfuToDevice message, discarded all drained packets {:?}",
                            err
                        );
                    }
                };
            });
        } else {
            Self::handle_sfu_to_device_inner(actor, header, msg);
        }
    }

    fn handle_sfu_to_device_inner(actor: &Actor<State>, header: rtp::Header, msg: SfuToDevice) {
        use protobuf::group_call::sfu_to_device::{CurrentDevices, RaisedHands, Removed, Speaker};
        let sys_now = SystemTime::now();
        // TODO: Use video_request to throttle down how much we send when it's not needed.
        let SfuToDevice {
            speaker,
            device_joined_or_left,
            current_devices,
            stats,
            video_request: _,
            removed,
            raised_hands,
            mrp_header: _,
            content,
            endorsements,
        } = msg;

        if let Some(content) = content {
            match SfuToDevice::decode(content.as_slice()) {
                Ok(msg) => Self::handle_sfu_to_device_inner(actor, header, msg),
                Err(err) => {
                    error!("Failed to decode content buffer in SfuToDevice: {:?}", err);
                }
            }
            // ignore all other fields to prevent ordering issues
            return;
        }

        if let Some(Speaker {
            demux_id: speaker_demux_id,
        }) = speaker
        {
            if let Some(speaker_demux_id) = speaker_demux_id {
                Self::handle_speaker_received(actor, header.timestamp, speaker_demux_id);
            } else {
                warn!("Ignoring speaker demux ID of None from SFU");
            }
        };
        if endorsements.is_some() || device_joined_or_left.is_some() {
            actor.send(move |state| {
                let expiration = endorsements.and_then(|endorsements| {
                    Self::handle_send_endorsements_response_inner(state, sys_now, endorsements)
                });

                if let Some(DeviceJoinedOrLeft { peek_info }) = device_joined_or_left {
                    if let Some(peek_info_proto) = peek_info {
                        match PeekInfo::deobfuscate_proto(
                            peek_info_proto,
                            &state.obfuscated_resolver,
                        ) {
                            Ok(peek_info) => {
                                Self::set_peek_result_inner(state, Ok(peek_info), expiration)
                            }
                            Err(err) => {
                                warn!(
                                    "Failed to deobfuscate peek info, falling back to http: {:?}",
                                    err
                                );
                                Self::request_remote_devices_as_soon_as_possible(state);
                            }
                        }
                    } else {
                        info!("SFU notified that a remote device has joined or left, requesting update");
                        Self::request_remote_devices_as_soon_as_possible(state);
                    }
                }
            });
        }

        // TODO: Use all_demux_ids to avoid polling
        if let Some(CurrentDevices {
            demux_ids_with_video,
            all_demux_ids: _,
            allocated_heights,
        }) = current_devices
        {
            Self::handle_forwarding_video_received(actor, demux_ids_with_video, allocated_heights);
        }
        if let Some(stats) = stats {
            info!(
                "ringrtc_stats!,sfu,recv,{},{},{}",
                stats.target_send_rate_kbps.unwrap_or(0),
                stats.ideal_send_rate_kbps.unwrap_or(0),
                stats.allocated_send_rate_kbps.unwrap_or(0)
            );
        }
        if let Some(Removed {}) = removed {
            Self::handle_removed_received(actor);
        }
        if let Some(RaisedHands {
            demux_ids,
            seqnums: _,
            target_seqnum: Some(target_seqnum),
        }) = raised_hands
        {
            Self::handle_raised_hands(actor, demux_ids, target_seqnum);
        }
    }

    fn handle_removed_received(actor: &Actor<State>) {
        actor.send(move |state| {
            if matches!(state.join_state, JoinState::Joined(_)) {
                Self::end(state, CallEndReason::RemovedFromCall);
            } else {
                Self::end(state, CallEndReason::DeniedRequestToJoinCall);
            }
        });
    }

    fn handle_speaker_received(actor: &Actor<State>, timestamp: rtp::Timestamp, demux_id: DemuxId) {
        actor.send(move |state| {
            if let Some(speaker_rtp_timestamp) = state.speaker_rtp_timestamp
                && timestamp <= speaker_rtp_timestamp
            {
                // Ignored packets received out of order
                debug!(
                    "Ignoring speaker change because the timestamp is old: {}",
                    timestamp
                );
                return;
            }
            state.speaker_rtp_timestamp = Some(timestamp);

            let latest_speaker_demux_id = state.remote_devices.latest_speaker_demux_id();

            if let Some(speaker_device) = state.remote_devices.find_by_demux_id_mut(demux_id) {
                if latest_speaker_demux_id == Some(speaker_device.demux_id) {
                    debug!(
                        "Already the latest speaker demux {:?} since {:?}",
                        speaker_device.demux_id, speaker_device.speaker_time
                    );
                    return;
                }

                speaker_device.speaker_time = Some(SystemTime::now());
                info!(
                    "New speaker {:?} at {:?}",
                    speaker_device.demux_id, speaker_device.speaker_time
                );
                let demux_id = speaker_device.demux_id;
                state.observer.handle_remote_devices_changed(
                    state.client_id,
                    &state.remote_devices,
                    RemoteDevicesChangedReason::SpeakerTimeChanged(demux_id),
                );
            } else {
                debug!(
                    "Ignoring speaker change because it isn't a known remote devices: {}",
                    demux_id
                );
                // Unknown speaker device. It's probably the local device.
            }
        });
    }

    fn handle_send_endorsements_response_inner(
        state: &mut State,
        sys_now: SystemTime,
        SendEndorsementsResponse {
            serialized,
            member_ciphertexts,
        }: SendEndorsementsResponse,
    ) -> Option<Timestamp> {
        if state.group_send_endorsement_cache.is_none() {
            warn!("Received endorsements when there is no group_send_endorsement_cache");
            return None;
        }
        let Some(serialized) = serialized else {
            state.observer.handle_endorsements_update(
                state.client_id,
                Err(EndorsementUpdateError::MissingField("serialized")),
            );
            return None;
        };
        if member_ciphertexts.is_empty() {
            state.observer.handle_endorsements_update(
                state.client_id,
                Err(EndorsementUpdateError::MissingField("member_ciphertexts")),
            );
            return None;
        }
        let response =
            match zkgroup::deserialize::<GroupSendEndorsementsResponse>(serialized.as_slice()) {
                Ok(response) => response,
                Err(_) => {
                    state.observer.handle_endorsements_update(
                        state.client_id,
                        Err(EndorsementUpdateError::InvalidEndorsementResponseFormat),
                    );
                    return None;
                }
            };
        let now = Timestamp::from_epoch_seconds(
            sys_now
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        if response.expiration() < now {
            state.observer.handle_endorsements_update(
                state.client_id,
                Err(EndorsementUpdateError::ExpiredEndorsements(
                    response.expiration(),
                )),
            );
            return None;
        }

        Self::validate_and_cache_endorsements(state, response, member_ciphertexts, now)
    }

    fn validate_and_cache_endorsements(
        state: &mut State,
        response: GroupSendEndorsementsResponse,
        member_ciphertexts: Vec<Vec<u8>>,
        now: Timestamp,
    ) -> Option<Timestamp> {
        let Some(endorsement_public_key) = state.obfuscated_resolver.get_endorsement_public_key()
        else {
            error!(
                "Cannot process SendEndorsementsResponse because call was not initialized with endorsement public key"
            );
            return None;
        };
        let Some(endorsement_cache) = state.group_send_endorsement_cache.as_mut() else {
            warn!(
                "Received endorsements when there is no group_send_endorsement_cache, likely not an adhoc call."
            );
            return None;
        };
        let expiration = response.expiration();
        let member_uuid_ciphertexts: Vec<UuidCiphertext> = member_ciphertexts
            .iter()
            .flat_map(|opaque_user_id| zkgroup::deserialize(opaque_user_id).ok())
            .collect();
        let member_ids: Vec<UserId> = member_ciphertexts
            .iter()
            .flat_map(|ciphertext| {
                state
                    .obfuscated_resolver
                    .resolve_user_id_bytes(ciphertext.as_ref())
            })
            .collect();
        if member_ids.len() != member_ciphertexts.len()
            || member_uuid_ciphertexts.len() != member_ciphertexts.len()
        {
            endorsement_cache.set_invalid(
                Some(expiration),
                "Received endorsements with invalid member ciphertexts".to_string(),
            );
            state.observer.handle_endorsements_update(
                state.client_id,
                Err(EndorsementUpdateError::InvalidEndorsementResponse),
            );
            state.observer.handle_endorsements_update(
                state.client_id,
                Err(EndorsementUpdateError::InvalidMemberCiphertexts),
            );
            return None;
        }

        match response.receive_with_ciphertexts(
            member_uuid_ciphertexts,
            now,
            endorsement_public_key,
        ) {
            Ok(endorsements) => {
                let endorsements = member_ids
                    .into_iter()
                    .zip(endorsements.into_iter().map(|e| e.decompressed))
                    .collect::<HashMap<_, _>>();
                endorsement_cache.insert(expiration, endorsements);
                if let Some(endorsement_update) =
                    endorsement_cache.get_endorsements_for_expiration(expiration)
                {
                    state
                        .observer
                        .handle_endorsements_update(state.client_id, Ok(endorsement_update));
                }
                Some(expiration)
            }
            Err(_) => {
                endorsement_cache.set_invalid(
                    Some(expiration),
                    "Failed to processing endorsement response in receive_with_ciphertext"
                        .to_string(),
                );
                state.observer.handle_endorsements_update(
                    state.client_id,
                    Err(EndorsementUpdateError::InvalidEndorsementResponse),
                );
                None
            }
        }
    }

    fn handle_forwarding_video_received(
        actor: &Actor<State>,
        mut demux_ids_with_video: Vec<DemuxId>,
        allocated_heights: Vec<u32>,
    ) {
        actor.send(move |state| {
            let forwarding_videos: HashMap<DemuxId, u16> = demux_ids_with_video
                .iter()
                .zip(allocated_heights.iter())
                .map(|(&demux_id, &height)| (demux_id, height as u16))
                .collect();
            if state.forwarding_videos != forwarding_videos {
                demux_ids_with_video.sort_unstable();
                info!(
                    "SFU notified that the forwarding videos changed. Demux IDs with video is now {:?}",
                    demux_ids_with_video
                );
                for remote_device in state.remote_devices.iter_mut() {
                    let server_allocated_height = forwarding_videos.get(&remote_device.demux_id);
                    let is_forwarding = server_allocated_height.is_some();
                    remote_device.forwarding_video = Some(is_forwarding);
                    remote_device.server_allocated_height = server_allocated_height.copied().unwrap_or(0);

                    if !is_forwarding {
                        remote_device.client_decoded_height = None;
                    }

                    remote_device.recalculate_higher_resolution_pending();
                }
                state.forwarding_videos = forwarding_videos;
                state.observer.handle_remote_devices_changed(
                    state.client_id,
                    &state.remote_devices,
                    RemoteDevicesChangedReason::ForwardedVideosChanged,
                )
            }
        })
    }

    fn handle_heartbeat_received(
        &self,
        demux_id: DemuxId,
        timestamp: u32,
        heartbeat: protobuf::group_call::device_to_device::Heartbeat,
    ) {
        self.actor.send(move |state| {
            if let Some(remote_device) = state.remote_devices.find_by_demux_id_mut(demux_id) {
                if timestamp > remote_device.heartbeat_rtp_timestamp.unwrap_or(0) {
                    // Record this even if nothing changed.  Otherwise an old packet could override
                    // a new packet.
                    remote_device.heartbeat_rtp_timestamp = Some(timestamp);
                    let heartbeat_state = HeartbeatState::from(heartbeat);
                    if remote_device.heartbeat_state != heartbeat_state {
                        if heartbeat_state.video_muted == Some(true) {
                            remote_device.client_decoded_height = None;
                            remote_device.recalculate_higher_resolution_pending();
                        }

                        // Ignore heartbeats that do not have changes in the state.
                        let new_source = if heartbeat_state.muted_by_demux_id
                            != remote_device.heartbeat_state.muted_by_demux_id
                        {
                            heartbeat_state.muted_by_demux_id
                        } else {
                            None
                        };

                        if let Some(new_source_demux_id) = new_source
                            && heartbeat_state.audio_muted == Some(true)
                            && remote_device.heartbeat_state.audio_muted == Some(false)
                        {
                            state.observer.handle_observed_remote_mute(
                                state.client_id,
                                new_source_demux_id,
                                demux_id,
                            );
                        }

                        remote_device.heartbeat_state = heartbeat_state;

                        state.observer.handle_remote_devices_changed(
                            state.client_id,
                            &state.remote_devices,
                            RemoteDevicesChangedReason::HeartbeatStateChanged(demux_id),
                        );
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
        if let Some(device) = state.remote_devices.find_by_demux_id_mut(demux_id)
            && !device.leaving_received
        {
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

    fn handle_reaction(
        &self,
        demux_id: DemuxId,
        reaction: protobuf::group_call::device_to_device::Reaction,
    ) {
        trace!("handle_reaction(): demux_id = {}", demux_id);

        let value = reaction.value.unwrap_or_default();

        if value.is_empty() {
            warn!("group_call::handle_reaction reaction value is empty");
        } else if value.len() > REACTION_STRING_MAX_SIZE {
            warn!(
                "group_call::handle_reaction reaction value size of {} exceeded allowed size of {}",
                value.len(),
                REACTION_STRING_MAX_SIZE
            );
        } else {
            self.actor.send(move |state| {
                state.reactions.push(Reaction { demux_id, value });
            });
        }
    }

    fn handle_remote_mute_request(
        &self,
        source_demux_id: DemuxId,
        remote_mute_request: protobuf::group_call::RemoteMuteRequest,
    ) {
        debug!(
            "handle_remote_mute_request(): demux_id = {}, target = {:?}",
            source_demux_id, remote_mute_request.target_demux_id,
        );

        let target = if let Some(target_demux) = remote_mute_request.target_demux_id {
            target_demux
        } else {
            warn!("group_call::handle_remote_mute_request target value is empty");
            return;
        };

        self.actor.send(move |state| match state.join_state {
            JoinState::Pending(our_demux_id) | JoinState::Joined(our_demux_id) => {
                if our_demux_id == target
                    && state.mute_request.is_none()
                    && source_demux_id != our_demux_id
                {
                    // Only bother with the first mute request in a tick; more would be redundant.
                    state.mute_request = Some(source_demux_id);
                }
            }
            _ => {}
        })
    }

    fn handle_raised_hands(actor: &Actor<State>, raised_hands: Vec<DemuxId>, server_seqnum: u32) {
        actor.send(move |state| {
            // The server has previously received a hand raise request from the client or admin
            if server_seqnum != 0 {
                if server_seqnum >= state.raise_hand_state.seqnum {
                    // Set the local raised hand seqnum to the latest from the server
                    state.raise_hand_state.seqnum = server_seqnum;

                    // Issue a callback when the seqnum value of the local demux id in the
                    // servers list is equal to or greater than the local seqnum and a raised
                    // hand is outstanding. This ensures that a client will get a callback to
                    // "unlock" the UI state even if the raised hand list is the same as before.
                    if state.raise_hand_state.outstanding {
                        state.raise_hand_state.outstanding = false;
                        state.raised_hands = raised_hands;

                        info!(
                            "group_call::Client(inner)::handle_raised_hands(client_id: {} raised_hands: {:?} seqnum: {} raise: {} outstanding: {})",
                            state.client_id,
                            state.raised_hands,
                            state.raise_hand_state.seqnum,
                            state.raise_hand_state.raise,
                            state.raise_hand_state.outstanding
                        );

                        state
                            .observer
                            .handle_raised_hands(state.client_id, state.raised_hands.clone());

                    } else if state.raised_hands != raised_hands {
                        // Issue a callback when a raised hand is not outstanding and the
                        // raised hand list has changed.
                        info!(
                            "group_call::Client(inner)::handle_raised_hands(client_id: {} raised_hands: {:?} server seqnum: {} local seqnum: {} local raise: {} outstanding: {})",
                            state.client_id,
                            raised_hands,
                            server_seqnum,
                            state.raise_hand_state.seqnum,
                            state.raise_hand_state.raise,
                            state.raise_hand_state.outstanding
                        );
                        state.raised_hands = raised_hands;
                        state
                            .observer
                            .handle_raised_hands(state.client_id, state.raised_hands.clone());
                    }
                }
            } else {
                // Issue a callback if the client has never raised their hand and the server
                // list is different than before.
                if state.raise_hand_state.seqnum == 0 && state.raised_hands != raised_hands {
                    state.raised_hands = raised_hands;
                    state
                        .observer
                        .handle_raised_hands(state.client_id, state.raised_hands.clone());
                }
            }
        });
    }

    #[cfg(feature = "sim")]
    pub fn synchronize(&self) {
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let barrier_for_task = barrier.clone();

        self.actor.send(move |_| {
            barrier_for_task.wait();
        });

        barrier.wait();
    }
}

// We need to wrap a Call to implement PeerConnectionObserverTrait
// because we need to pass an impl into PeerConnectionObserver::new
// before we call PeerConnectionFactory::create_peer_connection.
// So we need to either have an Option<PeerConnection> inside of the
// State or have an Option<Call> instead of here.  This seemed
// more convenient (fewer "if let Some(x) = x" to do).
struct PeerConnectionObserverImpl {
    client: Option<Client>,
    incoming_video_sink: Option<Box<dyn VideoSink>>,
    last_height_by_demux_id: CallMutex<HashMap<DemuxId, u32>>,
}

impl PeerConnectionObserverImpl {
    fn uninitialized(
        incoming_video_sink: Option<Box<dyn VideoSink>>,
    ) -> Result<(Box<Self>, PeerConnectionObserver<Self>)> {
        let enable_video_frame_content = incoming_video_sink.is_some();
        let boxed_observer_impl = Box::new(Self {
            client: None,
            incoming_video_sink,
            last_height_by_demux_id: CallMutex::new(HashMap::new(), "last_height_by_demux_id"),
        });
        let observer = PeerConnectionObserver::new(
            webrtc::ptr::Borrowed::from_ptr(&*boxed_observer_impl),
            true, /* enable_frame_encryption */
            true, /* enable_video_frame_event */
            enable_video_frame_content,
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
        _sdp_for_logging: &str,
        _relay_protocol: Option<webrtc::peer_connection_observer::TransportProtocol>,
    ) -> Result<()> {
        Ok(())
    }

    fn handle_ice_candidate_removed(&mut self, _removed_address: SocketAddr) -> Result<()> {
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
                        // ICE failed before we got connected :(
                        Client::end(state, CallEndReason::IceFailedWhileConnecting);
                    }
                    (ConnectionState::Connecting, IceConnectionState::Checking) => {
                        // Normal.  Not much to report.
                    }
                    (ConnectionState::Connecting, IceConnectionState::Connected) |
                    (ConnectionState::Connecting, IceConnectionState::Completed) => {
                        // ICE Connected!
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
                        Client::end(state, CallEndReason::IceFailedAfterConnected);
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

    fn handle_ice_network_route_changed(&mut self, network_route: NetworkRoute) -> Result<()> {
        debug!(
            "group_call::Client(outer)::handle_ice_network_route_changed(client_id: {}, network_route: {:?})",
            self.log_id(),
            network_route
        );
        if let Some(client) = &self.client {
            client.actor.send(move |state| {
                debug!("group_call::Client(inner)::handle_ice_network_route_changed(client_id: {}, network_route: {:?})", state.client_id, network_route);
                state
                    .observer
                    .handle_network_route_changed(state.client_id, network_route);
                state.call_summary.on_ice_network_route_changed(network_route);
            });
        } else {
            warn!("Call isn't setup yet!");
        }
        Ok(())
    }

    fn handle_incoming_video_added(
        &mut self,
        incoming_video_track: VideoTrack,
        demux_id: Option<DemuxId>,
    ) -> Result<()> {
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

                if let Some(remote_demux_id) = demux_id {
                    // When PeerConnection::SetRemoteDescription triggers PeerConnectionObserver::OnAddTrack,
                    // if it's a VideoTrack, this is where it comes.  Each platform does different things:
                    // - iOS: The VideoTrack is wrapped in an RTCVideoTrack and passed to the app
                    //        via handleIncomingVideoTrack and onRemoteDeviceStatesChanged, which adds a sink.
                    // - Android: The VideoTrack is wrapped in a Java VideoTrack and passed to the app via handleIncomingVideoTrack, which adds a sink.
                    // - Desktop: A VideoSink is added by the PeerConnectionObserverRffi.
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

    fn handle_incoming_video_frame(
        &self,
        demux_id: DemuxId,
        video_frame_metadata: VideoFrameMetadata,
        video_frame: Option<VideoFrame>,
    ) -> Result<()> {
        let height = video_frame_metadata.height;
        if let (Some(incoming_video_sink), Some(video_frame)) =
            (self.incoming_video_sink.as_ref(), video_frame)
        {
            incoming_video_sink.on_video_frame(demux_id, video_frame)
        }
        if let Some(client) = &self.client {
            let prev_height = self
                .last_height_by_demux_id
                .lock()
                .unwrap()
                .insert(demux_id, height);
            if prev_height != Some(height) {
                client.actor.send(move |state| {
                    if let Some(remote_device) = state.remote_devices.find_by_demux_id_mut(demux_id)
                    {
                        // The height needs to be checked again because last_height_by_demux_id
                        // doesn't account for video mute or forwarding state.
                        if remote_device.client_decoded_height != Some(height)
                        // Workaround for a race where a frame is received after video muting
                        && remote_device.heartbeat_state.video_muted != Some(true)
                        {
                            remote_device.client_decoded_height = Some(height);

                            let was_higher_resolution_pending =
                                remote_device.is_higher_resolution_pending;
                            remote_device.recalculate_higher_resolution_pending();

                            if remote_device.is_higher_resolution_pending
                                != was_higher_resolution_pending
                            {
                                state.observer.handle_remote_devices_changed(
                                    state.client_id,
                                    &state.remote_devices,
                                    RemoteDevicesChangedReason::HigherResolutionPendingChanged,
                                );
                            }
                        }
                    }
                });
            }
        }

        Ok(())
    }

    fn get_media_ciphertext_buffer_size(
        &mut self,
        _is_audio: bool,
        plaintext_size: usize,
    ) -> usize {
        Client::get_ciphertext_buffer_size(plaintext_size)
    }

    // See comment on FRAME_ENCRYPTION_FOOTER_LEN for more details on the format
    fn encrypt_media(&mut self, plaintext: &[u8], ciphertext_buffer: &mut [u8]) -> Result<usize> {
        if let Some(client) = &self.client {
            client.encrypt_media(plaintext, ciphertext_buffer)
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
        ciphertext: &[u8],
        plaintext_buffer: &mut [u8],
    ) -> Result<usize> {
        if let Some(client) = &self.client {
            let remote_demux_id = track_id;
            client.decrypt_media(remote_demux_id, ciphertext, plaintext_buffer)
        } else {
            warn!("Call isn't setup yet!  Can't decrypt");
            Err(RingRtcError::FailedToDecrypt.into())
        }
    }
}

// Wrapper for RtpObserver to handle RTP data events received from the peer.
struct RtpObserverImpl {
    client: Client,
}

impl RtpObserverTrait for RtpObserverImpl {
    fn handle_rtp_received(&mut self, header: rtp::Header, payload: &[u8]) {
        self.client.handle_rtp_received(header, payload);
    }
}

fn random_alphanumeric(len: usize) -> String {
    std::iter::repeat(())
        .map(|()| rand::rngs::OsRng.sample(rand::distributions::Alphanumeric))
        .take(len)
        .map(char::from)
        .collect()
}

// Should this go in some util class?
struct Writer<'buf> {
    buf: &'buf mut [u8],
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

    fn write_slice_overlapping(&mut self, input: &[u8]) -> Result<&mut [u8]> {
        if self.remaining_len() < input.len() {
            return Err(RingRtcError::BufferTooSmall.into());
        }
        let start = self.offset;
        let end = start + input.len();
        let output = &mut self.buf[start..end];

        // Use memmove to handle potentially overlapping memory. This is safe
        // because we've already checked the buffer lengths.
        unsafe {
            std::ptr::copy(input.as_ptr(), output.as_mut_ptr(), input.len());
        }

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
#[cfg(feature = "sim")]
mod tests {
    use std::sync::{
        Arc, Condvar, LazyLock, Mutex,
        atomic::{self, AtomicI64, AtomicU64, Ordering},
        mpsc,
    };

    use libsignal_core::Aci;
    use rand::random;
    use zkgroup::{
        EndorsementPublicKey, EndorsementServerRootKeyPair, RANDOMNESS_LEN, RandomnessBytes,
        ServerPublicParams, ServerSecretParams, UUID_LEN, call_links::CallLinkSecretParams,
    };

    use super::*;
    use crate::{
        common::time::saturating_epoch_time,
        core::endorsements::EndorsementUpdateResult,
        lite::{
            call_links::{CallLinkMemberResolver, CallLinkRootKey},
            sfu::{MemberResolver, PeekDeviceInfo},
        },
        protobuf::group_call::MrpHeader,
        webrtc::sim::media::FAKE_AUDIO_TRACK,
    };

    static RANDOMNESS: LazyLock<RandomnessBytes> = LazyLock::new(|| [0x44u8; RANDOMNESS_LEN]);
    static SERVER_SECRET_PARAMS: LazyLock<ServerSecretParams> =
        LazyLock::new(|| ServerSecretParams::generate(*RANDOMNESS));
    static ENDORSEMENT_SERVER_ROOT_KEY: LazyLock<EndorsementServerRootKeyPair> =
        LazyLock::new(|| SERVER_SECRET_PARAMS.get_endorsement_root_key_pair());
    static SERVER_PUBLIC_PARAMS: LazyLock<ServerPublicParams> =
        LazyLock::new(|| SERVER_SECRET_PARAMS.get_public_params());
    static ENDORSEMENT_PUBLIC_ROOT_KEY: LazyLock<EndorsementPublicKey> =
        LazyLock::new(|| SERVER_PUBLIC_PARAMS.get_endorsement_public_key());
    static MEMBER_IDS: LazyLock<Vec<Aci>> = LazyLock::new(|| {
        (1..=3)
            .map(|i| Aci::from_uuid_bytes([i; UUID_LEN]))
            .collect()
    });
    static CALL_LINK_ROOT_KEY: LazyLock<CallLinkRootKey> =
        LazyLock::new(|| CallLinkRootKey::try_from([0x43u8; 16].as_ref()).unwrap());
    static CALL_LINK_SECRET_PARAMS: LazyLock<CallLinkSecretParams> =
        LazyLock::new(|| CallLinkSecretParams::derive_from_root_key(&CALL_LINK_ROOT_KEY.bytes()));
    static MEMBER_CIPHERTEXTS: LazyLock<Vec<UuidCiphertext>> = LazyLock::new(|| {
        MEMBER_IDS
            .iter()
            .map(|id| CALL_LINK_SECRET_PARAMS.encrypt_uid(*id))
            .collect::<Vec<_>>()
    });

    impl Client {
        fn handle_send_endorsements_response(
            actor: &Actor<State>,
            sys_now: SystemTime,
            send_endorsements_response: SendEndorsementsResponse,
        ) {
            actor.send(move |state| {
                Self::handle_send_endorsements_response_inner(
                    state,
                    sys_now,
                    send_endorsements_response,
                );
            });
        }
    }

    #[derive(Clone)]
    struct FakeSfuClient {
        sfu_info: SfuInfo,
        local_demux_id: DemuxId,
        call_creator: Option<UserId>,
        request_count: Arc<AtomicU64>,
        era_id: String,
        response_join_state: Arc<Mutex<JoinState>>,
        joins_remaining: Option<Arc<AtomicI64>>,
    }

    #[derive(Default)]
    struct FakeSfuClientOptions {
        max_joins: Option<usize>,
    }

    impl FakeSfuClient {
        fn new(local_demux_id: DemuxId, call_creator: Option<UserId>) -> Self {
            Self::with_options(
                local_demux_id,
                call_creator,
                FakeSfuClientOptions::default(),
            )
        }

        fn with_options(
            local_demux_id: DemuxId,
            call_creator: Option<UserId>,
            options: FakeSfuClientOptions,
        ) -> Self {
            Self {
                sfu_info: SfuInfo {
                    udp_addresses: Vec::new(),
                    tcp_addresses: Vec::new(),
                    tls_addresses: Vec::new(),
                    hostname: None,
                    ice_ufrag: "fake ICE ufrag".to_string(),
                    ice_pwd: "fake ICE pwd".to_string(),
                },
                local_demux_id,
                call_creator,
                request_count: Arc::new(AtomicU64::new(0)),
                era_id: "1111111111111111".to_string(),
                response_join_state: Arc::new(Mutex::new(JoinState::Joined(local_demux_id))),
                joins_remaining: options
                    .max_joins
                    .map(|v| Arc::new(AtomicI64::new(v as i64))),
            }
        }

        fn get_response_join_state(&self) -> JoinState {
            *self.response_join_state.lock().unwrap()
        }

        fn set_response_join_state(&mut self, join_state: JoinState) {
            let mut data = self.response_join_state.lock().unwrap();
            *data = join_state;
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
            _dhe_pub_key: [u8; 32],
            client: Client,
        ) {
            if let Some(counter) = &self.joins_remaining
                && counter.fetch_sub(1, Ordering::SeqCst) <= 0
            {
                // No more joins allowed. Simulate a "group full" condition.
                client
                    .on_sfu_client_join_attempt_completed(Err(RingRtcError::GroupCallFull.into()));
                return;
            }
            client.on_sfu_client_join_attempt_completed(Ok(Joined {
                sfu_info: self.sfu_info.clone(),
                local_demux_id: self.local_demux_id,
                server_dhe_pub_key: [0u8; 32],
                hkdf_extra_info: b"hkdf_extra_info".to_vec(),
                creator: self.call_creator.clone(),
                era_id: self.era_id.clone(),
                join_state: self.get_response_join_state(),
            }));
        }
        fn peek(&mut self, _peek_result_callback: PeekResultCallback) {
            self.request_count.fetch_add(1, atomic::Ordering::SeqCst);
        }
        fn set_group_members(&mut self, _members: Vec<GroupMember>) {}
        fn set_membership_proof(&mut self, _proof: MembershipProof) {}
    }

    // TODO: Put this in common util area?
    #[derive(Clone)]
    struct Waitable<T> {
        val: Arc<Mutex<Option<T>>>,
        cvar: Arc<Condvar>,
    }

    impl<T> Default for Waitable<T> {
        fn default() -> Self {
            Self {
                val: Arc::default(),
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

        fn wait(&self, timeout: Duration) -> Option<T> {
            let mut val = self.val.lock().unwrap();
            while val.is_none() {
                let (wait_val, wait_result) = self.cvar.wait_timeout(val, timeout).unwrap();
                if wait_result.timed_out() {
                    return None;
                }
                val = wait_val
            }
            Some(val.take().unwrap())
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

        fn wait(&self, timeout: Duration) -> bool {
            self.waitable.wait(timeout).is_some()
        }
    }

    #[derive(Clone, Default)]
    struct FakeObserverPeekState {
        joined_members: Vec<UserId>,
        creator: Option<UserId>,
        era_id: Option<String>,
        max_devices: Option<u32>,
        device_count: usize,
    }

    #[derive(Clone)]
    struct FakeObserver {
        // For sending messages
        user_id: UserId,
        recipients: Arc<CallMutex<Vec<TestClient>>>,
        outgoing_signaling_blocked: Arc<CallMutex<bool>>,
        sent_group_signaling_messages: Arc<CallMutex<Vec<protobuf::signaling::CallMessage>>>,
        sent_adhoc_group_signaling_messages: Arc<CallMutex<Vec<protobuf::signaling::CallMessage>>>,

        connecting: Event,
        endorsement_update_event: Event,
        joined: Event,
        peek_changed: Event,
        reactions_called: Event,
        remote_devices_changed: Event,
        remote_devices: Arc<CallMutex<Vec<RemoteDeviceState>>>,
        remote_devices_at_join_time: Arc<CallMutex<Vec<RemoteDeviceState>>>,
        peek_state: Arc<CallMutex<FakeObserverPeekState>>,
        send_rates: Arc<CallMutex<Option<SendRates>>>,
        ended: Waitable<CallEndReason>,
        reactions: Arc<CallMutex<Vec<Reaction>>>,
        endorsement_update: Arc<CallMutex<Option<EndorsementUpdateResult>>>,

        request_membership_proof_invocation_count: Arc<AtomicU64>,
        request_group_members_invocation_count: Arc<AtomicU64>,
        handle_remote_devices_changed_invocation_count: Arc<AtomicU64>,
        handle_audio_levels_invocation_count: Arc<AtomicU64>,
        handle_speaking_notification_invocation_count: Arc<AtomicU64>,
        handle_reactions_invocation_count: Arc<AtomicU64>,
        reactions_count: Arc<AtomicU64>,
        send_signaling_message_invocation_count: Arc<AtomicU64>,
        send_signaling_message_to_group_invocation_count: Arc<AtomicU64>,
        send_signaling_message_to_adhoc_group_invocation_count: Arc<AtomicU64>,
        multi_recipient_count: Arc<AtomicU64>,

        remote_muted_by: Arc<CallMutex<Option<DemuxId>>>,
        observed_remote_mutes: Arc<CallMutex<Vec<(DemuxId, DemuxId)>>>,
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
                sent_group_signaling_messages: Arc::new(CallMutex::new(
                    Vec::new(),
                    "FakeObserver sent group messages",
                )),
                sent_adhoc_group_signaling_messages: Arc::new(CallMutex::new(
                    Vec::new(),
                    "FakeObserver sent group messages",
                )),
                connecting: Event::default(),
                endorsement_update_event: Event::default(),
                joined: Event::default(),
                peek_changed: Event::default(),
                reactions_called: Event::default(),
                remote_devices_changed: Event::default(),
                remote_devices: Arc::new(CallMutex::new(Vec::new(), "FakeObserver remote devices")),
                remote_devices_at_join_time: Arc::new(CallMutex::new(
                    Vec::new(),
                    "FakeObserver remote devices",
                )),
                peek_state: Arc::new(CallMutex::new(
                    FakeObserverPeekState::default(),
                    "FakeObserver peek state",
                )),
                send_rates: Arc::new(CallMutex::new(None, "FakeObserver send rates")),
                endorsement_update: Arc::new(CallMutex::new(
                    None,
                    "FakeObserver endorsement update",
                )),
                ended: Waitable::default(),
                reactions: Arc::new(CallMutex::new(Default::default(), "FakeObserver reactions")),
                request_membership_proof_invocation_count: Default::default(),
                request_group_members_invocation_count: Default::default(),
                handle_remote_devices_changed_invocation_count: Default::default(),
                handle_audio_levels_invocation_count: Default::default(),
                handle_speaking_notification_invocation_count: Default::default(),
                handle_reactions_invocation_count: Default::default(),
                reactions_count: Default::default(),
                send_signaling_message_invocation_count: Default::default(),
                send_signaling_message_to_group_invocation_count: Default::default(),
                send_signaling_message_to_adhoc_group_invocation_count: Default::default(),
                multi_recipient_count: Default::default(),
                remote_muted_by: Arc::new(CallMutex::new(None, "Most recent remote mute received")),
                observed_remote_mutes: Arc::new(CallMutex::new(
                    Vec::new(),
                    "FakeObserver-observed remote mutes",
                )),
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
            peek_state.joined_members.to_vec()
        }

        fn peek_state(&self) -> FakeObserverPeekState {
            let peek_state = self.peek_state.lock().expect("Lock peek state to read it");
            peek_state.clone()
        }

        fn send_rates(&self) -> Option<SendRates> {
            let send_rates = self.send_rates.lock().expect("Lock send rates to read it");
            send_rates.clone()
        }

        fn reactions(&self) -> Vec<Reaction> {
            let reactions = self.reactions.lock().expect("Lock reactions to read it");
            reactions.clone()
        }

        /// Gets the number of `request_membership_proof` since last checked.
        fn request_membership_proof_invocation_count(&self) -> u64 {
            self.request_membership_proof_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        /// Gets the number of `request_group_members` since last checked.
        fn request_group_members_invocation_count(&self) -> u64 {
            self.request_group_members_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        /// Gets the number of `handle_remote_devices_changed` since last checked.
        fn handle_remote_devices_changed_invocation_count(&self) -> u64 {
            self.handle_remote_devices_changed_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        /// Gets the number of `handle_audio_levels` since last checked.
        fn handle_audio_levels_invocation_count(&self) -> u64 {
            self.handle_audio_levels_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        /// Gets the number of `speaking_notification` since last checked.
        #[allow(unused)]
        fn handle_speaking_notification_count(&self) -> u64 {
            self.handle_speaking_notification_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        fn handle_reactions_invocation_count(&self) -> u64 {
            self.handle_reactions_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        fn reactions_count(&self) -> u64 {
            self.reactions_count.swap(0, Ordering::Relaxed)
        }

        fn send_signaling_message_invocation_count(&self) -> u64 {
            self.send_signaling_message_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        fn send_signaling_message_to_group_invocation_count(&self) -> u64 {
            self.send_signaling_message_to_group_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        #[allow(dead_code)]
        fn send_signaling_message_to_adhoc_group_invocation_count(&self) -> u64 {
            self.send_signaling_message_to_group_invocation_count
                .swap(0, Ordering::Relaxed)
        }

        fn multi_recipient_count(&self) -> u64 {
            self.multi_recipient_count.swap(0, Ordering::Relaxed)
        }
    }

    impl Observer for FakeObserver {
        fn request_membership_proof(&self, _client_id: ClientId) {
            self.request_membership_proof_invocation_count
                .fetch_add(1, Ordering::Relaxed);
        }

        fn request_group_members(&self, _client_id: ClientId) {
            self.request_group_members_invocation_count
                .fetch_add(1, Ordering::Relaxed);
        }

        fn handle_connection_state_changed(
            &self,
            _client_id: ClientId,
            connection_state: ConnectionState,
        ) {
            if connection_state == ConnectionState::Connecting {
                self.connecting.set();
            }
        }

        fn handle_join_state_changed(&self, _client_id: ClientId, join_state: JoinState) {
            if let JoinState::Joined(_) = join_state {
                let mut owned_remote_devices_at_join_time = self
                    .remote_devices_at_join_time
                    .lock()
                    .expect("Lock joined members at join time to handle update");
                *owned_remote_devices_at_join_time = self.remote_devices();
                self.joined.set();
            }
        }

        fn handle_network_route_changed(&self, _client_id: ClientId, _network_route: NetworkRoute) {
        }

        fn handle_remote_devices_changed(
            &self,
            _client_id: ClientId,
            remote_devices: &[RemoteDeviceState],
            _reason: RemoteDevicesChangedReason,
        ) {
            let mut owned_remote_devices = self
                .remote_devices
                .lock()
                .expect("Lock recipients to set remote devices");
            *owned_remote_devices = remote_devices.to_vec();
            self.handle_remote_devices_changed_invocation_count
                .fetch_add(1, Ordering::Relaxed);
            self.remote_devices_changed.set();
        }

        fn handle_speaking_notification(&mut self, _client_id: ClientId, _event: SpeechEvent) {
            self.handle_speaking_notification_invocation_count
                .fetch_add(1, Ordering::Relaxed);
        }

        fn handle_audio_levels(
            &self,
            _client_id: ClientId,
            _captured_level: AudioLevel,
            _received_levels: Vec<ReceivedAudioLevel>,
        ) {
            self.handle_audio_levels_invocation_count
                .fetch_add(1, Ordering::Relaxed);
        }

        fn handle_low_bandwidth_for_video(&self, _client_id: ClientId, _recovered: bool) {}

        fn handle_reactions(&self, _client_id: ClientId, reactions: Vec<Reaction>) {
            let mut owned = self
                .reactions
                .lock()
                .expect("Lock reactions to handle update");
            owned.clone_from(&reactions);

            self.handle_reactions_invocation_count
                .fetch_add(1, Ordering::Relaxed);
            self.reactions_count
                .fetch_add(reactions.len() as u64, Ordering::Relaxed);
            self.reactions_called.set();
        }

        fn handle_raised_hands(&self, _client_id: ClientId, _raised_hands: Vec<DemuxId>) {}

        fn handle_rtc_stats_report(&self, _report_json: String) {}

        fn handle_peek_changed(
            &self,
            _client_id: ClientId,
            peek_info: &PeekInfo,
            joined_members: &HashSet<UserId>,
        ) {
            let mut owned_state = self
                .peek_state
                .lock()
                .expect("Lock peek state to handle update");
            owned_state.joined_members = joined_members.iter().cloned().collect();
            owned_state.creator.clone_from(&peek_info.creator);
            owned_state.era_id.clone_from(&peek_info.era_id);
            owned_state.max_devices = peek_info.max_devices;
            owned_state.device_count = peek_info.device_count_including_pending_devices();
            self.peek_changed.set();
        }

        fn handle_send_rates_changed(&self, _client_id: ClientId, send_rates: SendRates) {
            let mut self_send_rates = self
                .send_rates
                .lock()
                .expect("Lock send rates to handle update");
            *self_send_rates = Some(send_rates);
        }

        fn send_signaling_message(
            &mut self,
            recipient_id: UserId,
            call_message: protobuf::signaling::CallMessage,
            _urgency: SignalingMessageUrgency,
        ) {
            self.send_signaling_message_invocation_count
                .fetch_add(1, Ordering::Relaxed);

            if self.outgoing_signaling_blocked() {
                info!(
                    "Dropping message from {:?} to {:?} because we blocked signaling.",
                    self.user_id, recipient_id
                );
                return;
            }
            let recipient_ids = self
                .recipients
                .lock()
                .expect("Lock recipients to add recipient");
            let mut sent = false;
            if let Some(message) = call_message.group_call_message {
                for recipient in recipient_ids.iter() {
                    if recipient.user_id == recipient_id {
                        recipient
                            .client
                            .on_signaling_message_received(self.user_id.clone(), message.clone());
                        sent = true;
                    }
                }
            }
            if sent {
                info!(
                    "Sent message from {:?} to {:?}.",
                    self.user_id, recipient_id
                );
            } else {
                info!(
                    "Did not sent message from {:?} to {:?} because it's not a known recipient.",
                    self.user_id, recipient_id
                );
            }
        }

        fn send_signaling_message_to_group(
            &mut self,
            _group: GroupId,
            call_message: protobuf::signaling::CallMessage,
            _urgency: SignalingMessageUrgency,
            recipients_override: HashSet<UserId>,
        ) {
            self.send_signaling_message_to_group_invocation_count
                .fetch_add(1, Ordering::Relaxed);

            if self.outgoing_signaling_blocked() {
                info!(
                    "Dropping message from {:?} to group because we blocked signaling.",
                    self.user_id,
                );
                return;
            }
            if !recipients_override.is_empty() {
                self.multi_recipient_count
                    .fetch_add(recipients_override.len() as u64, Ordering::Relaxed);

                for recipient_id in recipients_override {
                    assert_ne!(
                        self.user_id, recipient_id,
                        "User can't send to own UUID for multi-recipient API"
                    );

                    let recipient_ids = self
                        .recipients
                        .lock()
                        .expect("Lock recipients to add recipient");
                    let mut sent = false;
                    if let Some(message) = call_message.clone().group_call_message {
                        for recipient in recipient_ids.iter() {
                            if recipient.user_id == recipient_id {
                                recipient.client.on_signaling_message_received(
                                    self.user_id.clone(),
                                    message.clone(),
                                );
                                sent = true;
                            }
                        }
                    }
                    if sent {
                        info!(
                            "Sent message from {:?} to {:?}.",
                            self.user_id, recipient_id
                        );
                    } else {
                        info!(
                            "Did not sent message from {:?} to {:?} because it's not a known recipient.",
                            self.user_id, recipient_id
                        );
                    }
                }
            } else {
                self.sent_group_signaling_messages
                    .lock()
                    .expect("adding message")
                    .push(call_message);
                info!("Recorded group-wide call message from {:?}", self.user_id);
            }
        }

        fn send_signaling_message_to_adhoc_group(
            &mut self,
            call_message: protobuf::signaling::CallMessage,
            _urgency: SignalingMessageUrgency,
            _expiration: u64,
            recipients_to_endorsements: HashMap<UserId, Vec<u8>>,
        ) {
            self.send_signaling_message_to_adhoc_group_invocation_count
                .fetch_add(1, Ordering::Relaxed);

            if self.outgoing_signaling_blocked() {
                info!(
                    "Dropping message from {:?} to group because we blocked signaling.",
                    self.user_id,
                );
                return;
            }
            if !recipients_to_endorsements.is_empty() {
                self.multi_recipient_count
                    .fetch_add(recipients_to_endorsements.len() as u64, Ordering::Relaxed);

                for recipient_id in recipients_to_endorsements.into_keys() {
                    assert_ne!(
                        self.user_id, recipient_id,
                        "User can't send to own UUID for multi-recipient API"
                    );

                    let recipient_ids = self
                        .recipients
                        .lock()
                        .expect("Lock recipients to add recipient");
                    let mut sent = false;
                    if let Some(message) = call_message.clone().group_call_message {
                        for recipient in recipient_ids.iter() {
                            if recipient.user_id == recipient_id {
                                recipient.client.on_signaling_message_received(
                                    self.user_id.clone(),
                                    message.clone(),
                                );
                                sent = true;
                            }
                        }
                    }
                    if sent {
                        info!(
                            "Sent message from {:?} to {:?}.",
                            self.user_id, recipient_id
                        );
                    } else {
                        info!(
                            "Did not sent message from {:?} to {:?} because it's not a known recipient.",
                            self.user_id, recipient_id
                        );
                    }
                }
            } else {
                self.sent_adhoc_group_signaling_messages
                    .lock()
                    .expect("adding message")
                    .push(call_message);
                info!("Recorded group-wide call message from {:?}", self.user_id);
            }
        }

        fn handle_incoming_video_track(
            &mut self,
            _client_id: ClientId,
            _remote_demux_id: DemuxId,
            _incoming_video_track: VideoTrack,
        ) {
        }

        fn handle_ended(&self, _client_id: ClientId, reason: CallEndReason, _summary: CallSummary) {
            self.ended.set(reason);
        }

        fn handle_remote_mute_request(&self, _client_id: ClientId, mute_source: DemuxId) {
            *self.remote_muted_by.lock().unwrap() = Some(mute_source);
        }

        fn handle_observed_remote_mute(
            &self,
            _client_id: ClientId,
            mute_source: DemuxId,
            mute_target: DemuxId,
        ) {
            self.observed_remote_mutes
                .lock()
                .unwrap()
                .push((mute_source, mute_target));
        }

        fn handle_endorsements_update(
            &self,
            _client_id: ClientId,
            update: EndorsementUpdateResultRef,
        ) {
            let mut owned = self
                .endorsement_update
                .lock()
                .expect("Lock endorsement_update to handle update");

            info!(
                "Observer handling endorsement update: is_err={:?}",
                update.is_err()
            );
            *owned =
                Some(update.map(|(expiration, endorsements)| (expiration, endorsements.clone())));
            self.endorsement_update_event.set();
        }
    }

    #[derive(Clone)]
    struct TestClient {
        user_id: UserId,
        demux_id: DemuxId,
        sfu_client: FakeSfuClient,
        observer: FakeObserver,
        client: Client,
        sfu_rtp_packet_sender: Option<mpsc::Sender<(rtp::Header, Vec<u8>)>>,
        default_peek_info: PeekInfo,
    }

    impl TestClient {
        fn new(user_id: UserId, demux_id: DemuxId) -> Self {
            Self::with_sfu_client(user_id, demux_id, FakeSfuClient::new(demux_id, None))
        }

        fn with_sfu_client(user_id: UserId, demux_id: DemuxId, sfu_client: FakeSfuClient) -> Self {
            let observer = FakeObserver::new(user_id.clone());
            let fake_busy = Arc::new(CallMutex::new(false, "fake_busy"));
            let fake_self_uuid = Arc::new(CallMutex::new(Some(user_id.clone()), "fake_self_uuid"));
            let fake_audio_track = AudioTrack::new(
                webrtc::Arc::from_owned(unsafe {
                    webrtc::ptr::OwnedRc::from_ptr(&FAKE_AUDIO_TRACK as *const u32)
                }),
                None,
            );
            let group_send_endorsement_cache =
                Some(EndorsementsCache::new(*CALL_LINK_SECRET_PARAMS));
            let obfuscated_resolver = ObfuscatedResolver::new(
                Arc::new(CallLinkMemberResolver::from(&*CALL_LINK_ROOT_KEY)),
                Some(CALL_LINK_ROOT_KEY.clone()),
                Some(ENDORSEMENT_PUBLIC_ROOT_KEY.clone()),
            );
            let client = Client::start(ClientStartParams {
                group_id: b"fake group ID".to_vec(),
                client_id: demux_id,
                kind: GroupCallKind::SignalGroup,
                sfu_client: Box::new(sfu_client.clone()),
                obfuscated_resolver,
                observer: Box::new(observer.clone()),
                busy: fake_busy,
                self_uuid: fake_self_uuid,
                peer_connection_factory: None,
                outgoing_audio_track: fake_audio_track,
                outgoing_video_track: None,
                incoming_video_sink: None,
                ring_id: None,
                audio_levels_interval: Some(Duration::from_millis(200)),
                group_send_endorsement_cache,
            })
            .expect("Start Client");
            Self {
                user_id: user_id.clone(),
                demux_id,
                sfu_client,
                observer,
                client,
                sfu_rtp_packet_sender: None,
                default_peek_info: PeekInfo {
                    devices: vec![PeekDeviceInfo {
                        demux_id,
                        user_id: Some(user_id),
                    }],
                    ..Default::default()
                },
            }
        }

        fn connect_join_and_wait_until_joined(&self) {
            self.client.connect();
            self.client.join();
            self.client
                .set_peek_result(Ok(self.default_peek_info.clone()));
            assert!(self.observer.joined.wait(Duration::from_secs(5)));
        }

        fn set_up_rtp_with_remotes(&self, clients: Vec<TestClient>) {
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
        }

        fn set_remotes_and_wait_until_applied(&self, clients: &[&TestClient]) {
            let remote_devices = clients
                .iter()
                .map(|client| PeekDeviceInfo {
                    demux_id: client.demux_id,
                    user_id: Some(client.user_id.clone()),
                })
                .collect();
            // Need to clone to pass over to the actor and set in observer.
            let clients: Vec<TestClient> = clients.iter().copied().cloned().collect();
            self.observer.set_recipients(clients.clone());
            let peek_info = PeekInfo {
                devices: remote_devices,
                ..self.default_peek_info.clone()
            };
            self.client.set_peek_result(Ok(peek_info));
            self.set_up_rtp_with_remotes(clients);
            self.wait_for_client_to_process();
        }

        fn set_pending_clients_and_wait_until_applied(&self, clients: &[&TestClient]) {
            let remote_devices = clients
                .iter()
                .map(|client| PeekDeviceInfo {
                    demux_id: client.demux_id,
                    user_id: Some(client.user_id.clone()),
                })
                .collect();
            let peek_info = PeekInfo {
                pending_devices: remote_devices,
                ..self.default_peek_info.clone()
            };
            self.client.set_peek_result(Ok(peek_info));
            self.set_up_rtp_with_remotes(vec![]);
            self.wait_for_client_to_process();
        }

        fn wait_for_client_to_process(&self) {
            let event = Event::default();
            let cloned = event.clone();
            self.client.actor.send(move |_state| {
                cloned.set();
            });
            event.wait(Duration::from_secs(5));
        }

        fn wait_for_client_to_process_and_tick(&self) {
            let event = Event::default();
            let cloned = event.clone();
            self.client
                .actor
                .send_delayed(TICK_INTERVAL * 2, move |_state| {
                    cloned.set();
                });
            event.wait(Duration::from_secs(5));
        }

        fn encrypt_media(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
            let mut ciphertext = vec![0; plaintext.len() + Client::FRAME_ENCRYPTION_FOOTER_LEN];
            assert_eq!(
                ciphertext.len(),
                Client::get_ciphertext_buffer_size(plaintext.len())
            );
            assert_eq!(
                ciphertext.len(),
                self.client.encrypt_media(plaintext, &mut ciphertext)?
            );
            Ok(ciphertext)
        }

        fn decrypt_media(
            &mut self,
            remote_demux_id: DemuxId,
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
                self.client
                    .decrypt_media(remote_demux_id, ciphertext, &mut plaintext,)?
            );
            Ok(plaintext)
        }

        fn receive_speaker(&self, timestamp: u32, speaker_demux_id: DemuxId) {
            Client::handle_speaker_received(&self.client.actor, timestamp, speaker_demux_id);
            self.wait_for_client_to_process();
        }

        // DemuxIds sorted by speaker_time, then added_time, then demux_id.
        fn speakers(&self) -> Vec<DemuxId> {
            let mut devices = self.observer.remote_devices();
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
            self.observer.ended.wait(Duration::from_secs(5));
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
            client.set_remotes_and_wait_until_applied(clients);
        }
        for client in clients {
            client.wait_for_client_to_process();
        }
    }

    #[test]
    fn frame_encryption_normal() {
        let mut client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        client2.set_remotes_and_wait_until_applied(&[&client1]);

        // At this point, client2 knows about client1, so can receive encrypted media.
        // But client1 does not know about client1, so has not yet shared its encryption key
        // with it, so client2 cannot decrypt media from client1.
        // And while client2 has shared the key with client1, client1 has not yet learned
        // about client2 so can't decrypt either.

        let plaintext = &b"Fake Audio"[..];
        let ciphertext1 = client1.encrypt_media(plaintext).unwrap();
        let ciphertext2 = client2.encrypt_media(plaintext).unwrap();

        assert_ne!(plaintext, &ciphertext1[..plaintext.len()]);

        assert!(
            client1
                .decrypt_media(client2.demux_id, &ciphertext2)
                .is_err()
        );
        assert!(
            client2
                .decrypt_media(client1.demux_id, &ciphertext1)
                .is_err()
        );

        client1.set_remotes_and_wait_until_applied(&[&client2]);
        // We wait until client2 has processed the key from client1
        client2.wait_for_client_to_process();

        // At this point, both clients know about each other and have shared keys
        // and should be able to decrypt.

        // Because client1 just learned about client2, it advanced its key
        // and so we need to re-encrypt with that key.
        let mut ciphertext1 = client1.encrypt_media(plaintext).unwrap();

        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext1)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client1
                .decrypt_media(client2.demux_id, &ciphertext2)
                .unwrap()
        );

        // But if the footer is too small, decryption should fail
        assert!(client1.decrypt_media(client2.demux_id, b"small").is_err());

        // And if the unencrypted media header has been modified, it should fail (bad mac)
        ciphertext1[0] = ciphertext1[0].wrapping_add(1);
        assert!(
            client2
                .decrypt_media(client1.demux_id, &ciphertext1)
                .is_err()
        );

        // Finally, let's make sure video works as well

        let plaintext = &b"Fake Video Needs To Be Bigger"[..];
        let ciphertext1 = client1.encrypt_media(plaintext).unwrap();

        assert_ne!(plaintext, &ciphertext1[..plaintext.len()]);

        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext1)
                .unwrap()
        );

        client1.disconnect_and_wait_until_ended();
        client2.disconnect_and_wait_until_ended();
    }

    #[test]
    #[ignore] // Because it's too slow
    fn frame_encryption_rotation_is_delayed() {
        let mut client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        let mut client3 = TestClient::new(vec![3], 3);
        client3.connect_join_and_wait_until_joined();

        let mut client4 = TestClient::new(vec![4], 4);
        client4.connect_join_and_wait_until_joined();

        let mut client5 = TestClient::new(vec![5], 5);
        client5.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2, &client3]);

        // client2 and client3 can decrypt client1
        // client4 can't yet
        let plaintext = &b"Fake Audio"[..];
        let ciphertext = client1.encrypt_media(plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client3
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert!(
            client4
                .decrypt_media(client1.demux_id, &ciphertext)
                .is_err()
        );

        // Add client4 and remove client3
        set_group_and_wait_until_applied(&[&client1, &client2, &client4]);

        // client2 and client4 can decrypt client1
        // client3 can as well, at least for a little while
        let ciphertext = client1.encrypt_media(plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client3
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );

        std::thread::sleep(std::time::Duration::from_millis(2000));

        // client5 joins during the period between when the new key is generated
        // and when it is applied.  client 5 should receive this key and decrypt
        // both before and after the key is applied.
        // meanwhile, client2 leaves, which will cause another rotation after this
        // one.
        set_group_and_wait_until_applied(&[&client1, &client4, &client5]);

        let ciphertext = client1.encrypt_media(plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client3
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client5
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );

        std::thread::sleep(std::time::Duration::from_millis(2000));

        // client4 and client5 can still decrypt from client1
        // but client3 no longer can
        let ciphertext = client1.encrypt_media(plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert!(
            client3
                .decrypt_media(client1.demux_id, &ciphertext)
                .is_err()
        );
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client5
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );

        std::thread::sleep(std::time::Duration::from_millis(3000));

        // After the next key rotation is applied, now client2 cannot decrypt,
        // but client4 and client5 can.
        let ciphertext = client1.encrypt_media(plaintext).unwrap();
        assert!(
            client2
                .decrypt_media(client1.demux_id, &ciphertext)
                .is_err()
        );
        assert!(
            client3
                .decrypt_media(client1.demux_id, &ciphertext)
                .is_err()
        );
        assert_eq!(
            plaintext,
            client4
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
        assert_eq!(
            plaintext,
            client5
                .decrypt_media(client1.demux_id, &ciphertext)
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
        let mut client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        // Prevent client1 from sharing keys with client2
        client1.observer.set_outgoing_signaling_blocked(true);
        set_group_and_wait_until_applied(&[&client1, &client2]);

        let remote_devices = client2.observer.remote_devices();
        assert_eq!(1, remote_devices.len());
        assert!(!remote_devices[0].media_keys_received);

        let plaintext = &b"Fake Video is big"[..];
        let ciphertext = client1.encrypt_media(plaintext).unwrap();
        // We can't decrypt because the keys got dropped
        assert!(
            client2
                .decrypt_media(client1.demux_id, &ciphertext)
                .is_err()
        );

        client1.observer.set_outgoing_signaling_blocked(false);
        client1.client.resend_media_keys();
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices = client2.observer.remote_devices();
        assert_eq!(1, remote_devices.len());
        assert!(remote_devices[0].media_keys_received);

        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext)
                .unwrap()
        );
    }

    #[test]
    fn frame_encryption_send_advanced_key_to_same_user() {
        let mut client1a = TestClient::new(vec![1], 11);
        let mut client2a = TestClient::new(vec![2], 21);
        let mut client2b = TestClient::new(vec![2], 22);

        client1a.connect_join_and_wait_until_joined();
        client2a.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1a, &client2a]);

        let plaintext = &b"Fake Audio"[..];
        let ciphertext1a = client1a.encrypt_media(plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2a
                .decrypt_media(client1a.demux_id, &ciphertext1a)
                .unwrap()
        );

        // Make sure the advanced key gets sent to client2b even though it's the same user as 2a.
        client2b.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1a, &client2a, &client2b]);
        let ciphertext1a = client1a.encrypt_media(plaintext).unwrap();
        assert_eq!(
            plaintext,
            client2b
                .decrypt_media(client1a.demux_id, &ciphertext1a)
                .unwrap()
        );
    }

    #[test]
    fn frame_encryption_someone_forging_demux_id() {
        let mut client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let mut client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        // Client3 is pretending to have demux ID 1 when sending media keys
        let mut client3 = TestClient::with_sfu_client(vec![3], 3, FakeSfuClient::new(1, None));
        client3.client.connect();
        client3.client.join();

        set_group_and_wait_until_applied(&[&client1, &client2, &client3]);

        let plaintext = &b"Fake Audio"[..];
        let ciphertext1 = client1.encrypt_media(plaintext).unwrap();
        let ciphertext3 = client3.encrypt_media(plaintext).unwrap();
        // The forger doesn't mess anything up for the others
        assert_eq!(
            plaintext,
            client2
                .decrypt_media(client1.demux_id, &ciphertext1)
                .unwrap()
        );
        // And you can't decrypt from the forger.
        assert!(
            client2
                .decrypt_media(client3.demux_id, &ciphertext3)
                .is_err()
        );

        client1.disconnect_and_wait_until_ended();
        client2.disconnect_and_wait_until_ended();
        client3.disconnect_and_wait_until_ended();
    }

    #[test]
    fn ask_for_group_membership_when_receiving_unknown_media_keys() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();
        assert_eq!(1, client1.observer.request_group_members_invocation_count());

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        let client3 = TestClient::new(vec![3], 3);
        client3.connect_join_and_wait_until_joined();

        assert_eq!(0, client1.observer.request_group_members_invocation_count());

        // Request group membership for the first unknown media key...
        client2.set_remotes_and_wait_until_applied(&[&client1]);
        client1.wait_for_client_to_process();
        assert_eq!(1, client1.observer.request_group_members_invocation_count());

        // ...but not any after that.
        client3.set_remotes_and_wait_until_applied(&[&client1]);
        client1.wait_for_client_to_process();
        assert_eq!(0, client1.observer.request_group_members_invocation_count());

        // Re-process (and maybe re-request) when the list of active devices changes.
        client1.set_remotes_and_wait_until_applied(&[]);
        assert_eq!(1, client1.observer.request_group_members_invocation_count());

        // Resolving one member results in a re-request, just in case.
        client1.set_remotes_and_wait_until_applied(&[&client2]);
        assert_eq!(1, client1.observer.request_group_members_invocation_count());

        // But resolving the other member is enough to clear the saved list,
        // showing that we already processed the first.
        client1.set_remotes_and_wait_until_applied(&[&client3]);
        assert_eq!(0, client1.observer.request_group_members_invocation_count());
    }

    #[test]
    fn do_not_ask_for_group_membership_when_receiving_known_media_keys() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();
        assert_eq!(1, client1.observer.request_group_members_invocation_count());

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        assert_eq!(0, client1.observer.request_group_members_invocation_count());

        // This time, the receiver finds out about the sender first...
        client1.set_remotes_and_wait_until_applied(&[&client2]);

        // ...so the media key sent here won't be unknown.
        client2.set_remotes_and_wait_until_applied(&[&client1]);
        client1.wait_for_client_to_process();
        assert_eq!(0, client1.observer.request_group_members_invocation_count());
    }

    #[test]
    #[rustfmt::skip] // The line wrapping makes this test hard to read.
    fn send_media_keys_to_recipients() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1]);

        // With only one client in the call, no media keys should have been sent.
        assert_eq!(0, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1, &client2]);

        // Sending media keys to each-other after adding.
        assert_eq!(1, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(1, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_to_group_invocation_count());

        let client3 = TestClient::new(vec![3], 3);
        client3.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1, &client2, &client3]);

        // client1 and client2 add client3.
        assert_eq!(1, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(1, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_to_group_invocation_count());

        // client3 must send to both client1 and client2 using the multi-recipient API.
        assert_eq!(0, client3.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client3.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client3.observer.multi_recipient_count());

        let client4 = TestClient::new(vec![4], 4);
        client4.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1, &client2, &client3, &client4]);

        // client1, client2, and client3 add client4.
        assert_eq!(1, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(1, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(1, client3.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client3.observer.send_signaling_message_to_group_invocation_count());

        // client4 must send keys to all other clients using the multi-recipient API.
        assert_eq!(0, client4.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client4.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(3, client4.observer.multi_recipient_count());

        // client3 leaves, and should send a leave message to other clients. Also, it will
        // send a leaving message to its user to let other devices know.
        client3.disconnect_and_wait_until_ended();
        assert_eq!(1, client3.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client3.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(3, client3.observer.multi_recipient_count());

        // The other clients should all send rotated media keys to everyone else after
        // learning that client3 has left.
        set_group_and_wait_until_applied(&[&client1, &client2, &client4]);
        assert_eq!(0, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client1.observer.multi_recipient_count());
        assert_eq!(0, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client2.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client2.observer.multi_recipient_count());
        assert_eq!(0, client4.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client4.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client4.observer.multi_recipient_count());

        // client5 is another device from the user of client1.
        let client5 = TestClient::new(vec![1], 5);
        client5.connect_join_and_wait_until_joined();
        set_group_and_wait_until_applied(&[&client1, &client2, &client4, &client5]);

        // client1, client2, and client4 add client5 and sent it both their currently
        // advanced key *and* pending key because client3 just left.
        assert_eq!(2, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client4.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client4.observer.send_signaling_message_to_group_invocation_count());

        // client5 sends its key to client1 normally and to client2 and client4 using
        // the multi-recipient API.
        assert_eq!(1, client5.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client5.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client5.observer.multi_recipient_count());

        // Wait for keys to be fully rotated.
        std::thread::sleep(std::time::Duration::from_millis(
            MEDIA_SEND_KEY_ROTATION_DELAY_SECS * 1000 + 100,
        ));

        // client5 leaves the call.
        client5.disconnect_and_wait_until_ended();
        assert_eq!(1, client5.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client5.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client5.observer.multi_recipient_count());

        // The other clients don't react because the user is still in the call as client1.
        set_group_and_wait_until_applied(&[&client1, &client2, &client4]);
        assert_eq!(0, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(0, client4.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client4.observer.send_signaling_message_to_group_invocation_count());

        // client4 leaves the call.
        client4.disconnect_and_wait_until_ended();
        assert_eq!(1, client4.observer.send_signaling_message_invocation_count());
        assert_eq!(1, client4.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(2, client4.observer.multi_recipient_count());

        // Nothing should have happened to client1 or client2 yet.
        assert_eq!(0, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_to_group_invocation_count());

        // The other clients should all send rotated media keys to everyone else after
        // learning that client4 and client5 have left. No multi-recipient sends are
        // expected with just two clients remaining in the call.
        set_group_and_wait_until_applied(&[&client1, &client2]);
        assert_eq!(1, client1.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client1.observer.send_signaling_message_to_group_invocation_count());
        assert_eq!(1, client2.observer.send_signaling_message_invocation_count());
        assert_eq!(0, client2.observer.send_signaling_message_to_group_invocation_count());
    }

    #[test]
    fn remote_heartbeat_state() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2]);

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(None, remote_devices2[0].heartbeat_state.audio_muted);
        assert_eq!(None, remote_devices2[0].heartbeat_state.video_muted);
        assert_eq!(None, remote_devices2[0].heartbeat_state.presenting);
        assert_eq!(None, remote_devices2[0].heartbeat_state.sharing_screen);

        client1.client.set_outgoing_audio_muted(true);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.audio_muted);
        assert_eq!(None, remote_devices2[0].heartbeat_state.video_muted);
        assert_eq!(None, remote_devices2[0].heartbeat_state.presenting);
        assert_eq!(None, remote_devices2[0].heartbeat_state.sharing_screen);

        client1.client.set_outgoing_video_muted(false);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.audio_muted);
        assert_eq!(Some(false), remote_devices2[0].heartbeat_state.video_muted);
        assert_eq!(None, remote_devices2[0].heartbeat_state.presenting);
        assert_eq!(None, remote_devices2[0].heartbeat_state.sharing_screen);

        client1.client.set_presenting(true);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.audio_muted);
        assert_eq!(Some(false), remote_devices2[0].heartbeat_state.video_muted);
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.presenting);
        assert_eq!(None, remote_devices2[0].heartbeat_state.sharing_screen);

        client1.client.set_sharing_screen(true);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.audio_muted);
        assert_eq!(Some(false), remote_devices2[0].heartbeat_state.video_muted);
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.presenting);
        assert_eq!(
            Some(true),
            remote_devices2[0].heartbeat_state.sharing_screen
        );
    }

    #[test]
    fn remote_mute_call_observer() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2]);

        client1.client.set_outgoing_audio_muted(false);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        // Should ignore this request due to the wrong demux ID.
        client1.client.handle_remote_mute_request(
            client2.demux_id,
            protobuf::group_call::RemoteMuteRequest {
                target_demux_id: Some(client2.demux_id),
            },
        );
        client1.wait_for_client_to_process_and_tick();
        client2.wait_for_client_to_process();

        // Should not invoke the client callback.
        assert_eq!(*client1.observer.remote_muted_by.lock().unwrap(), None);

        // Should ignore this request due coming from the wrong demux ID.
        client1.client.handle_remote_mute_request(
            client1.demux_id,
            protobuf::group_call::RemoteMuteRequest {
                target_demux_id: Some(client1.demux_id),
            },
        );
        client1.wait_for_client_to_process_and_tick();
        client2.wait_for_client_to_process();

        // Should not invoke the client callback.
        assert_eq!(*client1.observer.remote_muted_by.lock().unwrap(), None);

        client1.client.handle_remote_mute_request(
            client2.demux_id,
            protobuf::group_call::RemoteMuteRequest {
                target_demux_id: Some(client1.demux_id),
            },
        );
        client1.wait_for_client_to_process_and_tick();
        client2.wait_for_client_to_process();
        // Should invoke the client callback
        assert_eq!(
            *client1.observer.remote_muted_by.lock().unwrap(),
            Some(client2.demux_id)
        );

        // Should not have modified heartbeat--that's the client's job.
        assert_eq!(
            client2.observer.observed_remote_mutes.lock().unwrap().len(),
            0
        );
    }

    #[test]
    fn remote_mute_client_response() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2]);

        // Muted before the request, so should still be muted but not attribute it to the
        // remote request.
        client1.client.set_outgoing_audio_muted(true);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.audio_muted);
        assert_eq!(None, remote_devices2[0].heartbeat_state.muted_by_demux_id);

        client1
            .client
            .set_outgoing_audio_muted_remotely(client2.demux_id);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        // Should be muted still!
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.audio_muted);
        assert_eq!(None, remote_devices2[0].heartbeat_state.muted_by_demux_id);

        // Unmuted before the request, so should mute and attribute it to the
        // remote request.
        client1.client.set_outgoing_audio_muted(false);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        client1
            .client
            .set_outgoing_audio_muted_remotely(client2.demux_id);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        // Should be muted now!
        assert_eq!(Some(true), remote_devices2[0].heartbeat_state.audio_muted);
        // Should attribute the mute correctly.
        assert_eq!(
            Some(client2.demux_id),
            remote_devices2[0].heartbeat_state.muted_by_demux_id
        );

        let client2_observed_mutes = client2
            .observer
            .observed_remote_mutes
            .lock()
            .unwrap()
            .clone();
        assert_eq!(1, client2_observed_mutes.len());
        assert_eq!(
            client2_observed_mutes[0],
            (client2.demux_id, client1.demux_id)
        );

        // Unmute should clear mute attribution.
        client1.client.set_outgoing_audio_muted(false);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        let remote_devices2 = client2.observer.remote_devices();
        assert_eq!(1, remote_devices2.len());
        assert_eq!(client1.demux_id, remote_devices2[0].demux_id);
        // Should be muted now!
        assert_eq!(Some(false), remote_devices2[0].heartbeat_state.audio_muted);
        // Should attribute the mute correctly.
        assert_eq!(None, remote_devices2[0].heartbeat_state.muted_by_demux_id);
    }

    #[test]
    fn send_remote_mute() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2]);

        client2.client.set_outgoing_audio_muted(false);
        client1.wait_for_client_to_process();
        client2.wait_for_client_to_process();

        client1.client.send_remote_mute_request(client2.demux_id);
        client2.wait_for_client_to_process_and_tick();

        assert_eq!(
            *client2.observer.remote_muted_by.lock().unwrap(),
            Some(client1.demux_id)
        );
    }

    fn hash_set<T: std::hash::Hash + Eq + Clone>(vals: impl IntoIterator<Item = T>) -> HashSet<T> {
        vals.into_iter().collect()
    }

    #[test]
    fn reactions() {
        let client1 = TestClient::new(vec![1], 1);
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2);
        client2.connect_join_and_wait_until_joined();

        set_group_and_wait_until_applied(&[&client1, &client2]);

        let value = "hello".to_string();

        client1.client.react(value.clone());
        assert!(
            client2
                .observer
                .reactions_called
                .wait(Duration::from_secs(5))
        );
        assert_eq!(1, client2.observer.handle_reactions_invocation_count());
        assert_eq!(1, client2.observer.reactions_count());
        assert_eq!(1, client2.observer.reactions().len());
        assert_eq!(value, client2.observer.reactions()[0].value.to_string());
        assert_eq!(1, client2.observer.reactions()[0].demux_id)
    }

    #[test]
    fn ignore_devices_that_arent_members() {
        let client = TestClient::new(vec![1], 1);
        client.connect_join_and_wait_until_joined();

        assert!(client.observer.remote_devices().is_empty());

        let peek_info = PeekInfo {
            devices: vec![
                PeekDeviceInfo {
                    demux_id: 2,
                    user_id: Some(b"2".to_vec()),
                },
                PeekDeviceInfo {
                    demux_id: 3,
                    user_id: None,
                },
            ],
            pending_devices: vec![],
            creator: None,
            era_id: None,
            max_devices: None,
            call_link_state: None,
        };
        client.client.set_peek_result(Ok(peek_info));
        client.wait_for_client_to_process();

        let remote_devices = client.observer.remote_devices();
        assert_eq!(1, remote_devices.len());
        assert_eq!(2, remote_devices[0].demux_id);

        assert_eq!(vec![b"2".to_vec()], client.observer.joined_members());
    }

    #[test]
    fn fire_events_on_first_peek_info() {
        let client = TestClient::new(vec![1], 1);

        client.client.connect();
        client.client.set_peek_result(Ok(PeekInfo::default()));

        assert!(client.observer.peek_changed.wait(Duration::from_secs(5)));

        client.client.join();
        client.client.set_peek_result(Ok(PeekInfo {
            // This gets filtered out.  Make sure we still fire the event.
            devices: vec![PeekDeviceInfo {
                demux_id: 1,
                user_id: Some(b"1".to_vec()),
            }],
            pending_devices: vec![],
            creator: None,
            era_id: None,
            max_devices: None,
            call_link_state: None,
        }));

        assert!(
            client
                .observer
                .remote_devices_changed
                .wait(Duration::from_secs(5))
        );

        assert_eq!(1, client.observer.peek_state().device_count);
    }

    #[test]
    fn joined_members() {
        // The peeker doesn't join
        let peeker = TestClient::new(vec![42], 42);
        peeker.client.connect();
        peeker.wait_for_client_to_process();

        assert_eq!(0, peeker.observer.joined_members().len());

        let joiner1 = TestClient::new(vec![1], 1);
        let joiner2 = TestClient::new(vec![2], 2);

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

        // Clear the observer state so we can verify we don't get a callback when
        // nothing changes
        peeker.observer.handle_peek_changed(
            0,
            &PeekInfo {
                pending_devices: vec![],
                creator: None,
                era_id: None,
                devices: vec![],
                max_devices: None,
                call_link_state: None,
            },
            &HashSet::default(),
        );
        assert_eq!(0, peeker.observer.joined_members().len());
        peeker.set_remotes_and_wait_until_applied(&[&joiner1, &joiner2]);
        assert_eq!(0, peeker.observer.joined_members().len());
        peeker.observer.handle_peek_changed(
            0,
            &PeekInfo {
                pending_devices: vec![],
                creator: None,
                era_id: None,
                devices: vec![],
                max_devices: None,
                call_link_state: None,
            },
            &([joiner1.user_id.clone(), joiner2.user_id.clone()]
                .iter()
                .cloned()
                .collect()),
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
            hash_set(&[joiner1.user_id, joiner2.user_id]),
            hash_set(&peeker.observer.joined_members())
        );

        peeker.set_remotes_and_wait_until_applied(&[]);
        assert_eq!(0, peeker.observer.joined_members().len());

        peeker.disconnect_and_wait_until_ended();
    }

    #[test]
    fn pending_clients() {
        let peeker = TestClient::new(vec![42], 42);
        peeker.connect_join_and_wait_until_joined();

        assert_eq!(
            vec![peeker.user_id.clone()],
            peeker.observer.joined_members()
        );

        let joiner1 = TestClient::new(vec![1], 1);
        let joiner2 = TestClient::new(vec![2], 2);

        peeker.set_pending_clients_and_wait_until_applied(&[&joiner1]);
        assert!(
            peeker
                .observer
                .peek_changed
                .wait(Duration::from_millis(200))
        );

        peeker.set_pending_clients_and_wait_until_applied(&[&joiner1, &joiner2]);
        assert!(
            peeker
                .observer
                .peek_changed
                .wait(Duration::from_millis(200))
        );

        peeker.set_pending_clients_and_wait_until_applied(&[&joiner2, &joiner1]);
        assert!(
            !peeker
                .observer
                .peek_changed
                .wait(Duration::from_millis(200))
        );

        peeker.set_pending_clients_and_wait_until_applied(&[&joiner1]);
        assert!(
            peeker
                .observer
                .peek_changed
                .wait(Duration::from_millis(200))
        );

        peeker.disconnect_and_wait_until_ended();
    }

    #[test]
    #[ignore] // Because it's too slow
    fn smart_polling() {
        let client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);

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
        client1.observer.joined.wait(Duration::from_secs(5));
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
        let client = TestClient::new(vec![1], 1);
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
            DeviceToSfu,
            device_to_sfu::{
                VideoRequestMessage, video_request_message::VideoRequest as VideoRequestProto,
            },
        };

        let mut client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);
        let client3 = TestClient::new(vec![3], 3);
        let client4 = TestClient::new(vec![4], 4);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[&client2, &client3, &client4]);

        let requests = vec![
            VideoRequest {
                demux_id: 2,
                width: 1920,
                height: 1080,
                framerate: None,
            },
            VideoRequest {
                demux_id: 3,
                // Rotated!
                width: 80,
                height: 120,
                framerate: Some(5),
            },
            VideoRequest {
                demux_id: 4,
                width: 0,
                height: 0,
                framerate: None,
            },
            // This should be filtered out
            VideoRequest {
                demux_id: 5,
                width: 1000,
                height: 1000,
                framerate: None,
            },
        ];
        client1.client.request_video(requests.clone(), 0);
        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                video_request: Some(VideoRequestMessage {
                    requests: vec![
                        VideoRequestProto {
                            demux_id: Some(2),
                            height: Some(1080),
                        },
                        VideoRequestProto {
                            demux_id: Some(3),
                            height: Some(80),
                        },
                        VideoRequestProto {
                            demux_id: Some(4),
                            height: Some(0),
                        },
                    ],
                    max_kbps: Some(NORMAL_MAX_RECEIVE_RATE.as_kbps() as u32),
                    active_speaker_height: Some(0),
                }),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.client.request_video(requests.clone(), 0);
        client1.client.request_video(requests.clone(), 0);
        client1.client.request_video(requests.clone(), 0);
        client1.client.request_video(requests.clone(), 0);

        let before = Instant::now();
        let _ = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("Get RTP packet to SFU");
        let elapsed = Instant::now() - before;
        assert!(elapsed > Duration::from_millis(980));
        assert!(elapsed < Duration::from_millis(1020));

        client1.client.request_video(requests.clone(), 1080);
        client1.client.request_video(requests.clone(), 1080);
        client1.client.request_video(requests.clone(), 1080);
        client1.client.request_video(requests, 1080);

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

        client1.client.set_data_mode(DataMode::Low);
        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                video_request: Some(VideoRequestMessage {
                    requests: vec![
                        VideoRequestProto {
                            demux_id: Some(2),
                            height: Some(1080),
                        },
                        VideoRequestProto {
                            demux_id: Some(3),
                            height: Some(80),
                        },
                        VideoRequestProto {
                            demux_id: Some(4),
                            height: Some(0),
                        },
                    ],
                    max_kbps: Some(500),
                    active_speaker_height: Some(1080),
                }),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.client.set_data_mode(DataMode::Normal);
        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                video_request: Some(VideoRequestMessage {
                    requests: vec![
                        VideoRequestProto {
                            demux_id: Some(2),
                            height: Some(1080),
                        },
                        VideoRequestProto {
                            demux_id: Some(3),
                            height: Some(80),
                        },
                        VideoRequestProto {
                            demux_id: Some(4),
                            height: Some(0),
                        },
                    ],
                    max_kbps: Some(NORMAL_MAX_RECEIVE_RATE.as_kbps() as u32),
                    active_speaker_height: Some(1080),
                }),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn audio_level_polling() {
        let client1 = TestClient::new(vec![1], 1);
        assert_eq!(0, client1.observer.handle_audio_levels_invocation_count());
        client1.connect_join_and_wait_until_joined();
        assert_eq!(1, client1.observer.handle_audio_levels_invocation_count());
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(1, client1.observer.handle_audio_levels_invocation_count());
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(1, client1.observer.handle_audio_levels_invocation_count());
    }

    #[test]
    fn device_to_sfu_leave() {
        use protobuf::group_call::{DeviceToSfu, device_to_sfu::LeaveMessage};

        let mut client1 = TestClient::new(vec![1], 1);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[]);
        client1.client.leave();

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                leave: Some(LeaveMessage {}),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );
    }

    #[test]
    fn device_to_sfu_remove() {
        use protobuf::group_call::{
            DeviceToSfu,
            device_to_sfu::{AdminAction, GenericAdminAction},
        };

        let mut client1 = TestClient::new(vec![1], 1);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[]);
        client1.client.remove_client(32);

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                admin_action: Some(AdminAction::Remove(GenericAdminAction {
                    target_demux_id: Some(32)
                })),
                mrp_header: Some(MrpHeader {
                    seqnum: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn device_to_sfu_block() {
        use protobuf::group_call::{
            DeviceToSfu,
            device_to_sfu::{AdminAction, GenericAdminAction},
        };

        let mut client1 = TestClient::new(vec![1], 1);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[]);
        client1.client.block_client(32);

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                admin_action: Some(AdminAction::Block(GenericAdminAction {
                    target_demux_id: Some(32)
                })),
                mrp_header: Some(MrpHeader {
                    seqnum: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn device_to_sfu_approve() {
        use protobuf::group_call::{
            DeviceToSfu,
            device_to_sfu::{AdminAction, GenericAdminAction},
        };

        let mut client1 = TestClient::new(vec![1], 1);

        let remote1 = TestClient::new(vec![11], 16);
        let remote2a = TestClient::new(vec![22], 32);
        let remote2b = TestClient::new(vec![22], 48);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_pending_clients_and_wait_until_applied(&[&remote1, &remote2a, &remote2b]);
        client1.client.approve_user(vec![22]);

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                admin_action: Some(AdminAction::Approve(GenericAdminAction {
                    target_demux_id: Some(32)
                })),
                mrp_header: Some(MrpHeader {
                    seqnum: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn approve_not_found() {
        let mut client1 = TestClient::new(vec![1], 1);

        let remote1 = TestClient::new(vec![11], 16);
        let remote2a = TestClient::new(vec![22], 32);
        let remote2b = TestClient::new(vec![22], 48);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_pending_clients_and_wait_until_applied(&[&remote1, &remote2a, &remote2b]);
        client1.client.approve_user(vec![33]);

        receiver
            .recv_timeout(Duration::from_millis(200))
            .expect_err("No packets to send");
    }

    #[test]
    fn device_to_sfu_deny() {
        use protobuf::group_call::{
            DeviceToSfu,
            device_to_sfu::{AdminAction, GenericAdminAction},
        };

        let mut client1 = TestClient::new(vec![1], 1);

        let remote1 = TestClient::new(vec![11], 16);
        let remote2a = TestClient::new(vec![22], 32);
        let remote2b = TestClient::new(vec![22], 48);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_pending_clients_and_wait_until_applied(&[&remote1, &remote2a, &remote2b]);
        client1.client.deny_user(vec![22]);

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        assert_eq!(1, header.ssrc);
        assert_eq!(
            DeviceToSfu {
                admin_action: Some(AdminAction::Deny(GenericAdminAction {
                    target_demux_id: Some(32)
                })),
                mrp_header: Some(MrpHeader {
                    seqnum: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            },
            DeviceToSfu::decode(&payload[..]).unwrap()
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn device_to_sfu_test_fragmented() {
        use protobuf::group_call::{
            DeviceToSfu,
            device_to_sfu::{
                self, StatsReport,
                client_error::{self, DecryptionError},
            },
        };

        let mut client1 = TestClient::new(vec![1], 1);

        let (sender, receiver) = mpsc::channel();
        client1.sfu_rtp_packet_sender = Some(sender);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[]);

        let now = SystemTime::now();
        let errors: HashMap<DemuxId, DecryptionErrorStats> = (0..100)
            .map(|i| {
                (
                    i,
                    DecryptionErrorStats {
                        start_time: now,
                        last_time: now + Duration::from_millis(1000),
                        count: i,
                    },
                )
            })
            .collect();

        let expected_proto = DeviceToSfu {
            stats: Some(StatsReport {
                client_errors: errors
                    .iter()
                    .map(|(demux_id, e)| device_to_sfu::ClientError {
                        error: Some(client_error::Error::Decryption(DecryptionError {
                            sender_demux_id: Some(*demux_id),
                            count: Some(e.count),
                            start_ts: Some(saturating_epoch_time(e.start_time).as_millis() as u64),
                            last_ts: Some(saturating_epoch_time(e.last_time).as_millis() as u64),
                        })),
                    })
                    .collect(),
            }),
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(2503, expected_proto.len());

        let expected_packets = [
            DeviceToSfu {
                mrp_header: Some(MrpHeader {
                    seqnum: Some(1),
                    ack_num: None,
                    num_packets: Some(3),
                }),
                content: Some(expected_proto[0..1140].to_vec()),
                ..Default::default()
            },
            DeviceToSfu {
                mrp_header: Some(MrpHeader {
                    seqnum: Some(2),
                    ack_num: None,
                    num_packets: None,
                }),
                content: Some(expected_proto[1140..2280].to_vec()),
                ..Default::default()
            },
            DeviceToSfu {
                mrp_header: Some(MrpHeader {
                    seqnum: Some(3),
                    ack_num: None,
                    num_packets: None,
                }),
                content: Some(expected_proto[2280..].to_vec()),
                ..Default::default()
            },
        ];

        client1.client.send_decryption_stats(errors);

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        let received_proto1 = DeviceToSfu::decode(&payload[..]).unwrap();
        assert_eq!(1, header.ssrc);
        assert_eq!(expected_packets[0], received_proto1);

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        let received_proto2 = DeviceToSfu::decode(&payload[..]).unwrap();
        assert_eq!(1, header.ssrc);
        assert_eq!(expected_packets[1], received_proto2);

        let (header, payload) = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Get RTP packet to SFU");
        let received_proto3 = DeviceToSfu::decode(&payload[..]).unwrap();
        assert_eq!(1, header.ssrc);
        assert_eq!(expected_packets[2], received_proto3);

        let combined_content = [
            received_proto1.content.unwrap(),
            received_proto2.content.unwrap(),
            received_proto3.content.unwrap(),
        ]
        .concat();
        assert_eq!(combined_content, expected_proto);

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn carry_over_devices_from_peeking_to_joined() {
        let client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);
        let client3 = TestClient::new(vec![3], 3);

        client1.client.set_membership_proof(b"proof".to_vec());
        client1.client.connect();
        client1.wait_for_client_to_process();

        client1.set_remotes_and_wait_until_applied(&[&client1, &client2, &client3]);
        assert_eq!(
            hash_set(vec![
                client1.user_id.clone(),
                client2.user_id,
                client3.user_id
            ]),
            hash_set(client1.observer.joined_members())
        );

        client1.client.join();
        client1.observer.joined.wait(Duration::from_secs(5));
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
        let mut client1 = TestClient::new(vec![1], 1);

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
        let client1 = TestClient::new(vec![1], 1);
        client1.client.set_membership_proof(b"proof".to_vec());
        client1.client.connect();
        client1.wait_for_client_to_process();
        let initial_count = client1.sfu_client.request_count();
        let user_a = GroupMember {
            user_id: b"a".to_vec(),
            member_id: b"A".to_vec(),
        };
        let user_b = GroupMember {
            user_id: b"b".to_vec(),
            member_id: b"B".to_vec(),
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
            .set_group_members(vec![user_b, user_a.clone()]);
        client1.wait_for_client_to_process();
        assert_eq!(initial_count + 1, client1.sfu_client.request_count());

        // Setting a different list triggers a poll
        client1.client.set_group_members(vec![user_a]);
        client1.wait_for_client_to_process();
        assert_eq!(initial_count + 2, client1.sfu_client.request_count());

        client1.set_remotes_and_wait_until_applied(&[]);

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn full_call_without_peeking() {
        let sfu_options = FakeSfuClientOptions { max_joins: Some(2) };
        let sfu_client = FakeSfuClient::with_options(1, None, sfu_options);

        let client1 = TestClient::with_sfu_client(vec![1], 1, sfu_client.clone());
        client1.client.connect();
        client1.client.join();
        assert!(client1.observer.joined.wait(Duration::from_secs(5)));

        let client2 = TestClient::with_sfu_client(vec![2], 1, sfu_client.clone());
        client2.client.connect();
        client2.client.join();
        assert!(client2.observer.joined.wait(Duration::from_secs(5)));

        let client3 = TestClient::with_sfu_client(vec![3], 1, sfu_client);
        client3.client.connect();
        client3.client.join();

        assert_eq!(
            Some(CallEndReason::HasMaxDevices),
            client3.observer.ended.wait(Duration::from_secs(5))
        );
    }

    #[test]
    #[ignore] // Because it's too slow
    fn membership_proof_requests() {
        let client1 = TestClient::new(vec![1], 1);
        client1.client.set_peek_result(Ok(PeekInfo {
            devices: vec![PeekDeviceInfo {
                demux_id: 2,
                user_id: None,
            }],
            max_devices: Some(2),
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        assert_eq!(
            0,
            client1.observer.request_membership_proof_invocation_count()
        );

        // Expect a request for connect and join.
        client1.connect_join_and_wait_until_joined();
        assert_eq!(
            2,
            client1.observer.request_membership_proof_invocation_count()
        );

        std::thread::sleep(
            std::time::Duration::from_millis(2000) + MEMBERSHIP_PROOF_REQUEST_INTERVAL,
        );
        assert_eq!(
            1,
            client1.observer.request_membership_proof_invocation_count()
        );

        client1.disconnect_and_wait_until_ended();
        assert_eq!(
            0,
            client1.observer.request_membership_proof_invocation_count()
        );
    }

    #[test]
    fn speakers() {
        let client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);
        let client3 = TestClient::new(vec![3], 3);
        let client4 = TestClient::new(vec![4], 4);
        client1.connect_join_and_wait_until_joined();
        client1.wait_for_client_to_process();
        assert_eq!(
            1,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        client1.set_remotes_and_wait_until_applied(&[&client1, &client3, &client4]);
        assert_eq!(vec![3, 4], client1.speakers());
        assert_eq!(
            1,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // New people put at the end regardless of DemuxId
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.set_remotes_and_wait_until_applied(&[&client2, &client4, &client3]);
        assert_eq!(vec![3, 4, 2], client1.speakers());
        assert_eq!(
            1,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Changed
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(1, 4);
        assert_eq!(vec![4, 3, 2], client1.speakers());
        assert_eq!(
            1,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Didn't change
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(2, 4);
        assert_eq!(vec![4, 3, 2], client1.speakers());
        assert_eq!(
            0,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Changed back
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(3, 3);
        assert_eq!(vec![3, 4, 2], client1.speakers());
        assert_eq!(
            1,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Ignore unknown demux ID
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(4, 5);
        assert_eq!(vec![3, 4, 2], client1.speakers());
        assert_eq!(
            0,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Didn't change
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(6, 3);
        assert_eq!(vec![3, 4, 2], client1.speakers());
        assert_eq!(
            0,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Ignore old messages
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(5, 4);
        assert_eq!(vec![3, 4, 2], client1.speakers());
        assert_eq!(
            0,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Ignore when the local device is the current speaker
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(7, 1);
        assert_eq!(vec![3, 4, 2], client1.speakers());
        assert_eq!(
            0,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Finally give 2 a chance
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(8, 2);
        assert_eq!(vec![2, 3, 4], client1.speakers());
        assert_eq!(
            1,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Swap only the top two; leave the third alone
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(9, 3);
        assert_eq!(vec![3, 2, 4], client1.speakers());
        assert_eq!(
            1,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        // Unchanged
        std::thread::sleep(std::time::Duration::from_millis(1));
        client1.receive_speaker(10, 3);
        assert_eq!(vec![3, 2, 4], client1.speakers());
        assert_eq!(
            0,
            client1
                .observer
                .handle_remote_devices_changed_invocation_count()
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn forwarding_video() {
        let get_forwarding_videos = |client: &TestClient| -> Vec<(DemuxId, Option<bool>, u16)> {
            client
                .observer
                .remote_devices()
                .iter()
                .map(|remote| {
                    (
                        remote.demux_id,
                        remote.forwarding_video,
                        remote.server_allocated_height,
                    )
                })
                .collect()
        };

        let client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);
        let client3 = TestClient::new(vec![3], 3);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[&client2, &client3]);

        assert_eq!(
            vec![(2, None, 0), (3, None, 0)],
            get_forwarding_videos(&client1)
        );

        Client::handle_forwarding_video_received(&client1.client.actor, vec![2, 3], vec![240, 120]);
        client1.wait_for_client_to_process();

        assert_eq!(
            vec![(2, Some(true), 240), (3, Some(true), 120)],
            get_forwarding_videos(&client1)
        );

        Client::handle_forwarding_video_received(&client1.client.actor, vec![2], vec![120]);
        client1.wait_for_client_to_process();

        assert_eq!(
            vec![(2, Some(true), 120), (3, Some(false), 0)],
            get_forwarding_videos(&client1)
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn client_decoded_height() {
        let get_client_decoded_height = |client: &TestClient| -> Option<u32> {
            client
                .observer
                .remote_devices()
                .iter()
                .map(|remote| remote.client_decoded_height)
                .next()
                .unwrap()
        };
        let set_client_decoded_height = |client: &TestClient, height: u32| {
            let mut remote_devices = client.observer.remote_devices.lock().unwrap();
            remote_devices.get_mut(0).unwrap().client_decoded_height = Some(height);
        };

        let client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[&client2]);

        assert_eq!(None, get_client_decoded_height(&client1));

        Client::handle_forwarding_video_received(&client1.client.actor, vec![2], vec![480]);
        client1.wait_for_client_to_process();

        set_client_decoded_height(&client1, 480);

        // There is no video when forwarding stops, so the height is None
        Client::handle_forwarding_video_received(&client1.client.actor, vec![], vec![]);
        client1.wait_for_client_to_process();

        assert_eq!(None, get_client_decoded_height(&client1));

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn is_higher_resolution_pending() {
        let get_forwarding_videos = |client: &TestClient| -> Vec<(DemuxId, u16)> {
            client
                .observer
                .remote_devices()
                .iter()
                .map(|remote| (remote.demux_id, remote.server_allocated_height))
                .collect()
        };
        let set_client_decoded_height = |client: &TestClient, height: u32| {
            let mut remote_devices = client.observer.remote_devices.lock().unwrap();
            let device = remote_devices.get_mut(0).unwrap();
            device.client_decoded_height = Some(height);
            device.recalculate_higher_resolution_pending();
        };
        let is_higher_resolution_pending = |client: &TestClient| -> bool {
            let mut remote_devices = client.observer.remote_devices.lock().unwrap();
            remote_devices
                .get_mut(0)
                .unwrap()
                .is_higher_resolution_pending
        };

        let client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[&client2]);

        assert_eq!(vec![(2, 0)], get_forwarding_videos(&client1));
        assert!(!is_higher_resolution_pending(&client1));

        Client::handle_forwarding_video_received(&client1.client.actor, vec![2], vec![240]);
        client1.wait_for_client_to_process();

        assert_eq!(vec![(2, 240)], get_forwarding_videos(&client1));

        // A higher resolution is pending because the server allocated a height of 240, but no
        // video has been decoded yet.
        assert!(is_higher_resolution_pending(&client1));

        // After receiving the higher resolution video, the pending status is cleared.
        set_client_decoded_height(&client1, 240);

        assert!(!is_higher_resolution_pending(&client1));

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn removal_before_approval() {
        let client1_demux_id = 1;
        let mut client1 = TestClient::new(vec![1], client1_demux_id);
        let client2 = TestClient::new(vec![2], 2);
        client1
            .sfu_client
            .set_response_join_state(JoinState::Pending(client1_demux_id));

        client1.client.connect();
        client1.client.join();
        client1.set_remotes_and_wait_until_applied(&[&client2]);

        Client::handle_removed_received(&client1.client.actor);
        assert_eq!(
            Some(CallEndReason::DeniedRequestToJoinCall),
            client1.observer.ended.wait(Duration::from_secs(5))
        );
    }

    #[test]
    fn removal_after_approval() {
        let client1 = TestClient::new(vec![1], 1);
        let client2 = TestClient::new(vec![2], 2);
        client1.client.connect();
        client1.client.join();
        client1.set_remotes_and_wait_until_applied(&[&client2, &client1]);

        Client::handle_removed_received(&client1.client.actor);
        assert_eq!(
            Some(CallEndReason::RemovedFromCall),
            client1.observer.ended.wait(Duration::from_secs(5))
        );
    }

    #[test]
    fn send_rates() {
        let client1 = TestClient::new(b"1".to_vec(), 1);
        client1.connect_join_and_wait_until_joined();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(1)),
            }),
            client1.observer.send_rates()
        );

        let devices: Vec<PeekDeviceInfo> = (1..=20)
            .map(|demux_id| {
                let user_id = format!("{}", demux_id);
                PeekDeviceInfo {
                    demux_id,
                    user_id: Some(user_id.as_bytes().to_vec()),
                }
            })
            .collect();
        client1.client.set_peek_result(Ok(PeekInfo {
            devices: vec![],
            max_devices: None,
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(1)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_peek_result(Ok(PeekInfo {
            devices: devices[..1].to_vec(),
            max_devices: None,
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(1)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_peek_result(Ok(PeekInfo {
            devices: devices[..2].to_vec(),
            max_devices: None,
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(1000)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_peek_result(Ok(PeekInfo {
            devices: devices[..5].to_vec(),
            max_devices: None,
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(1000)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_peek_result(Ok(PeekInfo {
            devices: devices[..20].to_vec(),
            max_devices: None,
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(671)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_sharing_screen(true);
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: Some(DataRate::from_kbps(500)),
                start: Some(DataRate::from_kbps(1000)),
                max: Some(DataRate::from_kbps(2000)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_sharing_screen(false);
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(671)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_peek_result(Ok(PeekInfo {
            devices: devices[..1].to_vec(),
            max_devices: None,
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(1)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_sharing_screen(true);
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: None,
                start: None,
                max: Some(DataRate::from_kbps(1)),
            }),
            client1.observer.send_rates()
        );

        client1.client.set_peek_result(Ok(PeekInfo {
            devices: devices[..20].to_vec(),
            max_devices: None,
            pending_devices: vec![],
            creator: None,
            era_id: None,
            call_link_state: None,
        }));
        client1.wait_for_client_to_process();
        assert_eq!(
            Some(SendRates {
                min: Some(DataRate::from_kbps(500)),
                start: Some(DataRate::from_kbps(1000)),
                max: Some(DataRate::from_kbps(2000)),
            }),
            client1.observer.send_rates()
        );

        client1.disconnect_and_wait_until_ended();
    }

    #[test]
    fn group_ring() {
        fn ring_once(era_id: &str) -> RingId {
            let user_id = vec![1];
            let demux_id = 1;

            let mut sfu_client = FakeSfuClient::new(demux_id, Some(user_id.clone()));
            sfu_client.era_id = era_id.to_string();

            let client1 = TestClient::with_sfu_client(user_id, demux_id, sfu_client);
            client1.connect_join_and_wait_until_joined();

            client1.client.ring(None);
            client1.wait_for_client_to_process();
            let sent_messages = std::mem::take(
                &mut *client1
                    .observer
                    .sent_group_signaling_messages
                    .lock()
                    .expect("finished processing"),
            );
            match &sent_messages[..] {
                [
                    protobuf::signaling::CallMessage {
                        ring_intention: Some(ring),
                        ..
                    },
                ] => {
                    assert_eq!(
                        Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
                        ring.r#type,
                    );
                    ring.ring_id.expect("should have an ID").into()
                }
                _ => {
                    panic!(
                        "group messages not as expected; here's what we got: {:?}",
                        sent_messages
                    );
                }
            }
        }

        // Check that the ring IDs are derived from the era ID.
        let first_ring_id = ring_once("1122334455667788");
        let first_ring_id_again = ring_once("1122334455667788");
        assert_eq!(first_ring_id, first_ring_id_again);
        let second_ring_id = ring_once("99aabbccddeeff00");
        assert_ne!(first_ring_id, second_ring_id, "ring IDs were the same");

        // Check that non-hex era IDs are okay too, just in case.
        let non_hex_ring_id = ring_once("mesozoic");
        assert_ne!(first_ring_id, non_hex_ring_id, "ring IDs were the same");
    }

    #[test]
    fn group_ring_cancel() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id.clone(),
            demux_id,
            FakeSfuClient::new(demux_id, Some(user_id)),
        );
        client1.connect_join_and_wait_until_joined();
        client1.client.ring(None);
        client1.client.leave();
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        match &sent_messages[..] {
            [
                protobuf::signaling::CallMessage {
                    ring_intention: Some(ring),
                    ..
                },
                protobuf::signaling::CallMessage {
                    ring_intention: Some(cancel),
                    ..
                },
            ] => {
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
                    ring.r#type,
                );
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Cancelled.into()),
                    cancel.r#type,
                );
                assert_eq!(ring.ring_id, cancel.ring_id, "ring IDs should be the same");
            }
            _ => {
                panic!(
                    "group messages not as expected; here's what we got: {:#?}",
                    sent_messages
                );
            }
        }
    }

    #[test]
    fn group_ring_no_cancel_if_someone_joins() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id.clone(),
            demux_id,
            FakeSfuClient::new(demux_id, Some(user_id)),
        );
        client1.connect_join_and_wait_until_joined();
        client1.client.ring(None);

        let client2 = TestClient::new(vec![2], 2);
        client1.set_remotes_and_wait_until_applied(&[&client2]);

        client1.client.leave();
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        match &sent_messages[..] {
            [
                protobuf::signaling::CallMessage {
                    ring_intention: Some(ring),
                    ..
                },
            ] => {
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
                    ring.r#type,
                );
            }
            _ => {
                panic!(
                    "group messages not as expected; here's what we got: {:#?}",
                    sent_messages
                );
            }
        }
    }

    #[test]
    fn group_ring_no_cancel_if_call_was_not_empty() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id.clone(),
            demux_id,
            FakeSfuClient::new(demux_id, Some(user_id)),
        );
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2);
        client1.set_remotes_and_wait_until_applied(&[&client2]);

        client1.client.ring(None);
        client1.client.leave();
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        match &sent_messages[..] {
            [
                protobuf::signaling::CallMessage {
                    ring_intention: Some(ring),
                    ..
                },
            ] => {
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
                    ring.r#type,
                );
            }
            _ => {
                panic!(
                    "group messages not as expected; here's what we got: {:#?}",
                    sent_messages
                );
            }
        }
    }

    #[test]
    fn group_ring_cancel_if_call_is_currently_empty() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id.clone(),
            demux_id,
            FakeSfuClient::new(demux_id, Some(user_id)),
        );
        client1.connect_join_and_wait_until_joined();

        let client2 = TestClient::new(vec![2], 2);
        client1.set_remotes_and_wait_until_applied(&[&client2]);
        client1.set_remotes_and_wait_until_applied(&[]);

        client1.client.ring(None);
        client1.client.leave();
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        match &sent_messages[..] {
            [
                protobuf::signaling::CallMessage {
                    ring_intention: Some(ring),
                    ..
                },
                protobuf::signaling::CallMessage {
                    ring_intention: Some(cancel),
                    ..
                },
            ] => {
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
                    ring.r#type,
                );
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Cancelled.into()),
                    cancel.r#type,
                );
                assert_eq!(ring.ring_id, cancel.ring_id, "ring IDs should be the same");
            }
            _ => {
                panic!(
                    "group messages not as expected; here's what we got: {:#?}",
                    sent_messages
                );
            }
        }
    }

    #[test]
    fn group_ring_cancel_if_call_is_just_you() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id.clone(),
            demux_id,
            FakeSfuClient::new(demux_id, Some(user_id)),
        );
        client1.connect_join_and_wait_until_joined();

        client1.set_remotes_and_wait_until_applied(&[&client1]);

        client1.client.ring(None);
        client1.client.leave();
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        match &sent_messages[..] {
            [
                protobuf::signaling::CallMessage {
                    ring_intention: Some(ring),
                    ..
                },
                protobuf::signaling::CallMessage {
                    ring_intention: Some(cancel),
                    ..
                },
            ] => {
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
                    ring.r#type,
                );
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Cancelled.into()),
                    cancel.r#type,
                );
                assert_eq!(ring.ring_id, cancel.ring_id, "ring IDs should be the same");
            }
            _ => {
                panic!(
                    "group messages not as expected; here's what we got: {:#?}",
                    sent_messages
                );
            }
        }
    }

    #[test]
    fn group_ring_not_sent_on_different_creator() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id,
            demux_id,
            FakeSfuClient::new(demux_id, Some(vec![2])),
        );
        client1.connect_join_and_wait_until_joined();
        client1.client.ring(None);
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        assert_eq!(&sent_messages, &[]);
    }

    #[test]
    fn group_ring_delayed_until_join() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id.clone(),
            demux_id,
            FakeSfuClient::new(demux_id, Some(user_id)),
        );
        client1.client.connect();
        client1.client.ring(None);
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        assert_eq!(&sent_messages, &[]);

        client1.connect_join_and_wait_until_joined();
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );

        match &sent_messages[..] {
            [
                protobuf::signaling::CallMessage {
                    ring_intention: Some(ring),
                    ..
                },
            ] => {
                assert_eq!(
                    Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
                    ring.r#type,
                );
            }
            _ => {
                panic!(
                    "group messages not as expected; here's what we got: {:#?}",
                    sent_messages
                );
            }
        }
    }

    #[test]
    fn group_ring_delayed_with_different_creator() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id,
            demux_id,
            FakeSfuClient::new(demux_id, Some(vec![2])),
        );
        client1.client.connect();
        client1.client.ring(None);
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        assert_eq!(&sent_messages, &[]);

        client1.connect_join_and_wait_until_joined();
        client1.wait_for_client_to_process();
        let sent_messages = std::mem::take(
            &mut *client1
                .observer
                .sent_group_signaling_messages
                .lock()
                .expect("finished processing"),
        );
        assert_eq!(&sent_messages, &[]);
    }

    fn endorsements_for(
        member_ciphertexts: &[UuidCiphertext],
        expiration: zkgroup::Timestamp,
        now: Timestamp,
    ) -> (
        GroupSendEndorsementsResponse,
        Vec<Vec<u8>>,
        HashMap<UserId, zkgroup::groups::GroupSendEndorsement>,
    ) {
        let todays_key = zkgroup::groups::GroupSendDerivedKeyPair::for_expiration(
            expiration,
            &*ENDORSEMENT_SERVER_ROOT_KEY,
        );
        let member_resolver = CallLinkMemberResolver::from(&*CALL_LINK_ROOT_KEY);
        let endorsements = GroupSendEndorsementsResponse::issue(
            member_ciphertexts.to_vec(),
            &todays_key,
            random(),
        );
        let endorsements_result = GroupSendEndorsementsResponse::issue(
            member_ciphertexts.to_vec(),
            &todays_key,
            random(),
        )
        .receive_with_ciphertexts(
            member_ciphertexts.to_vec(),
            now,
            &*ENDORSEMENT_PUBLIC_ROOT_KEY,
        )
        .unwrap()
        .into_iter()
        .map(|p| p.decompressed);
        let serialized_member_ciphertexts = member_ciphertexts
            .iter()
            .map(zkgroup::serialize)
            .collect::<Vec<_>>();
        let member_ids = serialized_member_ciphertexts
            .iter()
            .map(|ciphertext| member_resolver.resolve_bytes(ciphertext).unwrap());
        let endorsements_result = member_ids.zip(endorsements_result).collect();

        (
            endorsements,
            serialized_member_ciphertexts,
            endorsements_result,
        )
    }

    #[test]
    fn test_handle_send_endorsements_response() {
        let user_id = vec![1];
        let demux_id = 1;
        let client1 = TestClient::with_sfu_client(
            user_id.clone(),
            demux_id,
            FakeSfuClient::new(demux_id, Some(user_id)),
        );
        client1.connect_join_and_wait_until_joined();
        client1.set_remotes_and_wait_until_applied(&[&client1]);
        let sys_at = |epoch_secs| SystemTime::UNIX_EPOCH + Duration::from_secs(epoch_secs);
        let one_day_secs = 86400;
        let now = sys_at(10);
        let now_ts = Timestamp::from_epoch_seconds(
            now.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        let expected_expiration = Timestamp::from_epoch_seconds(one_day_secs);
        let (response, serialized_member_ciphertexts, expected_endorsements_map) =
            endorsements_for(&MEMBER_CIPHERTEXTS, expected_expiration, now_ts);

        // missing serialized GroupSendEndorsementResponse
        {
            let msg = SendEndorsementsResponse {
                serialized: None,
                member_ciphertexts: serialized_member_ciphertexts.clone(),
            };
            Client::handle_send_endorsements_response(&client1.client.actor, now, msg);
            client1
                .observer
                .endorsement_update_event
                .wait(Duration::from_secs(5));
            let update = client1
                .observer
                .endorsement_update
                .lock()
                .unwrap()
                .as_ref()
                .cloned()
                .unwrap();
            assert_eq!(
                update,
                Err(EndorsementUpdateError::MissingField("serialized")),
            );
        }

        // missing member ciphertexts
        {
            let msg = SendEndorsementsResponse {
                serialized: Some(zkgroup::serialize(&response)),
                member_ciphertexts: vec![],
            };
            Client::handle_send_endorsements_response(&client1.client.actor, now, msg);
            client1
                .observer
                .endorsement_update_event
                .wait(Duration::from_secs(5));
            let update = client1
                .observer
                .endorsement_update
                .lock()
                .unwrap()
                .as_ref()
                .cloned()
                .unwrap();
            assert_eq!(
                update,
                Err(EndorsementUpdateError::MissingField("member_ciphertexts")),
            );
        }

        // partial member ciphertexts is bad response
        {
            let msg = SendEndorsementsResponse {
                serialized: Some(zkgroup::serialize(&response)),
                member_ciphertexts: serialized_member_ciphertexts[..1].to_vec(),
            };
            Client::handle_send_endorsements_response(&client1.client.actor, now, msg);
            client1
                .observer
                .endorsement_update_event
                .wait(Duration::from_secs(5));
            let update = client1
                .observer
                .endorsement_update
                .lock()
                .unwrap()
                .as_ref()
                .cloned()
                .unwrap();
            assert_eq!(
                update,
                Err(EndorsementUpdateError::InvalidEndorsementResponse),
            );
        }

        // invalid member ciphertexts format
        {
            let msg = SendEndorsementsResponse {
                serialized: Some(zkgroup::serialize(&response)),
                member_ciphertexts: serialized_member_ciphertexts
                    .iter()
                    .map(|b| b[1..].to_vec())
                    .collect(),
            };
            Client::handle_send_endorsements_response(&client1.client.actor, now, msg);
            client1
                .observer
                .endorsement_update_event
                .wait(Duration::from_secs(5));
            let update = client1
                .observer
                .endorsement_update
                .lock()
                .unwrap()
                .as_ref()
                .cloned()
                .unwrap();
            assert_eq!(
                update,
                Err(EndorsementUpdateError::InvalidMemberCiphertexts),
            );
        }

        // invalid member ciphertexts secret
        {
            let bad_params = CallLinkSecretParams::derive_from_root_key(&[0x42u8; 16]);
            let wrong_key_ciphertexts = MEMBER_IDS
                .iter()
                .map(|id| bad_params.encrypt_uid(*id))
                .map(|ciphertext| zkgroup::serialize(&ciphertext))
                .collect::<Vec<_>>();
            let msg = SendEndorsementsResponse {
                serialized: Some(zkgroup::serialize(&response)),
                member_ciphertexts: wrong_key_ciphertexts,
            };
            Client::handle_send_endorsements_response(&client1.client.actor, now, msg);
            client1
                .observer
                .endorsement_update_event
                .wait(Duration::from_secs(5));
            let update = client1
                .observer
                .endorsement_update
                .lock()
                .unwrap()
                .as_ref()
                .cloned()
                .unwrap();
            assert_eq!(
                update,
                Err(EndorsementUpdateError::InvalidMemberCiphertexts),
            );
        }

        // successful endorsement update
        {
            let msg = SendEndorsementsResponse {
                serialized: Some(zkgroup::serialize(&response)),
                member_ciphertexts: serialized_member_ciphertexts,
            };
            Client::handle_send_endorsements_response(&client1.client.actor, now, msg);
            client1
                .observer
                .endorsement_update_event
                .wait(Duration::from_secs(5));
            let update = client1
                .observer
                .endorsement_update
                .lock()
                .unwrap()
                .as_ref()
                .cloned();
            assert!(
                update.is_some(),
                "should have processed an update and called observer"
            );
            assert_eq!(
                update.unwrap(),
                Ok((expected_expiration, expected_endorsements_map)),
                "Successful endorsements are only updated during client tick()"
            );
        }
    }
}

#[cfg(test)]
mod remote_devices_tests {
    use super::*;

    #[test]
    fn latest_speaker_of_empty_devices() {
        let remote_devices = RemoteDevices::default();
        assert_eq!(None, remote_devices.latest_speaker_demux_id());
    }

    #[test]
    fn latest_speaker_of_zero_speaking_devices() {
        let device_1 = remote_device_state(1, None);
        let device_2 = remote_device_state(2, None);
        let device_3 = remote_device_state(3, None);
        let remote_devices = RemoteDevices::from_iter(vec![device_1, device_2, device_3]);
        assert_eq!(None, remote_devices.latest_speaker_demux_id());
    }

    #[test]
    fn latest_speaker_of_multiple_speaking_devices() {
        let device_1 = remote_device_state(1, Some(time(100)));
        let device_2 = remote_device_state(2, Some(time(101)));
        let device_3 = remote_device_state(3, None);
        let remote_devices = RemoteDevices::from_iter(vec![device_1, device_2, device_3]);
        assert_eq!(Some(2), remote_devices.latest_speaker_demux_id());
    }

    #[test]
    fn find_by_demux_id_when_key_is_not_found() {
        let device_1 = remote_device_state(1, None);
        let device_2 = remote_device_state(2, None);
        let device_3 = remote_device_state(3, None);
        let absent_id = 4;
        let remote_devices = RemoteDevices::from_iter(vec![device_1, device_2, device_3]);
        let device_state = remote_devices.find_by_demux_id(absent_id);
        assert_eq!(None, device_state);
    }

    #[test]
    fn find_by_demux_id() {
        let device_1 = remote_device_state(1, None);
        let device_2 = remote_device_state(2, None);
        let device_3 = remote_device_state(3, None);
        let remote_devices = RemoteDevices::from_iter(vec![device_1, device_2.clone(), device_3]);
        assert_eq!(
            Some(&device_2),
            remote_devices.find_by_demux_id(device_2.demux_id)
        );
    }

    #[test]
    fn find_by_demux_id_mut_when_key_is_not_found() {
        let device_1 = remote_device_state(1, None);
        let device_2 = remote_device_state(2, None);
        let device_3 = remote_device_state(3, None);
        let absent_id = 4;
        let mut remote_devices = RemoteDevices::from_iter(vec![device_1, device_2, device_3]);
        let device_state = remote_devices.find_by_demux_id_mut(absent_id);
        assert_eq!(None, device_state);
    }

    #[test]
    fn find_by_demux_id_mut_and_edit_is_persisted() {
        let device_1 = remote_device_state(1, None);
        let device_2 = remote_device_state(2, None);
        let device_3 = remote_device_state(3, None);
        let device_2_demux_id = device_2.demux_id;
        let mut remote_devices = RemoteDevices::from_iter(vec![device_1, device_2, device_3]);
        let device_state = remote_devices
            .find_by_demux_id_mut(device_2_demux_id)
            .unwrap();
        device_state.speaker_time = Some(time(300));
        let device_state = remote_devices
            .find_by_demux_id_mut(device_2_demux_id)
            .unwrap();
        assert_eq!(Some(time(300)), device_state.speaker_time);
    }

    #[test]
    fn demux_id_set() {
        let device_1 = remote_device_state(1, None);
        let device_2 = remote_device_state(2, None);
        let device_3 = remote_device_state(3, None);
        let remote_devices = RemoteDevices::from_iter(vec![device_1, device_2, device_3]);
        assert_eq!(
            vec![1, 2, 3].into_iter().collect::<HashSet<_>>(),
            remote_devices.demux_id_set()
        );
    }

    fn time(timestamp: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp)
    }

    fn remote_device_state(id: u32, spoken_at: Option<SystemTime>) -> RemoteDeviceState {
        let mut remote_device_state =
            RemoteDeviceState::new(id, id.to_be_bytes().to_vec(), time(1));

        remote_device_state.speaker_time = spoken_at;

        remote_device_state
    }

    #[test]
    fn srtp_keys_from_master_key_material() {
        assert_eq!(
            SrtpKeys {
                client: SrtpKey {
                    suite: SrtpCryptoSuite::AeadAes128Gcm,
                    key: (1..=16).collect(),
                    salt: (17..=28).collect(),
                },
                server: SrtpKey {
                    suite: SrtpCryptoSuite::AeadAes128Gcm,
                    key: (29..=44).collect(),
                    salt: (45..=56).collect(),
                }
            },
            SrtpKeys::from_master_key_material(
                &((1..=56).collect::<Vec<u8>>().try_into().unwrap())
            )
        )
    }

    #[test]
    fn dhe_state() {
        struct NotCryptoRng<T: rand::RngCore>(T);

        impl<T: rand::RngCore> rand::RngCore for NotCryptoRng<T> {
            fn next_u32(&mut self) -> u32 {
                self.0.next_u32()
            }

            fn next_u64(&mut self) -> u64 {
                self.0.next_u64()
            }

            fn fill_bytes(&mut self, dest: &mut [u8]) {
                self.0.fill_bytes(dest)
            }

            fn try_fill_bytes(&mut self, dest: &mut [u8]) -> std::result::Result<(), rand::Error> {
                self.0.try_fill_bytes(dest)
            }
        }

        impl<T: rand::RngCore> rand::CryptoRng for NotCryptoRng<T> {}

        let mut rand = NotCryptoRng(rand::rngs::mock::StepRng::new(1, 1));
        let client_secret = EphemeralSecret::random_from_rng(&mut rand);
        let server_secret = EphemeralSecret::random_from_rng(&mut rand);
        let client_pub_key = PublicKey::from(&client_secret);
        let server_pub_key = PublicKey::from(&server_secret);
        let server_cert = &b"server_cert"[..];

        let mut state = DheState::default();
        assert!(matches!(state, DheState::NotYetStarted));
        state.negotiate_in_place(&server_pub_key, server_cert);
        assert!(matches!(state, DheState::NotYetStarted));

        state = DheState::start(client_secret);
        assert!(matches!(state, DheState::WaitingForServerPublicKey { .. }));
        state.negotiate_in_place(&server_pub_key, server_cert);
        assert!(matches!(state, DheState::Negotiated { .. }));
        if let DheState::Negotiated { srtp_keys } = state {
            let server_master_key_material = {
                // Code copied from the server
                let shared_secret = server_secret.diffie_hellman(&client_pub_key);
                let mut master_key_material = [0u8; 56];
                Hkdf::<Sha256>::new(Some(&[0u8; 32]), shared_secret.as_bytes())
                    .expand_multi_info(
                        &[
                            b"Signal_Group_Call_20211105_SignallingDH_SRTPKey_KDF",
                            server_cert,
                        ],
                        &mut master_key_material,
                    )
                    .unwrap();
                master_key_material
            };
            let expected_srtp_keys =
                SrtpKeys::from_master_key_material(&server_master_key_material);
            assert_eq!(expected_srtp_keys, srtp_keys);
        };
    }

    #[test]
    fn test_mrp_max_size_limit() {
        let content = [5u8; MAX_MRP_FRAGMENT_BYTE_SIZE];
        let sfu_to_device = SfuToDevice {
            mrp_header: Some(protobuf::group_call::MrpHeader {
                seqnum: Some(u64::MAX),
                num_packets: Some(u32::MAX),
                ack_num: Some(u64::MAX),
            }),
            content: Some(content.to_vec()),
            video_request: None,
            speaker: None,
            device_joined_or_left: None,
            current_devices: None,
            stats: None,
            removed: None,
            raised_hands: None,
            endorsements: None,
        };

        assert!(sfu_to_device.encode_to_vec().len() <= MAX_PACKET_SERIALIZED_BYTE_SIZE);
    }
}
