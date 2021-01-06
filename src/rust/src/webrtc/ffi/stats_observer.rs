//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Stats Observer

use crate::core::util::RustObject;
use std::ffi::c_void;

/// Incomplete type for C++ webrtc::rffi::StatsObserverRffi
#[repr(C)]
pub struct RffiStatsObserver {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_createStatsObserver(
        stats_observer: RustObject,
        stats_observer_cbs: *const c_void,
    ) -> *const RffiStatsObserver;
}
