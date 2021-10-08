//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation Create/Set SessionDescription

use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::ptr;

use libc::{size_t, strdup};

use crate::core::util::RustObject;
use crate::webrtc::sdp_observer::{
    CreateSessionDescriptionObserver, CreateSessionDescriptionObserverCallbacks,
    RffiConnectionParametersV4, SetSessionDescriptionObserver,
    SetSessionDescriptionObserverCallbacks, SrtpCryptoSuite,
};

/// Simulation type for SessionDescription.
pub type RffiSessionDescription = &'static str;

static mut FAKE_SDP: &str = "FAKE SDP";
static mut FAKE_SDP_OFFER: &str = "FAKE SDP OFFER";
static mut FAKE_SDP_ANSWER: &str = "FAKE SDP ANSWER";

/// Simulation type for webrtc::rffi::CreateSessionDescriptionObserverRffi
pub type RffiCreateSessionDescriptionObserver = u32;

static FAKE_CSD_OBSERVER: u32 = 13;

/// Simulation type for webrtc::rffi::SetSessionDescriptionObserverRffi
pub type RffiSetSessionDescriptionObserver = u32;

static FAKE_SSD_OBSERVER: u32 = 15;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createSetSessionDescriptionObserver(
    ssd_observer: RustObject,
    ssd_observer_cb: *const c_void,
) -> *const RffiSetSessionDescriptionObserver {
    info!("Rust_createSetSessionDescriptionObserver():");

    // Hit the onSuccess() callback
    let call_backs = ssd_observer_cb as *const SetSessionDescriptionObserverCallbacks;
    ((*call_backs).onSuccess)(ssd_observer as *mut SetSessionDescriptionObserver);

    &FAKE_SSD_OBSERVER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createCreateSessionDescriptionObserver(
    csd_observer: RustObject,
    csd_observer_cb: *const c_void,
) -> *const RffiCreateSessionDescriptionObserver {
    info!("Rust_createCreateSessionDescriptionObserver():");

    // Hit the onSuccess() callback
    let call_backs = csd_observer_cb as *const CreateSessionDescriptionObserverCallbacks;
    ((*call_backs).onSuccess)(
        csd_observer as *mut CreateSessionDescriptionObserver,
        &mut FAKE_SDP,
    );

    &FAKE_CSD_OBSERVER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_toSdp(rffi: *const RffiSessionDescription) -> *const c_char {
    info!("Rust_toSdp(): ");
    match CString::new(*rffi) {
        Ok(cstr) => strdup(cstr.as_ptr()),
        Err(_) => ptr::null(),
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_offerFromSdp(_sdp: *const c_char) -> *mut RffiSessionDescription {
    info!("Rust_offerFromSdp(): ");
    &mut FAKE_SDP_ANSWER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_answerFromSdp(_sdp: *const c_char) -> *mut RffiSessionDescription {
    info!("Rust_answerFromSdp(): ");
    &mut FAKE_SDP_OFFER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_disableDtlsAndSetSrtpKey(
    _session_description: *mut RffiSessionDescription,
    _crypto_suite: SrtpCryptoSuite,
    _key_ptr: *const u8,
    _key_len: size_t,
    _salt_ptr: *const u8,
    _salt_len: size_t,
) -> bool {
    info!("Rust_disableDtlsAndSetSrtpKey(): ");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_sessionDescriptionToV4(
    _session_description: *const RffiSessionDescription,
) -> *mut RffiConnectionParametersV4 {
    info!("Rust_sessionDescriptionToV4(): ");
    Box::leak(Box::new(RffiConnectionParametersV4 {
        ice_ufrag: std::ptr::null(),
        ice_pwd: std::ptr::null(),
        receive_video_codecs: std::ptr::null(),
        receive_video_codecs_size: 0,
    }))
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_releaseV4(_v4: *mut RffiConnectionParametersV4) {
    info!("Rust_releaseV4(): ");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_sessionDescriptionFromV4(
    offer: bool,
    _v4: *const RffiConnectionParametersV4,
) -> *mut RffiSessionDescription {
    info!("Rust_sessionDescriptionFromV4(): ");
    if offer {
        &mut FAKE_SDP_OFFER
    } else {
        &mut FAKE_SDP_ANSWER
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_localDescriptionForGroupCall(
    _ice_ufrag: *const c_char,
    _ice_pwd: *const c_char,
    _dtls_fingerprint_sha256: *const [u8; 32],
    _demux_id: u32,
) -> *mut RffiSessionDescription {
    info!("Rust_localDescriptionForGroupCall(): ");
    &mut FAKE_SDP_OFFER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_remoteDescriptionForGroupCall(
    _ice_ufrag: *const c_char,
    _ice_pwd: *const c_char,
    _dtls_fingerprint_sha256: *const [u8; 32],
    _demux_ids_data: *const u32,
    _demux_ids_len: size_t,
) -> *mut RffiSessionDescription {
    info!("Rust_remoteDescriptionForGroupCall(): ");
    &mut FAKE_SDP_ANSWER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_releaseSessionDescription(_sdi: *mut RffiSessionDescription) {
    info!("Rust_releaseSessionDescription(): ");
}
