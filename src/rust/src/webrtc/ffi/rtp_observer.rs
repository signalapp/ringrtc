//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI RTP Observer Interface.

use crate::webrtc;

/// Incomplete type for C++ RtpObserverRffi.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RffiRtpObserver {
    _private: [u8; 0],
}

impl webrtc::ptr::Delete for RffiRtpObserver {
    fn delete(owned: webrtc::ptr::Owned<Self>) {
        unsafe { Rust_deleteRtpObserver(owned) };
    }
}

unsafe extern "C" {
    // The passed-in observer must live as long as the returned value,
    // which in turn must live as long as the PeerConnection it is passed to.
    pub fn Rust_createRtpObserver(
        observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
        callbacks: webrtc::ptr::Borrowed<std::ffi::c_void>,
    ) -> webrtc::ptr::Owned<RffiRtpObserver>;

    pub fn Rust_deleteRtpObserver(observer: webrtc::ptr::Owned<RffiRtpObserver>);
}
