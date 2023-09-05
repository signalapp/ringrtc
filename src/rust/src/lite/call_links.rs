//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

mod base16;
mod member_resolver;
mod root_key;

use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use base64::engine::general_purpose::STANDARD as base64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::lite::http;

pub use member_resolver::CallLinkMemberResolver;
pub use root_key::CallLinkRootKey;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CallLinkRestrictions {
    None,
    AdminApproval,
    #[serde(other, skip_serializing)]
    Unknown,
}

#[derive(Deserialize)]
struct CallLinkResponse<'a> {
    #[serde(rename = "name")]
    encrypted_name: &'a [u8],
    restrictions: CallLinkRestrictions,
    revoked: bool,
    #[serde(rename = "expiration")]
    expiration_unix_timestamp: u64,
}

#[derive(Debug)]
pub struct CallLinkState {
    pub name: String,
    pub restrictions: CallLinkRestrictions,
    pub revoked: bool,
    pub expiration: SystemTime,
}

impl CallLinkState {
    fn from(deserialized: CallLinkResponse<'_>, root_key: &CallLinkRootKey) -> Self {
        let name = if deserialized.encrypted_name.is_empty() {
            "".to_string()
        } else {
            base64
                .decode(deserialized.encrypted_name)
                .ok()
                .and_then(|encrypted_bytes| root_key.decrypt(&encrypted_bytes).ok())
                .and_then(|name_bytes| String::from_utf8(name_bytes).ok())
                .unwrap_or_else(|| {
                    warn!("encrypted name of call failed to decrypt to a valid string");
                    Default::default()
                })
        };
        CallLinkState {
            name,
            restrictions: deserialized.restrictions,
            revoked: deserialized.revoked,
            expiration: SystemTime::UNIX_EPOCH
                + Duration::from_secs(deserialized.expiration_unix_timestamp),
        }
    }
}

pub type ReadCallLinkResultCallback =
    Box<dyn FnOnce(Result<CallLinkState, http::ResponseStatus>) + Send>;

fn call_link_url_from_sfu_url(sfu_url: &str) -> String {
    format!("{}/v1/call-link", sfu_url.trim_end_matches('/'))
}

pub fn auth_header_from_auth_credential(auth_presentation: &[u8]) -> String {
    format!("Bearer auth.{}", base64.encode(auth_presentation))
}

pub fn read_call_link(
    http_client: &dyn http::Client,
    sfu_url: &str,
    root_key: CallLinkRootKey,
    auth_presentation: &[u8],
    result_callback: ReadCallLinkResultCallback,
) {
    http_client.send_request(
        http::Request {
            method: http::Method::Get,
            url: call_link_url_from_sfu_url(sfu_url),
            headers: HashMap::from_iter([
                (
                    "Authorization".to_string(),
                    auth_header_from_auth_credential(auth_presentation),
                ),
                (
                    "X-Room-Id".to_string(),
                    hex::encode(root_key.derive_room_id()),
                ),
            ]),
            body: None,
        },
        Box::new(move |http_response| {
            let result = http::parse_json_response::<CallLinkResponse>(http_response.as_ref())
                .map(|deserialized| CallLinkState::from(deserialized, &root_key));
            result_callback(result);
        }),
    )
}

#[serde_as]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CallLinkCreateRequest<'a> {
    #[serde_as(as = "serde_with::base64::Base64")]
    admin_passkey: &'a [u8],

    #[serde_as(as = "serde_with::base64::Base64")]
    zkparams: &'a [u8],
}

#[serde_as]
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallLinkUpdateRequest<'a> {
    #[serde_as(as = "serde_with::base64::Base64")]
    pub admin_passkey: &'a [u8],

    #[serde(rename = "name", skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<serde_with::base64::Base64>")]
    pub encrypted_name: Option<&'a [u8]>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub restrictions: Option<CallLinkRestrictions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked: Option<bool>,
}

pub fn create_call_link(
    http_client: &dyn http::Client,
    sfu_url: &str,
    root_key: CallLinkRootKey,
    auth_presentation: &[u8],
    admin_passkey: &[u8],
    public_zkparams: &[u8],
    result_callback: ReadCallLinkResultCallback,
) {
    http_client.send_request(
        http::Request {
            method: http::Method::Put,
            url: call_link_url_from_sfu_url(sfu_url),
            headers: HashMap::from_iter([
                (
                    "Authorization".to_string(),
                    format!("Bearer create.{}", base64.encode(auth_presentation)),
                ),
                (
                    "X-Room-Id".to_string(),
                    hex::encode(root_key.derive_room_id()),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ]),
            body: Some(
                serde_json::to_vec(&CallLinkCreateRequest {
                    admin_passkey,
                    zkparams: public_zkparams,
                })
                .expect("cannot fail to serialize"),
            ),
        },
        Box::new(move |http_response| {
            let result = http::parse_json_response::<CallLinkResponse>(http_response.as_ref())
                .map(|deserialized| CallLinkState::from(deserialized, &root_key));
            result_callback(result);
        }),
    )
}

pub fn update_call_link(
    http_client: &dyn http::Client,
    sfu_url: &str,
    root_key: CallLinkRootKey,
    auth_presentation: &[u8],
    update_request: &CallLinkUpdateRequest,
    result_callback: ReadCallLinkResultCallback,
) {
    http_client.send_request(
        http::Request {
            method: http::Method::Put,
            url: call_link_url_from_sfu_url(sfu_url),
            headers: HashMap::from_iter([
                (
                    "Authorization".to_string(),
                    auth_header_from_auth_credential(auth_presentation),
                ),
                (
                    "X-Room-Id".to_string(),
                    hex::encode(root_key.derive_room_id()),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ]),
            body: Some(serde_json::to_vec(update_request).expect("cannot fail to serialize")),
        },
        Box::new(move |http_response| {
            let result = http::parse_json_response::<CallLinkResponse>(http_response.as_ref())
                .map(|deserialized| CallLinkState::from(deserialized, &root_key));
            result_callback(result);
        }),
    )
}

#[cfg(any(target_os = "ios", feature = "check-all"))]
pub mod ios {
    use super::*;

    use std::ffi::{c_char, c_void, CStr};

    use crate::lite::{
        ffi::ios::{cstr, rtc_Bytes, rtc_OptionalU16, rtc_String},
        http,
        sfu::ios::rtc_sfu_Response,
    };

    pub type Client = http::DelegatingClient;

    /// Wrapper around `CallLinkRootKey::try_from(&str)`
    ///
    /// # Safety
    /// - `string` must be a valid, non-null C string
    /// - `callback` must not be null.
    #[no_mangle]
    pub unsafe extern "C" fn rtc_calllinks_CallLinkRootKey_parse(
        string: *const c_char,
        context: *mut c_void,
        callback: extern "C" fn(context: *mut c_void, result: rtc_Bytes),
    ) -> bool {
        let string = CStr::from_ptr(string);
        let root_key = string
            .to_str()
            .ok()
            .and_then(|s| CallLinkRootKey::try_from(s).ok());
        match root_key {
            Some(key) => {
                callback(context, rtc_Bytes::from(key.bytes().as_slice()));
                true
            }
            None => false,
        }
    }

    #[no_mangle]
    pub extern "C" fn rtc_calllinks_CallLinkRootKey_validate(bytes: rtc_Bytes) -> bool {
        CallLinkRootKey::try_from(bytes.as_slice()).is_ok()
    }

    #[no_mangle]
    pub extern "C" fn rtc_calllinks_CallLinkRootKey_generate(
        context: *mut c_void,
        callback: extern "C" fn(context: *mut c_void, result: rtc_Bytes),
    ) {
        let root_key = CallLinkRootKey::generate(rand::rngs::OsRng);
        callback(context, rtc_Bytes::from(root_key.bytes().as_slice()));
    }

    #[no_mangle]
    pub extern "C" fn rtc_calllinks_CallLinkRootKey_generateAdminPasskey(
        context: *mut c_void,
        callback: extern "C" fn(context: *mut c_void, result: rtc_Bytes),
    ) {
        let passkey = CallLinkRootKey::generate_admin_passkey(rand::rngs::OsRng);
        callback(context, rtc_Bytes::from(&passkey));
    }

    #[no_mangle]
    pub extern "C" fn rtc_calllinks_CallLinkRootKey_deriveRoomId(
        root_key_bytes: rtc_Bytes,
        context: *mut c_void,
        callback: extern "C" fn(context: *mut c_void, result: rtc_Bytes),
    ) -> *const c_char {
        match CallLinkRootKey::try_from(root_key_bytes.as_slice()) {
            Ok(root_key) => {
                callback(
                    context,
                    rtc_Bytes::from(root_key.derive_room_id().as_slice()),
                );
                std::ptr::null()
            }
            Err(_) => cstr!("invalid root key").as_ptr(),
        }
    }

    #[no_mangle]
    pub extern "C" fn rtc_calllinks_CallLinkRootKey_toFormattedString(
        root_key_bytes: rtc_Bytes,
        context: *mut c_void,
        callback: extern "C" fn(context: *mut c_void, result: rtc_String),
    ) -> *const c_char {
        match CallLinkRootKey::try_from(root_key_bytes.as_slice()) {
            Ok(root_key) => {
                callback(
                    context,
                    rtc_String::from(root_key.to_formatted_string().as_str()),
                );
                std::ptr::null()
            }
            Err(_) => cstr!("invalid root key").as_ptr(),
        }
    }

    #[repr(C)]
    #[derive(Default, Debug)]
    pub struct rtc_calllinks_CallLinkState<'a> {
        pub name: rtc_String<'a>,
        pub expiration_epoch_seconds: u64,
        pub raw_restrictions: i8,
        pub revoked: bool,
    }

    impl<'a> From<&'a CallLinkState> for rtc_calllinks_CallLinkState<'a> {
        fn from(value: &'a CallLinkState) -> Self {
            Self {
                name: value.name.as_str().into(),
                expiration_epoch_seconds: value
                    .expiration
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                raw_restrictions: match value.restrictions {
                    CallLinkRestrictions::None => 0,
                    CallLinkRestrictions::AdminApproval => 1,
                    CallLinkRestrictions::Unknown => -1,
                },
                revoked: value.revoked,
            }
        }
    }

    #[repr(C)]
    pub struct rtc_sfu_CallLinkDelegate {
        pub retained: *mut c_void,
        pub release: extern "C" fn(retained: *mut c_void),
        pub handle_response: extern "C" fn(
            unretained: *const c_void,
            request_id: u32,
            response: rtc_sfu_Response<rtc_calllinks_CallLinkState<'_>>,
        ),
    }

    impl rtc_sfu_CallLinkDelegate {
        fn handle_response(
            &self,
            request_id: u32,
            result: Result<CallLinkState, http::ResponseStatus>,
        ) {
            let response = match result.as_ref() {
                Ok(state) => rtc_sfu_Response {
                    error_status_code: rtc_OptionalU16::default(),
                    value: state.into(),
                },
                Err(status) => rtc_sfu_Response {
                    error_status_code: status.code.into(),
                    value: Default::default(),
                },
            };
            (self.handle_response)(self.retained, request_id, response);
        }
    }

    unsafe impl Send for rtc_sfu_CallLinkDelegate {}

    impl Drop for rtc_sfu_CallLinkDelegate {
        fn drop(&mut self) {
            (self.release)(self.retained)
        }
    }

    /// # Safety
    ///
    /// - `http_client` must come from `rtc_http_Client_create` and not already be destroyed
    /// - `sfu_url` must be a valid, non-null C string.
    #[no_mangle]
    pub unsafe extern "C" fn rtc_sfu_readCallLink(
        http_client: *const http::ios::Client,
        request_id: u32,
        sfu_url: *const c_char,
        auth_credential_presentation: rtc_Bytes,
        link_root_key: rtc_Bytes,
        delegate: rtc_sfu_CallLinkDelegate,
    ) {
        info!("rtc_sfu_readCallLink():");

        if let Some(http_client) = http_client.as_ref() {
            if let Ok(sfu_url) = CStr::from_ptr(sfu_url).to_str() {
                if let Ok(link_root_key) = CallLinkRootKey::try_from(link_root_key.as_slice()) {
                    read_call_link(
                        http_client,
                        sfu_url,
                        link_root_key,
                        auth_credential_presentation.as_slice(),
                        Box::new(move |result| delegate.handle_response(request_id, result)),
                    )
                } else {
                    error!("invalid link_root_key");
                }
            } else {
                error!("invalid sfu_url");
            }
        } else {
            error!("null http_client passed into rtc_sfu_readCallLink");
        }
    }

    /// # Safety
    ///
    /// - `http_client` must come from `rtc_http_Client_create` and not already be destroyed
    /// - `sfu_url` must be a valid, non-null C string.
    #[no_mangle]
    pub unsafe extern "C" fn rtc_sfu_createCallLink(
        http_client: *const http::ios::Client,
        request_id: u32,
        sfu_url: *const c_char,
        create_credential_presentation: rtc_Bytes,
        link_root_key: rtc_Bytes,
        admin_passkey: rtc_Bytes,
        call_link_public_params: rtc_Bytes,
        delegate: rtc_sfu_CallLinkDelegate,
    ) {
        info!("rtc_sfu_createCallLink():");

        if let Some(http_client) = http_client.as_ref() {
            if let Ok(sfu_url) = CStr::from_ptr(sfu_url).to_str() {
                if let Ok(link_root_key) = CallLinkRootKey::try_from(link_root_key.as_slice()) {
                    create_call_link(
                        http_client,
                        sfu_url,
                        link_root_key,
                        create_credential_presentation.as_slice(),
                        admin_passkey.as_slice(),
                        call_link_public_params.as_slice(),
                        Box::new(move |result| delegate.handle_response(request_id, result)),
                    )
                } else {
                    error!("invalid link_root_key");
                }
            } else {
                error!("invalid sfu_url");
            }
        } else {
            error!("null http_client passed into rtc_sfu_createCallLink");
        }
    }

    /// # Safety
    ///
    /// - `http_client` must come from `rtc_http_Client_create` and not already be destroyed
    /// - `sfu_url` must be a valid, non-null C string.
    #[no_mangle]
    pub unsafe extern "C" fn rtc_sfu_updateCallLink(
        http_client: *const http::ios::Client,
        request_id: u32,
        sfu_url: *const c_char,
        auth_credential_presentation: rtc_Bytes,
        link_root_key: rtc_Bytes,
        admin_passkey: rtc_Bytes,
        new_name: *const c_char,
        new_restrictions: i8,
        new_revoked: i8,
        delegate: rtc_sfu_CallLinkDelegate,
    ) {
        info!("rtc_sfu_updateCallLink():");

        if let Some(http_client) = http_client.as_ref() {
            if let Ok(sfu_url) = CStr::from_ptr(sfu_url).to_str() {
                if let Ok(link_root_key) = CallLinkRootKey::try_from(link_root_key.as_slice()) {
                    let new_name = if new_name.is_null() {
                        None
                    } else {
                        Some(CStr::from_ptr(new_name))
                    };
                    let encrypted_name = new_name.map(|name| {
                        let name_bytes = name.to_bytes();
                        if name_bytes.is_empty() {
                            vec![]
                        } else {
                            link_root_key.encrypt(name_bytes, rand::rngs::OsRng)
                        }
                    });
                    update_call_link(
                        http_client,
                        sfu_url,
                        link_root_key,
                        auth_credential_presentation.as_slice(),
                        &CallLinkUpdateRequest {
                            admin_passkey: admin_passkey.as_slice(),
                            encrypted_name: encrypted_name.as_deref(),
                            restrictions: match new_restrictions {
                                0 => Some(CallLinkRestrictions::None),
                                1 => Some(CallLinkRestrictions::AdminApproval),
                                _ => None,
                            },
                            revoked: match new_revoked {
                                0 => Some(false),
                                1 => Some(true),
                                _ => None,
                            },
                        },
                        Box::new(move |result| delegate.handle_response(request_id, result)),
                    )
                } else {
                    error!("invalid link_root_key");
                }
            } else {
                error!("invalid sfu_url");
            }
        } else {
            error!("null http_client passed into rtc_sfu_createCallLink");
        }
    }
}
