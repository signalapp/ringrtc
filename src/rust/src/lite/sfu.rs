//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Make calls to an SFU to see who is in the call.
//! and define common types like PeekInfo, MembershipProof, MemberInfo

use std::{
    collections::{HashMap, HashSet},
    iter::FromIterator,
    net::IpAddr,
    net::SocketAddr,
};

use hex::ToHex;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::lite::http;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum ResponseCode {
    GroupCallNotStarted = 404,
    GroupCallFull = 413,
    // Artifical codes not actually returned by the server
    InvalidClientAuth = 601,
    RequestFailed = 602,
    InvalidResponseBodyUtf8 = 603,
    InvalidResponseBodyJson = 604,
}

impl From<ResponseCode> for http::ResponseStatus {
    fn from(code: ResponseCode) -> Self {
        http::ResponseStatus::from(code as u16)
    }
}

impl PartialEq<ResponseCode> for http::ResponseStatus {
    fn eq(&self, code: &ResponseCode) -> bool {
        self.code == (*code as u16)
    }
}

/// The state that can be observed by "peeking".
#[derive(Clone, Debug, Default)]
pub struct PeekInfo {
    /// Currently joined devices, excluding the local device and unknown users
    pub devices: Vec<PeekDeviceInfo>,
    /// The user who created the call
    pub creator: Option<UserId>,
    /// The "era" of this group call; changes every time the last partipant leaves and someone else joins again.
    pub era_id: Option<String>,
    /// The maximum number of devices that can join this group call.
    pub max_devices: Option<u32>,
    /// The number of devices currently joined (including the local device and unknown users).
    pub device_count: u32,
}

impl PeekInfo {
    pub fn unique_users(&self) -> HashSet<&UserId> {
        self.devices
            .iter()
            .filter_map(|device| device.user_id.as_ref())
            .collect()
    }
}
/// The per-device state observed by "peeking".
#[derive(Clone, Debug)]
pub struct PeekDeviceInfo {
    pub demux_id: DemuxId,
    pub user_id: Option<UserId>,
}

/// Form of PeekInfo sent over HTTP.
/// Notably, it has obfuscated user IDs.
#[derive(Deserialize, Debug)]
struct SerializedPeekInfo {
    #[serde(rename = "conferenceId")]
    era_id: Option<String>,
    #[serde(rename = "maxDevices")]
    max_devices: Option<u32>,
    #[serde(rename = "participants")]
    devices: Vec<SerializedPeekDeviceInfo>,
    creator: Option<String>,
}

/// Form of PeekDeviceInfo sent over HTTP.
/// Notable, it has obfuscated user IDs.
#[derive(Deserialize, Debug)]
struct SerializedPeekDeviceInfo {
    #[serde(rename = "opaqueUserId")]
    opaque_user_id: OpaqueUserId,
    #[serde(rename = "demuxId")]
    demux_id: u32,
}

impl SerializedPeekInfo {
    fn deobfuscate(self, opaque_user_id_mappings: &[OpaqueUserIdMapping]) -> PeekInfo {
        let device_count = self.devices.len() as u32;
        PeekInfo {
            devices: self
                .devices
                .into_iter()
                .map(|device| device.deobfuscate(opaque_user_id_mappings))
                .collect(),
            creator: self.creator.as_ref().and_then(|opaque_user_id| {
                SerializedPeekDeviceInfo::deobfuscate_user_id(
                    opaque_user_id_mappings,
                    opaque_user_id,
                )
            }),
            era_id: self.era_id,
            max_devices: self.max_devices,
            device_count,
        }
    }
}

impl SerializedPeekDeviceInfo {
    fn deobfuscate(self, opaque_user_id_mappings: &[OpaqueUserIdMapping]) -> PeekDeviceInfo {
        PeekDeviceInfo {
            demux_id: self.demux_id,
            user_id: Self::deobfuscate_user_id(opaque_user_id_mappings, &self.opaque_user_id),
        }
    }

    fn deobfuscate_user_id(
        opaque_user_id_mappings: &[OpaqueUserIdMapping],
        opaque_user_id: &str,
    ) -> Option<UserId> {
        opaque_user_id_mappings.iter().find_map(|mapping| {
            if opaque_user_id == mapping.opaque_user_id {
                Some(mapping.user_id.clone())
            } else {
                None
            }
        })
    }
}

#[derive(Deserialize, Debug)]
struct SerializedJoinResponse {
    #[serde(rename = "demuxId")]
    client_demux_id: u32,
    #[serde(rename = "ip")]
    server_ip: IpAddr,
    #[serde(rename = "port")]
    server_port: u16,
    #[serde(rename = "iceUfrag")]
    server_ice_ufrag: String,
    #[serde(rename = "icePwd")]
    server_ice_pwd: String,
    #[serde(rename = "dhePublicKey", with = "hex")]
    server_dhe_pub_key: [u8; 32],
}

#[derive(Debug)]
pub struct JoinResponse {
    pub client_demux_id: u32,
    pub server_addresses: Vec<SocketAddr>,
    pub server_ice_ufrag: String,
    pub server_ice_pwd: String,
    pub server_dhe_pub_key: [u8; 32],
}

impl From<SerializedJoinResponse> for JoinResponse {
    fn from(deserialized: SerializedJoinResponse) -> Self {
        Self {
            client_demux_id: deserialized.client_demux_id,
            server_addresses: vec![SocketAddr::new(
                deserialized.server_ip,
                deserialized.server_port,
            )],
            server_ice_ufrag: deserialized.server_ice_ufrag,
            server_ice_pwd: deserialized.server_ice_pwd,
            server_dhe_pub_key: deserialized.server_dhe_pub_key,
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

/// Associates a group member's UserId with their GroupMemberId.
/// This is passed from the client to RingRTC to be able to create OpaqueUserIdMappings.
#[derive(Clone, Debug)]
pub struct GroupMember {
    pub user_id: UserId,
    pub member_id: GroupMemberId,
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
        base64::encode(format!("{}:{}", uuid, token))
    ))
}

pub fn opaque_user_id_mappings_from_group_members(
    group_members: &[GroupMember],
) -> Vec<OpaqueUserIdMapping> {
    group_members
        .iter()
        .map(OpaqueUserIdMapping::from)
        .collect()
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
        sfu_url.trim_end_matches('/')
    )
}

fn parse_http_json_response<'a, D: Deserialize<'a>>(
    response: Option<&'a http::Response>,
) -> Result<D, http::ResponseStatus> {
    let response = response.ok_or(ResponseCode::RequestFailed)?;
    if !response.status.is_success() {
        return Err(response.status);
    }
    let body =
        std::str::from_utf8(&response.body).map_err(|_| ResponseCode::InvalidResponseBodyUtf8)?;
    let deserialized =
        serde_json::from_str(body).map_err(|_| ResponseCode::InvalidResponseBodyJson)?;
    Ok(deserialized)
}

pub type PeekResult = Result<PeekInfo, http::ResponseStatus>;
pub type PeekResultCallback = Box<dyn FnOnce(PeekResult) + Send>;

pub fn peek(
    http_client: &dyn http::Client,
    sfu_url: &str,
    auth_header: String,
    opaque_user_id_mappings: Vec<OpaqueUserIdMapping>,
    result_callback: PeekResultCallback,
) {
    http_client.send_request(
        http::Request {
            method: http::Method::Get,
            url: participants_url_from_sfu_url(sfu_url),
            headers: HashMap::from_iter([("Authorization".to_string(), auth_header)]),
            body: None,
        },
        Box::new(move |http_response| {
            let result =
                match parse_http_json_response::<SerializedPeekInfo>(http_response.as_ref()) {
                    Ok(deserialized) => {
                        info!(
                            "Got group call peek result with device count = {}",
                            deserialized.devices.len()
                        );
                        Ok(deserialized.deobfuscate(&opaque_user_id_mappings))
                    }
                    Err(status) if status == ResponseCode::GroupCallNotStarted => {
                        info!("Got group call peek result with device count = 0 (status code 404)");
                        Ok(PeekInfo::default())
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

pub fn join(
    http_client: &dyn http::Client,
    sfu_url: &str,
    auth_header: String,
    client_ice_ufrag: &str,
    client_dhe_pub_key: &[u8],
    hkdf_extra_info: &[u8],
    result_callback: JoinResultCallback,
) {
    info!("sfu::Join(): ");

    http_client.send_request(
        http::Request {
            method: http::Method::Put,
            url: participants_url_from_sfu_url(sfu_url),
            headers: HashMap::from_iter([
                ("Authorization".to_string(), auth_header),
                ("Content-Type".to_string(), "application/json".to_string()),
            ]),
            body: Some(
                json!({
                    "iceUfrag" : client_ice_ufrag,
                    "dhePublicKey": client_dhe_pub_key.encode_hex::<String>(),
                    "hkdfExtraInfo": hkdf_extra_info.encode_hex::<String>(),
                })
                .to_string()
                .into_bytes(),
            ),
        },
        Box::new(move |http_response| {
            let result = parse_http_json_response::<SerializedJoinResponse>(http_response.as_ref())
                .map(JoinResponse::from);
            result_callback(result)
        }),
    );
}

#[cfg(any(target_os = "ios", feature = "check-all"))]
pub mod ios {
    use crate::lite::{
        ffi::ios::{rtc_Bytes, rtc_OptionalU16, rtc_OptionalU32, rtc_String, FromOrDefault},
        http, sfu,
        sfu::{Delegate, GroupMember, PeekInfo, PeekResult},
    };
    use libc::{c_void, size_t};

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
                    let opaque_user_id_mappings =
                        sfu::opaque_user_id_mappings_from_group_members(&group_members);
                    super::peek(
                        http_client,
                        &sfu_url,
                        auth_header,
                        opaque_user_id_mappings,
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

    impl<'a> rtc_sfu_GroupMember<'a> {
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
            peek_response: rtc_sfu_PeekResponse,
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
            let response = rtc_sfu_PeekResponse {
                error_status_code,
                peek_info: rtc_sfu_PeekInfo {
                    joined_members: rtc_UserIds::from(&rtc_joined_members),
                    creator: rtc_Bytes::from_or_default(peek_info.creator.as_ref()),
                    era_id: rtc_String::from_or_default(peek_info.era_id.as_ref()),
                    max_devices: rtc_OptionalU32::from_or_default(peek_info.max_devices),
                    device_count: peek_info.device_count,
                },
            };
            let unretained = self.retained;
            (self.handle_peek_response)(unretained, request_id, response);
        }
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_sfu_PeekResponse<'a> {
        error_status_code: rtc_OptionalU16,
        peek_info: rtc_sfu_PeekInfo<'a>,
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_sfu_PeekInfo<'a> {
        creator: rtc_Bytes<'a>,
        era_id: rtc_String<'a>,
        max_devices: rtc_OptionalU32,
        device_count: u32,
        joined_members: rtc_UserIds<'a>,
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
