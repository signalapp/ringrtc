//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Statistics

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{Debug, Display},
    ops::Sub,
    slice,
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::stats_observer as stats;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::stats_observer::RffiStatsObserver;
#[cfg(feature = "sim")]
use crate::webrtc::sim::stats_observer as stats;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::stats_observer::RffiStatsObserver;
use crate::{common::CallId, webrtc};

/// How often to clean up old stats.
const CLEAN_UP_STATS_TICKS: u32 = 60;

const MAX_STATS_AGE: Duration = Duration::from_secs(60 * 10);

/// Implements a simple, "partial", checked division operation for f32/f64.
/// Returns None if the result of division is NaN, +infinity or -infinity. This
/// can happen if an attempt is made to use inifinities or to divide by zero.
trait NaiveCheckedDiv<T = Self> {
    fn naive_checked_div(&self, other: T) -> Option<T>;
}

macro_rules! impl_naive_checked_div {
    ($type:ty) => {
        impl NaiveCheckedDiv for $type {
            fn naive_checked_div(&self, denominator: Self) -> Option<Self> {
                let r = self / denominator;
                if r.is_nan() || r == <$type>::INFINITY || r == <$type>::NEG_INFINITY {
                    None
                } else {
                    Some(r)
                }
            }
        }
    };
}

impl_naive_checked_div!(f32);
impl_naive_checked_div!(f64);

trait Zero {
    const ZERO: Self;
}

macro_rules! impl_zero {
    ($type:ty, $value:tt) => {
        impl Zero for $type {
            const ZERO: Self = $value;
        }
    };
}

impl_zero!(f32, 0.0);
impl_zero!(f64, 0.0);
impl_zero!(i32, 0);
impl_zero!(u32, 0);
impl_zero!(u64, 0);

fn delta_fn<T, F, V>(lhs: &T, rhs: &T, extract: F) -> V
where
    F: Fn(&T) -> V,
    V: Sub<Output = V> + PartialOrd + Zero,
{
    let l_val = extract(lhs);
    let r_val = extract(rhs);
    if l_val > r_val {
        l_val - r_val
    } else {
        V::ZERO
    }
}

macro_rules! delta {
    ($lhs:tt, $rhs:tt, $field:tt) => {
        delta_fn($lhs, $rhs, |stats| stats.$field)
    };
}

fn compute_packets_lost_pct(packets_lost: u32, packets_total: u32) -> f32 {
    (packets_lost as f32 * 100.0)
        .naive_checked_div(packets_total as f32)
        .unwrap_or(0.0)
}

fn compute_bitrate(byte_count: u64, seconds_elapsed: f32) -> f32 {
    (byte_count as f32 * 8.0)
        .naive_checked_div(seconds_elapsed)
        .unwrap_or(0.0)
}

fn compute_packets_per_second(packet_count: u32, seconds_elapsed: f32) -> f32 {
    (packet_count as f32)
        .naive_checked_div(seconds_elapsed)
        .unwrap_or(0.0)
}

#[derive(Debug)]
pub enum StatsSnapshot {
    Begin,
    End,
    AudioSender(AudioSenderStatsSnapshot),
    AudioReceiver(AudioReceiverStatsSnapshot),
    VideoSender(VideoSenderStatsSnapshot),
    VideoReceiver(VideoReceiverStatsSnapshot),
    Connection(ConnectionStatsSnapshot),
    #[cfg(not(target_os = "android"))]
    System(SystemStatsSnapshot),
}

macro_rules! impl_snapshot {
    ($type:ty, $name:tt) => {
        impl From<$type> for StatsSnapshot {
            fn from(value: $type) -> Self {
                StatsSnapshot::$name(value)
            }
        }
    };
}

impl_snapshot!(AudioSenderStatsSnapshot, AudioSender);
impl_snapshot!(AudioReceiverStatsSnapshot, AudioReceiver);
impl_snapshot!(VideoSenderStatsSnapshot, VideoSender);
impl_snapshot!(VideoReceiverStatsSnapshot, VideoReceiver);
impl_snapshot!(ConnectionStatsSnapshot, Connection);
#[cfg(not(target_os = "android"))]
impl_snapshot!(SystemStatsSnapshot, System);

pub trait StatsSnapshotConsumer: Debug {
    fn on_stats_snapshot_ready(&self, stats: &StatsSnapshot);
}

/// The default snapshot consumer is used by a StatsObserver instance if/while
/// no stats snapshot consumer is attached to it. It is a no-op.
#[derive(Debug, Default)]
struct DefaultStatsSnapshotConsumer;

impl StatsSnapshotConsumer for DefaultStatsSnapshotConsumer {
    fn on_stats_snapshot_ready(&self, _stats: &StatsSnapshot) {}
}

/// Instances of AudioSenderStatsSnapshot capture audio sender statistics over a
/// certain period of time, usually some number of seconds.
#[derive(Debug)]
pub struct AudioSenderStatsSnapshot {
    pub ssrc: u32,
    pub packets_per_second: f32,
    pub average_packet_size: f32,
    pub bitrate: f32,
    pub remote_packets_lost_pct: f32,
    pub remote_jitter: f64,
    pub remote_rtt: f64,
    pub audio_energy: f64,
}

impl Display for AudioSenderStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            ssrc,
            packets_per_second,
            average_packet_size,
            bitrate,
            remote_packets_lost_pct,
            remote_jitter,
            remote_rtt,
            audio_energy,
        } = self;
        write!(
            f,
            "{},\
            {ssrc},\
            {packets_per_second:.1},\
            {average_packet_size:.1},\
            {bitrate:.1}bps,\
            {remote_packets_lost_pct:.1}%,\
            {remote_jitter:.0}ms,\
            {remote_rtt:.0}ms,\
            {audio_energy:.3}",
            Self::LOG_MARKER,
        )
    }
}

impl AudioSenderStatsSnapshot {
    const LOG_MARKER: &str = "ringrtc_stats!,audio,send";
    const LOG_HEADER: &str = "ringrtc_stats!,audio,send,\
        ssrc,\
        packets_per_second,\
        average_packet_size,\
        bitrate,\
        remote_packets_lost_pct,\
        remote_jitter,\
        remote_round_trip_time,\
        audio_energy";

    fn derive(
        curr_stats: &AudioSenderStatistics,
        prev_stats: &AudioSenderStatistics,
        seconds_elapsed: f32,
    ) -> Self {
        let packets_lost_delta = delta!(curr_stats, prev_stats, remote_packets_lost);
        let packets_sent_delta = delta!(curr_stats, prev_stats, packets_sent);
        let audio_energy_delta = delta!(curr_stats, prev_stats, total_audio_energy);
        let bytes_sent_delta = delta!(curr_stats, prev_stats, bytes_sent);

        let packets_per_second = compute_packets_per_second(packets_sent_delta, seconds_elapsed);
        let average_packet_size = (bytes_sent_delta as f32)
            .naive_checked_div(packets_sent_delta as f32)
            .unwrap_or(0.0);
        let bitrate = compute_bitrate(bytes_sent_delta, seconds_elapsed);
        let remote_packets_lost_pct =
            compute_packets_lost_pct(packets_lost_delta.max(0) as u32, packets_sent_delta);

        let remote_jitter = 1000.0 * curr_stats.remote_jitter;
        let remote_rtt = 1000.0 * curr_stats.remote_round_trip_time;

        Self {
            ssrc: curr_stats.ssrc,
            packets_per_second,
            average_packet_size,
            bitrate,
            remote_packets_lost_pct,
            remote_jitter,
            remote_rtt,
            audio_energy: audio_energy_delta,
        }
    }
}

/// Instances of AudioReceiverStatsSnapshot capture audio receiver statistics
/// over a certain period of time, usually some number of seconds.
#[derive(Debug)]
pub struct AudioReceiverStatsSnapshot {
    pub ssrc: u32,
    pub packets_per_second: f32,
    pub packets_lost_pct: f32,
    pub bitrate: f32,
    pub jitter: f64,
    pub jitter_buffer_delay: f64,
    pub audio_energy: f64,
}

impl Display for AudioReceiverStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            ssrc,
            packets_per_second,
            packets_lost_pct,
            bitrate,
            jitter,
            jitter_buffer_delay,
            audio_energy,
        } = self;
        write!(
            f,
            "{},\
            {ssrc},\
            {packets_per_second:.1},\
            {packets_lost_pct:.1}%,\
            {bitrate:.1}bps,\
            {jitter:.0}ms,\
            {audio_energy:.3},\
            {jitter_buffer_delay:.0}ms",
            Self::LOG_MARKER
        )
    }
}

impl AudioReceiverStatsSnapshot {
    const LOG_MARKER: &str = "ringrtc_stats!,audio,recv";
    const LOG_HEADER: &str = "ringrtc_stats!,audio,recv,\
        ssrc,\
        packets_per_second,\
        packets_lost_pct,\
        bitrate,\
        jitter,\
        audio_energy,\
        jitter_buffer_delay";

    fn derive(
        curr_stats: &AudioReceiverStatistics,
        prev_stats: &AudioReceiverStatistics,
        seconds_elapsed: f32,
    ) -> Self {
        let packets_lost_delta = delta!(curr_stats, prev_stats, packets_lost);
        let packets_received_delta = delta!(curr_stats, prev_stats, packets_received);
        let jitter_buffer_delay_delta = delta!(curr_stats, prev_stats, jitter_buffer_delay);
        let jitter_buffer_emitted_delta =
            delta!(curr_stats, prev_stats, jitter_buffer_emitted_count);
        let bytes_received_delta = delta!(curr_stats, prev_stats, bytes_received);
        let audio_energy_delta = delta!(curr_stats, prev_stats, total_audio_energy);

        let packets_per_second =
            compute_packets_per_second(packets_received_delta, seconds_elapsed);
        let packets_lost = packets_lost_delta.max(0) as u32;
        let packets_lost_pct =
            compute_packets_lost_pct(packets_lost, packets_received_delta + packets_lost);
        let bitrate = compute_bitrate(bytes_received_delta, seconds_elapsed);

        let jitter = 1000.0 * curr_stats.jitter;
        let jitter_buffer_delay = 1000.0
            * jitter_buffer_delay_delta
                .naive_checked_div(jitter_buffer_emitted_delta as f64)
                .unwrap_or(0.0);

        Self {
            ssrc: curr_stats.ssrc,
            packets_per_second,
            packets_lost_pct,
            bitrate,
            jitter,
            jitter_buffer_delay,
            audio_energy: audio_energy_delta,
        }
    }
}

#[derive(Debug)]
pub struct VideoSenderStatsSnapshot {
    pub ssrc: u32,
    pub packets_per_second: f32,
    pub average_packet_size: f32,
    pub bitrate: f32,
    pub framerate: f32,
    pub key_frames_encoded: u32,
    pub encode_time_per_frame: f64,
    pub width: u32,
    pub height: u32,
    pub retransmitted_packets_sent: u64,
    pub retransmitted_bitrate: f32,
    pub send_delay_per_packet: f64,
    pub nack_count: u32,
    pub pli_count: u32,
    pub quality_limitation_reason: Cow<'static, str>,
    pub quality_limitation_resolution_changes: u32,
    pub remote_packets_lost_pct: f32,
    pub remote_jitter: f64,
    pub remote_round_trip_time: f64,
}

impl Display for VideoSenderStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            ssrc,
            packets_per_second,
            average_packet_size,
            bitrate,
            framerate,
            key_frames_encoded,
            encode_time_per_frame,
            width,
            height,
            retransmitted_packets_sent,
            retransmitted_bitrate,
            send_delay_per_packet,
            nack_count,
            pli_count,
            quality_limitation_reason,
            quality_limitation_resolution_changes,
            remote_packets_lost_pct,
            remote_jitter,
            remote_round_trip_time,
        } = self;
        write!(
            f,
            "{},\
            {ssrc},\
            {packets_per_second:.1},\
            {average_packet_size:.1},\
            {bitrate:.0}bps,\
            {framerate:.1}fps,\
            {key_frames_encoded},\
            {encode_time_per_frame:.1}ms,\
            {width}x{height},\
            {retransmitted_packets_sent},\
            {retransmitted_bitrate:.1}bps,\
            {send_delay_per_packet:.1}ms,\
            {nack_count},\
            {pli_count},\
            {quality_limitation_reason},\
            {quality_limitation_resolution_changes},\
            {remote_packets_lost_pct:.1}%,\
            {remote_jitter:.1}ms,\
            {remote_round_trip_time:.1}ms",
            Self::LOG_MARKER
        )
    }
}

impl VideoSenderStatsSnapshot {
    const LOG_MARKER: &str = "ringrtc_stats!,video,send";
    const LOG_HEADER: &str = "ringrtc_stats!,\
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
                 remote_round_trip_time";

    fn derive(
        curr_stats: &VideoSenderStatistics,
        prev_stats: &VideoSenderStatistics,
        seconds_elapsed: f32,
    ) -> Self {
        let packets_lost_delta = delta!(curr_stats, prev_stats, remote_packets_lost);
        let packets_sent_delta = delta!(curr_stats, prev_stats, packets_sent);
        let bytes_sent_delta = delta!(curr_stats, prev_stats, bytes_sent);
        let retransmitted_bytes_sent_delta =
            delta!(curr_stats, prev_stats, retransmitted_bytes_sent);
        let frames_encoded_delta = delta!(curr_stats, prev_stats, frames_encoded);
        let key_frames_encoded_delta = delta!(curr_stats, prev_stats, key_frames_encoded);
        let total_encode_time_delta = delta!(curr_stats, prev_stats, total_encode_time);
        let retransmitted_packets_sent_delta =
            delta!(curr_stats, prev_stats, retransmitted_packets_sent);
        let total_packets_send_delay_delta =
            delta!(curr_stats, prev_stats, total_packet_send_delay);
        let nack_count_delta = delta!(curr_stats, prev_stats, nack_count);
        let pli_count_delta = delta!(curr_stats, prev_stats, pli_count);
        let quality_limitation_resolution_changes_delta = delta!(
            curr_stats,
            prev_stats,
            quality_limitation_resolution_changes
        );

        let packets_per_second = compute_packets_per_second(packets_sent_delta, seconds_elapsed);
        let average_packet_size = (packets_sent_delta as f32)
            .naive_checked_div(seconds_elapsed)
            .unwrap_or(0.0);
        let bitrate = compute_bitrate(bytes_sent_delta, seconds_elapsed);
        let retransmitted_bitrate =
            compute_bitrate(retransmitted_bytes_sent_delta, seconds_elapsed);
        let framerate = (frames_encoded_delta as f32)
            .naive_checked_div(seconds_elapsed)
            .unwrap_or(0.0);
        let encode_time_per_frame = 1000.0
            * total_encode_time_delta
                .naive_checked_div(frames_encoded_delta as f64)
                .unwrap_or(0.0);
        let send_delay_per_packet = 1000.0
            * total_packets_send_delay_delta
                .naive_checked_div(frames_encoded_delta as f64)
                .unwrap_or(0.0);
        let remote_packets_lost_pct =
            compute_packets_lost_pct(packets_lost_delta.max(0) as u32, packets_sent_delta);
        let remote_jitter = 1000.0 * curr_stats.remote_jitter;
        let remote_round_trip_time = 1000.0 * curr_stats.remote_round_trip_time;

        let quality_limitation_reason = curr_stats.quality_limitation_reason_description();

        Self {
            ssrc: curr_stats.ssrc,
            packets_per_second,
            average_packet_size,
            bitrate,
            framerate,
            key_frames_encoded: key_frames_encoded_delta,
            encode_time_per_frame,
            width: curr_stats.frame_width,
            height: curr_stats.frame_height,
            retransmitted_packets_sent: retransmitted_packets_sent_delta,
            retransmitted_bitrate,
            send_delay_per_packet,
            nack_count: nack_count_delta,
            pli_count: pli_count_delta,
            quality_limitation_reason,
            quality_limitation_resolution_changes: quality_limitation_resolution_changes_delta,
            remote_packets_lost_pct,
            remote_jitter,
            remote_round_trip_time,
        }
    }
}

#[derive(Debug)]
pub struct VideoReceiverStatsSnapshot {
    pub ssrc: u32,
    pub packets_per_second: f32,
    pub packets_lost_pct: f32,
    pub bitrate: f32,
    pub framerate: f32,
    pub key_frames_decoded: u32,
    pub decode_time_per_frame: f64,
    pub width: u32,
    pub height: u32,
    pub jitter: f64,
}

impl Display for VideoReceiverStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            ssrc,
            packets_per_second,
            packets_lost_pct,
            bitrate,
            framerate,
            key_frames_decoded,
            decode_time_per_frame,
            width,
            height,
            jitter,
        } = self;
        write!(
            f,
            "{},\
            {ssrc},\
            {packets_per_second:.1},\
            {packets_lost_pct:.1}%,\
            {bitrate:.0}bps,\
            {framerate:.1}fps,\
            {key_frames_decoded},\
            {decode_time_per_frame:.1}ms,\
            {width}x{height},\
            {jitter}ms",
            Self::LOG_MARKER,
        )
    }
}

impl VideoReceiverStatsSnapshot {
    const LOG_MARKER: &str = "ringrtc_stats!,video,recv";
    const LOG_HEADER: &str = "ringrtc_stats!,\
        video,\
        recv,\
        ssrc,\
        packets_per_second,\
        packets_lost_pct,\
        bitrate,\
        framerate,\
        key_frames_decoded,\
        decode_time_per_frame,\
        resolution\
        jitter";

    fn derive(
        curr_stats: &VideoReceiverStatistics,
        prev_stats: &VideoReceiverStatistics,
        seconds_elapsed: f32,
    ) -> Self {
        let packets_lost_delta = delta!(curr_stats, prev_stats, packets_lost);
        let packets_received_delta = delta!(curr_stats, prev_stats, packets_received);
        let frames_decoded_delta = delta!(curr_stats, prev_stats, frames_decoded);
        let key_frames_decoded_delta = delta!(curr_stats, prev_stats, key_frames_decoded);
        let bytes_received_delta = delta!(curr_stats, prev_stats, bytes_received);
        let total_decode_time_delta = delta!(curr_stats, prev_stats, total_decode_time);
        let jitter = delta!(curr_stats, prev_stats, jitter);

        let packets_per_second =
            compute_packets_per_second(packets_received_delta, seconds_elapsed);
        let packets_lost = packets_lost_delta.max(0) as u32;
        let packets_lost_pct =
            compute_packets_lost_pct(packets_lost, packets_received_delta + packets_lost);
        let bitrate = compute_bitrate(bytes_received_delta, seconds_elapsed);
        let framerate = frames_decoded_delta as f32 / seconds_elapsed;
        let decode_time_per_frame = 1000.0
            * total_decode_time_delta
                .naive_checked_div(frames_decoded_delta as f64)
                .unwrap_or(0.0);

        Self {
            ssrc: curr_stats.ssrc,
            packets_per_second,
            packets_lost_pct,
            bitrate,
            framerate,
            key_frames_decoded: key_frames_decoded_delta,
            decode_time_per_frame,
            width: curr_stats.frame_width,
            height: curr_stats.frame_height,
            jitter,
        }
    }
}

#[derive(Debug)]
pub struct ConnectionStatsSnapshot {
    pub call_id: CallId,
    pub timestamp_us: i64,
    pub current_round_trip_time: f64,
    pub available_outgoing_bitrate: f64,
    pub requests_sent: u64,
    pub responses_received: u64,
    pub requests_received: u64,
    pub responses_sent: u64,
}

impl ConnectionStatsSnapshot {
    const LOG_MARKER: &str = "ringrtc_stats!,connection";
    const LOG_HEADER: &str = "ringrtc_stats!,\
        connection,\
        call_id,\
        timestamp_us,\
        current_round_trip_time,\
        available_outgoing_bitrate,\
        requests_sent,\
        responses_received,\
        requests_received,\
        responses_sent";

    fn derive(call_id: CallId, timestamp_us: i64, stats: &ConnectionStatistics) -> Self {
        Self {
            call_id,
            timestamp_us,
            current_round_trip_time: 1000.0 * stats.current_round_trip_time,
            available_outgoing_bitrate: stats.available_outgoing_bitrate,
            requests_sent: stats.requests_sent,
            responses_received: stats.responses_received,
            requests_received: stats.requests_received,
            responses_sent: stats.responses_sent,
        }
    }
}

impl Display for ConnectionStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            call_id,
            timestamp_us,
            current_round_trip_time,
            available_outgoing_bitrate,
            requests_sent,
            responses_received,
            requests_received,
            responses_sent,
        } = self;
        write!(
            f,
            "{},\
            {call_id},\
            {timestamp_us},\
            {current_round_trip_time:.0}ms,\
            {available_outgoing_bitrate:.0}bps,\
            {requests_sent},\
            {responses_received},\
            {requests_received},\
            {responses_sent}",
            Self::LOG_MARKER
        )
    }
}

#[cfg(not(target_os = "android"))]
#[derive(Debug)]
pub struct SystemStatsSnapshot {
    pub cpu_pct: f32,
}

#[cfg(not(target_os = "android"))]
impl SystemStatsSnapshot {
    const LOG_MARKER: &str = "ringrtc_stats!,system";
    const LOG_HEADER: &str = "ringrtc_stats!,system,cpu_usage_pct";

    fn derive(system_stats: &sysinfo::System) -> Self {
        // Be careful when adding new stats; some have a fair amount of
        // persistent state that raises memory usage.
        Self {
            cpu_pct: system_stats.global_cpu_usage(),
        }
    }
}

#[cfg(not(target_os = "android"))]
impl Display for SystemStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { cpu_pct } = self;
        write!(
            f,
            "{},\
            {cpu_pct:.0}",
            Self::LOG_MARKER
        )
    }
}

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
    connections: HashMap<String, (Instant, ConnectionStatistics)>,
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
    stats_snapshot_consumer: Box<dyn StatsSnapshotConsumer>,
    #[cfg(not(target_os = "android"))]
    system_stats: sysinfo::System,
}

impl StatsObserver {
    pub fn print_headers() {
        info!("{}", ConnectionStatsSnapshot::LOG_HEADER);
        #[cfg(not(target_os = "android"))]
        info!("{}", SystemStatsSnapshot::LOG_HEADER);
        info!("{}", AudioSenderStatsSnapshot::LOG_HEADER);
        info!("{}", VideoSenderStatsSnapshot::LOG_HEADER);
        info!("{}", AudioReceiverStatsSnapshot::LOG_HEADER);
        info!("{}", VideoReceiverStatsSnapshot::LOG_HEADER);
    }

    /// Create a new StatsObserver.
    fn new(call_id: CallId, stats_initial_offset: Duration) -> Self {
        #[cfg(not(target_os = "android"))]
        let system_stats = {
            let mut stats = sysinfo::System::new();
            // Do an initial refresh for meaningful results on the first log.
            // Be careful adding new stats;
            // some have a fair amount of persistent state that raises memory usage.
            stats.refresh_cpu_usage();
            stats
        };

        let stats_snapshot_consumer = Box::new(DefaultStatsSnapshotConsumer);

        Self {
            call_id,
            rffi: webrtc::Arc::null(),
            stats: Default::default(),
            stats_initial_offset,
            stats_received_count: 0,
            stats_snapshot_consumer,
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

        let stats = &mut self.stats;
        let mut stats_report_json = stats.report_json.lock().unwrap();
        *stats_report_json = report_json;
        drop(stats_report_json);

        self.stats_snapshot_consumer
            .on_stats_snapshot_ready(&StatsSnapshot::Begin);

        // System

        #[cfg(not(target_os = "android"))]
        {
            let system_stats_snapshot = SystemStatsSnapshot::derive(&self.system_stats);
            info!("{system_stats_snapshot}");
            self.stats_snapshot_consumer
                .on_stats_snapshot_ready(&system_stats_snapshot.into());
        }

        // Connection

        let connection_stats_snapshot = ConnectionStatsSnapshot::derive(
            self.call_id,
            media_statistics.timestamp_us,
            &media_statistics.nominated_connection_statistics,
        );
        info!("{connection_stats_snapshot}");
        self.stats_snapshot_consumer
            .on_stats_snapshot_ready(&connection_stats_snapshot.into());

        // Audio senders

        for audio_sender in media_statistics.get_audio_sender_statistics() {
            let prev_audio_send_stats = self.stats.audio_send.entry(audio_sender.ssrc).or_default();
            let audio_sender_stats_snapshot = AudioSenderStatsSnapshot::derive(
                audio_sender,
                prev_audio_send_stats,
                seconds_elapsed,
            );
            info!("{audio_sender_stats_snapshot}");
            self.stats_snapshot_consumer
                .on_stats_snapshot_ready(&audio_sender_stats_snapshot.into());
            *prev_audio_send_stats = audio_sender.clone();
        }

        // Video senders

        for video_sender in media_statistics.get_video_sender_statistics() {
            let prev_video_send_stats = self.stats.video_send.entry(video_sender.ssrc).or_default();
            if video_sender.is_new_stream(prev_video_send_stats) {
                *prev_video_send_stats = Default::default();
            }
            let video_sender_stats_snapshot = VideoSenderStatsSnapshot::derive(
                video_sender,
                prev_video_send_stats,
                seconds_elapsed,
            );
            info!("{video_sender_stats_snapshot}");
            self.stats_snapshot_consumer
                .on_stats_snapshot_ready(&video_sender_stats_snapshot.into());
            *prev_video_send_stats = video_sender.clone();
        }

        // Audio receivers

        for audio_receiver in media_statistics.get_audio_receiver_statistics() {
            let (updated_at, prev_audio_recv_stats) = self
                .stats
                .audio_recv
                .entry(audio_receiver.ssrc)
                .or_insert_with(|| (Instant::now(), Default::default()));
            let audio_receiver_stats_snapshot = AudioReceiverStatsSnapshot::derive(
                audio_receiver,
                prev_audio_recv_stats,
                seconds_elapsed,
            );
            info!("{audio_receiver_stats_snapshot}");
            self.stats_snapshot_consumer
                .on_stats_snapshot_ready(&audio_receiver_stats_snapshot.into());
            *updated_at = Instant::now();
            *prev_audio_recv_stats = audio_receiver.clone();
        }

        // Video receivers

        for video_receiver in media_statistics.get_video_receiver_statistics() {
            let (updated_at, prev_video_recv_stats) = self
                .stats
                .video_recv
                .entry(video_receiver.ssrc)
                .or_insert_with(|| (Instant::now(), Default::default()));
            let video_receiver_stats_snapshot = VideoReceiverStatsSnapshot::derive(
                video_receiver,
                prev_video_recv_stats,
                seconds_elapsed,
            );
            info!("{video_receiver_stats_snapshot}");
            self.stats_snapshot_consumer
                .on_stats_snapshot_ready(&video_receiver_stats_snapshot.into());
            *updated_at = Instant::now();
            *prev_video_recv_stats = video_receiver.clone();
        }

        self.stats_snapshot_consumer
            .on_stats_snapshot_ready(&StatsSnapshot::End);

        self.stats.timestamp_us = media_statistics.timestamp_us;
        self.stats_received_count += 1;

        let now = Instant::now();
        self.stats.connections = media_statistics
            .get_connection_statistics()
            .iter()
            .flat_map(|c| {
                let pair_id = c.get_candidate_pair_id()?;
                Some((pair_id, (now, c.clone_without_ptr())))
            })
            .collect();

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

    pub fn set_stats_snapshot_consumer(&mut self, consumer: Box<dyn StatsSnapshotConsumer>) {
        self.stats_snapshot_consumer = consumer;
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
    pub jitter_buffer_flushes: u64,
    pub estimated_playout_timestamp: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct VideoReceiverStatistics {
    pub ssrc: u32,
    pub packets_received: u32,
    pub packets_lost: i32,
    pub bytes_received: u64,
    pub frames_received: u32,
    pub frames_decoded: u32,
    pub key_frames_decoded: u32,
    pub total_decode_time: f64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub freeze_count: u32,
    pub total_freezes_duration: f64,
    pub jitter: f64,
    pub jitter_buffer_delay: f64,
    pub jitter_buffer_emitted_count: u64,
    pub jitter_buffer_flushes: u64,
    pub estimated_playout_timestamp: f64,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct ConnectionStatistics {
    raw_candidate_pair_id: webrtc::ptr::Borrowed<std::os::raw::c_char>,

    pub current_round_trip_time: f64,
    pub available_outgoing_bitrate: f64,

    // stats related to ICE connectivity checks
    pub requests_sent: u64,
    pub responses_received: u64,
    pub requests_received: u64,
    pub responses_sent: u64,
}

impl Default for ConnectionStatistics {
    fn default() -> Self {
        Self {
            raw_candidate_pair_id: webrtc::ptr::Borrowed::null(),
            current_round_trip_time: 0.0,
            available_outgoing_bitrate: 0.0,
            requests_sent: 0,
            responses_received: 0,
            requests_received: 0,
            responses_sent: 0,
        }
    }
}

impl ConnectionStatistics {
    fn get_candidate_pair_id(&self) -> Option<String> {
        if !self.raw_candidate_pair_id.is_null() {
            Some(unsafe {
                std::ffi::CStr::from_ptr(self.raw_candidate_pair_id.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            })
        } else {
            None
        }
    }

    fn clone_without_ptr(&self) -> Self {
        let mut c = self.clone();
        c.raw_candidate_pair_id = webrtc::ptr::Borrowed::null();
        c
    }
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
    pub nominated_connection_statistics: ConnectionStatistics,
    pub connection_statistics: *const ConnectionStatistics,
    pub connection_statistics_size: u32,
}

impl MediaStatistics {
    unsafe fn from_ptr<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
        if ptr.is_null() {
            &[]
        } else {
            unsafe { slice::from_raw_parts(ptr, len) }
        }
    }

    pub fn get_connection_statistics(&self) -> &[ConnectionStatistics] {
        unsafe {
            Self::from_ptr(
                self.connection_statistics,
                self.connection_statistics_size as usize,
            )
        }
    }

    pub fn get_audio_sender_statistics(&self) -> &[AudioSenderStatistics] {
        unsafe {
            Self::from_ptr(
                self.audio_sender_statistics,
                self.audio_sender_statistics_size as usize,
            )
        }
    }

    pub fn get_video_sender_statistics(&self) -> &[VideoSenderStatistics] {
        unsafe {
            Self::from_ptr(
                self.video_sender_statistics,
                self.video_sender_statistics_size as usize,
            )
        }
    }

    pub fn get_audio_receiver_statistics(&self) -> &[AudioReceiverStatistics] {
        unsafe {
            Self::from_ptr(
                self.audio_receiver_statistics,
                self.audio_receiver_statistics_size as usize,
            )
        }
    }

    pub fn get_video_receiver_statistics(&self) -> &[VideoReceiverStatistics] {
        unsafe {
            Self::from_ptr(
                self.video_receiver_statistics,
                self.video_receiver_statistics_size as usize,
            )
        }
    }
}

/// StatsObserver OnStatsComplete() callback.
extern "C" fn stats_observer_on_stats_complete(
    mut stats_observer: webrtc::ptr::Borrowed<StatsObserver>,
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
            error!("stats_observer_on_stats_complete() with null values");
        }
    } else {
        error!("stats_observer_on_stats_complete() with null observer");
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
    onStatsComplete: stats_observer_on_stats_complete,
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
