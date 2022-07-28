//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Peer Connection Observer Interface.

use crate::webrtc;

/// Incomplete type for C++ PeerConnectionObserver.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RffiPeerConnectionObserver {
    _private: [u8; 0],
}

impl webrtc::ptr::Delete for RffiPeerConnectionObserver {
    fn delete(owned: webrtc::ptr::Owned<Self>) {
        unsafe { Rust_deletePeerConnectionObserver(owned) };
    }
}

extern "C" {
    // The passed-in observer must live as long as the returned value,
    // which in turn must live as long as the PeerConnections it is passed to.
    pub fn Rust_createPeerConnectionObserver(
        pc_observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
        pc_observer_cb: webrtc::ptr::Borrowed<std::ffi::c_void>,
        enable_frame_encryption: bool,
        enable_video_frame_event: bool,
        enable_video_frame_content: bool,
    ) -> webrtc::ptr::Owned<RffiPeerConnectionObserver>;

    pub fn Rust_deletePeerConnectionObserver(
        observer: webrtc::ptr::Owned<RffiPeerConnectionObserver>,
    );
}
