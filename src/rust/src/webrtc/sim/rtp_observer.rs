//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation RtpObserver

use crate::webrtc;

/// Simulation type for RtpObserver.
pub type RffiRtpObserver = u32;

static FAKE_RTP_OBSERVER: u32 = 17;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createRtpObserver(
    _observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
    _callbacks: webrtc::ptr::Borrowed<std::ffi::c_void>,
) -> webrtc::ptr::Owned<RffiRtpObserver> {
    info!("Rust_createRtpObserver():");
    unsafe { webrtc::ptr::Owned::from_ptr(&FAKE_RTP_OBSERVER) }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_deleteRtpObserver(_observer: webrtc::ptr::Owned<RffiRtpObserver>) {
    info!("Rust_deleteRtpObserver():");
}
