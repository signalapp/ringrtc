//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign},
    sync::{Arc, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use prost::Message;
use sketches_ddsketch::{Config, DDSketch};

use crate::{
    common::{CallEndReason, CallState, ConnectionState, Result},
    core::{
        call_fsm::CallEvent, call_mutex::CallMutex, connection::ConnectionObserverEvent,
        group_call::RemoteDeviceState,
    },
    protobuf::{
        self,
        call_summary::{CallTelemetry, Event, StreamStats},
    },
    webrtc::{
        peer_connection_observer::NetworkRoute,
        stats_observer::{
            AudioReceiverStatsSnapshot, AudioSenderStatsSnapshot, StatsSnapshot,
            StatsSnapshotConsumer, VideoReceiverStatsSnapshot, VideoSenderStatsSnapshot,
        },
    },
};

#[derive(Debug, Default)]
pub struct CallSummary {
    pub start_time: Timestamp,
    pub end_time: Timestamp,
    pub quality_stats: QualityStats,
    pub raw_stats: Option<Vec<u8>>,
    pub raw_stats_text: Option<String>,
    pub is_survey_candidate: bool,
    pub call_end_reason_text: String,
}

#[derive(Debug, Default)]
pub struct QualityStats {
    pub rtt_median_connection: Option<f32>,
    pub audio_stats: MediaQualityStats,
    pub video_stats: MediaQualityStats,
}

#[derive(Debug, Default)]
pub struct MediaQualityStats {
    pub rtt_median: Option<f32>,
    pub jitter_median_send: Option<f32>,
    pub jitter_median_recv: Option<f32>,
    pub packet_loss_fraction_send: Option<f32>,
    pub packet_loss_fraction_recv: Option<f32>,
}

const CALL_TELEMETRY_VERSION: u32 = 1;

/// Maximum number of stream summaries that will be captured. The number is
/// large to accommodate summary creation for larger group calls in which
/// participants come and go frequently.
const MAX_STREAM_SUMMARIES: usize = 500;

/// Maximum size of the encoded telemetry record.
const MAX_TELEMETRY_ENCODED_SIZE: usize = 65536;

/// Maximum number of stats sets that we capture before we start dropping them,
/// oldest first.
const DEFAULT_MAX_STATS_SETS: usize = 30;

/// A timestamp is represented as the number of milliseconds since January 1,
/// 1970 0:0:0 UTC.
#[derive(Debug, Default, Clone, Copy)]
pub struct Timestamp(u64);

impl From<Timestamp> for u64 {
    fn from(timestamp: Timestamp) -> Self {
        timestamp.0
    }
}

impl From<Timestamp> for f64 {
    fn from(timestamp: Timestamp) -> Self {
        timestamp.0 as f64
    }
}

impl Timestamp {
    pub fn now() -> Option<Self> {
        Self::from_system_time(&SystemTime::now())
    }

    pub fn from_system_time(time: &SystemTime) -> Option<Self> {
        time.duration_since(UNIX_EPOCH)
            .map(|v| Self(v.as_millis() as u64))
            .ok()
    }
}

/// We need to be able to perform basic arithmetic operations on sample values
/// and have the ability to compare them. We also want to be able to create them
/// from counter values, and calculate their square roots.
trait Sample:
    PartialOrd
    + Add<Output = Self>
    + AddAssign
    + Sub<Output = Self>
    + SubAssign
    + Div<Output = Self>
    + DivAssign
    + Mul<Output = Self>
    + MulAssign
    + Copy
    + Debug
    + Default
{
    const ZERO: Self;
    const MAX: Self;
    const MIN: Self;

    /// Checks whether the sample value is valid. For floating point values, we
    /// +treat NaN, inf, and -inf as invalid values.
    fn is_valid(&self) -> bool;
    /// Running variance.
    fn variance(variance: Self, mean: Self, count: usize, sample: Self) -> (Self, Self);
}

/// This macro is used to generate trait implementations that are necessary
/// to satisfy the the `Sample<T>` trait. It is designed to work with floating
/// point primitive types only!
macro_rules! impl_sample_trait {
    ($type: ty) => {
        impl Sample for $type {
            const ZERO: Self = 0.0;
            const MAX: Self = <$type>::MAX;
            const MIN: Self = <$type>::MIN;

            /// Checks whether the sample value is valid. For floating point
            /// values, we treat NaN, inf, and -inf as invalid values.
            fn is_valid(&self) -> bool {
                !self.is_nan() && *self != Self::INFINITY && *self != Self::NEG_INFINITY
            }

            /// Running variance and mean (Welford's method).
            fn variance(variance: Self, mean: Self, count: usize, sample: Self) -> (Self, Self) {
                let n = count as $type;
                let new_mean = mean + (sample - mean) / (n + 1.0);
                let new_variance = if n > 1.0 {
                    ((n - 1.0) / n) * variance + (sample - mean) * (sample - new_mean) / (n + 1.0)
                } else {
                    0.0
                };
                (new_variance, new_mean)
            }
        }
    };
}

impl_sample_trait!(f32);

/// A summary of the behavior of a variable. This structure captures the mean,
/// standard deviation, as well as the minimum and maximum values of a variable.
#[derive(Debug)]
struct DistributionSummary<T: Sample> {
    mean: T,
    variance: T,
    min_val: T,
    max_val: T,
    count: usize,
}

impl<T: Sample> Default for DistributionSummary<T> {
    fn default() -> Self {
        Self {
            mean: T::ZERO,
            variance: T::ZERO,
            min_val: T::MAX,
            max_val: T::MIN,
            count: 0,
        }
    }
}

impl<T: Sample> DistributionSummary<T> {
    /// Updates the distribution summary. If the sample is *not valid* (i.e.
    /// [`Sample::is_valid`] returns `false`), no updates are performed.
    pub fn push(&mut self, sample: T) {
        if sample.is_valid() {
            if self.min_val > sample {
                self.min_val = sample;
            }
            if self.max_val < sample {
                self.max_val = sample;
            }
            let (variance, mean) = T::variance(self.variance, self.mean, self.count, sample);
            self.mean = mean;
            self.variance = variance;
            self.count += 1;
        }
    }
}

/// `StreamSummary` is converted into `protobuf::call_summary::StreamSummary`
/// and serialized into the call telemetry blob.
#[derive(Debug, Default)]
struct StreamSummary {
    bitrate: DistributionSummary<f32>,
    packet_loss: DistributionSummary<f32>,
    jitter: DistributionSummary<f32>,
    freeze_count: DistributionSummary<f32>,
}

/// `StreamSummaries` is converted into `protobuf::call_summary::StreamSummaries`
/// and serialized into the call telemetry blob.
#[derive(Debug, Default)]
struct StreamSummaries {
    audio_recv_stream_summaries: HashMap<u32, StreamSummary>,
    audio_send_stream_summaries: HashMap<u32, StreamSummary>,
    video_recv_stream_summaries: HashMap<u32, StreamSummary>,
    video_send_stream_summaries: HashMap<u32, StreamSummary>,
}

impl StreamSummaries {
    fn update_audio_recv_stream_summary<F>(&mut self, ssrc: u32, mut func: F)
    where
        F: FnMut(&mut StreamSummary),
    {
        if self.audio_recv_stream_summaries.len() <= MAX_STREAM_SUMMARIES {
            let summary = self.audio_recv_stream_summaries.entry(ssrc).or_default();
            func(summary);
        } else if let Some(summary) = self.audio_recv_stream_summaries.get_mut(&ssrc) {
            func(summary);
        }
    }

    fn update_audio_send_stream_summary<F>(&mut self, ssrc: u32, mut func: F)
    where
        F: FnMut(&mut StreamSummary),
    {
        if self.audio_send_stream_summaries.len() <= MAX_STREAM_SUMMARIES {
            let summary = self.audio_send_stream_summaries.entry(ssrc).or_default();
            func(summary);
        } else if let Some(summary) = self.audio_send_stream_summaries.get_mut(&ssrc) {
            func(summary);
        }
    }

    fn update_video_recv_stream_summary<F>(&mut self, ssrc: u32, mut func: F)
    where
        F: FnMut(&mut StreamSummary),
    {
        if self.video_recv_stream_summaries.len() <= MAX_STREAM_SUMMARIES {
            let summary = self.video_recv_stream_summaries.entry(ssrc).or_default();
            func(summary);
        } else if let Some(summary) = self.video_recv_stream_summaries.get_mut(&ssrc) {
            func(summary);
        }
    }

    fn update_video_send_stream_summary<F>(&mut self, ssrc: u32, mut func: F)
    where
        F: FnMut(&mut StreamSummary),
    {
        if self.video_send_stream_summaries.len() <= MAX_STREAM_SUMMARIES {
            let summary = self.video_send_stream_summaries.entry(ssrc).or_default();
            func(summary);
        } else if let Some(summary) = self.video_send_stream_summaries.get_mut(&ssrc) {
            func(summary);
        }
    }

    fn to_proto(&self) -> protobuf::call_summary::StreamSummaries {
        let audio_send_stream_summaries = self
            .audio_send_stream_summaries
            .iter()
            .map(|(ssrc, summary)| (*ssrc, summary.into()))
            .collect::<HashMap<u32, protobuf::call_summary::StreamSummary>>();
        let audio_recv_stream_summaries = self
            .audio_recv_stream_summaries
            .iter()
            .map(|(ssrc, summary)| (*ssrc, summary.into()))
            .collect::<HashMap<u32, protobuf::call_summary::StreamSummary>>();
        let video_send_stream_summaries = self
            .video_send_stream_summaries
            .iter()
            .map(|(ssrc, summary)| (*ssrc, summary.into()))
            .collect::<HashMap<u32, protobuf::call_summary::StreamSummary>>();
        let video_recv_stream_summaries = self
            .video_recv_stream_summaries
            .iter()
            .map(|(ssrc, summary)| (*ssrc, summary.into()))
            .collect::<HashMap<u32, protobuf::call_summary::StreamSummary>>();
        protobuf::call_summary::StreamSummaries {
            audio_send_stream_summaries,
            audio_recv_stream_summaries,
            video_send_stream_summaries,
            video_recv_stream_summaries,
        }
    }
}

/// `StatsSets` is a ring buffer used to capture stats sets. In other words,
/// once we reach the maximum number of stats sets, the oldest set will be
/// dropped to accommodate a new one.
#[derive(Debug)]
struct StatsSets {
    stats_sets: VecDeque<protobuf::call_summary::StatsSet>,
    max_stats_sets: usize,
}

impl Default for StatsSets {
    fn default() -> Self {
        Self {
            stats_sets: Default::default(),
            max_stats_sets: DEFAULT_MAX_STATS_SETS,
        }
    }
}

impl StatsSets {
    fn new(time_limit: Duration, stats_period: Duration) -> Result<Self> {
        let max_stats_sets = Self::calculate_max_stats_sets(time_limit, stats_period)?;
        Ok(Self {
            stats_sets: Default::default(),
            max_stats_sets,
        })
    }

    fn establish_current_stats_set(&mut self) {
        if self.stats_sets.len() == self.max_stats_sets {
            self.stats_sets.pop_front();
        }
        self.stats_sets.push_back(protobuf::call_summary::StatsSet {
            timestamp: Timestamp::now().map(Into::into),
            ..Default::default()
        });
    }

    fn calculate_max_stats_sets(time_limit: Duration, stats_period: Duration) -> Result<usize> {
        let time_limit_millis = time_limit.as_millis();
        let stats_period_millis = stats_period.as_millis();
        if time_limit_millis == 0 || stats_period_millis == 0 {
            Err(anyhow!("time limit/stats period must be greater than 0 ms"))
        } else if time_limit_millis <= stats_period_millis {
            Err(anyhow!("time limit too short for stats period"))
        } else {
            Ok((time_limit_millis / stats_period_millis) as usize)
        }
    }

    fn update_limits(&mut self, time_limit: Duration, stats_period: Duration) -> Result<()> {
        let max_stats_sets = Self::calculate_max_stats_sets(time_limit, stats_period)?;
        // Truncate the deque, if necessary.
        if max_stats_sets < self.stats_sets.len() {
            self.stats_sets
                .rotate_left(self.stats_sets.len() - max_stats_sets);
            self.stats_sets.truncate(max_stats_sets);
        }
        self.max_stats_sets = max_stats_sets;
        Ok(())
    }

    fn push_audio_recv_stream_stats(&mut self, stats: StreamStats) {
        let last = self.stats_sets.len() - 1;
        self.stats_sets[last].audio_recv_stats.push(stats);
    }

    fn push_audio_send_stream_stats(&mut self, stats: StreamStats) {
        let last = self.stats_sets.len() - 1;
        self.stats_sets[last].audio_send_stats.push(stats);
    }

    fn push_video_recv_stream_stats(&mut self, stats: StreamStats) {
        let last = self.stats_sets.len() - 1;
        self.stats_sets[last].video_recv_stats.push(stats);
    }

    fn push_video_send_stream_stats(&mut self, stats: StreamStats) {
        let last = self.stats_sets.len() - 1;
        self.stats_sets[last].video_send_stats.push(stats);
    }

    fn push_event(&mut self, event: Event) {
        let last = self.stats_sets.len() - 1;
        self.stats_sets[last].events.push(event.into());
    }

    fn push_stun_rtt(&mut self, rtt: f32) {
        let last = self.stats_sets.len() - 1;
        self.stats_sets[last].rtt_stun.push(rtt);
    }

    fn to_proto(&self) -> Vec<protobuf::call_summary::StatsSet> {
        Vec::from(self.stats_sets.clone())
    }
}

impl From<&AudioReceiverStatsSnapshot> for StreamStats {
    fn from(snapshot: &AudioReceiverStatsSnapshot) -> Self {
        Self {
            ssrc: Some(snapshot.ssrc),
            bitrate: Some(snapshot.bitrate),
            packet_loss: Some(snapshot.packets_lost_pct),
            jitter: Some(snapshot.jitter as f32),
            jitter_buffer_delay: Some(snapshot.jitter_buffer_delay as f32),
            ..Default::default()
        }
    }
}

impl From<&AudioSenderStatsSnapshot> for StreamStats {
    fn from(snapshot: &AudioSenderStatsSnapshot) -> Self {
        Self {
            ssrc: Some(snapshot.ssrc),
            bitrate: Some(snapshot.bitrate),
            packet_loss: Some(snapshot.remote_packets_lost_pct),
            jitter: Some(snapshot.remote_jitter as f32),
            rtt: Some(snapshot.remote_rtt as f32),
            ..Default::default()
        }
    }
}

impl From<&VideoReceiverStatsSnapshot> for StreamStats {
    fn from(snapshot: &VideoReceiverStatsSnapshot) -> Self {
        Self {
            ssrc: Some(snapshot.ssrc),
            bitrate: Some(snapshot.bitrate),
            packet_loss: Some(snapshot.packets_lost_pct),
            jitter: Some(snapshot.jitter as f32),
            framerate: Some(snapshot.framerate),
            ..Default::default()
        }
    }
}

impl From<&VideoSenderStatsSnapshot> for StreamStats {
    fn from(snapshot: &VideoSenderStatsSnapshot) -> Self {
        Self {
            ssrc: Some(snapshot.ssrc),
            bitrate: Some(snapshot.bitrate),
            packet_loss: Some(snapshot.remote_packets_lost_pct),
            jitter: Some(snapshot.remote_jitter as f32),
            rtt: Some(snapshot.remote_round_trip_time as f32),
            framerate: Some(snapshot.framerate),
            ..Default::default()
        }
    }
}

impl CallTelemetry {
    /// Generates the "opaque" telemetry blob and the corresponding description
    /// string that is suitable for being shown to the user. This implementation
    /// will serialize the telemetry BLOB to a JSON string so that it can easily
    /// be ingested by user provided tools.
    ///
    /// This method will attempt to prune the telemetry before serializing
    /// it into a blob. See [`CallTelemetry::prune_if_too_large`] for more
    /// information.
    fn generate_blob_and_description(&mut self) -> (Option<Vec<u8>>, Option<String>) {
        if let Err(e) = self.prune_if_too_large() {
            warn!("Call summary construction failure: {e}");
            (None, None)
        } else {
            let blob = self.encode_to_vec();
            let text = serde_json::to_string(&self).ok();
            (Some(blob), text)
        }
    }

    /// If the encoded length of the telemtry message is too large (larger
    /// than MAX_TELEMETRY_ENCODED_SIZE bytes), this method will remove stats
    /// sets, one by one, starting with the oldest stats set, until the encoded
    /// message size is less than or equal to MAX_TELEMETRY_ENCODED_SIZE bytes.
    ///
    /// In the unlikely event that the complete removal of the recorded stats
    /// sets does not bring the encoded size below the acceptable maximum, the
    /// entire call summary will be removed.
    ///
    /// Once the encoded size of the message is within acceptable limits, this
    /// method will return `Ok`.
    ///
    /// Under normal circumstances, this method is guaranteed to bring the encoded
    /// size down below MAX_TELEMETRY_ENCODED_SIZE. If that is still not the
    /// case, `Err` is returned. The content of the telemetry message will *not*
    /// be preserved.
    fn prune_if_too_large(&mut self) -> Result<usize> {
        let stats_set_count = self.stats_sets.len();
        while !self.stats_sets.is_empty() {
            if self.encoded_len() <= MAX_TELEMETRY_ENCODED_SIZE {
                return Ok(stats_set_count - self.stats_sets.len());
            }
            self.stats_sets.remove(0);
        }

        self.group_call_summary = None;
        self.direct_call_summary = None;

        if self.encoded_len() <= MAX_TELEMETRY_ENCODED_SIZE {
            Ok(stats_set_count)
        } else {
            Err(anyhow!("Telemetry message large after pruning."))
        }
    }
}

impl DistributionSummary<f32> {
    /// Converts a `DistributionSummary` into its protobuf counterpart. In
    /// contrast to the `From` implementation, this method will return `None` if
    /// the distribution summary is empty (i.e. sample count is 0).
    fn to_proto(&self) -> Option<protobuf::call_summary::DistributionSummary> {
        if self.count == 0 {
            None
        } else {
            Some(self.into())
        }
    }
}

impl From<&StreamSummary> for protobuf::call_summary::StreamSummary {
    fn from(summary: &StreamSummary) -> Self {
        Self {
            bitrate: summary.bitrate.to_proto(),
            packet_loss_pct: summary.packet_loss.to_proto(),
            jitter: summary.jitter.to_proto(),
            freeze_count: summary.freeze_count.to_proto(),
        }
    }
}

impl From<&DistributionSummary<f32>> for protobuf::call_summary::DistributionSummary {
    fn from(stats: &DistributionSummary<f32>) -> Self {
        Self {
            mean: Some(stats.mean),
            std_dev: Some(stats.variance.sqrt()),
            min_val: Some(stats.min_val),
            max_val: Some(stats.max_val),
            sample_count: Some(stats.count as u32),
        }
    }
}

/// Simple wrapper around DDSketch.
struct QuantileSketch(DDSketch);

impl Default for QuantileSketch {
    fn default() -> Self {
        Self(DDSketch::new(Config::defaults()))
    }
}

impl Debug for QuantileSketch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.length())
    }
}

impl QuantileSketch {
    fn add(&mut self, sample: f64) {
        self.0.add(sample);
    }

    fn quantile(&self, quantile: f64) -> Result<Option<f64>> {
        self.0
            .quantile(quantile)
            .map_err(|e| anyhow!("Sketch error: {e}"))
    }
}

#[derive(Debug, Default)]
struct MediaQualityStatsSketches {
    jitter_send: QuantileSketch,
    jitter_recv: QuantileSketch,
    packet_loss_send: QuantileSketch,
    packet_loss_recv: QuantileSketch,
    rtt: QuantileSketch,
}

#[derive(Debug, Default)]
struct QualityStatsSketches {
    stun_rtt: QuantileSketch,
    audio: MediaQualityStatsSketches,
    video: MediaQualityStatsSketches,
}

impl From<&QualityStatsSketches> for QualityStats {
    fn from(sketches: &QualityStatsSketches) -> Self {
        fn get_median(sketch: &QuantileSketch) -> Option<f32> {
            sketch.quantile(0.5).unwrap_or_default().map(|v| v as f32)
        }

        QualityStats {
            rtt_median_connection: get_median(&sketches.stun_rtt),
            audio_stats: MediaQualityStats {
                rtt_median: get_median(&sketches.audio.rtt),
                jitter_median_send: get_median(&sketches.audio.jitter_send),
                jitter_median_recv: get_median(&sketches.audio.jitter_recv),
                packet_loss_fraction_send: get_median(&sketches.audio.packet_loss_send)
                    .map(|v| v / 100.0),
                packet_loss_fraction_recv: get_median(&sketches.audio.packet_loss_recv)
                    .map(|v| v / 100.0),
            },
            video_stats: MediaQualityStats {
                rtt_median: get_median(&sketches.video.rtt),
                jitter_median_send: get_median(&sketches.video.jitter_send),
                jitter_median_recv: get_median(&sketches.video.jitter_recv),
                packet_loss_fraction_send: get_median(&sketches.video.packet_loss_send)
                    .map(|v| v / 100.0),
                packet_loss_fraction_recv: get_median(&sketches.video.packet_loss_recv)
                    .map(|v| v / 100.0),
            },
        }
    }
}

#[derive(Debug)]
struct CallInfo {
    start_time: SystemTime,
    connect_time: Option<SystemTime>,
    stream_summaries: StreamSummaries,
    // This value is set to true if the data was being exchanged over the
    // cellular interface at least at one point in the call.
    cellular: bool,
    // Time series stats sets. The total number of direct call state sets is
    // capped and may not (probably will not) contain the stats sets for the
    // entire call.
    stats_sets: StatsSets,
    needs_stats_set: bool,
    // Quantile sketches
    quality_stats_sketches: QualityStatsSketches,
}

impl Default for CallInfo {
    fn default() -> Self {
        Self {
            start_time: SystemTime::now(),
            connect_time: None,
            stream_summaries: Default::default(),
            cellular: false,
            stats_sets: Default::default(),
            needs_stats_set: false,
            quality_stats_sketches: Default::default(),
        }
    }
}

impl CallInfo {
    fn on_event(&mut self, event: Event) {
        if self.needs_stats_set {
            self.stats_sets.establish_current_stats_set();
            self.needs_stats_set = false;
        }
        self.stats_sets.push_event(event);
    }

    fn set_connect_time(&mut self) {
        if self.connect_time.is_none() {
            self.connect_time = Some(SystemTime::now());
        }
    }

    fn on_stats_snapshot_ready(&mut self, stats: &StatsSnapshot) {
        if self.needs_stats_set {
            self.stats_sets.establish_current_stats_set();
            self.needs_stats_set = false;
        }
        match stats {
            StatsSnapshot::Begin => {
                // Ignored
            }
            #[cfg(not(target_os = "android"))]
            StatsSnapshot::System(_) => {
                // Ignored
            }
            StatsSnapshot::End => {
                self.needs_stats_set = true;
            }
            StatsSnapshot::Connection(snapshot) => {
                self.quality_stats_sketches
                    .stun_rtt
                    .add(snapshot.current_round_trip_time);
                self.stats_sets
                    .push_stun_rtt(snapshot.current_round_trip_time as f32);
            }
            StatsSnapshot::AudioSender(snapshot) => {
                self.quality_stats_sketches
                    .audio
                    .jitter_send
                    .add(snapshot.remote_jitter);
                self.quality_stats_sketches
                    .audio
                    .packet_loss_send
                    .add(snapshot.remote_packets_lost_pct as f64);
                self.quality_stats_sketches
                    .audio
                    .rtt
                    .add(snapshot.remote_rtt);
                self.stream_summaries
                    .update_audio_send_stream_summary(snapshot.ssrc, |summary| {
                        summary.bitrate.push(snapshot.bitrate);
                        summary.packet_loss.push(snapshot.remote_packets_lost_pct);
                        summary.jitter.push(snapshot.remote_jitter as f32);
                    });
                self.stats_sets
                    .push_audio_send_stream_stats(snapshot.into());
            }
            StatsSnapshot::AudioReceiver(snapshot) => {
                self.quality_stats_sketches
                    .audio
                    .jitter_recv
                    .add(snapshot.jitter);
                self.quality_stats_sketches
                    .audio
                    .packet_loss_recv
                    .add(snapshot.packets_lost_pct as f64);
                self.stream_summaries
                    .update_audio_recv_stream_summary(snapshot.ssrc, |summary| {
                        summary.bitrate.push(snapshot.bitrate);
                        summary.packet_loss.push(snapshot.packets_lost_pct);
                        summary.jitter.push(snapshot.jitter as f32);
                    });
                self.stats_sets
                    .push_audio_recv_stream_stats(snapshot.into());
            }
            StatsSnapshot::VideoSender(snapshot) => {
                self.quality_stats_sketches
                    .video
                    .jitter_send
                    .add(snapshot.remote_jitter);
                self.quality_stats_sketches
                    .video
                    .packet_loss_send
                    .add(snapshot.remote_packets_lost_pct as f64);
                self.quality_stats_sketches
                    .video
                    .rtt
                    .add(snapshot.remote_round_trip_time);
                self.stream_summaries
                    .update_video_send_stream_summary(snapshot.ssrc, |summary| {
                        summary.bitrate.push(snapshot.bitrate);
                        summary.packet_loss.push(snapshot.remote_packets_lost_pct);
                        summary.jitter.push(snapshot.remote_jitter as f32);
                    });
                self.stats_sets
                    .push_video_send_stream_stats(snapshot.into());
            }
            StatsSnapshot::VideoReceiver(snapshot) => {
                self.quality_stats_sketches
                    .video
                    .jitter_recv
                    .add(snapshot.jitter);
                self.quality_stats_sketches
                    .video
                    .packet_loss_recv
                    .add(snapshot.packets_lost_pct as f64);
                self.stream_summaries
                    .update_video_recv_stream_summary(snapshot.ssrc, |summary| {
                        summary.bitrate.push(snapshot.bitrate);
                        summary.packet_loss.push(snapshot.packets_lost_pct);
                        summary.jitter.push(snapshot.jitter as f32);
                        summary.freeze_count.push(snapshot.freeze_count as f32);
                    });
                self.stats_sets
                    .push_video_recv_stream_stats(snapshot.into());
            }
        }
    }
}

/// [`DirectCallSummary`] consumes data from the stats collector and from the
/// direct call infrastructure. The data is used to generate a [`CallSummary`]
/// instance that is passed to the upper layers at the end of each call.
#[derive(Debug, Clone)]
pub struct DirectCallSummary(Arc<CallMutex<DirectCallSummaryInner>>);

impl Default for DirectCallSummary {
    fn default() -> Self {
        let call_info = CallInfo {
            needs_stats_set: true,
            ..Default::default()
        };
        Self(Arc::new(CallMutex::new(
            DirectCallSummaryInner {
                call_info,
                ..Default::default()
            },
            Self::CALL_MUTEX_NAME,
        )))
    }
}

impl DirectCallSummary {
    const CALL_MUTEX_NAME: &str = "direct-call-summary-mutex";

    /// Creates a new [`DirectCallSummary`]. `time_limit` controls the maximum
    /// amount of stats data that will be retained and included in the telemetry
    /// structure. Once this limit is exceeded, the older data will be purged
    /// to accommodate new data. `stats_period` specifies the stats collection
    /// frequency. This function will fail if `time_limit` is shorter than
    /// `stats_period`.
    pub fn new(time_limit: Duration, stats_period: Duration) -> Result<Self> {
        let stats_sets = StatsSets::new(time_limit, stats_period)?;
        let call_info = CallInfo {
            stats_sets,
            needs_stats_set: true,
            ..Default::default()
        };
        let result = Self(Arc::new(CallMutex::new(
            DirectCallSummaryInner {
                call_info,
                ..Default::default()
            },
            Self::CALL_MUTEX_NAME,
        )));
        Ok(result)
    }

    fn lock(&self) -> Result<MutexGuard<'_, DirectCallSummaryInner>> {
        self.0.lock()
    }

    /// Updates stats capture limits. `time_limit` controls the maximum amount
    /// of stats data that will be retained and included in the telemetry
    /// structure. Once this limit is exceeded, the older data will be purged
    /// to accommodate new data. `stats_period` specifies the stats collection
    /// frequency. This function will fail if `time_limit` is shorter than
    /// `stats_period`.
    pub fn update_limits(&self, time_limit: Duration, stats_period: Duration) -> Result<()> {
        match self.lock() {
            Ok(mut guard) => guard.update_limits(time_limit, stats_period),
            Err(error) => {
                error!("Failed to update stats collector limits: {:?}", error);
                Ok(())
            }
        }
    }

    /// Processes a call event.
    pub fn on_call_event(&self, state: CallState, event: &CallEvent) {
        match self.lock() {
            Ok(mut guard) => guard.on_call_event(state, event),
            Err(error) => {
                error!("Failed to process call event: {:?}", error);
            }
        }
    }

    pub fn build_call_summary(&self, reason: CallEndReason) -> CallSummary {
        match self.lock() {
            Ok(guard) => guard.build_call_summary(reason),
            Err(error) => {
                error!("Failed to build call summary: {:?}", error);
                CallSummary::default()
            }
        }
    }

    /// Processes a statistics snapshot.
    pub fn on_stats_snapshot_ready(&self, stats: &StatsSnapshot) {
        match self.lock() {
            Ok(mut guard) => guard.on_stats_snapshot_ready(stats),
            Err(error) => {
                error!("Failed to process stats snapshot: {:?}", error);
            }
        }
    }

    pub fn as_stats_consumer(&self) -> Box<dyn StatsSnapshotConsumer> {
        Box::new(DirectCallStatsSnapshotConsumer(self.clone()))
    }
}

/// Call stats snapshot consumer delegate. Implements the
/// [`StatsSnapshotConsumer`] interface and delegates calls to the wrapped
/// [`DirectCallSummary`] instance.
#[derive(Debug)]
struct DirectCallStatsSnapshotConsumer(DirectCallSummary);

impl StatsSnapshotConsumer for DirectCallStatsSnapshotConsumer {
    fn on_stats_snapshot_ready(&self, stats: &StatsSnapshot) {
        self.0.on_stats_snapshot_ready(stats);
    }
}

#[derive(Debug, Default)]
struct DirectCallSummaryInner {
    call_info: CallInfo,
    relayed: bool,
    ice_candidate_switch_count: u32,
    ice_reconnect_count: u32,
}

impl DirectCallSummaryInner {
    pub fn update_limits(&mut self, time_limit: Duration, stats_period: Duration) -> Result<()> {
        self.call_info
            .stats_sets
            .update_limits(time_limit, stats_period)
    }

    fn build_call_summary(&self, reason: CallEndReason) -> CallSummary {
        let connect_time = self
            .call_info
            .connect_time
            .and_then(|sys_time| Timestamp::from_system_time(&sys_time));

        let start_time =
            Timestamp::from_system_time(&self.call_info.start_time).unwrap_or_else(|| {
                warn!(
                    "Failed to create start timestamp {:?}",
                    self.call_info.start_time
                );
                Timestamp(0)
            });

        let now = SystemTime::now();
        let end_time = Timestamp::from_system_time(&now).unwrap_or_else(|| {
            warn!("Failed to create end timestamp: {:?}", now);
            Timestamp(0)
        });

        let mut telemetry = CallTelemetry {
            version: CALL_TELEMETRY_VERSION,
            start_time: start_time.into(),
            connect_time: connect_time.map(Into::into),
            end_time: end_time.into(),
            cellular: Some(self.call_info.cellular),
            direct_call_summary: Some(protobuf::call_summary::DirectCallSummary {
                stream_summaries: Some(self.call_info.stream_summaries.to_proto()),
                ice_candidate_switch_count: Some(self.ice_candidate_switch_count),
                ice_reconnect_count: Some(self.ice_reconnect_count),
                relayed: Some(self.relayed),
            }),
            stats_sets: self.call_info.stats_sets.to_proto(),
            ..Default::default()
        };

        let (raw_stats, raw_stats_text) = telemetry.generate_blob_and_description();

        let quality_stats = (&self.call_info.quality_stats_sketches).into();

        // If the call connected then we present the connect time as the start
        // time to the upper layers since that will enable them to correctly
        // measure the duration of the call. On the other hand, if the call
        // never connected we set the end time equal to the start time.
        let (start_time, end_time) = match connect_time {
            Some(connect_time) => (connect_time, end_time),
            _ => (start_time, start_time),
        };

        // Any call that connected at some point is a survey candidate, unless
        // it is a call explicitly dropped by the app.
        let is_survey_candidate = self.call_info.connect_time.is_some()
            && reason != CallEndReason::AppDroppedCall
            && reason != CallEndReason::RemoteReCall;

        CallSummary {
            start_time,
            end_time,
            quality_stats,
            raw_stats,
            raw_stats_text,
            is_survey_candidate,
            call_end_reason_text: reason.to_string(),
        }
    }

    fn process_connection_observer_event(
        &mut self,
        state: CallState,
        event: &ConnectionObserverEvent,
    ) {
        match event {
            ConnectionObserverEvent::IceConnected => {
                self.call_info.on_event(Event::IceConnected);
            }
            ConnectionObserverEvent::IceDisconnected => {
                self.call_info.on_event(Event::IceDisconnected);
            }
            ConnectionObserverEvent::IceNetworkRouteChanged(route) => {
                self.call_info.on_event(Event::IceNetworkRouteChanged);
                self.ice_candidate_switch_count += 1;
                if !self.relayed && (route.local_relayed || route.remote_relayed) {
                    self.relayed = true;
                }
                if !self.call_info.cellular && route.local_adapter_type.is_cellular() {
                    self.call_info.cellular = true;
                }
            }
            ConnectionObserverEvent::StateChanged(connection_state) => {
                match (state, connection_state) {
                    (
                        CallState::ConnectedBeforeAccepted | CallState::ConnectedAndAccepted,
                        ConnectionState::ConnectedAndAccepted,
                    ) => {
                        self.call_info.set_connect_time();
                    }
                    (
                        CallState::ConnectedAndAccepted,
                        ConnectionState::ReconnectingAfterAccepted,
                    ) => {
                        self.ice_reconnect_count += 1;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn on_call_event(&mut self, state: CallState, event: &CallEvent) {
        if let CallEvent::ConnectionObserverEvent(event, _) = event {
            self.process_connection_observer_event(state, event);
        }
    }

    fn on_stats_snapshot_ready(&mut self, stats: &StatsSnapshot) {
        self.call_info.on_stats_snapshot_ready(stats);
    }
}

type GroupCallConnectionState = crate::core::group_call::ConnectionState;

/// [`GroupCallSummary`] consumes data from the stats collector and from the
/// group call infrastructure. The data is used to generate a [`CallSummary`]
/// instance that is passed to the upper layers at the end of each call.
#[derive(Debug, Clone)]
pub struct GroupCallSummary(Arc<CallMutex<GroupCallSummaryInner>>);

impl GroupCallSummary {
    const CALL_MUTEX_NAME: &str = "group-call-summary-mutex";

    /// Creates a new [`GroupCallSummary`]. `time_limit` controls the maximum
    /// amount of stats data that will be retained and included in the telemetry
    /// structure. Once this limit is exceeded, the older data will be purged
    /// to accommodate new data. `stats_period` specifies the stats collection
    /// frequency. This function will fail if `time_limit` is shorter than
    /// `stats_period`.
    pub fn new(time_limit: Duration, stats_period: Duration) -> Result<Self> {
        let stats_sets = StatsSets::new(time_limit, stats_period)?;
        let call_info = CallInfo {
            stats_sets,
            needs_stats_set: true,
            ..Default::default()
        };
        let result = Self(Arc::new(CallMutex::new(
            GroupCallSummaryInner {
                call_info,
                ..Default::default()
            },
            Self::CALL_MUTEX_NAME,
        )));
        Ok(result)
    }

    fn lock(&self) -> Result<MutexGuard<'_, GroupCallSummaryInner>> {
        self.0.lock()
    }

    /// Handles ICE network route changes.
    pub fn on_ice_network_route_changed(&self, route: NetworkRoute) {
        match self.lock() {
            Ok(mut guard) => guard.on_ice_network_route_changed(route),
            Err(error) => {
                error!("Failed to process network route change: {:?}", error);
            }
        }
    }

    /// Handles connection state change events.
    pub fn on_connection_state_changed(&self, state: GroupCallConnectionState) {
        match self.lock() {
            Ok(mut guard) => guard.on_connection_state_changed(state),
            Err(error) => {
                error!("Failed to process connection state change: {:?}", error);
            }
        }
    }

    pub fn on_remote_devices_changed(&self, remote_devices: &[RemoteDeviceState]) {
        match self.lock() {
            Ok(mut guard) => guard.on_remote_devices_changed(remote_devices),
            Err(error) => {
                error!("Failed to process remote devices change: {:?}", error);
            }
        }
    }

    pub fn build_call_summary(&self, reason: CallEndReason) -> CallSummary {
        match self.lock() {
            Ok(guard) => guard.build_call_summary(reason),
            Err(error) => {
                error!("Failed to build call summary: {:?}", error);
                CallSummary::default()
            }
        }
    }

    /// Processes a statistics snapshot.
    pub fn on_stats_snapshot_ready(&self, stats: &StatsSnapshot) {
        match self.lock() {
            Ok(mut guard) => guard.on_stats_snapshot_ready(stats),
            Err(error) => {
                error!("Failed to process stats snapshot: {:?}", error);
            }
        }
    }

    pub fn as_stats_consumer(&self) -> Box<dyn StatsSnapshotConsumer> {
        Box::new(GroupCallStatsSnapshotConsumer(self.clone()))
    }
}

/// Call stats snapshot consumer delegate. Implements the
/// [`StatsSnapshotConsumer`] interface and delegates calls to the wrapped
/// [`GroupCallSummary`] instance.
#[derive(Debug)]
struct GroupCallStatsSnapshotConsumer(GroupCallSummary);

impl StatsSnapshotConsumer for GroupCallStatsSnapshotConsumer {
    fn on_stats_snapshot_ready(&self, stats: &StatsSnapshot) {
        self.0.on_stats_snapshot_ready(stats);
    }
}

#[derive(Debug, Default)]
pub struct GroupCallSummaryInner {
    call_info: CallInfo,
    remote_device_count_max: usize,
}

impl GroupCallSummaryInner {
    fn build_call_summary(&self, reason: CallEndReason) -> CallSummary {
        let connect_time = self
            .call_info
            .connect_time
            .and_then(|sys_time| Timestamp::from_system_time(&sys_time));

        let start_time =
            Timestamp::from_system_time(&self.call_info.start_time).unwrap_or_else(|| {
                warn!(
                    "Failed to create start timestamp for {:?}",
                    self.call_info.start_time
                );
                Timestamp(0)
            });

        let now = SystemTime::now();
        let end_time = Timestamp::from_system_time(&now).unwrap_or_else(|| {
            warn!("Failed to create end timestamp: {:?}", now);
            Timestamp(0)
        });

        let mut telemetry = CallTelemetry {
            version: CALL_TELEMETRY_VERSION,
            start_time: start_time.into(),
            connect_time: connect_time.map(Into::into),
            end_time: end_time.into(),
            cellular: Some(self.call_info.cellular),
            group_call_summary: Some(protobuf::call_summary::GroupCallSummary {
                stream_summaries: Some(self.call_info.stream_summaries.to_proto()),
            }),
            stats_sets: self.call_info.stats_sets.to_proto(),
            ..Default::default()
        };

        let (raw_stats, raw_stats_text) = telemetry.generate_blob_and_description();

        let quality_stats = (&self.call_info.quality_stats_sketches).into();

        // If the call connected then we present the connect time as the start
        // time to the upper layers since that will enable them to correctly
        // measure the duration of the call. On the other hand, if the call
        // never connected we set the end time equal to the start time.
        let (start_time, end_time) = match connect_time {
            Some(connect_time) => (connect_time, end_time),
            _ => (start_time, start_time),
        };

        // Any call that connected at some point and if the number of
        // participants in the call exceeded 1 at any point during the call is a
        // potential survey candidate.
        let is_survey_candidate = raw_stats.is_some()
            && self.call_info.connect_time.is_some()
            && self.remote_device_count_max > 0;

        CallSummary {
            start_time,
            end_time,
            quality_stats,
            raw_stats,
            raw_stats_text,
            is_survey_candidate,
            call_end_reason_text: reason.to_string(),
        }
    }

    fn on_connection_state_changed(&mut self, state: GroupCallConnectionState) {
        let event = match state {
            GroupCallConnectionState::NotConnected => Event::GroupCallDisconnected,
            GroupCallConnectionState::Connected => {
                // Only update the connect time if it has not been updated
                // already. Any connects that occur after the initial connect
                // are, in fact, reconnects.
                self.call_info.set_connect_time();
                Event::GroupCallConnected
            }
            GroupCallConnectionState::Reconnecting => Event::GroupCallReconnecting,
            GroupCallConnectionState::Connecting => Event::GroupCallConnecting,
        };
        self.call_info.on_event(event);
    }

    fn on_ice_network_route_changed(&mut self, route: NetworkRoute) {
        if !self.call_info.cellular && route.local_adapter_type.is_cellular() {
            self.call_info.cellular = true;
        }
    }

    fn on_stats_snapshot_ready(&mut self, stats: &StatsSnapshot) {
        self.call_info.on_stats_snapshot_ready(stats);
    }

    fn on_remote_devices_changed(&mut self, remote_devices: &[RemoteDeviceState]) {
        if remote_devices.len() > self.remote_device_count_max {
            self.remote_device_count_max = remote_devices.len();
        }
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashMap, time::Duration};

    use prost::Message;
    use rand::Rng;

    use crate::{
        core::call_summary::{MAX_TELEMETRY_ENCODED_SIZE, Sample, StatsSets},
        protobuf,
    };

    fn within(expected: f32, calculated: f32, percentage: f32) -> bool {
        let v = (expected * percentage).abs();
        (expected - calculated).abs() < v
    }

    #[test]
    fn test_welford() {
        let mut rng = rand::thread_rng();
        let samples: Vec<f32> = (0..1000)
            .map(|_| rng.gen_range(-50000.0..50000.0))
            .collect();

        let mean_0 = samples.iter().sum::<f32>() / (samples.len() as f32);
        let variance_0 = samples
            .iter()
            .fold(0.0, |acc, x| acc + (x - mean_0) * (x - mean_0))
            / (samples.len() as f32);

        let (variance_1, mean_1) = samples
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(variance, mean), (i, sample)| {
                f32::variance(variance, mean, i, *sample)
            });

        println!(
            "mean: {mean_0}, {mean_1}, {}%",
            ((mean_0 - mean_1).abs() / mean_0) * 100.0
        );
        println!(
            "variance: {variance_0}, {variance_1}, {}%",
            ((variance_0 - variance_1).abs() / variance_0) * 100.0
        );

        assert!(within(mean_0, mean_1, 0.01));
        assert!(within(variance_0, variance_1, 0.01));
    }

    fn create_stats_sets(
        participant_count: usize,
        call_length: Duration,
        time_limit: Duration,
        stats_period: Duration,
    ) -> Vec<protobuf::call_summary::StatsSet> {
        let mut stats_sets = StatsSets::new(time_limit, stats_period).unwrap();
        let count = call_length.as_secs() / stats_period.as_secs();
        for _ in 0..count {
            stats_sets.establish_current_stats_set();
            // Three outbound video streams
            for ssrc in 0..3 {
                stats_sets.push_video_send_stream_stats(protobuf::call_summary::StreamStats {
                    ssrc: Some(ssrc),
                    bitrate: Some(1000.0),
                    packet_loss: Some(50.0),
                    jitter: Some(20.0),
                    rtt: Some(5.0),
                    jitter_buffer_delay: Some(10.0),
                    framerate: Some(30.0),
                });
            }
            // One outbound audio stream
            stats_sets.push_audio_send_stream_stats(protobuf::call_summary::StreamStats {
                ssrc: Some(4),
                bitrate: Some(1000.0),
                packet_loss: Some(50.0),
                jitter: Some(20.0),
                rtt: Some(5.0),
                jitter_buffer_delay: Some(10.0),
                framerate: None,
            });
            // One inbound audio stream, and one inbound video stream for each participant
            for ssrc in 0..participant_count {
                stats_sets.push_audio_recv_stream_stats(protobuf::call_summary::StreamStats {
                    ssrc: Some((ssrc + 2000) as u32),
                    bitrate: Some(1000.0),
                    packet_loss: Some(50.0),
                    jitter: Some(20.0),
                    rtt: Some(5.0),
                    jitter_buffer_delay: Some(10.0),
                    framerate: None,
                });
                stats_sets.push_video_recv_stream_stats(protobuf::call_summary::StreamStats {
                    ssrc: Some((ssrc + 3000) as u32),
                    bitrate: Some(1000.0),
                    packet_loss: Some(50.0),
                    jitter: Some(20.0),
                    rtt: Some(5.0),
                    jitter_buffer_delay: Some(10.0),
                    framerate: Some(30.0),
                });
                stats_sets.push_stun_rtt(100.0);
            }
        }
        stats_sets.to_proto()
    }

    fn create_stream_summaries(
        count: usize,
    ) -> HashMap<u32, protobuf::call_summary::StreamSummary> {
        (0..count as u32)
            .map(|v| {
                (
                    v,
                    protobuf::call_summary::StreamSummary {
                        bitrate: Some(protobuf::call_summary::DistributionSummary {
                            mean: Some(45000.0),
                            std_dev: Some(45000.0),
                            min_val: Some(0.0),
                            max_val: Some(100000.0),
                            sample_count: Some(50000),
                        }),
                        packet_loss_pct: Some(protobuf::call_summary::DistributionSummary {
                            mean: Some(100.0),
                            std_dev: Some(0.0),
                            min_val: Some(100.0),
                            max_val: Some(100.0),
                            sample_count: Some(50000),
                        }),
                        jitter: Some(protobuf::call_summary::DistributionSummary {
                            mean: Some(20.0),
                            std_dev: Some(20.0),
                            min_val: Some(0.0),
                            max_val: Some(20.0),
                            sample_count: Some(50000),
                        }),
                        freeze_count: None,
                    },
                )
            })
            .collect::<HashMap<u32, protobuf::call_summary::StreamSummary>>()
    }

    struct CreateGroupCallParams {
        size: usize,
        call_length: Duration,
        time_limit: Duration,
        stats_period: Duration,
    }

    fn create_telemetry_for_group_call_with_size(
        params: CreateGroupCallParams,
    ) -> protobuf::call_summary::CallTelemetry {
        let CreateGroupCallParams {
            size,
            call_length,
            time_limit,
            stats_period,
        } = params;

        let stats_sets = create_stats_sets(size, call_length, time_limit, stats_period);

        let stream_summaries = protobuf::call_summary::StreamSummaries {
            audio_send_stream_summaries: create_stream_summaries(1),
            audio_recv_stream_summaries: create_stream_summaries(size),
            video_send_stream_summaries: create_stream_summaries(3),
            video_recv_stream_summaries: create_stream_summaries(size),
        };

        protobuf::call_summary::CallTelemetry {
            group_call_summary: Some(protobuf::call_summary::GroupCallSummary {
                stream_summaries: Some(stream_summaries),
            }),
            stats_sets,
            ..Default::default()
        }
    }

    #[test]
    fn test_telemetry_pruning_with_full_group_call() {
        let mut telemetry = create_telemetry_for_group_call_with_size(CreateGroupCallParams {
            size: 75,
            call_length: Duration::from_secs(86400),
            time_limit: Duration::from_secs(300),
            stats_period: Duration::from_secs(10),
        });

        println!("Telemetry size: {}", telemetry.encoded_len());
        println!(" -- Stats set count: {}", telemetry.stats_sets.len());

        assert!(telemetry.encoded_len() > MAX_TELEMETRY_ENCODED_SIZE);
        assert!(telemetry.prune_if_too_large().is_ok());
        assert!(telemetry.encoded_len() <= MAX_TELEMETRY_ENCODED_SIZE);

        println!("Telemetry size: {}", telemetry.encoded_len());
        println!(" -- Stats sets remaining: {}", telemetry.stats_sets.len());
    }

    #[test]
    fn test_telemetry_pruning_minimum_call_size_requiring_pruning() {
        let mut telemetry = create_telemetry_for_group_call_with_size(CreateGroupCallParams {
            size: 27,
            call_length: Duration::from_secs(86400),
            time_limit: Duration::from_secs(300),
            stats_period: Duration::from_secs(10),
        });

        println!("Telemetry size: {}", telemetry.encoded_len());
        println!(" -- Stats set count: {}", telemetry.stats_sets.len());

        assert!(telemetry.encoded_len() > MAX_TELEMETRY_ENCODED_SIZE);
        assert!(telemetry.prune_if_too_large().is_ok());
        assert!(telemetry.encoded_len() <= MAX_TELEMETRY_ENCODED_SIZE);

        println!("Telemetry size: {}", telemetry.encoded_len());
        println!(" -- Stats sets remaining: {}", telemetry.stats_sets.len());
    }

    #[test]
    fn test_telemetry_pruning_maximum_call_size_not_requiring_pruning() {
        let telemetry = create_telemetry_for_group_call_with_size(CreateGroupCallParams {
            size: 26,
            call_length: Duration::from_secs(86400),
            time_limit: Duration::from_secs(300),
            stats_period: Duration::from_secs(10),
        });

        println!("Telemetry size: {}", telemetry.encoded_len());
        println!(" -- Stats set count: {}", telemetry.stats_sets.len());

        assert!(telemetry.encoded_len() < MAX_TELEMETRY_ENCODED_SIZE);
    }
}
