//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Create / Set Session Description Interface.

use std::os::raw::c_char;

use libc::size_t;

use crate::webrtc::{
    self,
    sdp_observer::{RffiConnectionParametersV4, RffiSrtpKey, SrtpCryptoSuite},
};

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

// See "class CreateSessionDescriptionObserver: public rtc::RefCountInterface
// in webrtc/api/jsep.h
impl webrtc::RefCounted for RffiCreateSessionDescriptionObserver {}

/// Incomplete type for C++ CreateSessionDescriptionObserverRffi
#[repr(C)]
pub struct RffiSetSessionDescriptionObserver {
    _private: [u8; 0],
}

// See "class SetSessionDescriptionObserver: public rtc::RefCountInterface
// in webrtc/api/jsep.h
impl webrtc::RefCounted for RffiSetSessionDescriptionObserver {}

unsafe extern "C" {
    // The passed-in observer must live as long as the returned value,
    // which in turn must live as long as the call to PeerConnection::SetLocalDescription/SetRemoteDescription.
    pub fn Rust_createSetSessionDescriptionObserver(
        ssd_observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
        ssd_observer_cb: webrtc::ptr::Borrowed<std::ffi::c_void>,
    ) -> webrtc::ptr::OwnedRc<RffiSetSessionDescriptionObserver>;

    // The passed-in observer must live as long as the returned value,
    // which in turn must live as long as the call to PeerConnection::CreateOffer/CreateAnswer.
    pub fn Rust_createCreateSessionDescriptionObserver(
        csd_observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
        csd_observer_cb: webrtc::ptr::Borrowed<std::ffi::c_void>,
    ) -> webrtc::ptr::OwnedRc<RffiCreateSessionDescriptionObserver>;

    pub fn Rust_toSdp(
        desc: webrtc::ptr::Borrowed<RffiSessionDescription>,
    ) -> webrtc::ptr::Owned<c_char>;

    pub fn Rust_disableDtlsAndSetSrtpKey(
        session_description: webrtc::ptr::Borrowed<RffiSessionDescription>,
        crypto_suite: SrtpCryptoSuite,
        key_data: webrtc::ptr::Borrowed<u8>,
        key_len: size_t,
        salt_data: webrtc::ptr::Borrowed<u8>,
        salt_len: size_t,
    ) -> bool;

    // Only for use when we detect that the remote does not support asymmetric codecs.
    // TODO: Remove this when the 90-day for asymmetric codecs expires.
    pub fn Rust_sessionDescriptionToV4Legacy(
        v4: webrtc::ptr::Borrowed<RffiSessionDescription>,
        enable_vp9: bool,
    ) -> webrtc::ptr::Owned<RffiConnectionParametersV4>;

    pub fn Rust_sessionDescriptionToV4(
        v4: webrtc::ptr::Borrowed<RffiSessionDescription>,
        enable_vp9_encode: bool,
        enable_vp9_decode: bool,
    ) -> webrtc::ptr::Owned<RffiConnectionParametersV4>;

    pub fn Rust_deleteV4(session_description: webrtc::ptr::Owned<RffiConnectionParametersV4>);

    // Only for use when we detect that the remote does not support asymmetric codecs.
    // TODO: Remove this when the 90-day for asymmetric codecs expires.
    pub fn Rust_sessionDescriptionFromV4Legacy(
        offer: bool,
        v4: webrtc::ptr::Borrowed<RffiConnectionParametersV4>,
        enable_tcc_audio: bool,
        enable_vp9: bool,
    ) -> webrtc::ptr::Owned<RffiSessionDescription>;

    pub fn Rust_sessionDescriptionFromV4(
        offer: bool,
        v4: webrtc::ptr::Borrowed<RffiConnectionParametersV4>,
        enable_tcc_audio: bool,
        enable_vp9_encode: bool,
        enable_vp9_decode: bool,
        v4_is_local: bool,
    ) -> webrtc::ptr::Owned<RffiSessionDescription>;

    pub fn Rust_localDescriptionForGroupCall(
        ice_ufrag: webrtc::ptr::Borrowed<c_char>,
        ice_pwd: webrtc::ptr::Borrowed<c_char>,
        client_srtp_key: RffiSrtpKey,
        local_demux_id: u32,
        remote_demux_ids_data: webrtc::ptr::Borrowed<u32>,
        remote_demux_ids_len: size_t,
    ) -> webrtc::ptr::Owned<RffiSessionDescription>;

    pub fn Rust_remoteDescriptionForGroupCall(
        ice_ufrag: webrtc::ptr::Borrowed<c_char>,
        ice_pwd: webrtc::ptr::Borrowed<c_char>,
        server_srtp_key: RffiSrtpKey,
        local_demux_id: u32,
        remote_demux_ids_data: webrtc::ptr::Borrowed<u32>,
        remote_demux_ids_len: size_t,
    ) -> webrtc::ptr::Owned<RffiSessionDescription>;

    pub fn Rust_deleteSessionDescription(sdi: webrtc::ptr::Owned<RffiSessionDescription>);
}
