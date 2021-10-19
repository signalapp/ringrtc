//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Stats Observer

use crate::webrtc;

/// Incomplete type for C++ webrtc::rffi::StatsObserverRffi
#[repr(C)]
pub struct RffiStatsObserver {
    _private: [u8; 0],
}

// See "class StatsObserver : public rtc::RefCountInterface"
// in webrtc/api/peer_connection_interface.h.
impl webrtc::RefCounted for RffiStatsObserver {}

extern "C" {
    // The passed-in values observer must live as long as the returned value,
    // which in turn must live as long as the call to PeerConnection::getStats.
    pub fn Rust_createStatsObserver(
        stats_observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
        stats_observer_cbs: webrtc::ptr::Borrowed<std::ffi::c_void>,
    ) -> webrtc::ptr::OwnedRc<RffiStatsObserver>;
}
