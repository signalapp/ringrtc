//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Statistics

use std::{
    borrow::Cow,
    collections::HashMap,
    slice,
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(not(target_os = "android"))]
use sysinfo::{CpuExt, SystemExt};

use crate::{common::CallId, webrtc};

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::stats_observer as stats;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::stats_observer::RffiStatsObserver;

#[cfg(feature = "sim")]
use crate::webrtc::sim::stats_observer as stats;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::stats_observer::RffiStatsObserver;

/// How often to clean up old stats.
const CLEAN_UP_STATS_TICKS: u32 = 60;

const MAX_STATS_AGE: Duration = Duration::from_secs(60 * 10);

#[derive(Debug, Default)]
struct Stats {
    timestamp_us: i64,

    // Stats are kept in hash maps by ssrc. It is assumed that ssrcs are unique and
    // stop being used if that client leaves (in group calls). Stats just won't be
    // logged if not updated. For the purposes of logging, these assumptions should
    // be okay.
    audio_send: HashMap<u32, AudioSenderStatistics>,
    video_send: HashMap<u32, VideoSenderStatistics>,
    audio_recv: HashMap<u32, (Instant, AudioReceiverStatistics)>,
    video_recv: HashMap<u32, (Instant, VideoReceiverStatistics)>,

    report_json: Mutex<String>,
}
/// Collector object for obtaining statistics.
#[derive(Debug)]
pub struct StatsObserver {
    call_id: CallId,
    rffi: webrtc::Arc<RffiStatsObserver>,
    stats: Stats,
    stats_initial_offset: Duration,
    stats_received_count: u32,
    #[cfg(not(target_os = "android"))]
    system_stats: sysinfo::System,
}

impl StatsObserver {
    fn print_headers() {
        info!(
            "ringrtc_stats!,\
                connection,\
                call_id,\
                timestamp_us,\
                current_round_trip_time,\
                available_outgoing_bitrate"
        );
        info!(
            "ringrtc_stats!,\
                system,\
                cpu_usage_pct"
        );
        info!(
            "ringrtc_stats!,\
                audio,\
                send,\
                ssrc,\
                packets_per_second,\
                average_packet_size,\
                bitrate,\
                remote_packets_lost_pct,\
                remote_jitter,\
                remote_round_trip_time,\
                audio_energy"
        );
        info!(
            "ringrtc_stats!,\
                video,\
                send,\
                ssrc,\
                packets_per_second,\
                average_packet_size,\
                bitrate,\
                framerate,\
                key_frames_encoded,\
                encode_time_per_frame,\
                resolution,\
                retransmitted_packets_sent,\
                retransmitted_bitrate,\
                send_delay_per_packet,\
                nack_count,\
                pli_count,\
                quality_limitation_reason,\
                quality_limitation_resolution_changes,\
                remote_packets_lost_pct,\
                remote_jitter,\
                remote_round_trip_time"
        );
        info!(
            "ringrtc_stats!,\
                audio,\
                recv,\
                ssrc,\
                packets_per_second,\
                packets_lost_pct,\
                bitrate,\
                jitter,\
                audio_energy,\
                jitter_buffer_delay"
        );
        info!(
            "ringrtc_stats!,\
                video,\
                recv,\
                ssrc,\
                packets_per_second,\
                packets_lost_pct,\
                bitrate,\
                framerate,\
                key_frames_decoded,\
                decode_time_per_frame,\
                resolution"
        );
    }

    fn print_connection(&self, media_statistics: &MediaStatistics) {
        info!(
            "ringrtc_stats!,connection,{call_id},{timestamp_us},{current_round_trip_time:.0}ms,{available_outgoing_bitrate:.0}bps",
            call_id = self.call_id,
            timestamp_us = media_statistics.timestamp_us,
            current_round_trip_time = media_statistics
                .connection_statistics
                .current_round_trip_time
                * 1000.0,
            available_outgoing_bitrate = media_statistics
                .connection_statistics
                .available_outgoing_bitrate,
        );
    }

    fn print_system(&mut self) {
        #[cfg(not(target_os = "android"))]
        {
            // Be careful adding new stats;
            // some have a fair amount of persistent state that raises memory usage.
            self.system_stats.refresh_cpu();
            info!(
                "ringrtc_stats!,system,{cpu_pct:.0}%",
                cpu_pct = self.system_stats.global_cpu_info().cpu_usage(),
            )
        }
    }

    fn print_audio_sender(
        audio_sender: &AudioSenderStatistics,
        prev_audio_sender: &AudioSenderStatistics,
        seconds_elapsed: f32,
    ) {
        let packets_lost = audio_sender.remote_packets_lost - prev_audio_sender.remote_packets_lost;
        let packets_sent = audio_sender.packets_sent - prev_audio_sender.packets_sent;
        let bytes_sent = audio_sender.bytes_sent - prev_audio_sender.bytes_sent;

        info!(
            "ringrtc_stats!,audio,send,{ssrc},{packets_per_second:.1},{average_packet_size:.1},{bitrate:.1}bps,{remote_packets_lost_pct:.1}%,{remote_jitter:.0}ms,{remote_round_trip_time:.0}ms,{audio_energy:.3}",
            ssrc = audio_sender.ssrc,
            packets_per_second = packets_sent as f32 / seconds_elapsed,
            average_packet_size = if packets_sent > 0 { bytes_sent as f32 / packets_sent as f32 } else { 0.0 },
            bitrate = bytes_sent as f32 * 8.0 / seconds_elapsed,
            remote_packets_lost_pct = Self::compute_packets_lost_pct(packets_lost, packets_sent as i32),
            remote_jitter = audio_sender.remote_jitter * 1000.0,
            remote_round_trip_time = audio_sender.remote_round_trip_time * 1000.0,
            audio_energy = audio_sender.total_audio_energy - prev_audio_sender.total_audio_energy,
        );
    }

    fn print_video_sender(
        video_sender: &VideoSenderStatistics,
        prev_video_sender: &VideoSenderStatistics,
        seconds_elapsed: f32,
    ) {
        let packets_lost = video_sender.remote_packets_lost - prev_video_sender.remote_packets_lost;
        let packets_sent = video_sender.packets_sent - prev_video_sender.packets_sent;
        let bytes_sent = video_sender.bytes_sent - prev_video_sender.bytes_sent;
        let frames_encoded = video_sender.frames_encoded - prev_video_sender.frames_encoded;

        info!(
            "ringrtc_stats!,video,send,{ssrc},{packets_per_second:.1},{average_packet_size:.1},{bitrate:.0}bps,{framerate:.1}fps,{key_frames_encoded},{encode_time_per_frame:.1}ms,{width}x{height},{retransmitted_packets_sent},{retransmitted_bitrate:.1}bps,{send_delay_per_packet:.1}ms,{nack_count},{pli_count},{quality_limitation_reason},{quality_limitation_resolution_changes},{remote_packets_lost_pct:.1}%,{remote_jitter:.1}ms,{remote_round_trip_time:.1}ms",
            ssrc = video_sender.ssrc,
            packets_per_second = packets_sent as f32 / seconds_elapsed,
            average_packet_size = if packets_sent > 0 { bytes_sent as f32 / packets_sent as f32 } else { 0.0 },
            bitrate = bytes_sent as f32 * 8.0 / seconds_elapsed,
            framerate = frames_encoded as f32 / seconds_elapsed,
            key_frames_encoded = video_sender.key_frames_encoded - prev_video_sender.key_frames_encoded,
            encode_time_per_frame = if frames_encoded > 0 { (video_sender.total_encode_time - prev_video_sender.total_encode_time) * 1000.0 / frames_encoded as f64 } else { 0.0 },
            width = video_sender.frame_width,
            height = video_sender.frame_height,
            retransmitted_packets_sent = video_sender.retransmitted_packets_sent - prev_video_sender.retransmitted_packets_sent,
            retransmitted_bitrate = (video_sender.retransmitted_bytes_sent - prev_video_sender.retransmitted_bytes_sent) as f32 / seconds_elapsed,
            send_delay_per_packet = if packets_sent > 0 { (video_sender.total_packet_send_delay - prev_video_sender.total_packet_send_delay) * 1000.0 / packets_sent as f64 } else { 0.0 },
            nack_count = video_sender.nack_count - prev_video_sender.nack_count,
            pli_count = video_sender.pli_count - prev_video_sender.pli_count,
            quality_limitation_reason = video_sender.quality_limitation_reason_description(),
            quality_limitation_resolution_changes = video_sender.quality_limitation_resolution_changes - prev_video_sender.quality_limitation_resolution_changes,
            remote_packets_lost_pct = Self::compute_packets_lost_pct(packets_lost, packets_sent as i32),
            remote_jitter = video_sender.remote_jitter * 1000.0,
            remote_round_trip_time = video_sender.remote_round_trip_time * 1000.0,
        );
    }

    fn print_audio_receiver(
        audio_receiver: &AudioReceiverStatistics,
        prev_audio_receiver: &AudioReceiverStatistics,
        seconds_elapsed: f32,
    ) {
        let packets_lost = audio_receiver.packets_lost - prev_audio_receiver.packets_lost;
        let packets_received =
            audio_receiver.packets_received - prev_audio_receiver.packets_received;
        let jitter_buffer_emitted_count = audio_receiver.jitter_buffer_emitted_count
            - prev_audio_receiver.jitter_buffer_emitted_count;

        info!(
            "ringrtc_stats!,audio,recv,{ssrc},{packets_per_second:.1},{packets_lost_pct:.1}%,{bitrate:.1}bps,{jitter:.0}ms,{audio_energy:.3},{jitter_buffer_delay:.0}ms",
            ssrc = audio_receiver.ssrc,
            packets_per_second = (audio_receiver.packets_received - prev_audio_receiver.packets_received) as f32
                / seconds_elapsed,
            packets_lost_pct = Self::compute_packets_lost_pct(packets_lost, packets_received as i32 + packets_lost),
            bitrate = (audio_receiver.bytes_received - prev_audio_receiver.bytes_received) as f32 * 8.0
                / seconds_elapsed,
            jitter = audio_receiver.jitter * 1000.0,
            audio_energy = audio_receiver.total_audio_energy - prev_audio_receiver.total_audio_energy,
            jitter_buffer_delay = if jitter_buffer_emitted_count > 0 {
                (audio_receiver.jitter_buffer_delay - prev_audio_receiver.jitter_buffer_delay)
                    / (jitter_buffer_emitted_count as f64)
                    * 1000.0
            } else {
                0.0
            },
        );
    }

    fn print_video_receiver(
        video_receiver: &VideoReceiverStatistics,
        prev_video_receiver: &VideoReceiverStatistics,
        seconds_elapsed: f32,
    ) {
        let packets_lost = video_receiver.packets_lost - prev_video_receiver.packets_lost;
        let packets_received =
            video_receiver.packets_received - prev_video_receiver.packets_received;
        let frames_decoded = video_receiver.frames_decoded - prev_video_receiver.frames_decoded;

        info!(
            "ringrtc_stats!,video,recv,{ssrc},{packets_per_second:.1},{packets_lost_pct:.1}%,{bitrate:.0}bps,{framerate:.1}fps,{key_frames_decoded},{decode_time_per_frame:.1}ms,{width}x{height}",
            ssrc = video_receiver.ssrc,
            packets_per_second = (video_receiver.packets_received - prev_video_receiver.packets_received) as f32
                / seconds_elapsed,
            packets_lost_pct = Self::compute_packets_lost_pct(packets_lost, packets_received as i32 + packets_lost),
            bitrate = (video_receiver.bytes_received - prev_video_receiver.bytes_received) as f32 * 8.0
                / seconds_elapsed,
            framerate = frames_decoded as f32 / seconds_elapsed,
            key_frames_decoded = video_receiver.key_frames_decoded - prev_video_receiver.key_frames_decoded,
            decode_time_per_frame = if frames_decoded > 0 {
                (video_receiver.total_decode_time - prev_video_receiver.total_decode_time) * 1000.0 / frames_decoded as f64
            } else {
                0.0
            },
            width = video_receiver.frame_width,
            height = video_receiver.frame_height,
        );
    }

    fn compute_packets_lost_pct(packets_lost: i32, packets: i32) -> f32 {
        if packets > 0 {
            packets_lost as f32 / packets as f32 * 100.0
        } else if packets_lost < 0 {
            // Only negative packets lost
            -100.0
        } else if packets_lost > 0 {
            // Only positive packets lost
            100.0
        } else {
            0.0
        }
    }

    /// Create a new StatsObserver.
    fn new(call_id: CallId, stats_initial_offset: Duration) -> Self {
        Self::print_headers();

        #[cfg(not(target_os = "android"))]
        let system_stats = {
            let mut stats = sysinfo::System::new();
            // Do an initial refresh for meaningful results on the first log.
            // Be careful adding new stats;
            // some have a fair amount of persistent state that raises memory usage.
            stats.refresh_cpu();
            stats
        };

        Self {
            call_id,
            rffi: webrtc::Arc::null(),
            stats: Default::default(),
            stats_initial_offset,
            stats_received_count: 0,
            #[cfg(not(target_os = "android"))]
            system_stats,
        }
    }

    /// Invoked when statistics are received via the stats observer callback.
    fn on_stats_complete(&mut self, media_statistics: &MediaStatistics, report_json: String) {
        let seconds_elapsed = if self.stats.timestamp_us > 0 {
            (media_statistics.timestamp_us - self.stats.timestamp_us) as f32 / 1_000_000.0
        } else {
            self.stats_initial_offset.as_secs() as f32
        };

        self.print_connection(media_statistics);
        self.print_system();

        let stats = &mut self.stats;
        let mut stats_report_json = stats.report_json.lock().unwrap();
        *stats_report_json = report_json;
        drop(stats_report_json);

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
                let prev_audio_send_stats = stats.audio_send.entry(audio_sender.ssrc).or_default();

                Self::print_audio_sender(audio_sender, prev_audio_send_stats, seconds_elapsed);

                *prev_audio_send_stats = audio_sender.clone();
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
                let prev_video_send_stats = stats.video_send.entry(video_sender.ssrc).or_default();

                if video_sender.is_new_stream(prev_video_send_stats) {
                    *prev_video_send_stats = Default::default();
                }

                Self::print_video_sender(video_sender, prev_video_send_stats, seconds_elapsed);

                *prev_video_send_stats = video_sender.clone();
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
                let (updated_at, prev_audio_recv_stats) = stats
                    .audio_recv
                    .entry(audio_receiver.ssrc)
                    .or_insert_with(|| (Instant::now(), Default::default()));

                Self::print_audio_receiver(audio_receiver, prev_audio_recv_stats, seconds_elapsed);

                *updated_at = Instant::now();
                *prev_audio_recv_stats = audio_receiver.clone();
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
            for video_receiver in video_receivers.iter() {
                let (updated_at, prev_video_recv_stats) = stats
                    .video_recv
                    .entry(video_receiver.ssrc)
                    .or_insert_with(|| (Instant::now(), Default::default()));

                Self::print_video_receiver(video_receiver, prev_video_recv_stats, seconds_elapsed);

                *updated_at = Instant::now();
                *prev_video_recv_stats = video_receiver.clone();
            }
        }

        stats.timestamp_us = media_statistics.timestamp_us;

        self.stats_received_count += 1;

        if self.stats_received_count % CLEAN_UP_STATS_TICKS == 0 {
            self.remove_old_stats();
        }
    }

    /// Removes stats that were received before [MAX_STATS_AGE].
    fn remove_old_stats(&mut self) {
        self.stats
            .audio_recv
            .retain(|_, (ts, _)| ts.elapsed() < MAX_STATS_AGE);

        self.stats
            .video_recv
            .retain(|_, (ts, _)| ts.elapsed() < MAX_STATS_AGE);
    }

    /// Set the RFFI observer object.
    pub fn set_rffi(&mut self, rffi: webrtc::Arc<RffiStatsObserver>) {
        self.rffi = rffi
    }

    /// Return the RFFI observer object.
    pub fn rffi(&self) -> &webrtc::Arc<RffiStatsObserver> {
        &self.rffi
    }

    pub fn take_stats_report(&self) -> Option<String> {
        let mut stats_report_json = self.stats.report_json.lock().unwrap();
        if !stats_report_json.is_empty() {
            Some(std::mem::take(&mut *stats_report_json))
        } else {
            None
        }
    }

    pub fn set_collect_raw_stats_report(&self, collect_raw_stats_report: bool) {
        unsafe {
            stats::Rust_setCollectRawStatsReport(self.rffi.as_borrowed(), collect_raw_stats_report)
        };
    }
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct AudioSenderStatistics {
    pub ssrc: u32,
    pub packets_sent: u32,
    pub bytes_sent: u64,
    pub remote_packets_lost: i32,
    pub remote_jitter: f64,
    pub remote_round_trip_time: f64,
    pub total_audio_energy: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
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
    pub pli_count: u32,
    pub quality_limitation_reason: u32,
    pub quality_limitation_resolution_changes: u32,
    pub remote_packets_lost: i32,
    pub remote_jitter: f64,
    pub remote_round_trip_time: f64,
}

impl VideoSenderStatistics {
    /// Returns whether the stats for this stream have been reset.
    ///
    /// Most of the values in [VideoSenderStatistics] are nondecreasing values for a specific
    /// stream. If one of these values decreases, that's a sign that the stream was reset. This can
    /// happen when entering or exiting screenshare mode.
    fn is_new_stream(&self, prev_stats: &VideoSenderStatistics) -> bool {
        self.packets_sent < prev_stats.packets_sent
            || self.bytes_sent < prev_stats.bytes_sent
            || self.frames_encoded < prev_stats.frames_encoded
            || self.key_frames_encoded < prev_stats.key_frames_encoded
            || self.nack_count < prev_stats.nack_count
            || self.pli_count < prev_stats.pli_count
    }

    fn quality_limitation_reason_description(&self) -> Cow<'static, str> {
        // See https://w3c.github.io/webrtc-stats/#rtcqualitylimitationreason-enum.
        match self.quality_limitation_reason {
            0 => "none".into(),
            1 => "cpu".into(),
            2 => "bandwidth".into(),
            3 => "other".into(),
            x => x.to_string().into(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct AudioReceiverStatistics {
    pub ssrc: u32,
    pub packets_received: u32,
    pub packets_lost: i32,
    pub bytes_received: u64,
    pub jitter: f64,
    pub total_audio_energy: f64,
    pub jitter_buffer_delay: f64,
    pub jitter_buffer_emitted_count: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct VideoReceiverStatistics {
    pub ssrc: u32,
    pub packets_received: u32,
    pub packets_lost: i32,
    pub bytes_received: u64,
    pub frames_decoded: u32,
    pub key_frames_decoded: u32,
    pub total_decode_time: f64,
    pub frame_width: u32,
    pub frame_height: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct ConnectionStatistics {
    pub current_round_trip_time: f64,
    pub available_outgoing_bitrate: f64,
}

/// MediaStatistics struct that holds all the statistics.
#[repr(C)]
#[derive(Debug)]
pub struct MediaStatistics {
    pub timestamp_us: i64,
    pub audio_sender_statistics_size: u32,
    pub audio_sender_statistics: *const AudioSenderStatistics,
    pub video_sender_statistics_size: u32,
    pub video_sender_statistics: *const VideoSenderStatistics,
    pub audio_receiver_statistics_size: u32,
    pub audio_receiver_statistics: *const AudioReceiverStatistics,
    pub video_receiver_statistics_size: u32,
    pub video_receiver_statistics: *const VideoReceiverStatistics,
    pub connection_statistics: ConnectionStatistics,
}

/// StatsObserver OnStatsComplete() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn stats_observer_OnStatsComplete(
    stats_observer: webrtc::ptr::Borrowed<StatsObserver>,
    values: webrtc::ptr::Borrowed<MediaStatistics>,
    report_json: webrtc::ptr::Borrowed<std::os::raw::c_char>,
) {
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(stats_observer) = unsafe { stats_observer.as_mut() } {
        let report_json = unsafe {
            std::ffi::CStr::from_ptr(report_json.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        // Safe because the values should still be alive (it was just passed to us)
        if let Some(values) = unsafe { values.as_ref() } {
            stats_observer.on_stats_complete(values, report_json);
        } else {
            error!("stats_observer_OnStatsComplete() with null values");
        }
    } else {
        error!("stats_observer_OnStatsComplete() with null observer");
    }
}

/// StatsObserver callback function pointers.
#[repr(C)]
#[allow(non_snake_case)]
pub struct StatsObserverCallbacks {
    pub onStatsComplete: extern "C" fn(
        stats_observer: webrtc::ptr::Borrowed<StatsObserver>,
        values: webrtc::ptr::Borrowed<MediaStatistics>,
        report_json: webrtc::ptr::Borrowed<std::os::raw::c_char>,
    ),
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
pub fn create_stats_observer(
    call_id: CallId,
    stats_initial_offset: Duration,
) -> Box<StatsObserver> {
    let stats_observer = Box::new(StatsObserver::new(call_id, stats_initial_offset));
    let stats_observer_ptr = Box::into_raw(stats_observer);
    let rffi_stats_observer = webrtc::Arc::from_owned(unsafe {
        stats::Rust_createStatsObserver(
            webrtc::ptr::Borrowed::from_ptr(stats_observer_ptr).to_void(),
            webrtc::ptr::Borrowed::from_ptr(STATS_OBSERVER_CBS_PTR).to_void(),
        )
    });
    let mut stats_observer = unsafe { Box::from_raw(stats_observer_ptr) };

    stats_observer.set_rffi(rffi_stats_observer);
    stats_observer
}
