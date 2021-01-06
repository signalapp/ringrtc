//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation Create/Set SessionDescription

use std::ffi::c_void;
use std::ptr;

use crate::core::util::RustObject;
use crate::webrtc::stats_observer::{MediaStatistics, StatsObserver, StatsObserverCallbacks};

/// Simulation type for webrtc::rffi::StatsObserverRffi
pub type RffiStatsObserver = u32;

static FAKE_STATS_OBSERVER: u32 = 21;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createStatsObserver(
    stats_observer: RustObject,
    stats_observer_cbs: *const c_void,
) -> *const RffiStatsObserver {
    info!("Rust_createStatsObserver():");

    let dummy = MediaStatistics {
        timestamp_us:                   0,
        audio_sender_statistics_size:   0,
        audio_sender_statistics:        ptr::null(),
        video_sender_statistics_size:   0,
        video_sender_statistics:        ptr::null(),
        audio_receiver_statistics_size: 0,
        audio_receiver_statistics:      ptr::null(),
        video_receiver_statistics_size: 0,
        video_receiver_statistics:      ptr::null(),
    };

    // Hit on the onComplete() callback
    let callbacks = stats_observer_cbs as *const StatsObserverCallbacks;
    ((*callbacks).onStatsComplete)(stats_observer as *mut StatsObserver, &dummy);

    &FAKE_STATS_OBSERVER
}
