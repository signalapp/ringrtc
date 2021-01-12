//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Create / Set Session Description Interface.

use crate::core::util::RustObject;
use libc::size_t;
use std::ffi::c_void;
use std::os::raw::c_char;

use crate::webrtc::sdp_observer::RffiConnectionParametersV4;

/// Incomplete type for SessionDescription, used by
/// CreateSessionDescriptionObserver callbacks.
#[repr(C)]
pub struct RffiSessionDescription {
    _private: [u8; 0],
}

/// Incomplete type for C++ webrtc::rffi::CreateSessionDescriptionObserverRffi
#[repr(C)]
pub struct RffiCreateSessionDescriptionObserver {
    _private: [u8; 0],
}

/// Incomplete type for C++ CreateSessionDescriptionObserverRffi
#[repr(C)]
pub struct RffiSetSessionDescriptionObserver {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_createSetSessionDescriptionObserver(
        ssd_observer: RustObject,
        ssd_observer_cb: *const c_void,
    ) -> *const RffiSetSessionDescriptionObserver;

    pub fn Rust_createCreateSessionDescriptionObserver(
        csd_observer: RustObject,
        csd_observer_cb: *const c_void,
    ) -> *const RffiCreateSessionDescriptionObserver;

    pub fn Rust_toSdp(offer: *const RffiSessionDescription) -> *const c_char;

    pub fn Rust_answerFromSdp(sdp: *const c_char) -> *mut RffiSessionDescription;

    pub fn Rust_offerFromSdp(sdp: *const c_char) -> *mut RffiSessionDescription;

    pub fn Rust_disableDtlsAndSetSrtpKey(
        session_description: *mut RffiSessionDescription,
        crypto_suite: crate::webrtc::sdp_observer::SrtpCryptoSuite,
        key_ptr: *const u8,
        key_len: size_t,
        salt_ptr: *const u8,
        salt_len: size_t,
    ) -> bool;

    pub fn Rust_sessionDescriptionToV4(
        session_description: *const RffiSessionDescription,
    ) -> *mut RffiConnectionParametersV4;

    pub fn Rust_releaseV4(session_description: *mut RffiConnectionParametersV4);

    pub fn Rust_sessionDescriptionFromV4(
        offer: bool,
        v4: *const RffiConnectionParametersV4,
    ) -> *mut RffiSessionDescription;

    pub fn Rust_localDescriptionForGroupCall(
        ice_ufrag: *const c_char,
        ice_pwd: *const c_char,
        _dtls_fingerprint_sha256: *const [u8; 32],
        demux_id: u32,
    ) -> *mut RffiSessionDescription;

    pub fn Rust_remoteDescriptionForGroupCall(
        ice_ufrag: *const c_char,
        ice_pwd: *const c_char,
        _dtls_fingerprint_sha256: *const [u8; 32],
        demux_ids_data: *const u32,
        demux_ids_len: size_t,
    ) -> *mut RffiSessionDescription;

    pub fn Rust_releaseSessionDescription(sdi: *mut RffiSessionDescription);
}
