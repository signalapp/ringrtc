//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Make calls to an SFU to see who is in the call.
//! and define common types like PeekInfo, MembershipProof, MemberInfo

use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::Arc,
};

use base64::{engine::general_purpose::STANDARD as base64, Engine};
use hex::ToHex;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sha2::{Digest, Sha256};

use crate::{
    lite::{
        call_links::{CallLinkResponse, CallLinkRootKey, CallLinkState},
        http,
    },
    protobuf::group_call::sfu_to_device::{
        peek_info::PeekDeviceInfo as ProtoPeekDeviceInfo, PeekInfo as ProtoPeekInfo,
    },
};

/// The state that can be observed by "peeking".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeekInfo {
    /// All currently participating devices
    pub devices: Vec<PeekDeviceInfo>,
    /// Devices waiting to be approved by an admin
    pub pending_devices: Vec<PeekDeviceInfo>,
    /// The user who created the call
    pub creator: Option<UserId>,
    /// The "era" of this group call; changes every time the last participant leaves and someone else joins again.
    pub era_id: Option<String>,
    /// The maximum number of devices that can join this group call.
    pub max_devices: Option<u32>,
    /// The call link state of the group call
    pub call_link_state: Option<CallLinkState>,
}

impl PeekInfo {
    pub fn unique_users(&self) -> HashSet<&UserId> {
        self.devices
            .iter()
            .filter_map(|device| device.user_id.as_ref())
            .collect()
    }

    /// Returns pending users in the order they requested approval
    /// Currently relies on the SFU returning the clients in order
    pub fn unique_pending_users(&self) -> Vec<&UserId> {
        let mut seen: HashSet<Option<&UserId>> = HashSet::new();

        self.pending_devices
            .iter()
            .filter_map(|device| {
                let user_ref = device.user_id.as_ref();

                if seen.insert(user_ref) {
                    user_ref
                } else {
                    None
                }
            })
            .collect()
    }

    /// The number of devices currently joined (including the local device, any pending devices, and
    /// unknown users).
    pub fn device_count_including_pending_devices(&self) -> usize {
        self.devices.len() + self.pending_devices.len()
    }

    pub fn deobfuscate_proto(
        proto: ProtoPeekInfo,
        obfuscated_resolver: &ObfuscatedResolver,
    ) -> Result<Self, String> {
        let expected_devices = proto.devices.len();
        let expected_pending_devices = proto.pending_devices.len();
        let expect_call_link = proto.call_link_state.is_some();
        let serialized_peek: SerializedPeekInfo = SerializedPeekInfo {
            era_id: proto.era_id,
            max_devices: proto.max_devices,
            devices: proto
                .devices
                .into_iter()
                .flat_map(TryInto::try_into)
                .collect(),
            creator: proto.creator,
            pending_clients: proto
                .pending_devices
                .into_iter()
                .flat_map(TryInto::try_into)
                .collect(),
            call_link_state: proto
                .call_link_state
                .as_ref()
                .map(TryInto::try_into)
                .transpose()
                .ok()
                .flatten(),
        };

        if serialized_peek.devices.len() != expected_devices
            || serialized_peek.pending_clients.len() != expected_pending_devices
            || serialized_peek.call_link_state.is_some() != expect_call_link
        {
            return Err("Invalid PeekInfo proto".to_string());
        }

        Ok(serialized_peek.deobfuscate(
            obfuscated_resolver,
            obfuscated_resolver.call_link_root_key.as_ref(),
        ))
    }
}

/// The per-device state observed by "peeking".
#[derive(Clone, Debug, PartialEq)]
pub struct PeekDeviceInfo {
    pub demux_id: DemuxId,
    pub user_id: Option<UserId>,
}

/// Form of PeekInfo sent over HTTP.
/// Notably, it has obfuscated user IDs.
#[derive(Deserialize, Debug)]
struct SerializedPeekInfo<'a> {
    #[serde(rename = "conferenceId")]
    era_id: Option<String>,
    #[serde(rename = "maxDevices")]
    max_devices: Option<u32>,
    #[serde(rename = "participants")]
    devices: Vec<SerializedPeekDeviceInfo>,
    creator: Option<String>,
    #[serde(rename = "pendingClients", default)]
    pending_clients: Vec<SerializedPeekDeviceInfo>,
    #[serde(rename = "callLinkState", borrow)]
    call_link_state: Option<CallLinkResponse<'a>>,
}

/// Form of PeekDeviceInfo sent over HTTP.
/// Notable, it has obfuscated user IDs.
#[derive(Deserialize, Debug)]
struct SerializedPeekDeviceInfo {
    #[serde(rename = "opaqueUserId")]
    opaque_user_id: Option<OpaqueUserId>,
    #[serde(rename = "demuxId")]
    demux_id: u32,
}

impl SerializedPeekInfo<'_> {
    fn deobfuscate(
        self,
        member_resolver: &dyn MemberResolver,
        root_key: Option<&CallLinkRootKey>,
    ) -> PeekInfo {
        let state: Option<CallLinkState> = match (self.call_link_state, root_key) {
            (Some(s), Some(r)) => {
                let s = CallLinkState::from_serialized(s, r);
                Some(s)
            }
            _ => None,
        };

        PeekInfo {
            devices: self
                .devices
                .into_iter()
                .map(|device| device.deobfuscate(member_resolver))
                .collect(),
            pending_devices: self
                .pending_clients
                .into_iter()
                .map(|device| device.deobfuscate(member_resolver))
                .collect(),
            creator: self
                .creator
                .as_ref()
                .and_then(|opaque_user_id| member_resolver.resolve(opaque_user_id)),
            era_id: self.era_id,
            max_devices: self.max_devices,
            call_link_state: state,
        }
    }
}

impl SerializedPeekDeviceInfo {
    fn deobfuscate(self, member_resolver: &dyn MemberResolver) -> PeekDeviceInfo {
        PeekDeviceInfo {
            demux_id: self.demux_id,
            user_id: self
                .opaque_user_id
                .and_then(|user_id| member_resolver.resolve(&user_id)),
        }
    }
}

impl TryFrom<ProtoPeekDeviceInfo> for SerializedPeekDeviceInfo {
    type Error = String;
    fn try_from(
        ProtoPeekDeviceInfo {
            demux_id,
            opaque_user_id,
        }: ProtoPeekDeviceInfo,
    ) -> Result<Self, Self::Error> {
        if demux_id.is_none() {
            return Err("Missing required fields in PeekDeviceInfo".to_string());
        }

        Ok(Self {
            opaque_user_id: opaque_user_id.map(OpaqueUserId::from),
            demux_id: demux_id.unwrap(),
        })
    }
}

#[derive(Deserialize, Debug)]
struct SerializedPeekFailure<'a> {
    reason: &'a str,
}

#[derive(Deserialize, Debug)]
struct SerializedJoinResponse {
    #[serde(rename = "demuxId")]
    client_demux_id: u32,
    #[serde(rename = "ips")]
    server_ips: Vec<IpAddr>,
    #[serde(rename = "port")]
    server_port: u16,
    #[serde(rename = "portTcp")]
    server_port_tcp: u16,
    #[serde(rename = "portTls", default)]
    server_port_tls: Option<u16>,
    #[serde(rename = "hostname", default)]
    server_hostname: Option<String>,
    #[serde(rename = "iceUfrag")]
    server_ice_ufrag: String,
    #[serde(rename = "icePwd")]
    server_ice_pwd: String,
    #[serde(rename = "dhePublicKey", with = "hex")]
    server_dhe_pub_key: [u8; 32],
    #[serde(rename = "callCreator", default)]
    call_creator: String,
    #[serde(rename = "conferenceId")]
    era_id: String,
    #[serde(rename = "clientStatus")]
    client_status: String,
}

#[derive(PartialEq, Eq, Debug)]
pub enum ClientStatus {
    Active,
    Pending,
    Blocked,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseClientStatusError;

impl std::str::FromStr for ClientStatus {
    type Err = ParseClientStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" => Ok(ClientStatus::Active),
            "PENDING" => Ok(ClientStatus::Pending),
            "BLOCKED" => Ok(ClientStatus::Blocked),
            _ => Err(ParseClientStatusError),
        }
    }
}

#[derive(Debug)]
pub struct JoinResponse {
    pub client_demux_id: u32,
    pub server_udp_addresses: Vec<SocketAddr>,
    pub server_tcp_addresses: Vec<SocketAddr>,
    pub server_tls_addresses: Vec<SocketAddr>,
    pub server_hostname: Option<String>,
    pub server_ice_ufrag: String,
    pub server_ice_pwd: String,
    pub server_dhe_pub_key: [u8; 32],
    pub call_creator: Option<UserId>,
    pub era_id: String,
    pub client_status: ClientStatus,
}

impl JoinResponse {
    fn from(deserialized: SerializedJoinResponse, member_resolver: &dyn MemberResolver) -> Self {
        let server_udp_addresses = deserialized
            .server_ips
            .iter()
            .map(|ip| SocketAddr::new(*ip, deserialized.server_port))
            .collect();

        let server_tcp_addresses = deserialized
            .server_ips
            .iter()
            .map(|ip| SocketAddr::new(*ip, deserialized.server_port_tcp))
            .collect();
        let server_tls_addresses = deserialized
            .server_port_tls
            .map_or(vec![], |server_port_tls| {
                deserialized
                    .server_ips
                    .iter()
                    .map(|ip| SocketAddr::new(*ip, server_port_tls))
                    .collect()
            });

        Self {
            client_demux_id: deserialized.client_demux_id,
            server_udp_addresses,
            server_tcp_addresses,
            server_tls_addresses,
            server_hostname: deserialized.server_hostname,
            server_ice_ufrag: deserialized.server_ice_ufrag,
            server_ice_pwd: deserialized.server_ice_pwd,
            server_dhe_pub_key: deserialized.server_dhe_pub_key,
            call_creator: member_resolver.resolve(&deserialized.call_creator),
            era_id: deserialized.era_id,
            client_status: ClientStatus::from_str(&deserialized.client_status)
                .ok()
                .unwrap_or(ClientStatus::Pending),
        }
    }
}

/// Proof to the SFU that we are a member of a group.
/// Used as authentication for peeking and other operations.
pub type MembershipProof = Vec<u8>;

// User UUID cipher text within the context of the group
pub type GroupMemberId = Vec<u8>;

// hex(sha256(GroupMemberId))
// This is what the SFU knows and is used to communicate with the SFU.
// It must be mapped to a UserId to be useful.
pub type OpaqueUserId = String;

/// The SFU doesn't actually know this value.
/// It knows an obfuscated GroupMemberId which we then
/// map to this value using GroupMember.
pub type UserId = Vec<u8>;

// Each device joined to a group call is assigned a DemuxID
// which is used for demuxing media, but also identifying
// the device.
// 0 is not a valid value
// When given as remote devices, these must have "gaps"
// That allow for enough SSRCs to be derived from them.
// Currently that gap is 16.
pub type DemuxId = u32;

pub trait MemberResolver {
    fn resolve(&self, opaque_user_id: &str) -> Option<UserId>;
}

pub struct ObfuscatedResolver {
    member_resolver: Arc<dyn MemberResolver + Send + Sync>,
    call_link_root_key: Option<CallLinkRootKey>,
}

impl ObfuscatedResolver {
    pub fn new(
        member_resolver: Arc<dyn MemberResolver + Send + Sync>,
        call_link_root_key: Option<CallLinkRootKey>,
    ) -> Self {
        Self {
            member_resolver,
            call_link_root_key,
        }
    }

    pub fn resolve_user_id(&self, opaque_user_id: &str) -> Option<UserId> {
        self.member_resolver.resolve(opaque_user_id)
    }

    pub fn resolve_call_link_name(&self, opaque_call_link_name: &str) -> Option<String> {
        self.call_link_root_key.as_ref().map(|root_key| {
            base64
                .decode(opaque_call_link_name)
                .ok()
                .and_then(|encrypted_bytes| root_key.decrypt(&encrypted_bytes).ok())
                .and_then(|name_bytes| String::from_utf8(name_bytes).ok())
                .unwrap_or_else(|| {
                    warn!("encrypted name of call failed to decrypt to a valid string");
                    Default::default()
                })
        })
    }

    pub fn set_member_resolver(&mut self, member_resolver: Arc<dyn MemberResolver + Send + Sync>) {
        self.member_resolver = member_resolver;
    }

    pub fn get_call_link_root_key(&self) -> Option<&CallLinkRootKey> {
        self.call_link_root_key.as_ref()
    }
}

impl MemberResolver for ObfuscatedResolver {
    fn resolve(&self, user_id: &str) -> Option<UserId> {
        self.resolve_user_id(user_id)
    }
}

/// Associates a group member's UserId with their GroupMemberId.
/// This is passed from the client to RingRTC to be able to create OpaqueUserIdMappings.
#[derive(Clone, Debug)]
pub struct GroupMember {
    pub user_id: UserId,
    pub member_id: GroupMemberId,
}

#[derive(Default)]
pub struct MemberMap {
    members: Vec<OpaqueUserIdMapping>,
}

impl MemberMap {
    pub fn new(group_members: &[GroupMember]) -> Self {
        Self {
            members: group_members
                .iter()
                .map(OpaqueUserIdMapping::from)
                .collect(),
        }
    }
}

impl MemberResolver for MemberMap {
    fn resolve(&self, opaque_user_id: &str) -> Option<UserId> {
        self.members.iter().find_map(|entry| {
            if entry.opaque_user_id == opaque_user_id {
                Some(entry.user_id.clone())
            } else {
                None
            }
        })
    }
}

/// Associates a group member's OpaqueUserId with their UUID.
/// This is kept by RingRTC to be able to turn an OpaqueUserId into a UserId.
#[derive(Clone, Debug)]
pub struct OpaqueUserIdMapping {
    pub opaque_user_id: OpaqueUserId,
    pub user_id: UserId,
}

impl From<&GroupMember> for OpaqueUserIdMapping {
    fn from(member: &GroupMember) -> Self {
        Self {
            opaque_user_id: sha256_as_hexstring(&member.member_id),
            user_id: member.user_id.clone(),
        }
    }
}

/// Computes a SHA-256 hash of the input value and returns it as a hex string.
///
/// ```
/// use ringrtc::lite::sfu::sha256_as_hexstring;
///
/// assert_eq!(sha256_as_hexstring("abc".as_bytes()), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
/// ```
pub fn sha256_as_hexstring(data: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(data);
    hash.finalize().encode_hex::<String>()
}

pub fn auth_header_from_membership_proof(proof: &[u8]) -> Option<String> {
    // TODO: temporary until the SFU is updated to ignore the username part of the token and make it truly opaque.
    let token = std::str::from_utf8(proof);
    if token.is_err() {
        error!("Membership token isn't valid UTF-8");
    }
    let token = token.ok()?;
    let uuid = token.split(':').next();
    if uuid.is_none() {
        error!("No UUID part in the membership token");
    }
    let uuid = uuid?;
    Some(format!(
        "Basic {}",
        base64.encode(format!("{}:{}", uuid, token))
    ))
}

/// The platform-specific methods the application must provide in order to
/// make SFU calls.
pub trait Delegate {
    /// Called as a response to peek()
    fn handle_peek_result(&self, request_id: u32, peek_result: PeekResult);
}

fn participants_url_from_sfu_url(sfu_url: &str) -> String {
    format!(
        "{}/v2/conference/participants",
        sfu_url.trim_end_matches('/'),
    )
}

fn classify_not_found(body: &[u8]) -> Option<http::ResponseStatus> {
    let parsed: SerializedPeekFailure = match serde_json::from_slice(body) {
        Ok(parsed) => parsed,
        Err(e) => {
            error!("invalid JSON returned from SFU on peek failure: {e}");
            return None;
        }
    };
    info!(
        "Got group call peek result with status code 404 ({})",
        parsed.reason
    );
    match parsed.reason {
        "expired" => Some(http::ResponseStatus::CALL_LINK_EXPIRED),
        "invalid" => Some(http::ResponseStatus::CALL_LINK_INVALID),
        _ => None,
    }
}

fn http_request_headers(
    authorization: String,
    room_id: Option<String>,
    epoch: Option<String>,
    content_type: Option<String>,
) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), authorization);
    if let Some(room_id) = room_id {
        headers.insert("X-Room-Id".to_string(), room_id);
    }
    if let Some(epoch) = epoch {
        headers.insert("X-Epoch".to_string(), epoch);
    }
    if let Some(content_type) = content_type {
        headers.insert("Content-Type".to_string(), content_type);
    }
    headers
}

pub type PeekResult = Result<PeekInfo, http::ResponseStatus>;
pub type PeekResultCallback = Box<dyn FnOnce(PeekResult) + Send>;

#[allow(clippy::too_many_arguments)]
pub fn peek(
    http_client: &dyn http::Client,
    sfu_url: &str,
    room_id_header: Option<String>,
    epoch_header: Option<String>,
    auth_header: String,
    member_resolver: Arc<dyn MemberResolver + Send + Sync>,
    call_link_root_key: Option<CallLinkRootKey>,
    result_callback: PeekResultCallback,
) {
    http_client.send_request(
        http::Request {
            method: http::Method::Get,
            url: participants_url_from_sfu_url(sfu_url),
            headers: http_request_headers(auth_header, room_id_header, epoch_header, None),
            body: None,
        },
        Box::new(move |http_response| {
            let result = match http::parse_json_response::<SerializedPeekInfo>(
                http_response.as_ref(),
            ) {
                Ok(deserialized) => {
                    info!(
                        "Got group call peek result with device count = {}, pending count = {}",
                        deserialized.devices.len(),
                        deserialized.pending_clients.len(),
                    );
                    Ok(deserialized.deobfuscate(&*member_resolver, call_link_root_key.as_ref()))
                }
                Err(status) if status == http::ResponseStatus::GROUP_CALL_NOT_STARTED => {
                    if let Some(body) = http_response
                        .as_ref()
                        .map(|r| &r.body)
                        .filter(|body| !body.is_empty())
                    {
                        Err(classify_not_found(body).unwrap_or(status))
                    } else {
                        info!("Got group call peek result with device count = 0 (status code 404)");
                        Ok(PeekInfo::default())
                    }
                }
                Err(status) => {
                    info!(
                        "Got group call peek result with status code = {}",
                        status.code
                    );
                    Err(status)
                }
            };
            result_callback(result);
        }),
    )
}

pub type JoinResult = Result<JoinResponse, http::ResponseStatus>;
pub type JoinResultCallback = Box<dyn FnOnce(JoinResult) + Send>;

#[serde_as]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JoinRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<serde_with::base64::Base64>")]
    admin_passkey: Option<&'a [u8]>,

    ice_ufrag: &'a str,
    ice_pwd: &'a str,

    #[serde_as(as = "serde_with::hex::Hex")]
    dhe_public_key: &'a [u8],

    #[serde_as(as = "serde_with::hex::Hex")]
    hkdf_extra_info: &'a [u8],
}

#[allow(clippy::too_many_arguments)]
pub fn join(
    http_client: &dyn http::Client,
    sfu_url: &str,
    room_id_header: Option<String>,
    epoch_header: Option<String>,
    auth_header: String,
    admin_passkey: Option<&[u8]>,
    client_ice_ufrag: &str,
    client_ice_pwd: &str,
    client_dhe_pub_key: &[u8],
    hkdf_extra_info: &[u8],
    member_resolver: Arc<dyn MemberResolver + Send + Sync>,
    result_callback: JoinResultCallback,
) {
    info!("sfu:Join(): ");

    http_client.send_request(
        http::Request {
            method: http::Method::Put,
            url: participants_url_from_sfu_url(sfu_url),
            headers: http_request_headers(
                auth_header,
                room_id_header,
                epoch_header,
                Some("application/json".to_string()),
            ),
            body: Some(
                serde_json::to_vec(&JoinRequest {
                    admin_passkey,
                    ice_ufrag: client_ice_ufrag,
                    ice_pwd: client_ice_pwd,
                    dhe_public_key: client_dhe_pub_key,
                    hkdf_extra_info,
                })
                .expect("always valid"),
            ),
        },
        Box::new(move |http_response| {
            let result =
                http::parse_json_response::<SerializedJoinResponse>(http_response.as_ref())
                    .map(|deserialized| JoinResponse::from(deserialized, &*member_resolver));
            result_callback(result)
        }),
    );
}

#[cfg(any(target_os = "ios", feature = "check-all"))]
pub mod ios {
    use std::{
        ffi::{c_char, CStr},
        sync::Arc,
    };

    use libc::{c_void, size_t};

    use crate::lite::{
        call_links::{
            self, ios::from_optional_u32_to_epoch, CallLinkMemberResolver, CallLinkRootKey,
        },
        ffi::ios::{rtc_Bytes, rtc_OptionalU16, rtc_OptionalU32, rtc_String, FromOrDefault},
        http,
        sfu::{self, Delegate, GroupMember, PeekInfo, PeekResult},
    };

    /// # Safety
    ///
    /// http_client_ptr must come from rtc_http_Client_create and not already be destroyed
    #[no_mangle]
    pub unsafe extern "C" fn rtc_sfu_peek(
        http_client: *const http::ios::Client,
        request_id: u32,
        request: rtc_sfu_PeekRequest,
        delegate: rtc_sfu_Delegate,
    ) {
        info!("rtc_sfu_peek():");

        if let Some(http_client) = http_client.as_ref() {
            if let Some(sfu_url) = request.sfu_url.to_string() {
                if let Some(auth_header) =
                    sfu::auth_header_from_membership_proof(request.membership_proof.as_slice())
                {
                    let group_members = request.group_members.to_vec();
                    let opaque_user_id_mappings = sfu::MemberMap::new(&group_members);
                    super::peek(
                        http_client,
                        &sfu_url,
                        None,
                        None,
                        auth_header,
                        Arc::new(opaque_user_id_mappings),
                        None,
                        Box::new(move |peek_result| {
                            delegate.handle_peek_result(request_id, peek_result)
                        }),
                    );
                } else {
                    error!("Invalid membership proof");
                }
            } else {
                error!("Invalid SFU URL");
            }
        } else {
            error!("null http_client passed into rtc_sfu_peek");
        }
    }

    /// # Safety
    ///
    /// - `http_client` must come from `rtc_http_Client_create` and not already be destroyed
    /// - `sfu_url` must be a valid, non-null C string.
    #[no_mangle]
    pub unsafe extern "C" fn rtc_sfu_peekCallLink(
        http_client: *const http::ios::Client,
        request_id: u32,
        sfu_url: *const c_char,
        auth_credential_presentation: rtc_Bytes,
        link_root_key: rtc_Bytes,
        epoch: rtc_OptionalU32,
        delegate: rtc_sfu_Delegate,
    ) {
        info!("rtc_sfu_peekCallLink():");

        if let Some(http_client) = http_client.as_ref() {
            if let Ok(sfu_url) = CStr::from_ptr(sfu_url).to_str() {
                if let Ok(link_root_key) = CallLinkRootKey::try_from(link_root_key.as_slice()) {
                    let epoch = from_optional_u32_to_epoch(epoch).map(|e| e.to_string());
                    super::peek(
                        http_client,
                        sfu_url,
                        Some(hex::encode(link_root_key.derive_room_id())),
                        epoch,
                        call_links::auth_header_from_auth_credential(
                            auth_credential_presentation.as_slice(),
                        ),
                        Arc::new(CallLinkMemberResolver::from(&link_root_key)),
                        Some(link_root_key),
                        Box::new(move |peek_result| {
                            delegate.handle_peek_result(request_id, peek_result)
                        }),
                    );
                } else {
                    error!("invalid link_root_key");
                }
            } else {
                error!("invalid sfu_url");
            }
        } else {
            error!("null http_client passed into rtc_sfu_peekCallLink");
        }
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_sfu_PeekRequest<'a> {
        sfu_url: rtc_String<'a>,
        membership_proof: rtc_Bytes<'a>,
        group_members: rtc_sfu_GroupMembers<'a>,
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_sfu_GroupMembers<'a> {
        pub ptr: *const rtc_sfu_GroupMember<'a>,
        pub count: size_t,
        phantom: std::marker::PhantomData<&'a rtc_sfu_GroupMembers<'a>>,
    }

    impl<'a> rtc_sfu_GroupMembers<'a> {
        fn as_slice(&self) -> &'a [rtc_sfu_GroupMember<'a>] {
            if self.ptr.is_null() {
                return &[];
            }
            unsafe { std::slice::from_raw_parts(self.ptr, self.count) }
        }

        fn to_vec(&self) -> Vec<GroupMember> {
            self.as_slice()
                .iter()
                .map(rtc_sfu_GroupMember::to_group_member)
                .collect()
        }
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_sfu_GroupMember<'a> {
        pub user_id: rtc_Bytes<'a>,
        pub member_id: rtc_Bytes<'a>,
    }

    impl rtc_sfu_GroupMember<'_> {
        fn to_group_member(&self) -> GroupMember {
            GroupMember {
                user_id: self.user_id.to_vec(),
                member_id: self.member_id.to_vec(),
            }
        }
    }
    #[repr(C)]
    pub struct rtc_sfu_Delegate {
        pub retained: *mut c_void,
        pub release: extern "C" fn(retained: *mut c_void),
        pub handle_peek_response: extern "C" fn(
            unretained: *const c_void,
            request_id: u32,
            peek_response: rtc_sfu_Response<rtc_sfu_PeekInfo<'_>>,
        ),
    }

    unsafe impl Send for rtc_sfu_Delegate {}

    impl Drop for rtc_sfu_Delegate {
        fn drop(&mut self) {
            (self.release)(self.retained)
        }
    }

    impl sfu::Delegate for rtc_sfu_Delegate {
        fn handle_peek_result(&self, request_id: u32, peek_result: PeekResult) {
            let (peek_info, error_status_code) = match peek_result {
                Ok(peek_info) => (peek_info, rtc_OptionalU16::default()),
                Err(status) => (PeekInfo::default(), rtc_OptionalU16::from(status.code)),
            };
            let joined_members = peek_info.unique_users();
            let rtc_joined_members: Vec<rtc_Bytes<'_>> =
                joined_members.iter().map(rtc_Bytes::from).collect();
            let pending_users = peek_info.unique_pending_users();
            let rtc_pending_users: Vec<rtc_Bytes<'_>> =
                pending_users.iter().map(rtc_Bytes::from).collect();
            let response = rtc_sfu_Response {
                error_status_code,
                value: rtc_sfu_PeekInfo {
                    joined_members: rtc_UserIds::from(&rtc_joined_members),
                    creator: rtc_Bytes::from_or_default(peek_info.creator.as_ref()),
                    era_id: rtc_String::from_or_default(peek_info.era_id.as_ref()),
                    max_devices: rtc_OptionalU32::from_or_default(peek_info.max_devices),
                    device_count_including_pending_devices: peek_info
                        .device_count_including_pending_devices()
                        as u32,
                    device_count_excluding_pending_devices: peek_info.devices.len() as u32,
                    pending_users: rtc_UserIds::from(&rtc_pending_users),
                },
            };
            let unretained = self.retained;
            (self.handle_peek_response)(unretained, request_id, response);
        }
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_sfu_Response<T> {
        pub error_status_code: rtc_OptionalU16,
        pub value: T,
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_sfu_PeekInfo<'a> {
        creator: rtc_Bytes<'a>,
        era_id: rtc_String<'a>,
        max_devices: rtc_OptionalU32,
        device_count_including_pending_devices: u32,
        device_count_excluding_pending_devices: u32,
        joined_members: rtc_UserIds<'a>,
        pending_users: rtc_UserIds<'a>,
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_UserIds<'a> {
        pub ptr: *const rtc_Bytes<'a>,
        pub count: size_t,
        phantom: std::marker::PhantomData<&'a rtc_UserIds<'a>>,
    }

    impl<'a, T: AsRef<[rtc_Bytes<'a>]>> From<&'a T> for rtc_UserIds<'a> {
        fn from(user_ids: &'a T) -> Self {
            let user_ids = user_ids.as_ref();
            Self {
                ptr: user_ids.as_ptr(),
                count: user_ids.len(),
                phantom: std::marker::PhantomData,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::lite::call_links::{CallLinkMemberResolver, CallLinkRootKey};

    #[test]
    fn endpoint_ids_to_user_ids_by_map() {
        let user_map = MemberMap {
            members: vec![
                OpaqueUserIdMapping {
                    user_id: vec![1u8; 4],
                    opaque_user_id: "u1".to_string(),
                },
                OpaqueUserIdMapping {
                    user_id: vec![2u8; 4],
                    opaque_user_id: "u2".to_string(),
                },
            ],
        };

        let peek_response = SerializedPeekInfo {
            era_id: Some("paleozoic".to_string()),
            max_devices: Some(16),
            devices: vec![
                SerializedPeekDeviceInfo {
                    opaque_user_id: Some("u1".to_string()),
                    demux_id: 0x11111110,
                },
                SerializedPeekDeviceInfo {
                    opaque_user_id: Some("u2".to_string()),
                    demux_id: 0x22222220,
                },
            ],
            pending_clients: vec![],
            creator: None,
            call_link_state: None,
        };

        let peek_info = peek_response.deobfuscate(&user_map, None);
        assert_eq!(
            peek_info
                .devices
                .iter()
                .filter_map(|device| device.user_id.as_ref())
                .collect::<Vec<_>>(),
            vec![[1u8; 4].as_ref(), [2u8; 4].as_ref()]
        );
    }

    #[test]
    fn endpoint_ids_to_user_ids_by_map_in_pending_clients() {
        let user_map = MemberMap {
            members: vec![
                OpaqueUserIdMapping {
                    user_id: vec![1u8; 4],
                    opaque_user_id: "u1".to_string(),
                },
                OpaqueUserIdMapping {
                    user_id: vec![2u8; 4],
                    opaque_user_id: "u2".to_string(),
                },
            ],
        };

        let peek_response = SerializedPeekInfo {
            era_id: Some("paleozoic".to_string()),
            max_devices: Some(16),
            devices: vec![],
            pending_clients: vec![
                SerializedPeekDeviceInfo {
                    opaque_user_id: Some("u1".to_string()),
                    demux_id: 0x11111110,
                },
                SerializedPeekDeviceInfo {
                    opaque_user_id: Some("u2".to_string()),
                    demux_id: 0x22222220,
                },
            ],
            creator: None,
            call_link_state: None,
        };

        let peek_info = peek_response.deobfuscate(&user_map, None);
        assert_eq!(
            peek_info
                .pending_devices
                .iter()
                .filter_map(|device| device.user_id.as_ref())
                .collect::<Vec<_>>(),
            vec![[1u8; 4].as_ref(), [2u8; 4].as_ref()]
        );
    }

    #[test]
    fn deobfuscate_proto_peek_info() {
        let mut members = vec![
            OpaqueUserIdMapping {
                user_id: vec![1u8; 4],
                opaque_user_id: "u1".to_string(),
            },
            OpaqueUserIdMapping {
                user_id: vec![2u8; 4],
                opaque_user_id: "u2".to_string(),
            },
            OpaqueUserIdMapping {
                user_id: vec![3u8; 4],
                opaque_user_id: "u3".to_string(),
            },
            OpaqueUserIdMapping {
                user_id: vec![4u8; 4],
                opaque_user_id: "u4".to_string(),
            },
        ];

        let mut obfuscated_resolver = ObfuscatedResolver::new(
            Arc::new(MemberMap {
                members: members.clone(),
            }),
            None,
        );

        let proto_peek = ProtoPeekInfo {
            era_id: Some("paleozoic".to_string()),
            max_devices: Some(16),
            devices: vec![
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u1".to_string()),
                    demux_id: Some(0x11111110),
                },
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u2".to_string()),
                    demux_id: Some(0x22222220),
                },
            ],
            pending_devices: vec![
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u3".to_string()),
                    demux_id: Some(0x33333330),
                },
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u4".to_string()),
                    demux_id: Some(0x44444440),
                },
            ],
            creator: Some("u1".to_string()),
            call_link_state: None,
        };

        let peek_info = PeekInfo::deobfuscate_proto(proto_peek, &obfuscated_resolver);
        assert_eq!(
            peek_info,
            Ok(PeekInfo {
                devices: vec![
                    PeekDeviceInfo {
                        demux_id: 0x11111110,
                        user_id: Some(vec![1u8; 4]),
                    },
                    PeekDeviceInfo {
                        demux_id: 0x22222220,
                        user_id: Some(vec![2u8; 4]),
                    },
                ],
                pending_devices: vec![
                    PeekDeviceInfo {
                        demux_id: 0x33333330,
                        user_id: Some(vec![3u8; 4]),
                    },
                    PeekDeviceInfo {
                        demux_id: 0x44444440,
                        user_id: Some(vec![4u8; 4]),
                    },
                ],
                creator: Some(vec![1u8; 4]),
                era_id: Some("paleozoic".to_string()),
                max_devices: Some(16),
                call_link_state: None,
            })
        );

        members.push(OpaqueUserIdMapping {
            user_id: vec![5u8; 4],
            opaque_user_id: "u5".to_string(),
        });
        obfuscated_resolver.set_member_resolver(Arc::new(MemberMap { members }));

        let proto_peek = ProtoPeekInfo {
            era_id: Some("paleozoic".to_string()),
            max_devices: Some(16),
            devices: vec![
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u1".to_string()),
                    demux_id: Some(0x11111110),
                },
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u2".to_string()),
                    demux_id: Some(0x22222220),
                },
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u5".to_string()),
                    demux_id: Some(0x55555550),
                },
            ],
            pending_devices: vec![
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u3".to_string()),
                    demux_id: Some(0x33333330),
                },
                ProtoPeekDeviceInfo {
                    opaque_user_id: Some("u4".to_string()),
                    demux_id: Some(0x44444440),
                },
            ],
            creator: Some("u1".to_string()),
            call_link_state: None,
        };

        let peek_info = PeekInfo::deobfuscate_proto(proto_peek, &obfuscated_resolver);
        assert_eq!(
            peek_info,
            Ok(PeekInfo {
                devices: vec![
                    PeekDeviceInfo {
                        demux_id: 0x11111110,
                        user_id: Some(vec![1u8; 4]),
                    },
                    PeekDeviceInfo {
                        demux_id: 0x22222220,
                        user_id: Some(vec![2u8; 4]),
                    },
                    PeekDeviceInfo {
                        demux_id: 0x55555550,
                        user_id: Some(vec![5u8; 4]),
                    },
                ],
                pending_devices: vec![
                    PeekDeviceInfo {
                        demux_id: 0x33333330,
                        user_id: Some(vec![3u8; 4]),
                    },
                    PeekDeviceInfo {
                        demux_id: 0x44444440,
                        user_id: Some(vec![4u8; 4]),
                    },
                ],
                creator: Some(vec![1u8; 4]),
                era_id: Some("paleozoic".to_string()),
                max_devices: Some(16),
                call_link_state: None,
            })
        );
    }

    #[allow(clippy::unusual_byte_groupings)]
    #[test]
    fn endpoint_ids_to_user_ids_by_zk_encryption() {
        let uuid_1 = 0x_aaaaaaaa_7000_11eb_b32a_33b8a8a487a6_u128.to_be_bytes();
        let uuid_2 = 0x_bbbbbbbb_7000_11eb_b32a_33b8a8a487a6_u128.to_be_bytes();

        let root_key = CallLinkRootKey::try_from(
            0x_0011_2233_4455_6677_8899_aabb_ccdd_eeff_u128
                .to_be_bytes()
                .as_slice(),
        )
        .unwrap();
        let secret_params =
            zkgroup::call_links::CallLinkSecretParams::derive_from_root_key(&root_key.bytes());

        fn encrypt(uuid: [u8; 16], params: &zkgroup::call_links::CallLinkSecretParams) -> String {
            hex::encode(
                bincode::serialize(&params.encrypt_uid(Uuid::from_bytes(uuid).into())).unwrap(),
            )
        }

        let resolver = CallLinkMemberResolver::from(&root_key);

        for i in 0..2 {
            let peek_response = SerializedPeekInfo {
                era_id: Some("paleozoic".to_string()),
                max_devices: Some(16),
                devices: vec![
                    SerializedPeekDeviceInfo {
                        opaque_user_id: Some(encrypt(uuid_1, &secret_params)),
                        demux_id: 0x11111110,
                    },
                    SerializedPeekDeviceInfo {
                        opaque_user_id: Some(encrypt(uuid_2, &secret_params)),
                        demux_id: 0x22222220,
                    },
                ],
                pending_clients: vec![],
                creator: None,
                call_link_state: None,
            };

            let peek_info = peek_response.deobfuscate(&resolver, Some(&root_key));
            assert_eq!(
                peek_info
                    .devices
                    .iter()
                    .filter_map(|device| device.user_id.as_ref())
                    .collect::<Vec<_>>(),
                vec![uuid_1.as_slice(), uuid_2.as_slice()]
            );
            // The second time around the resolver should use its cache.
            assert_eq!(
                i * 2,
                resolver
                    .cache_hits
                    .load(std::sync::atomic::Ordering::Relaxed)
            );
        }
    }
}
