//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation PeerConnectionObserver

use crate::webrtc;

/// Simulation type for PeerConnectionObserver.
pub type RffiPeerConnectionObserver = u32;

static FAKE_OBSERVER: u32 = 7;

impl webrtc::ptr::Delete for u32 {
    fn delete(_owned: webrtc::ptr::Owned<Self>) {}
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createPeerConnectionObserver(
    _cc_ptr: webrtc::ptr::Borrowed<std::ffi::c_void>,
    _pc_observer_cb: webrtc::ptr::Borrowed<std::ffi::c_void>,
    _enable_frame_encryption: bool,
    _enable_video_frame_event: bool,
    _enable_video_frame_content: bool,
) -> webrtc::ptr::Owned<RffiPeerConnectionObserver> {
    info!("Rust_createPeerConnectionObserver():");
    webrtc::ptr::Owned::from_ptr(&FAKE_OBSERVER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_deletePeerConnectionObserver(
    _observer: webrtc::ptr::Owned<RffiPeerConnectionObserver>,
) {
    info!("Rust_deletePeerConnectionObserver():");
}
