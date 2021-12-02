//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation Create/Set SessionDescription

use std::ffi::CString;
use std::os::raw::c_char;

use libc::{size_t, strdup};

use crate::webrtc;
use crate::webrtc::sdp_observer::{
    CreateSessionDescriptionObserver, CreateSessionDescriptionObserverCallbacks,
    RffiConnectionParametersV4, RffiSrtpKey, SetSessionDescriptionObserver,
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
    ssd_observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
    callbacks: webrtc::ptr::Borrowed<std::ffi::c_void>,
) -> webrtc::ptr::OwnedRc<RffiSetSessionDescriptionObserver> {
    info!("Rust_createSetSessionDescriptionObserver():");

    // Hit the onSuccess() callback
    let callbacks = callbacks.as_ptr() as *const SetSessionDescriptionObserverCallbacks;
    ((*callbacks).onSuccess)(webrtc::ptr::Borrowed::from_ptr(
        ssd_observer.as_ptr() as *mut SetSessionDescriptionObserver
    ));

    webrtc::ptr::OwnedRc::from_ptr(&FAKE_SSD_OBSERVER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createCreateSessionDescriptionObserver(
    csd_observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
    callbacks: webrtc::ptr::Borrowed<std::ffi::c_void>,
) -> webrtc::ptr::OwnedRc<RffiCreateSessionDescriptionObserver> {
    info!("Rust_createCreateSessionDescriptionObserver():");

    // Hit the onSuccess() callback
    let callbacks = callbacks.as_ptr() as *const CreateSessionDescriptionObserverCallbacks;
    ((*callbacks).onSuccess)(
        webrtc::ptr::Borrowed::from_ptr(
            csd_observer.as_ptr() as *mut CreateSessionDescriptionObserver
        ),
        webrtc::ptr::Owned::from_ptr(&FAKE_SDP),
    );

    webrtc::ptr::OwnedRc::from_ptr(&FAKE_CSD_OBSERVER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_toSdp(
    rffi: webrtc::ptr::Borrowed<RffiSessionDescription>,
) -> webrtc::ptr::Owned<c_char> {
    info!("Rust_toSdp(): ");
    match CString::new(*rffi.as_ptr()) {
        Ok(cstr) => webrtc::ptr::Owned::from_ptr(strdup(cstr.as_ptr())),
        Err(_) => webrtc::ptr::Owned::null(),
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_offerFromSdp(
    _sdp: webrtc::ptr::Borrowed<c_char>,
) -> webrtc::ptr::Owned<RffiSessionDescription> {
    info!("Rust_offerFromSdp(): ");
    webrtc::ptr::Owned::from_ptr(&FAKE_SDP_ANSWER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_answerFromSdp(
    _sdp: webrtc::ptr::Borrowed<c_char>,
) -> webrtc::ptr::Owned<RffiSessionDescription> {
    info!("Rust_answerFromSdp(): ");
    webrtc::ptr::Owned::from_ptr(&FAKE_SDP_OFFER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_disableDtlsAndSetSrtpKey(
    _session_description: webrtc::ptr::Borrowed<RffiSessionDescription>,
    _crypto_suite: SrtpCryptoSuite,
    _key_data: webrtc::ptr::Borrowed<u8>,
    _key_len: size_t,
    _salt_data: webrtc::ptr::Borrowed<u8>,
    _salt_len: size_t,
) -> bool {
    info!("Rust_disableDtlsAndSetSrtpKey(): ");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_sessionDescriptionToV4(
    _session_description: webrtc::ptr::Borrowed<RffiSessionDescription>,
) -> webrtc::ptr::Owned<RffiConnectionParametersV4> {
    info!("Rust_sessionDescriptionToV4(): ");
    webrtc::ptr::Owned::from_ptr(Box::leak(Box::new(RffiConnectionParametersV4 {
        ice_ufrag: webrtc::ptr::Borrowed::null(),
        ice_pwd: webrtc::ptr::Borrowed::null(),
        receive_video_codecs: webrtc::ptr::Borrowed::null(),
        receive_video_codecs_size: 0,
    })))
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_deleteV4(_v4: webrtc::ptr::Owned<RffiConnectionParametersV4>) {
    info!("Rust_deleteV4(): ");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_sessionDescriptionFromV4(
    offer: bool,
    _v4: webrtc::ptr::Borrowed<RffiConnectionParametersV4>,
) -> webrtc::ptr::Owned<RffiSessionDescription> {
    info!("Rust_sessionDescriptionFromV4(): ");
    if offer {
        webrtc::ptr::Owned::from_ptr(&FAKE_SDP_OFFER)
    } else {
        webrtc::ptr::Owned::from_ptr(&FAKE_SDP_ANSWER)
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_localDescriptionForGroupCall(
    _ice_ufrag: webrtc::ptr::Borrowed<c_char>,
    _ice_pwd: webrtc::ptr::Borrowed<c_char>,
    _client_srtp_key: RffiSrtpKey,
    _demux_id: u32,
) -> webrtc::ptr::Owned<RffiSessionDescription> {
    info!("Rust_localDescriptionForGroupCall(): ");
    webrtc::ptr::Owned::from_ptr(&FAKE_SDP_OFFER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_remoteDescriptionForGroupCall(
    _ice_ufrag: webrtc::ptr::Borrowed<c_char>,
    _ice_pwd: webrtc::ptr::Borrowed<c_char>,
    _server_srtp_key: RffiSrtpKey,
    _demux_ids_data: webrtc::ptr::Borrowed<u32>,
    _demux_ids_len: size_t,
) -> webrtc::ptr::Owned<RffiSessionDescription> {
    info!("Rust_remoteDescriptionForGroupCall(): ");
    webrtc::ptr::Owned::from_ptr(&FAKE_SDP_ANSWER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_deleteSessionDescription(_sdi: webrtc::ptr::Owned<RffiSessionDescription>) {
    info!("Rust_deleteSessionDescription(): ");
}
