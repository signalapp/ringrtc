//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation Create / Set Session Description Interface.

use std::ffi::c_void;

use crate::core::util::RustObject;
use crate::webrtc::stats_observer::{StatsObserver, StatsObserverCallbacks, StatsObserverValues};

/// Simulation type for webrtc::rffi::StatsObserverRffi
pub type RffiStatsObserver = u32;

static FAKE_STATS_OBSERVER: u32 = 21;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createStatsObserver(
    stats_observer: RustObject,
    stats_observer_cbs: *const c_void,
) -> *const RffiStatsObserver {
    info!("Rust_createStatsObserver():");

    let dummy: StatsObserverValues = StatsObserverValues {
        audio_packets_sent:             1,
        audio_packets_sent_lost:        1,
        audio_rtt:                      1,
        audio_packets_received:         1,
        audio_packets_received_lost:    1,
        audio_jitter_received:          1,
        audio_expand_rate:              1.0,
        audio_accelerate_rate:          1.0,
        audio_preemptive_rate:          1.0,
        audio_speech_expand_rate:       1.0,
        audio_preferred_buffer_size_ms: 1,
    };

    // Hit on the onComplete() callback
    let callbacks = stats_observer_cbs as *const StatsObserverCallbacks;
    ((*callbacks).onStatsComplete)(stats_observer as *mut StatsObserver, &dummy);

    &FAKE_STATS_OBSERVER
}
