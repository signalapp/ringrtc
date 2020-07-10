//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Create Session Description Interface.

use std::ffi::c_void;
use std::ptr;

use crate::core::util::{ptr_as_ref, RustObject};

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count::release_ref;
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::stats_observer as stats;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::stats_observer::RffiStatsObserver;

#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count::release_ref;
#[cfg(feature = "sim")]
use crate::webrtc::sim::stats_observer as stats;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::stats_observer::RffiStatsObserver;

/// Collector object for obtaining statistics.
#[derive(Debug)]
pub struct StatsObserver {
    /// Pointer to C++ webrtc::rffi::StatsObserverRffi object.
    rffi_stats_observer: *const RffiStatsObserver,
}

unsafe impl Send for StatsObserver {}

impl Drop for StatsObserver {
    fn drop(&mut self) {
        if !self.rffi_stats_observer.is_null() {
            release_ref(self.rffi_stats_observer as *const c_void);
        }
    }
}

impl StatsObserver {
    /// Create a new StatsObserver.
    fn new() -> Self {
        info!(
            "ringrtc_stats!,\
                audio_packets_sent,\
                audio_packets_sent_lost,\
                audio_rtt,\
                audio_packets_received,\
                audio_packets_received_lost,\
                audio_jitter_received,\
                audio_expand_rate,\
                audio_accelerate_rate,\
                audio_preemptive_rate,\
                audio_speech_expand_rate,\
                audio_preferred_buffer_size_ms"
        );

        Self {
            rffi_stats_observer: ptr::null(),
        }
    }

    /// Called back when statistics are received via the stats observer
    /// callback.
    fn on_stats_complete(&self, values: &StatsObserverValues) {
        info!(
            "ringrtc_stats!,{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{}",
            values.audio_packets_sent,
            values.audio_packets_sent_lost,
            values.audio_rtt,
            values.audio_packets_received,
            values.audio_packets_received_lost,
            values.audio_jitter_received,
            values.audio_expand_rate,
            values.audio_accelerate_rate,
            values.audio_preemptive_rate,
            values.audio_speech_expand_rate,
            values.audio_preferred_buffer_size_ms
        );
    }

    /// Set the RFFI observer object.
    pub fn set_rffi_stats_observer(&mut self, rffi_stats_observer: *const RffiStatsObserver) {
        self.rffi_stats_observer = rffi_stats_observer
    }

    /// Return the RFFI observer object.
    pub fn rffi_stats_observer(&self) -> *const RffiStatsObserver {
        self.rffi_stats_observer
    }
}

/// StatsObserverValues struct that holds all the statistics.
#[repr(C)]
#[derive(Debug)]
pub struct StatsObserverValues {
    pub audio_packets_sent:             i32,
    pub audio_packets_sent_lost:        i32,
    pub audio_rtt:                      i64,
    pub audio_packets_received:         i32,
    pub audio_packets_received_lost:    i32,
    pub audio_jitter_received:          i32,
    pub audio_expand_rate:              f32,
    pub audio_accelerate_rate:          f32,
    pub audio_preemptive_rate:          f32,
    pub audio_speech_expand_rate:       f32,
    pub audio_preferred_buffer_size_ms: i32,
}

/// StatsObserver OnStatsComplete() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn stats_observer_OnStatsComplete(
    stats_observer: *mut StatsObserver,
    values: &StatsObserverValues,
) {
    match unsafe { ptr_as_ref(stats_observer) } {
        Ok(v) => v.on_stats_complete(values),
        Err(e) => error!("stats_observer_OnStatsComplete(): {}", e),
    };
}

/// StatsObserver callback function pointers.
#[repr(C)]
#[allow(non_snake_case)]
pub struct StatsObserverCallbacks {
    pub onStatsComplete:
        extern "C" fn(stats_observer: *mut StatsObserver, values: &StatsObserverValues),
}

const STATS_OBSERVER_CBS: StatsObserverCallbacks = StatsObserverCallbacks {
    onStatsComplete: stats_observer_OnStatsComplete,
};
const STATS_OBSERVER_CBS_PTR: *const StatsObserverCallbacks = &STATS_OBSERVER_CBS;

/// Create a new Rust StatsObserver object.
///
/// Creates a new WebRTC C++ StatsObserver object,
/// registering the collector callbacks to this module, and wraps the
/// result in a Rust StatsObserver object.
pub fn create_stats_observer() -> Box<StatsObserver> {
    let stats_observer = Box::new(StatsObserver::new());
    let stats_observer_ptr = Box::into_raw(stats_observer);
    let rffi_stats_observer = unsafe {
        stats::Rust_createStatsObserver(
            stats_observer_ptr as RustObject,
            STATS_OBSERVER_CBS_PTR as *const c_void,
        )
    };
    let mut stats_observer = unsafe { Box::from_raw(stats_observer_ptr) };

    stats_observer.set_rffi_stats_observer(rffi_stats_observer);
    stats_observer
}
