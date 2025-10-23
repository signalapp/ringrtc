//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation Create/Set SessionDescription

use std::ptr;

use crate::webrtc::{
    self,
    stats_observer::{
        ConnectionStatistics, MediaStatistics, StatsObserver, StatsObserverCallbacks,
    },
};

/// Simulation type for webrtc::rffi::StatsObserverRffi
pub type RffiStatsObserver = u32;

static FAKE_STATS_OBSERVER: u32 = 21;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createStatsObserver(
    stats_observer: webrtc::ptr::Borrowed<std::ffi::c_void>,
    callbacks: webrtc::ptr::Borrowed<std::ffi::c_void>,
) -> webrtc::ptr::OwnedRc<RffiStatsObserver> {
    info!("Rust_createStatsObserver():");

    let dummy = MediaStatistics {
        timestamp_us: 0,
        audio_sender_statistics_size: 0,
        audio_sender_statistics: ptr::null(),
        video_sender_statistics_size: 0,
        video_sender_statistics: ptr::null(),
        audio_receiver_statistics_size: 0,
        audio_receiver_statistics: ptr::null(),
        video_receiver_statistics_size: 0,
        video_receiver_statistics: ptr::null(),
        nominated_connection_statistics: ConnectionStatistics::default(),
        connection_statistics: ptr::null(),
        connection_statistics_size: 0,
    };

    // Hit on the onComplete() callback
    let callbacks = callbacks.as_ptr() as *const StatsObserverCallbacks;
    let report_json = std::ffi::CString::new("{}").expect("CString::new failed");
    unsafe {
        ((*callbacks).onStatsComplete)(
            webrtc::ptr::Borrowed::from_ptr(stats_observer.as_ptr() as *mut StatsObserver),
            webrtc::ptr::Borrowed::from_ptr(&dummy),
            webrtc::ptr::Borrowed::from_ptr(report_json.as_ptr()),
        );

        webrtc::ptr::OwnedRc::from_ptr(&FAKE_STATS_OBSERVER)
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setCollectRawStatsReport(
    _stats_observer: webrtc::ptr::BorrowedRc<RffiStatsObserver>,
    collect_raw_stats_report: bool,
) {
    info!(
        "Rust_setCollectRawStatsReport: {}",
        collect_raw_stats_report
    );
}
