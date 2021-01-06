//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Create Session Description

use std::ffi::c_void;
use std::{ptr, slice};

use crate::core::util::{ptr_as_mut, RustObject};

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
                timestamp_us,\
                audio,\
                send,\
                ssrc,\
                packets_sent,\
                bytes_sent,\
                remote_packets_lost,\
                remote_jitter,\
                remote_round_trip_time,\
                audio_level,\
                total_audio_energy"
        );
        info!(
            "ringrtc_stats!,\
                timestamp_us,\
                video,\
                send,\
                ssrc,\
                packets_sent,\
                bytes_sent,\
                frames_encoded,\
                key_frames_encoded,\
                total_encode_time,\
                frame_width,\
                frame_height,\
                retransmitted_packets_sent,\
                retransmitted_bytes_sent,\
                total_packet_send_delay,\
                nack_count,\
                fir_count,\
                pli_count,\
                quality_limitation_reason,\
                quality_limitation_resolution_changes,\
                remote_packets_lost,\
                remote_jitter,\
                remote_round_trip_time"
        );
        info!(
            "ringrtc_stats!,\
                timestamp_us,\
                audio,\
                recv,\
                ssrc,\
                packets_received,\
                packets_lost,\
                bytes_received,\
                jitter,\
                frames_decoded,\
                total_decode_time,\
                audio_level,\
                total_audio_energy"
        );
        info!(
            "ringrtc_stats!,\
                timestamp_us,\
                video,\
                recv,\
                ssrc,\
                packets_received,\
                packets_lost,\
                packets_repaired,\
                bytes_received,\
                frames_decoded,\
                key_frames_decoded,\
                total_decode_time,\
                frame_width,\
                frame_height"
        );

        Self {
            rffi_stats_observer: ptr::null(),
        }
    }

    /// Invoked when statistics are received via the stats observer callback.
    fn on_stats_complete(&mut self, media_statistics: &MediaStatistics) {
        if media_statistics.audio_sender_statistics_size > 0 {
            let audio_senders = unsafe {
                if media_statistics.audio_sender_statistics.is_null() {
                    &[]
                } else {
                    slice::from_raw_parts(
                        media_statistics.audio_sender_statistics,
                        media_statistics.audio_sender_statistics_size as usize,
                    )
                }
            };
            for audio_sender in audio_senders.iter() {
                info!(
                    "ringrtc_stats!,{},audio,send,{},{},{},{},{:.5},{:.3},{:.5},{:.3}",
                    media_statistics.timestamp_us,
                    audio_sender.ssrc,
                    audio_sender.packets_sent,
                    audio_sender.bytes_sent,
                    audio_sender.remote_packets_lost,
                    audio_sender.remote_jitter,
                    audio_sender.remote_round_trip_time,
                    audio_sender.audio_level,
                    audio_sender.total_audio_energy,
                );
            }
        }

        if media_statistics.video_sender_statistics_size > 0 {
            let video_senders = unsafe {
                if media_statistics.video_sender_statistics.is_null() {
                    &[]
                } else {
                    slice::from_raw_parts(
                        media_statistics.video_sender_statistics,
                        media_statistics.video_sender_statistics_size as usize,
                    )
                }
            };
            for video_sender in video_senders.iter() {
                info!("ringrtc_stats!,{},video,send,{},{},{},{},{},{:.3},{},{},{},{},{:.3},{},{},{},{},{},{},{:.5},{:.3}",
                      media_statistics.timestamp_us,
                      video_sender.ssrc,
                      video_sender.packets_sent,
                      video_sender.bytes_sent,
                      video_sender.frames_encoded,
                      video_sender.key_frames_encoded,
                      video_sender.total_encode_time,
                      video_sender.frame_width,
                      video_sender.frame_height,
                      video_sender.retransmitted_packets_sent,
                      video_sender.retransmitted_bytes_sent,
                      video_sender.total_packet_send_delay,
                      video_sender.nack_count,
                      video_sender.fir_count,
                      video_sender.pli_count,
                      video_sender.quality_limitation_reason,
                      video_sender.quality_limitation_resolution_changes,
                      video_sender.remote_packets_lost,
                      video_sender.remote_jitter,
                      video_sender.remote_round_trip_time,
                );
            }
        }

        if media_statistics.audio_receiver_statistics_size > 0 {
            let audio_receivers = unsafe {
                if media_statistics.audio_receiver_statistics.is_null() {
                    &[]
                } else {
                    slice::from_raw_parts(
                        media_statistics.audio_receiver_statistics,
                        media_statistics.audio_receiver_statistics_size as usize,
                    )
                }
            };
            for audio_receiver in audio_receivers.iter() {
                info!(
                    "ringrtc_stats!,{},audio,recv,{},{},{},{},{:.5},{},{:.3},{:.5},{:.3}",
                    media_statistics.timestamp_us,
                    audio_receiver.ssrc,
                    audio_receiver.packets_received,
                    audio_receiver.packets_lost,
                    audio_receiver.bytes_received,
                    audio_receiver.jitter,
                    audio_receiver.frames_decoded,
                    audio_receiver.total_decode_time,
                    audio_receiver.audio_level,
                    audio_receiver.total_audio_energy,
                );
            }
        }

        if media_statistics.video_receiver_statistics_size > 0 {
            let video_receivers = unsafe {
                if media_statistics.video_receiver_statistics.is_null() {
                    &[]
                } else {
                    slice::from_raw_parts(
                        media_statistics.video_receiver_statistics,
                        media_statistics.video_receiver_statistics_size as usize,
                    )
                }
            };
            for video_receive in video_receivers.iter() {
                info!(
                    "ringrtc_stats!,{},video,recv,{},{},{},{},{},{},{},{:.3},{},{}",
                    media_statistics.timestamp_us,
                    video_receive.ssrc,
                    video_receive.packets_received,
                    video_receive.packets_lost,
                    video_receive.packets_repaired,
                    video_receive.bytes_received,
                    video_receive.frames_decoded,
                    video_receive.key_frames_decoded,
                    video_receive.total_decode_time,
                    video_receive.frame_width,
                    video_receive.frame_height,
                );
            }
        }
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

#[repr(C)]
#[derive(Debug)]
pub struct AudioSenderStatistics {
    pub ssrc:                   u32,
    pub packets_sent:           u32,
    pub bytes_sent:             u64,
    pub remote_packets_lost:    i32,
    pub remote_jitter:          f64,
    pub remote_round_trip_time: f64,
    pub audio_level:            f64,
    pub total_audio_energy:     f64,
}

#[repr(C)]
#[derive(Debug)]
pub struct VideoSenderStatistics {
    pub ssrc: u32,
    pub packets_sent: u32,
    pub bytes_sent: u64,
    pub frames_encoded: u32,
    pub key_frames_encoded: u32,
    pub total_encode_time: f64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub retransmitted_packets_sent: u64,
    pub retransmitted_bytes_sent: u64,
    pub total_packet_send_delay: f64,
    pub nack_count: u32,
    pub fir_count: u32,
    pub pli_count: u32,
    pub quality_limitation_reason: u32,
    pub quality_limitation_resolution_changes: u32,
    pub remote_packets_lost: i32,
    pub remote_jitter: f64,
    pub remote_round_trip_time: f64,
}

#[repr(C)]
#[derive(Debug)]
pub struct AudioReceiverStatistics {
    pub ssrc:               u32,
    pub packets_received:   u32,
    pub packets_lost:       i32,
    pub bytes_received:     u64,
    pub jitter:             f64,
    pub frames_decoded:     u32,
    pub total_decode_time:  f64,
    pub audio_level:        f64,
    pub total_audio_energy: f64,
}

#[repr(C)]
#[derive(Debug)]
pub struct VideoReceiverStatistics {
    pub ssrc:               u32,
    pub packets_received:   u32,
    pub packets_lost:       i32,
    pub packets_repaired:   u32,
    pub bytes_received:     u64,
    pub frames_decoded:     u32,
    pub key_frames_decoded: u32,
    pub total_decode_time:  f64,
    pub frame_width:        u32,
    pub frame_height:       u32,
}

/// MediaStatistics struct that holds all the statistics.
#[repr(C)]
#[derive(Debug)]
pub struct MediaStatistics {
    pub timestamp_us:                   i64,
    pub audio_sender_statistics_size:   u32,
    pub audio_sender_statistics:        *const AudioSenderStatistics,
    pub video_sender_statistics_size:   u32,
    pub video_sender_statistics:        *const VideoSenderStatistics,
    pub audio_receiver_statistics_size: u32,
    pub audio_receiver_statistics:      *const AudioReceiverStatistics,
    pub video_receiver_statistics_size: u32,
    pub video_receiver_statistics:      *const VideoReceiverStatistics,
}

/// StatsObserver OnStatsComplete() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn stats_observer_OnStatsComplete(
    stats_observer: *mut StatsObserver,
    values: &MediaStatistics,
) {
    match unsafe { ptr_as_mut(stats_observer) } {
        Ok(v) => v.on_stats_complete(values),
        Err(e) => error!("stats_observer_OnStatsComplete(): {}", e),
    };
}

/// StatsObserver callback function pointers.
#[repr(C)]
#[allow(non_snake_case)]
pub struct StatsObserverCallbacks {
    pub onStatsComplete:
        extern "C" fn(stats_observer: *mut StatsObserver, values: &MediaStatistics),
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
