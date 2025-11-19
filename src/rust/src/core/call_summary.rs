//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    fmt::Debug,
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign},
    sync::{Arc, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use prost::Message;

use crate::{
    common::{CallEndReason, CallState, ConnectionState, Result},
    core::{call_fsm::CallEvent, call_mutex::CallMutex, connection::ConnectionObserverEvent},
    protobuf::{
        self,
        call_summary::{CallTelemetry, Event, StreamStats},
    },
    webrtc::{
        peer_connection_observer::NetworkRoute,
        stats_observer::{
            AudioReceiverStatsSnapshot, AudioSenderStatsSnapshot, ConnectionStatsSnapshot,
            StatsSnapshot, StatsSnapshotConsumer, VideoReceiverStatsSnapshot,
            VideoSenderStatsSnapshot,
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

impl From<&MediaQualityStatsRawData> for MediaQualityStats {
    fn from(raw_data: &MediaQualityStatsRawData) -> Self {
        Self {
            rtt_median: get_median(&raw_data.rtt),
            jitter_median_send: get_median(&raw_data.send_jitter),
            jitter_median_recv: get_median(&raw_data.recv_jitter),
            packet_loss_fraction_send: get_median(&raw_data.send_packet_loss),
            packet_loss_fraction_recv: get_median(&raw_data.recv_packet_loss),
        }
    }
}

const CALL_TELEMETRY_VERSION: u32 = 1;

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

fn get_median<T: Sample>(samples: &[T]) -> Option<T> {
    if samples.is_empty() {
        None
    } else {
        let mut samples = samples.to_vec();
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Less));
        Some(samples[samples.len() / 2])
    }
}

/// `StreamSummary` is converted into `protobuf::call_summary::StreamSummary`
/// and serialized into the call telemetry blob.
#[derive(Debug, Default)]
struct StreamSummary {
    bitrate: DistributionSummary<f32>,
    packet_loss: DistributionSummary<f32>,
    jitter: DistributionSummary<f32>,
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
        let summary = self.audio_recv_stream_summaries.entry(ssrc).or_default();
        func(summary);
    }

    fn update_audio_send_stream_summary<F>(&mut self, ssrc: u32, mut func: F)
    where
        F: FnMut(&mut StreamSummary),
    {
        let summary = self.audio_send_stream_summaries.entry(ssrc).or_default();
        func(summary);
    }

    fn update_video_recv_stream_summary<F>(&mut self, ssrc: u32, mut func: F)
    where
        F: FnMut(&mut StreamSummary),
    {
        let summary = self.video_recv_stream_summaries.entry(ssrc).or_default();
        func(summary);
    }

    fn update_video_send_stream_summary<F>(&mut self, ssrc: u32, mut func: F)
    where
        F: FnMut(&mut StreamSummary),
    {
        let summary = self.video_send_stream_summaries.entry(ssrc).or_default();
        func(summary);
    }

    fn has_recv_summaries(&self) -> bool {
        !self.audio_recv_stream_summaries.is_empty() || !self.video_recv_stream_summaries.is_empty()
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

const DEFAULT_MAX_STATS_SETS: usize = 300;

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
/// [`QualityStatsRawData`] contains arrays of samples are used to calculate
/// the top-level stats (RTT median, packet loss percentage mean, and the jitter
/// median).
#[derive(Debug, Default)]
struct QualityStatsRawData {
    stun_rtt: Vec<f32>,
    audio_stats: MediaQualityStatsRawData,
    video_stats: MediaQualityStatsRawData,
}

/// [`MediaQualityStatsRawData`] contains arrays of samples that are used to
/// determine the top level stats for the particulr media type (audio or video).
#[derive(Debug, Default)]
struct MediaQualityStatsRawData {
    rtt: Vec<f32>,
    send_packet_loss: Vec<f32>,
    recv_packet_loss: Vec<f32>,
    send_jitter: Vec<f32>,
    recv_jitter: Vec<f32>,
}

impl MediaQualityStatsRawData {
    fn update_with_sender_stats(
        &mut self,
        rtt: f32,
        remote_packets_lost_pct: f32,
        remote_jitter: f32,
    ) {
        self.send_packet_loss.push(remote_packets_lost_pct);
        self.send_jitter.push(remote_jitter);
        self.rtt.push(rtt);
    }

    fn update_with_receiver_stats(&mut self, packets_lost_pct: f32, jitter: f32) {
        self.recv_packet_loss.push(packets_lost_pct);
        self.recv_jitter.push(jitter);
    }
}

impl QualityStatsRawData {
    fn update_with_audio_sender_stats(&mut self, snapshot: &AudioSenderStatsSnapshot) {
        self.audio_stats.update_with_sender_stats(
            snapshot.remote_rtt as f32,
            snapshot.remote_packets_lost_pct,
            snapshot.remote_jitter as f32,
        );
    }

    fn update_with_audio_receiver_stats(&mut self, snapshot: &AudioReceiverStatsSnapshot) {
        self.audio_stats
            .update_with_receiver_stats(snapshot.packets_lost_pct, snapshot.jitter as f32);
    }

    fn update_with_connection_stats(&mut self, snapshot: &ConnectionStatsSnapshot) {
        self.stun_rtt.push(snapshot.current_round_trip_time as f32);
    }

    fn update_with_video_sender_stats(&mut self, snapshot: &VideoSenderStatsSnapshot) {
        self.video_stats.update_with_sender_stats(
            snapshot.remote_round_trip_time as f32,
            snapshot.remote_packets_lost_pct,
            snapshot.remote_jitter as f32,
        );
    }

    fn update_with_video_receiver_stats(&mut self, snapshot: &VideoReceiverStatsSnapshot) {
        self.video_stats
            .update_with_receiver_stats(snapshot.packets_lost_pct, snapshot.jitter as f32);
    }

    fn build_quality_stats(&self) -> QualityStats {
        QualityStats {
            rtt_median_connection: get_median(&self.stun_rtt),
            audio_stats: (&self.audio_stats).into(),
            video_stats: (&self.video_stats).into(),
        }
    }
}

impl CallTelemetry {
    /// Generates the "opaque" telemetry blob and the corresponding description
    /// string that is suitable for being shown to the user. This implementation
    /// will serialize the telemetry BLOB to a JSON string so that it can easily
    /// be ingested by user provided tools.
    fn generate_blob_and_description(&self) -> (Vec<u8>, String) {
        let blob = self.encode_to_vec();
        let text = serde_json::to_string(&self)
            .unwrap_or_else(|_| "Failed to generate call quality info text".to_string());
        (blob, text)
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

#[derive(Debug)]
struct CallInfo {
    start_time: SystemTime,
    connect_time: Option<SystemTime>,
    top_level_quality_stats_raw: QualityStatsRawData,
    stream_summaries: StreamSummaries,
    // This value is set to true if the data was being exchanged over the
    // cellular interface at least at one point in the call.
    cellular: bool,
    // Time series stats sets. The total number of direct call state sets is
    // capped and may not (probably will not) contain the stats sets for the
    // entire call.
    stats_sets: StatsSets,
    needs_stats_set: bool,
}

impl Default for CallInfo {
    fn default() -> Self {
        Self {
            start_time: SystemTime::now(),
            connect_time: None,
            top_level_quality_stats_raw: Default::default(),
            stream_summaries: Default::default(),
            cellular: false,
            stats_sets: Default::default(),
            needs_stats_set: false,
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
                self.top_level_quality_stats_raw
                    .update_with_connection_stats(snapshot);
            }
            StatsSnapshot::AudioSender(snapshot) => {
                self.top_level_quality_stats_raw
                    .update_with_audio_sender_stats(snapshot);
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
                self.top_level_quality_stats_raw
                    .update_with_audio_receiver_stats(snapshot);
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
                self.top_level_quality_stats_raw
                    .update_with_video_sender_stats(snapshot);
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
                self.top_level_quality_stats_raw
                    .update_with_video_receiver_stats(snapshot);
                self.stream_summaries
                    .update_video_recv_stream_summary(snapshot.ssrc, |summary| {
                        summary.bitrate.push(snapshot.bitrate);
                        summary.packet_loss.push(snapshot.packets_lost_pct);
                        summary.jitter.push(snapshot.jitter as f32);
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

        let telemetry = CallTelemetry {
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

        let quality_stats = self
            .call_info
            .top_level_quality_stats_raw
            .build_quality_stats();

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
            raw_stats: Some(raw_stats),
            raw_stats_text: Some(raw_stats_text),
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
            GroupCallSummaryInner { call_info },
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

        let telemetry = CallTelemetry {
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

        let quality_stats = self
            .call_info
            .top_level_quality_stats_raw
            .build_quality_stats();

        // If the call connected then we present the connect time as the start
        // time to the upper layers since that will enable them to correctly
        // measure the duration of the call. On the other hand, if the call
        // never connected we set the end time equal to the start time.
        let (start_time, end_time) = match connect_time {
            Some(connect_time) => (connect_time, end_time),
            _ => (start_time, start_time),
        };

        // Any call that connected at some point and if audio or video was
        // received is a potential survey candidate.
        let is_survey_candidate = self.call_info.connect_time.is_some()
            && self.call_info.stream_summaries.has_recv_summaries();

        CallSummary {
            start_time,
            end_time,
            quality_stats,
            raw_stats: Some(raw_stats),
            raw_stats_text: Some(raw_stats_text),
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
}

#[cfg(test)]
mod test {
    use rand::Rng;

    use crate::core::call_summary::Sample;

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
}
