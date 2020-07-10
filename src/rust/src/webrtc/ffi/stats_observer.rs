//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC FFI Stats Observer Interface.

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
