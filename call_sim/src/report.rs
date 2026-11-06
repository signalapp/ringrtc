//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{collections::HashMap, fmt::Write, str::FromStr};

use anyhow::{Result, anyhow};
use itertools::Itertools;
use log::{error, info};
use plotters::{
    backend::SVGBackend,
    chart::ChartBuilder,
    drawing::IntoDrawingArea,
    element::Circle,
    prelude::IntoSegmentedCoord,
    series::{Histogram, LineSeries},
    style::{Color, IntoFont, RGBColor, full_palette::WHITE},
};
use regex::Regex;
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    task::JoinSet,
};

use crate::{
    common::{
        CallConfig, ChartDimension, GroupConfig, NetworkConfigWithOffset, NetworkProfile,
        TestCaseConfig,
    },
    test::{AudioTestResults, Client, GroupRun, Sound, TestCase},
};

// Chart colors.
const DIM_GRAY: RGBColor = RGBColor(105, 105, 105);
const VERY_LIGHT_GRAY: RGBColor = RGBColor(250, 250, 250);
const STEEL_BLUE: RGBColor = RGBColor(70, 130, 180);

type ChartPoint = (f32, f32);

#[derive(Debug, Clone)]
pub enum LineShape {
    Linear,
    Hv,
}

#[derive(Debug, Clone)]
pub struct StatsConfig {
    pub title: String,
    pub chart_name: String,
    pub x_label: String,
    pub y_label: String,
    pub x_min: Option<f32>,
    /// By default, charts will use the StatsData.points.len for x_max.
    pub x_max: Option<f32>,
    pub y_min: Option<f32>,
    /// By default, charts will use the StatsData.overall_max + 10% for y_max.
    pub y_max: Option<f32>,
    /// Line presentation, the default is `Linear`, which connects each point to the
    /// next. Some charts look better with the `Hv` type, which maintains its value until
    /// the next point along the x-axis.
    pub line_shape: LineShape,
    /// Whether to show a total alongside min/max/ave for counting stats.
    pub show_total: bool,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            title: "".to_string(),
            chart_name: "".to_string(),
            x_label: "".to_string(),
            y_label: "".to_string(),
            x_min: Option::None,
            x_max: Option::None,
            y_min: Option::None,
            y_max: Option::None,
            line_shape: LineShape::Linear,
            show_total: false,
        }
    }
}

/// Our current standard value for ignoring the first "5 seconds" of garbage data.
const STATS_SKIP_N: usize = 5;

#[derive(Debug, Clone)]
pub struct StatsData {
    /// Calculated statistics for the entire range (for better charting).
    overall_min: f32,
    overall_max: f32,

    /// Filter settings to only calculate statistics over a sub-range. Usually, the first
    /// data point or set of data points is not useful and should be filtered out. And
    /// sometimes we might want to stop calculation as soon as we know the media part of
    /// the test is over.
    /// Defaults to STATS_SKIP_N > .. usize::MAX. We default to this since most stats use
    /// this min, only to reduce some of the complexity in this file.
    filter_min: usize,
    filter_max: usize,

    /// The period that each data point represents on the x-axis.
    /// Defaults to 1 (i.e. each point implies a value for the prior second).
    period: f32,

    /// Point data in the form (index, value).
    pub points: Vec<ChartPoint>,

    /// Running sum of squares of the values within the filtered range, used to
    /// calculate the standard deviation.
    sum_sq: f64,

    /// Calculated statistics within the filtered range.
    pub min: f32,
    pub max: f32,
    pub ave: f32,
    pub std_dev: f32,
    pub total: f64,

    /// Track the max inserted index in case data is aperiodic.
    pub max_index: f32,
}

impl Default for StatsData {
    fn default() -> Self {
        Self {
            overall_min: f32::MAX,
            overall_max: 0.0,
            filter_min: STATS_SKIP_N,
            filter_max: usize::MAX,
            period: 1.0,
            points: vec![],
            sum_sq: 0.0,
            min: f32::MAX,
            max: 0.0,
            ave: 0.0,
            std_dev: 0.0,
            total: 0.0,
            max_index: 0.0,
        }
    }
}

impl StatsData {
    /// Creates a StatsData but skipping the first N items so that they aren't taken into
    /// account when calculating the statistics. This is actually useful to avoid skipping
    /// as is currently done by default.
    pub fn new_skip_n(n: usize) -> Self {
        Self {
            filter_min: n,
            ..Default::default()
        }
    }

    pub fn set_filter(&mut self, min: usize, max: usize) {
        self.filter_min = min;
        self.filter_max = max;
    }

    pub fn set_period(&mut self, period: f32) {
        self.period = period;
    }

    /// Push data to an arbitrary index and update statistics.
    pub fn push_with_index(&mut self, index: f32, value: f32) {
        self.points.push((index, value));

        if self.points.len() > self.filter_min && self.points.len() <= self.filter_max {
            let n = (self.points.len() - self.filter_min) as f64;
            self.total += value as f64;
            self.sum_sq += (value as f64) * (value as f64);
            let mean = self.total / n;
            self.ave = mean as f32;
            // Population standard deviation: sqrt(Σx²/N − mean²), max(0.0) guards float error.
            self.std_dev = ((self.sum_sq / n) - mean * mean).max(0.0).sqrt() as f32;
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }

        // To ensure good ranges for charting, we need to keep the overall min/max.
        self.overall_min = self.overall_min.min(value);
        self.overall_max = self.overall_max.max(value);
        self.max_index = self.max_index.max(index);
    }

    /// Push data to the next periodic index and update statistics.
    pub fn push(&mut self, value: f32) {
        self.push_with_index(((self.points.len() + 1) as f32) * self.period, value);
    }
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub config: StatsConfig,
    pub data: StatsData,
}

#[derive(Debug, Default)]
pub enum AnalysisReportMos {
    /// No mos value is available.
    #[default]
    None,
    /// There is a single mos value available.
    Single(f32),
    /// There is a stats collection of mos values available.
    Series(Box<Stats>),
}

impl AnalysisReportMos {
    /// Return a single MOS value (i.e. the average) or None for display.
    fn get_mos_for_display(&self) -> Option<f32> {
        match self {
            AnalysisReportMos::None => None,
            AnalysisReportMos::Single(mos) => Some(*mos),
            AnalysisReportMos::Series(stats) => Some(stats.data.ave),
        }
    }
}

#[derive(Debug)]
pub struct AnalysisReport {
    /// Hold the audio results for reporting.
    pub audio_test_results: AudioTestResults,
    /// Store video results for reporting.
    pub vmaf: Option<f32>,
}

impl AnalysisReport {
    pub async fn parse_visqol_mos_results(file_name: &str) -> Result<Option<f32>> {
        // Look through the file until we find the MOS line and return the value.
        let file = File::open(file_name).await?;
        let reader = BufReader::new(file);

        // Example: MOS-LQO:		4.14442
        let re_mos_line = Regex::new(r"MOS-LQO:\s*(?P<mos>[-+]?[0-9]*\.?[0-9]+)")?;

        let mut mos = None;

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(cap) = re_mos_line.captures(&line) {
                mos = Some(f32::from_str(&cap["mos"])?);
                break;
            }
        }

        Ok(mos)
    }

    pub async fn parse_pesq_mos_results(file_name: &str) -> Result<Option<f32>> {
        // Look through the file until we find the MOS line and return the value.
        let file = File::open(file_name).await?;
        let reader = BufReader::new(file);

        // Example: 4.470762252807617
        let re_mos_line = Regex::new(r"(?P<mos>[-+]?[0-9]*\.?[0-9]+)")?;

        let mut mos = None;

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(cap) = re_mos_line.captures(&line) {
                mos = Some(f32::from_str(&cap["mos"])?);
                break;
            }
        }

        Ok(mos)
    }

    pub async fn parse_plc_mos_results(file_name: &str) -> Result<Option<f32>> {
        // Look through the file until we find the MOS line and return the value.
        let file = File::open(file_name).await?;
        let reader = BufReader::new(file);

        // Example: 0  /degraded/loss-0_16k.wav   4.193227
        let re_mos_line = Regex::new(r"0.*wav\s*(?P<mos>[-+]?[0-9]*\.?[0-9]+)")?;

        let mut mos = None;

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(cap) = re_mos_line.captures(&line) {
                mos = Some(f32::from_str(&cap["mos"])?);
                break;
            }
        }

        Ok(mos)
    }

    async fn parse_video_analysis(file_name: &str) -> Result<f32> {
        let mut file = File::open(file_name).await?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).await?;
        if contents.is_empty() {
            // The analysis step failed.
            // The most common reason for this is because no frames were successfully sent.
            return Ok(0.0);
        }
        let json: serde_json::Value = serde_json::from_slice(&contents)?;
        json["pooled_metrics"]["vmaf"]["mean"]
            .as_f64()
            .map(|x| x as f32)
            .ok_or_else(|| anyhow!("invalid vmaf json"))
    }

    // There isn't much to build for audio, now that it is pre-calculated.
    pub async fn build(
        audio_test_results: AudioTestResults,
        video_analysis_file_name: Option<&str>,
    ) -> Result<Self> {
        let vmaf = if let Some(video_analysis_file_name) = video_analysis_file_name
            && let Ok(vmaf) = Self::parse_video_analysis(video_analysis_file_name).await
        {
            Some(vmaf)
        } else {
            None
        };

        Ok(Self {
            audio_test_results,
            vmaf,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct DockerStatsReport {
    timestamp: Vec<u64>,
    cpu_usage: Stats,
    mem_usage: Stats,
    tx_bitrate: Stats,
    rx_bitrate: Stats,
    item_count: usize,
}

impl DockerStatsReport {
    async fn parse(
        file_name: &str,
    ) -> Result<(Vec<u64>, StatsData, StatsData, StatsData, StatsData)> {
        // Look through the file and pull out the periodic (1 second) docker stats.
        let file = File::open(file_name).await?;
        let reader = BufReader::new(file);

        // Timestamp\tCPU\tMEM\nTX_Bitrate\nRX_Bitrate
        // 1234567890 21.84	8994816	70845	71137
        let re_stats_line = Regex::new(
            r"(?P<time>\d+)\s*(?P<cpu>[0-9]*\.?[0-9]+)\s*(?P<mem>\d+)\s*(?P<tx>\d+)\s*(?P<rx>\d+)(.*)",
        )?;

        let mut timestamp = vec![];
        let mut cpu_usage = StatsData::default();
        let mut mem_usage = StatsData::default();
        let mut tx_bitrate = StatsData::default();
        let mut rx_bitrate = StatsData::default();

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(cap) = re_stats_line.captures(&line) {
                timestamp.push(u64::from_str(&cap["time"])?);
                cpu_usage.push(f32::from_str(&cap["cpu"])?);
                mem_usage.push(f32::from_str(&cap["mem"])? / 1048576.0);
                tx_bitrate.push(f32::from_str(&cap["tx"])? / 1000.0);
                rx_bitrate.push(f32::from_str(&cap["rx"])? / 1000.0);
            }
        }

        Ok((timestamp, cpu_usage, mem_usage, tx_bitrate, rx_bitrate))
    }

    pub async fn build(file_name: &str, client_name: &str) -> Result<Self> {
        let (timestamp, cpu_usage, mem_usage, tx_bitrate, rx_bitrate) =
            DockerStatsReport::parse(file_name).await?;

        // We'll use the timestamp length as representative of the common size.
        let item_count = timestamp.len();

        let cpu_usage_stats = Stats {
            config: StatsConfig {
                title: "Container CPU Usage".to_string(),
                chart_name: format!("{}.container.cpu_usage.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "% of Core".to_string(),
                ..Default::default()
            },
            data: cpu_usage,
        };

        let mem_usage_stats = Stats {
            config: StatsConfig {
                title: "Container Memory Usage".to_string(),
                chart_name: format!("{}.container.mem_usage.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Mebibytes".to_string(),
                ..Default::default()
            },
            data: mem_usage,
        };

        let tx_bitrate_stats = Stats {
            config: StatsConfig {
                title: "Container Send Bitrate".to_string(),
                chart_name: format!("{}.container.send_bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: tx_bitrate,
        };

        let rx_bitrate_stats = Stats {
            config: StatsConfig {
                title: "Container Receive Bitrate".to_string(),
                chart_name: format!("{}.container.receive_bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: rx_bitrate,
        };

        Ok(Self {
            timestamp,
            cpu_usage: cpu_usage_stats,
            mem_usage: mem_usage_stats,
            tx_bitrate: tx_bitrate_stats,
            rx_bitrate: rx_bitrate_stats,
            item_count,
        })
    }
}

#[derive(Debug, Default)]
pub struct ConnectionStats {
    pub current_round_trip_time: Stats,
    pub available_outgoing_bitrate: Stats,
}

#[derive(Debug, Default)]
pub struct AudioSendStats {
    pub ssrc: String,
    pub packets_per_second: Stats,
    pub average_packet_size: Stats,
    pub bitrate: Stats,
    pub remote_packet_loss: Stats,
    pub remote_jitter: Stats,
    pub remote_round_trip_time: Stats,
    pub audio_energy: Stats,
}

#[derive(Debug, Default)]
pub struct VideoSendStats {
    pub ssrc: String,
    pub packets_per_second: Stats,
    pub average_packet_size: Stats,
    pub bitrate: Stats,
    pub source_framerate: Stats,
    pub source_resolution: Stats,
    pub framerate: Stats,
    pub key_frames_encoded: Stats,
    pub retransmitted_packets_sent: Stats,
    pub retransmitted_bitrate: Stats,
    pub send_delay_per_packet: Stats,
    pub nack_count: Stats,
    pub pli_count: Stats,
    pub remote_packet_loss: Stats,
    pub remote_jitter: Stats,
    pub remote_round_trip_time: Stats,
    pub resolution: Stats,
}

#[derive(Debug, Default)]
pub struct AudioReceiveStats {
    pub ssrc: String,
    /// The name of the client this stream came from, once it can be resolved. The SFU forwards
    /// audio without rewriting the SSRC, so a receiver's inbound SSRC is the sender's outbound
    /// SSRC, which is how the two get matched up.
    pub sender_name: Option<String>,
    pub packets_per_second: Stats,
    pub packet_loss: Stats,
    pub bitrate: Stats,
    pub jitter: Stats,
    pub audio_energy: Stats,
    pub jitter_buffer_delay: Stats,
    pub jitter_buffer_target_delay: Stats,
    pub jitter_buffer_flushes: Stats,
    pub concealed_samples_pct: Stats,
    pub fec_packets_received: Stats,
    pub relative_arrival_delay_per_packet: Stats,
}

#[derive(Debug, Default)]
pub struct VideoReceiveStats {
    pub ssrc: String,
    /// The name of the client this stream came from, once it can be resolved. See
    /// `AudioReceiveStats::sender_name`.
    pub sender_name: Option<String>,
    pub packets_per_second: Stats,
    pub packet_loss: Stats,
    pub bitrate: Stats,
    pub framerate: Stats,
    pub key_frames_decoded: Stats,
    pub resolution: Stats,
}

#[derive(Debug, Default)]
pub struct AudioAdaptation {
    pub out_bitrate: Stats,
    pub out_packet_length: Stats,
    pub out_loss_floor: Stats,
    pub in_bandwidth: Stats,
    pub in_rtt: Stats,
    pub in_loss: Stats,
}

#[derive(Debug)]
pub struct ClientLogReport {
    pub connection_stats: ConnectionStats,
    pub audio_send_stats: AudioSendStats,
    pub video_send_stats: Vec<VideoSendStats>,
    pub audio_receive_stats_list: Vec<AudioReceiveStats>,
    pub video_receive_stats_list: Vec<VideoReceiveStats>,
    pub audio_adaptation: AudioAdaptation,
}

impl ClientLogReport {
    async fn parse(
        file_name: &str,
    ) -> Result<(
        ConnectionStats,
        AudioSendStats,
        HashMap<String, VideoSendStats>,
        HashMap<String, AudioReceiveStats>,
        HashMap<String, VideoReceiveStats>,
    )> {
        // Look through the file and pull out RingRTC logs, particularly the `stats!` details.
        let file = File::open(file_name).await?;
        let reader = BufReader::new(file);

        // Example: ringrtc_stats!,connection,0xca111d,1667611058243536,0ms,100000bps
        let re_connection_line = Regex::new(
            r".*ringrtc_stats!,connection,0x[0-9a-fA-F]+,\d+,(?P<current_round_trip_time>\d+)ms,(?P<available_outgoing_bitrate>\d+)bps",
        )?;

        // Example: ringrtc_stats!,audio,send,2002,40.0,100.0,32000.0bps,0.0%,0ms,0ms,0.000
        let re_audio_send_line = Regex::new(
            r".*ringrtc_stats!,audio,send,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<average_packet_size>[-+]?[0-9]*\.?[0-9]+),(?P<bitrate>[-+]?[0-9]*\.?[0-9]+)bps,(?P<remote_packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<remote_jitter>\d+)ms,(?P<remote_round_trip_time>\d+)ms,(?P<audio_energy>[-+]?[0-9]*\.?[0-9]+)",
        )?;

        // Example: ringrtc_stats!,video,send,2003,8.0,1052.9,67430bps,1280x720,8.0fps,1280x720,2.0fps,0,4.0ms,0,0.0bps,162.4ms,0,0,bandwidth,0,0.0%,170.2ms,1.0ms
        let re_video_send_line = Regex::new(
            r".*ringrtc_stats!,video,send,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<average_packet_size>[-+]?[0-9]*\.?[0-9]+),(?P<bitrate>[-+]?[0-9]*\.?[0-9]+)bps,(?P<source_resolution>\d+x\d+),(?P<source_framerate>[0-9]*\.?[0-9]+)fps,(?P<resolution>\d+x\d+),(?P<framerate>[0-9]*\.?[0-9]+)fps,(?P<key_frames_encoded>\d+),(?P<encode_time_per_frame>[0-9]*\.?[0-9]+)ms,(?P<retransmitted_packets_sent>\d+),(?P<retransmitted_bitrate>[0-9]*\.?[0-9]+)bps,(?P<send_delay_per_packet>[0-9]*\.?[0-9]+)ms,(?P<nack_count>\d+),(?P<pli_count>\d+),(?P<quality_limitation_reason>\w+),(?P<quality_limitation_resolution_changes>\d+),(?P<remote_packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<remote_jitter>[0-9]*\.?[0-9]+)ms,(?P<remote_round_trip_time>[0-9]*\.?[0-9]+)ms",
        )?;

        // Example: ringrtc_stats!,audio,recv,1002,40.0,0.00%,32000.0bps,0ms,0.000,50ms,40ms,0,0.00%,0,0ms
        let re_audio_receive_line = Regex::new(
            r".*ringrtc_stats!,audio,recv,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<bitrate>[-+]?[0-9]*\.?[0-9]+)bps,(?P<jitter>\d+)ms,(?P<audio_energy>[-+]?[0-9]*\.?[0-9]+),(?P<jitter_buffer_delay>\d+)ms,(?P<jitter_buffer_target_delay>\d+)ms,(?P<jitter_buffer_flushes>\d+),(?P<concealed_samples_pct>[-+]?[0-9]*\.?[0-9]+)%,(?P<fec_packets_received>\d+),(?P<relative_arrival_delay_per_packet>\d+)ms",
        )?;

        // Example: ringrtc_stats!,video,recv,2003,7.0,0.0%,61305bps,1.0fps,1,3.3ms,1280x720
        let re_video_receive_line = Regex::new(
            r".*ringrtc_stats!,video,recv,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<bitrate>[0-9]+)bps,(?P<framerate>[0-9]*\.?[0-9]+)fps,(?P<key_frames_decoded>\d+),(?P<decode_time_per_frame>[0-9]*\.?[0-9]+)ms,(?P<resolution>\d+x\d+)",
        )?;

        let mut connection_stats = ConnectionStats::default();
        let mut audio_send_stats = AudioSendStats::default();
        let mut video_send_stats_map = HashMap::<String, VideoSendStats>::new();
        let mut audio_receive_stats_map = HashMap::<String, AudioReceiveStats>::new();
        let mut video_receive_stats_map = HashMap::<String, VideoReceiveStats>::new();

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(cap) = re_connection_line.captures(&line) {
                connection_stats
                    .current_round_trip_time
                    .data
                    .push(f32::from_str(&cap["current_round_trip_time"])?);
                connection_stats
                    .available_outgoing_bitrate
                    .data
                    .push(f32::from_str(&cap["available_outgoing_bitrate"])? / 1000.0);
                continue;
            }

            if let Some(cap) = re_audio_send_line.captures(&line) {
                audio_send_stats.ssrc = cap["ssrc"].to_string();
                audio_send_stats
                    .packets_per_second
                    .data
                    .push(f32::from_str(&cap["packets_per_second"])?);
                audio_send_stats
                    .average_packet_size
                    .data
                    .push(f32::from_str(&cap["average_packet_size"])?);
                audio_send_stats
                    .bitrate
                    .data
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                audio_send_stats
                    .remote_packet_loss
                    .data
                    .push(f32::from_str(&cap["remote_packet_loss"])?);
                audio_send_stats
                    .remote_jitter
                    .data
                    .push(f32::from_str(&cap["remote_jitter"])?);
                audio_send_stats
                    .remote_round_trip_time
                    .data
                    .push(f32::from_str(&cap["remote_round_trip_time"])?);
                audio_send_stats
                    .audio_energy
                    .data
                    .push(f32::from_str(&cap["audio_energy"])?);
                continue;
            }

            if let Some(cap) = re_video_send_line.captures(&line) {
                let ssrc = cap["ssrc"].to_string();
                let video_send_stats = video_send_stats_map.entry(ssrc.clone()).or_default();

                video_send_stats.ssrc = ssrc;
                video_send_stats
                    .packets_per_second
                    .data
                    .push(f32::from_str(&cap["packets_per_second"])?);
                video_send_stats
                    .average_packet_size
                    .data
                    .push(f32::from_str(&cap["average_packet_size"])?);
                video_send_stats
                    .bitrate
                    .data
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                video_send_stats
                    .source_framerate
                    .data
                    .push(f32::from_str(&cap["source_framerate"])?);
                video_send_stats
                    .framerate
                    .data
                    .push(f32::from_str(&cap["framerate"])?);
                video_send_stats
                    .key_frames_encoded
                    .data
                    .push(f32::from_str(&cap["key_frames_encoded"])?);
                video_send_stats
                    .retransmitted_packets_sent
                    .data
                    .push(f32::from_str(&cap["retransmitted_packets_sent"])?);
                video_send_stats
                    .retransmitted_bitrate
                    .data
                    .push(f32::from_str(&cap["retransmitted_bitrate"])?);
                video_send_stats
                    .send_delay_per_packet
                    .data
                    .push(f32::from_str(&cap["send_delay_per_packet"])?);
                video_send_stats
                    .nack_count
                    .data
                    .push(f32::from_str(&cap["nack_count"])?);
                video_send_stats
                    .pli_count
                    .data
                    .push(f32::from_str(&cap["pli_count"])?);
                video_send_stats
                    .remote_packet_loss
                    .data
                    .push(f32::from_str(&cap["remote_packet_loss"])?);
                video_send_stats
                    .remote_jitter
                    .data
                    .push(f32::from_str(&cap["remote_jitter"])?);
                video_send_stats
                    .remote_round_trip_time
                    .data
                    .push(f32::from_str(&cap["remote_round_trip_time"])?);

                let res = &cap["source_resolution"];
                let res: usize = res
                    .split('x')
                    .map(|n| n.parse::<usize>().unwrap())
                    .product();
                video_send_stats.source_resolution.data.push(res as f32);

                let res = &cap["resolution"];
                let res: usize = res
                    .split('x')
                    .map(|n| n.parse::<usize>().unwrap())
                    .product();
                video_send_stats.resolution.data.push(res as f32);

                continue;
            }

            if let Some(cap) = re_audio_receive_line.captures(&line) {
                let ssrc = cap["ssrc"].to_string();
                let audio_receive_stats = audio_receive_stats_map.entry(ssrc.clone()).or_default();

                audio_receive_stats.ssrc = ssrc;
                audio_receive_stats
                    .packets_per_second
                    .data
                    .push(f32::from_str(&cap["packets_per_second"])?);
                audio_receive_stats
                    .bitrate
                    .data
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                audio_receive_stats
                    .audio_energy
                    .data
                    .push(f32::from_str(&cap["audio_energy"])?);
                audio_receive_stats
                    .packet_loss
                    .data
                    .push(f32::from_str(&cap["packet_loss"])?);
                audio_receive_stats
                    .jitter
                    .data
                    .push(f32::from_str(&cap["jitter"])?);
                audio_receive_stats
                    .jitter_buffer_delay
                    .data
                    .push(f32::from_str(&cap["jitter_buffer_delay"])?);
                audio_receive_stats
                    .jitter_buffer_target_delay
                    .data
                    .push(f32::from_str(&cap["jitter_buffer_target_delay"])?);
                audio_receive_stats
                    .jitter_buffer_flushes
                    .data
                    .push(f32::from_str(&cap["jitter_buffer_flushes"])?);
                audio_receive_stats
                    .concealed_samples_pct
                    .data
                    .push(f32::from_str(&cap["concealed_samples_pct"])?);
                audio_receive_stats
                    .fec_packets_received
                    .data
                    .push(f32::from_str(&cap["fec_packets_received"])?);
                audio_receive_stats
                    .relative_arrival_delay_per_packet
                    .data
                    .push(f32::from_str(&cap["relative_arrival_delay_per_packet"])?);
                continue;
            }

            if let Some(cap) = re_video_receive_line.captures(&line) {
                let ssrc = cap["ssrc"].to_string();
                let video_receive_stats = video_receive_stats_map.entry(ssrc.clone()).or_default();
                video_receive_stats.ssrc = ssrc;
                video_receive_stats
                    .packets_per_second
                    .data
                    .push(f32::from_str(&cap["packets_per_second"])?);
                video_receive_stats
                    .bitrate
                    .data
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                video_receive_stats
                    .packet_loss
                    .data
                    .push(f32::from_str(&cap["packet_loss"])?);
                video_receive_stats
                    .framerate
                    .data
                    .push(f32::from_str(&cap["framerate"])?);
                video_receive_stats
                    .key_frames_decoded
                    .data
                    .push(f32::from_str(&cap["key_frames_decoded"])?);

                let res = &cap["resolution"];
                let res: usize = res
                    .split('x')
                    .map(|n| n.parse::<usize>().unwrap())
                    .product();
                video_receive_stats.resolution.data.push(res as f32);
                continue;
            }
        }

        Ok((
            connection_stats,
            audio_send_stats,
            video_send_stats_map,
            audio_receive_stats_map,
            video_receive_stats_map,
        ))
    }

    /// Adaptation records are aperiodic, so they are charted against their log time.
    async fn parse_adaptation(file_name: &str) -> Result<AudioAdaptation> {
        let file = File::open(file_name).await?;
        let reader = BufReader::new(file);

        // Example: [2026-09-30 20:37:26.735 WARN ...] ...: ringrtc_adapt!,audio,out_bitrate,32000,16000
        let re_adaptation_line = Regex::new(
            r"^\[(?P<time>\S+ \S+) .*ringrtc_adapt!,audio,(?P<record>\w+),(?P<values>\S*)",
        )?;

        let mut audio_adaptation = AudioAdaptation {
            out_bitrate: Stats {
                data: StatsData::new_skip_n(0),
                ..Default::default()
            },
            out_packet_length: Stats {
                data: StatsData::new_skip_n(0),
                ..Default::default()
            },
            out_loss_floor: Stats {
                data: StatsData::new_skip_n(0),
                ..Default::default()
            },
            in_bandwidth: Stats {
                data: StatsData::new_skip_n(0),
                ..Default::default()
            },
            in_rtt: Stats {
                data: StatsData::new_skip_n(0),
                ..Default::default()
            },
            in_loss: Stats {
                data: StatsData::new_skip_n(0),
                ..Default::default()
            },
        };

        let mut start = None;
        let mut index = 0.0;

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            let Some(cap) = re_adaptation_line.captures(&line) else {
                continue;
            };

            let time = chrono::NaiveDateTime::parse_from_str(&cap["time"], "%Y-%m-%d %H:%M:%S%.f")?;
            index = (time - *start.get_or_insert(time)).num_milliseconds() as f32 / 1000.0;

            let values: Vec<f32> = cap["values"]
                .split(',')
                .filter_map(|value| f32::from_str(value).ok())
                .collect();
            match (&cap["record"], values.as_slice()) {
                ("config", [_, bitrate, _, _, packet_length, ..]) => {
                    audio_adaptation
                        .out_bitrate
                        .data
                        .push_with_index(index, bitrate / 1000.0);
                    audio_adaptation
                        .out_packet_length
                        .data
                        .push_with_index(index, *packet_length);
                }
                ("out_bitrate", [_, bitrate]) => {
                    audio_adaptation
                        .out_bitrate
                        .data
                        .push_with_index(index, bitrate / 1000.0);
                }
                ("out_ptime", [_, packet_length]) => {
                    audio_adaptation
                        .out_packet_length
                        .data
                        .push_with_index(index, *packet_length);
                }
                ("loss_floor", [fraction]) => {
                    audio_adaptation
                        .out_loss_floor
                        .data
                        .push_with_index(index, fraction * 100.0);
                }
                ("bandwidth" | "allocation", [bitrate, ..]) => {
                    audio_adaptation
                        .in_bandwidth
                        .data
                        .push_with_index(index, bitrate / 1000.0);
                }
                ("rtt", [rtt]) => audio_adaptation.in_rtt.data.push_with_index(index, *rtt),
                ("loss", [fraction]) => {
                    audio_adaptation
                        .in_loss
                        .data
                        .push_with_index(index, fraction * 100.0);
                }
                _ => {}
            }
        }

        // Hold each output's last value until the last adaptation record.
        for stats in [
            &mut audio_adaptation.out_bitrate,
            &mut audio_adaptation.out_packet_length,
            &mut audio_adaptation.out_loss_floor,
        ] {
            if let Some(&(_, value)) = stats.data.points.last() {
                stats.data.push_with_index(index, value);
            }
        }

        Ok(audio_adaptation)
    }

    pub async fn build(file_name: &str, client_name: &str, adaptation: bool) -> Result<Self> {
        let (
            connection_stats,
            audio_send_stats,
            video_send_stats_map,
            audio_receive_stats_map,
            video_receive_stats_map,
        ) = ClientLogReport::parse(file_name).await?;
        let audio_adaptation = if adaptation {
            ClientLogReport::parse_adaptation(file_name).await?
        } else {
            AudioAdaptation::default()
        };

        // We assume that all entries in the send stats vectors are in sync.
        // stats in the receive_stats are dependent on the clients join time
        // for now we ignore discrepancies in the receive_stats length
        let connection_point_count = connection_stats.current_round_trip_time.data.points.len();
        if (connection_point_count != audio_send_stats.bitrate.data.points.len())
            || video_send_stats_map
                .values()
                .any(|stats| connection_point_count != stats.bitrate.data.points.len())
        {
            return Err(anyhow!("RingRTC stats were not in sync!"));
        }

        let current_round_trip_time = Stats {
            config: StatsConfig {
                title: "Current Round Trip Time".to_string(),
                chart_name: format!("{}.log.connection.rtt.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: connection_stats.current_round_trip_time.data,
        };

        let available_outgoing_bitrate = Stats {
            config: StatsConfig {
                title: "Available Outgoing Bitrate".to_string(),
                chart_name: format!("{}.log.connection.outgoing_bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: connection_stats.available_outgoing_bitrate.data,
        };

        let connection_stats = ConnectionStats {
            current_round_trip_time,
            available_outgoing_bitrate,
        };

        let packets_per_second = Stats {
            config: StatsConfig {
                title: "Audio Send Packet Rate".to_string(),
                chart_name: format!("{}.log.audio.send.packet_rate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Packets/Second".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.packets_per_second.data,
        };

        let average_packet_size = Stats {
            config: StatsConfig {
                title: "Audio Send Packet Size".to_string(),
                chart_name: format!("{}.log.audio.send.packet_size.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Average Size Per Period".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.average_packet_size.data,
        };

        let bitrate = Stats {
            config: StatsConfig {
                title: "Audio Send Bitrate".to_string(),
                chart_name: format!("{}.log.audio.send.bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.bitrate.data,
        };

        let remote_packet_loss = Stats {
            config: StatsConfig {
                title: "Audio Send Remote Packet Loss".to_string(),
                chart_name: format!("{}.log.audio.send.remote_loss.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "%".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.remote_packet_loss.data,
        };

        let remote_jitter = Stats {
            config: StatsConfig {
                title: "Audio Send Remote Jitter".to_string(),
                chart_name: format!("{}.log.audio.send.remote_jitter.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.remote_jitter.data,
        };

        let remote_round_trip_time = Stats {
            config: StatsConfig {
                title: "Audio Send Remote Round Trip Time".to_string(),
                chart_name: format!("{}.log.audio.send.remote_rtt.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.remote_round_trip_time.data,
        };

        let audio_energy = Stats {
            config: StatsConfig {
                title: "Audio Send Audio Energy".to_string(),
                chart_name: format!("{}.log.audio.send.audio_energy.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Energy".to_string(),
                y_max: Some(1.0),
                ..Default::default()
            },
            data: audio_send_stats.audio_energy.data,
        };

        let audio_send_stats = AudioSendStats {
            ssrc: audio_send_stats.ssrc,
            packets_per_second,
            average_packet_size,
            bitrate,
            remote_packet_loss,
            remote_jitter,
            remote_round_trip_time,
            audio_energy,
        };

        let mut video_send_stats_list = Vec::with_capacity(video_send_stats_map.len());
        for (ssrc, video_send_stats) in video_send_stats_map {
            let chart =
                |metric: &str| format!("{client_name}.log.video.send.{metric}.ssrc-{ssrc}.svg");
            let packets_per_second = Stats {
                config: StatsConfig {
                    title: format!("Video Send Packet Rate (ssrc={ssrc})"),
                    chart_name: chart("packet_rate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Packets/Second".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.packets_per_second.data,
            };

            let average_packet_size = Stats {
                config: StatsConfig {
                    title: format!("Video Send Packet Size (ssrc={ssrc})"),
                    chart_name: chart("packet_size"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Average Size Per Period".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.average_packet_size.data,
            };

            let bitrate = Stats {
                config: StatsConfig {
                    title: format!("Video Send Bitrate (ssrc={ssrc})"),
                    chart_name: chart("bitrate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Kbps".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.bitrate.data,
            };

            let source_framerate = Stats {
                config: StatsConfig {
                    title: format!("Video Source Framerate (ssrc={ssrc})"),
                    chart_name: format!("{}.log.video.send.source_framerate.svg", client_name),
                    x_label: "Test Seconds".to_string(),
                    y_label: "fps".to_string(),
                    y_max: Some(32.0),
                    ..Default::default()
                },
                data: video_send_stats.source_framerate.data,
            };

            let framerate = Stats {
                config: StatsConfig {
                    title: format!("Video Send Framerate (ssrc={ssrc})"),
                    chart_name: chart("framerate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "fps".to_string(),
                    y_max: Some(32.0),
                    ..Default::default()
                },
                data: video_send_stats.framerate.data,
            };

            let key_frames_encoded = Stats {
                config: StatsConfig {
                    title: format!("Video Key Frames Encoded (ssrc={ssrc})"),
                    chart_name: chart("key_frames_encoded"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "# frames".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.key_frames_encoded.data,
            };

            let retransmitted_packets_sent = Stats {
                config: StatsConfig {
                    title: format!("Video Retransmitted Packets (ssrc={ssrc})"),
                    chart_name: chart("retransmitted_packets"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "# Packets".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.retransmitted_packets_sent.data,
            };

            let retransmitted_bitrate = Stats {
                config: StatsConfig {
                    title: format!("Video Send Retransmitted Bitrate (ssrc={ssrc})"),
                    chart_name: chart("retransmitted_bitrate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Kbps".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.retransmitted_bitrate.data,
            };

            let send_delay_per_packet = Stats {
                config: StatsConfig {
                    title: format!("Video Send Send Delay Per Packet (ssrc={ssrc})"),
                    chart_name: chart("delay_per_packet"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "ms".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.send_delay_per_packet.data,
            };

            let nack_count = Stats {
                config: StatsConfig {
                    title: format!("Video Received NACK Count (ssrc={ssrc})"),
                    chart_name: chart("nack_count"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "# NACKs".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.nack_count.data,
            };

            let pli_count = Stats {
                config: StatsConfig {
                    title: format!("Video Received PLI Count (ssrc={ssrc})"),
                    chart_name: chart("pli_count"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "# PLIs".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.pli_count.data,
            };

            let remote_packet_loss = Stats {
                config: StatsConfig {
                    title: format!("Video Send Remote Packet Loss (ssrc={ssrc})"),
                    chart_name: chart("remote_loss"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "%".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.remote_packet_loss.data,
            };

            let remote_jitter = Stats {
                config: StatsConfig {
                    title: format!("Video Send Remote Jitter (ssrc={ssrc})"),
                    chart_name: chart("remote_jitter"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "milliseconds".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.remote_jitter.data,
            };

            let remote_round_trip_time = Stats {
                config: StatsConfig {
                    title: "Video Send Remote Round Trip Time".to_string(),
                    chart_name: chart("remote_rtt"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "milliseconds".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.remote_round_trip_time.data,
            };

            let source_resolution = Stats {
                config: StatsConfig {
                    title: "Video Source Resolution".to_string(),
                    chart_name: format!("{}.log.video.send.source_resolution.svg", client_name),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Resolution (sq. pixels)".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.source_resolution.data,
            };

            let resolution = Stats {
                config: StatsConfig {
                    title: "Video Send Resolution".to_string(),
                    chart_name: chart("resolution"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Resolution (sq. pixels)".to_string(),
                    ..Default::default()
                },
                data: video_send_stats.resolution.data,
            };

            video_send_stats_list.push(VideoSendStats {
                ssrc: video_send_stats.ssrc,
                packets_per_second,
                average_packet_size,
                bitrate,
                source_framerate,
                source_resolution,
                framerate,
                key_frames_encoded,
                retransmitted_packets_sent,
                retransmitted_bitrate,
                send_delay_per_packet,
                nack_count,
                pli_count,
                remote_packet_loss,
                remote_jitter,
                remote_round_trip_time,
                resolution,
            });
        }

        let mut audio_receive_stats_list = Vec::with_capacity(audio_receive_stats_map.len());
        for (ssrc, audio_receive_stats) in audio_receive_stats_map {
            // One chart per stream. Without the ssrc in the file name, streams from different
            // senders overwrite each other and whichever renders last wins.
            let chart =
                |metric: &str| format!("{client_name}.log.audio.receive.{metric}.ssrc-{ssrc}.svg");
            let packets_per_second = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Packet Rate (ssrc={ssrc})"),
                    chart_name: chart("packet_rate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Packets/Second".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.packets_per_second.data,
            };

            let packet_loss = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Packet Loss (ssrc={ssrc})"),
                    chart_name: chart("loss"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "%".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.packet_loss.data,
            };

            let bitrate = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Bitrate (ssrc={ssrc})"),
                    chart_name: chart("bitrate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Kbps".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.bitrate.data,
            };

            let jitter = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Jitter (ssrc={ssrc})"),
                    chart_name: chart("jitter"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "milliseconds".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.jitter.data,
            };

            let audio_energy = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Audio Energy (ssrc={ssrc})"),
                    chart_name: chart("audio_energy"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "dB".to_string(),
                    y_max: Some(1.0),
                    ..Default::default()
                },
                data: audio_receive_stats.audio_energy.data,
            };

            let jitter_buffer_delay = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Jitter Buffer Delay (ssrc={ssrc})"),
                    chart_name: chart("jitter_buffer_delay"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "milliseconds".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.jitter_buffer_delay.data,
            };

            let jitter_buffer_target_delay = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Jitter Buffer Target Delay (ssrc={ssrc})"),
                    chart_name: chart("jitter_buffer_target_delay"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "milliseconds".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.jitter_buffer_target_delay.data,
            };

            let jitter_buffer_flushes = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Jitter Buffer Flushes (ssrc={ssrc})"),
                    chart_name: chart("jitter_buffer_flushes"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Flushes".to_string(),
                    show_total: true,
                    ..Default::default()
                },
                data: audio_receive_stats.jitter_buffer_flushes.data,
            };

            let concealed_samples_pct = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Concealed Samples (ssrc={ssrc})"),
                    chart_name: chart("concealed_samples_pct"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "%".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.concealed_samples_pct.data,
            };

            let fec_packets_received = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive FEC Packets Received (ssrc={ssrc})"),
                    chart_name: chart("fec_packets_received"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Packets".to_string(),
                    show_total: true,
                    ..Default::default()
                },
                data: audio_receive_stats.fec_packets_received.data,
            };

            let relative_arrival_delay_per_packet = Stats {
                config: StatsConfig {
                    title: format!("Audio Receive Relative Arrival Delay Per Packet (ssrc={ssrc})"),
                    chart_name: chart("relative_arrival_delay_per_packet"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "milliseconds".to_string(),
                    ..Default::default()
                },
                data: audio_receive_stats.relative_arrival_delay_per_packet.data,
            };

            audio_receive_stats_list.push(AudioReceiveStats {
                ssrc: audio_receive_stats.ssrc,
                // Resolved later, once every client's outbound SSRC is known.
                sender_name: None,
                packets_per_second,
                packet_loss,
                bitrate,
                jitter,
                audio_energy,
                jitter_buffer_delay,
                jitter_buffer_target_delay,
                jitter_buffer_flushes,
                concealed_samples_pct,
                fec_packets_received,
                relative_arrival_delay_per_packet,
            });
        }

        let mut video_receive_stats_list = Vec::with_capacity(video_receive_stats_map.len());
        for (ssrc, video_receive_stats) in video_receive_stats_map {
            let chart =
                |metric: &str| format!("{client_name}.log.video.receive.{metric}.ssrc-{ssrc}.svg");
            let packets_per_second = Stats {
                config: StatsConfig {
                    title: format!("Video Receive Packet Rate (ssrc={ssrc})"),
                    chart_name: chart("packet_rate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Packets/Second".to_string(),
                    ..Default::default()
                },
                data: video_receive_stats.packets_per_second.data,
            };

            let packet_loss = Stats {
                config: StatsConfig {
                    title: format!("Video Receive Packet Loss (ssrc={ssrc})"),
                    chart_name: chart("loss"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "%".to_string(),
                    ..Default::default()
                },
                data: video_receive_stats.packet_loss.data,
            };

            let bitrate = Stats {
                config: StatsConfig {
                    title: format!("Video Receive Bitrate (ssrc={ssrc})"),
                    chart_name: chart("bitrate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Kbps".to_string(),
                    ..Default::default()
                },
                data: video_receive_stats.bitrate.data,
            };

            let framerate = Stats {
                config: StatsConfig {
                    title: format!("Video Receive Framerate (ssrc={ssrc})"),
                    chart_name: chart("framerate"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "fps".to_string(),
                    y_max: Some(32.0),
                    ..Default::default()
                },
                data: video_receive_stats.framerate.data,
            };

            let key_frames_decoded = Stats {
                config: StatsConfig {
                    title: format!("Video Key Frames Decoded (ssrc={ssrc})"),
                    chart_name: chart("key_frames_decoded"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "# frames".to_string(),
                    ..Default::default()
                },
                data: video_receive_stats.key_frames_decoded.data,
            };

            let resolution = Stats {
                config: StatsConfig {
                    title: format!("Video Receive Resolution (ssrc={ssrc})"),
                    chart_name: chart("resolution"),
                    x_label: "Test Seconds".to_string(),
                    y_label: "Resolution (sq. pixels)".to_string(),
                    ..Default::default()
                },
                data: video_receive_stats.resolution.data,
            };

            video_receive_stats_list.push(VideoReceiveStats {
                ssrc: video_receive_stats.ssrc,
                // Resolved later, once every client's outbound SSRCs are known.
                sender_name: None,
                packets_per_second,
                packet_loss,
                bitrate,
                framerate,
                key_frames_decoded,
                resolution,
            });
        }

        let out_bitrate = Stats {
            config: StatsConfig {
                title: "Adaptation Bitrate Changes".to_string(),
                chart_name: format!("{}.audio.adaptation.out_bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                x_max: Some(audio_adaptation.out_bitrate.data.max_index + 5.0),
                line_shape: LineShape::Hv,
                ..Default::default()
            },
            data: audio_adaptation.out_bitrate.data,
        };

        let out_packet_length = Stats {
            config: StatsConfig {
                title: "Adaptation Packet Length Changes".to_string(),
                chart_name: format!("{}.audio.adaptation.out_packet_length.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                x_max: Some(audio_adaptation.out_packet_length.data.max_index + 5.0),
                line_shape: LineShape::Hv,
                ..Default::default()
            },
            data: audio_adaptation.out_packet_length.data,
        };

        let out_loss_floor = Stats {
            config: StatsConfig {
                title: "Adaptation Loss Floor Changes".to_string(),
                chart_name: format!("{}.audio.adaptation.out_loss_floor.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "%".to_string(),
                x_max: Some(audio_adaptation.out_loss_floor.data.max_index + 5.0),
                line_shape: LineShape::Hv,
                ..Default::default()
            },
            data: audio_adaptation.out_loss_floor.data,
        };

        let in_bandwidth = Stats {
            config: StatsConfig {
                title: "Adaptation Bandwidth Input".to_string(),
                chart_name: format!("{}.audio.adaptation.in_bandwidth.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                x_max: Some(audio_adaptation.in_bandwidth.data.max_index + 5.0),
                ..Default::default()
            },
            data: audio_adaptation.in_bandwidth.data,
        };

        let in_rtt = Stats {
            config: StatsConfig {
                title: "Adaptation RTT Input".to_string(),
                chart_name: format!("{}.audio.adaptation.in_rtt.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                x_max: Some(audio_adaptation.in_rtt.data.max_index + 5.0),
                ..Default::default()
            },
            data: audio_adaptation.in_rtt.data,
        };

        let in_loss = Stats {
            config: StatsConfig {
                title: "Adaptation Loss Input".to_string(),
                chart_name: format!("{}.audio.adaptation.in_loss.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "%".to_string(),
                x_max: Some(audio_adaptation.in_loss.data.max_index + 5.0),
                ..Default::default()
            },
            data: audio_adaptation.in_loss.data,
        };

        let audio_adaptation = AudioAdaptation {
            out_bitrate,
            out_packet_length,
            out_loss_floor,
            in_bandwidth,
            in_rtt,
            in_loss,
        };

        Ok(Self {
            connection_stats,
            audio_send_stats,
            video_send_stats: video_send_stats_list,
            audio_receive_stats_list,
            video_receive_stats_list,
            audio_adaptation,
        })
    }
}

#[derive(Debug)]
pub struct Report {
    pub report_name: String,
    pub test_path: String,

    pub test_case_name: String,
    pub sound_name: String,
    pub video_name: String,
    pub network_profile: NetworkProfile,

    pub client_name: String,
    pub client_name_wav: String,

    /// Only available if there were actually media files generated.
    pub analysis_report: Option<AnalysisReport>,

    pub docker_stats_report: DockerStatsReport,
    pub client_log_report: ClientLogReport,

    /// If there is no video being tested, don't generate charts or columns for it.
    pub show_video: bool,
    /// If the client does not use adaptation, don't generate charts for it.
    pub show_adaptation: bool,
    /// Keep track of how many iterations were assigned for the test case.
    pub iterations: u16,
}

impl Report {
    /// Build a report from client_b's perspective.
    /// Builds the report for a single client. Video analysis is only ever produced for clients
    /// other than A, since client A's sound is the reference everyone else is measured against.
    pub async fn build(
        client: &Client<'_>,
        client_config: &CallConfig,
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
        audio_test_results: AudioTestResults,
    ) -> Result<Self> {
        let analysis_report = if test_case_config.save_media_files {
            Some(
                AnalysisReport::build(
                    // Note: _Move_ the audio_test_results to the report.
                    audio_test_results,
                    client
                        .output_yuv
                        .as_ref()
                        .map(|output_yuv| format!("{}/{}.json", test_case.test_path, output_yuv))
                        .as_deref(),
                )
                .await?,
            )
        } else {
            None
        };
        let docker_stats_report = DockerStatsReport::build(
            &format!("{}/{}_stats.log", test_case.test_path, client.name),
            &client.name,
        )
        .await?;
        let client_log_report = ClientLogReport::build(
            &format!("{}/{}.log", test_case.test_path, client.name),
            &client.name,
            client_config.audio.adaptation > 0,
        )
        .await?;

        Ok(Report {
            report_name: test_case.report_name.to_string(),
            test_path: test_case.test_path.to_string(),
            test_case_name: test_case.test_case_name.to_string(),
            sound_name: test_case.client_a().sound.name.to_string(),
            video_name: test_case
                .client_a()
                .video
                .map(|v| v.name.clone())
                .unwrap_or_default(),
            network_profile: test_case.network_profile.clone(),
            client_name: client.name.to_string(),
            client_name_wav: client.output_wav.to_string(),
            analysis_report,
            docker_stats_report,
            client_log_report,
            // Video columns are shown if anyone in the call is sending video.
            show_video: test_case_config
                .usable_client_configs()
                .iter()
                .any(|config| config.video.input_name.is_some())
                || client_config.video.input_name.is_some(),
            show_adaptation: client_config.audio.adaptation > 0,
            iterations: test_case_config.iterations,
        })
    }

    /// Resolves each inbound audio stream to the client that sent it, using the map of every
    /// client's outbound audio SSRC. Streams with no match keep a `None` sender and are
    /// reported by raw SSRC.
    pub fn label_senders(&mut self, sender_name_by_ssrc: &HashMap<String, String>) {
        for stats in &mut self.client_log_report.audio_receive_stats_list {
            stats.sender_name = sender_name_by_ssrc.get(&stats.ssrc).cloned();
        }
        for stats in &mut self.client_log_report.video_receive_stats_list {
            stats.sender_name = sender_name_by_ssrc.get(&stats.ssrc).cloned();
        }
    }

    /// Identifies an inbound stream by SSRC and the client that sent it. The sender reads
    /// `unknown` when it could not be resolved, for example if that sender's own report failed
    /// to build.
    fn stream_label(sender_name: Option<&str>, ssrc: &str) -> String {
        format!("(ssrc={ssrc},sender={})", sender_name.unwrap_or("unknown"))
    }

    /// Every SSRC this client sends on. The SFU forwards media without rewriting the SSRC, so a
    /// receiver's inbound SSRC is the sender's outbound SSRC, which is how the two get matched
    /// up. Video is simulcast over several SSRCs and the receiver only sees whichever layer was
    /// forwarded, so all of them are included.
    pub fn send_ssrcs(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.client_log_report.audio_send_stats.ssrc.as_str()).chain(
            self.client_log_report
                .video_send_stats
                .iter()
                .map(|stats| stats.ssrc.as_str()),
        )
    }

    fn create_bar_chart(
        test_path: &str,
        stats: &Stats,
        domain: Vec<String>,
        data: Vec<f32>,
    ) -> Result<()> {
        let output_path = format!("{}/{}", test_path, stats.config.chart_name);
        let root = SVGBackend::new(&output_path, (800, 600)).into_drawing_area();
        root.fill(&WHITE)?;

        let y_min = stats.config.y_min.unwrap_or(0.0);
        let y_max = stats
            .config
            .y_max
            .unwrap_or((stats.data.overall_max * 1.1).max(1.0));

        let mut chart = ChartBuilder::on(&root)
            .caption(
                &stats.config.title,
                ("Arial", 30).into_font().color(&DIM_GRAY),
            )
            .margin(5)
            .margin_right(25)
            .x_label_area_size(60)
            .y_label_area_size(60)
            .build_cartesian_2d(domain.into_segmented(), y_min..y_max)?;

        chart
            .configure_mesh()
            .x_desc(&stats.config.x_label)
            .y_desc(&stats.config.y_label)
            .y_label_formatter(&|y| format!("{:.1}", y))
            .axis_style(DIM_GRAY)
            .light_line_style(VERY_LIGHT_GRAY)
            .max_light_lines(1)
            .disable_x_mesh()
            .label_style(("Arial", 16).into_font().color(&DIM_GRAY))
            .axis_desc_style(("Arial", 18).into_font().color(&DIM_GRAY))
            .draw()?;

        chart.draw_series(
            Histogram::vertical(&chart)
                .style(STEEL_BLUE.filled())
                .margin(25)
                .data(domain.iter().zip(data.iter()).map(|(x, y)| (x, *y))),
        )?;

        root.present()?;

        Ok(())
    }

    fn create_line_chart(test_path: &str, stats: &Stats) -> Result<()> {
        let output_path = format!("{}/{}", test_path, stats.config.chart_name);
        let root = SVGBackend::new(&output_path, (800, 600)).into_drawing_area();
        root.fill(&WHITE)?;

        let x_min = stats.config.x_min.unwrap_or(0.0);
        let x_max = stats
            .config
            .x_max
            .unwrap_or(stats.data.points.len() as f32 + 5.0);
        let y_min = stats.config.y_min.unwrap_or(0.0);
        let y_max = stats
            .config
            .y_max
            .unwrap_or((stats.data.overall_max * 1.1).max(1.0));

        let mut chart = ChartBuilder::on(&root)
            .caption(
                &stats.config.title,
                ("Arial", 30).into_font().color(&DIM_GRAY),
            )
            .margin(5)
            .margin_right(25)
            .x_label_area_size(60)
            .y_label_area_size(60)
            .build_cartesian_2d(x_min..x_max, y_min..y_max)?;

        chart
            .configure_mesh()
            .x_desc(&stats.config.x_label)
            .y_desc(&stats.config.y_label)
            .x_label_formatter(&|x| format!("{:.1}", x))
            .y_label_formatter(&|y| format!("{:.1}", y))
            .axis_style(DIM_GRAY)
            .light_line_style(VERY_LIGHT_GRAY)
            .max_light_lines(1)
            .label_style(("Arial", 16).into_font().color(&DIM_GRAY))
            .axis_desc_style(("Arial", 18).into_font().color(&DIM_GRAY))
            .draw()?;

        let marker_size = if stats.data.points.len() > 60 { 1 } else { 5 };

        // Apply line shaping to the data points.
        let data_points: Vec<(f32, f32)> = if matches!(stats.config.line_shape, LineShape::Hv) {
            // Convert to horizontal-vertical line shape since plotters doesn't support it.
            let mut hv_points = Vec::new();
            for i in 0..stats.data.points.len() {
                let (x, y) = stats.data.points[i];
                hv_points.push((x, y));

                if i < stats.data.points.len() - 1 {
                    let next_x = stats.data.points[i + 1].0;
                    hv_points.push((next_x, y));
                }
            }
            hv_points
        } else {
            // Default to LineShape::Linear.
            stats.data.points.clone()
        };

        chart.draw_series(LineSeries::new(
            data_points.iter().cloned(),
            STEEL_BLUE.stroke_width(2),
        ))?;

        chart.draw_series(
            stats
                .data
                .points
                .iter()
                .cloned()
                .map(|(x, y)| Circle::new((x, y), marker_size, STEEL_BLUE.filled())),
        )?;

        root.present()?;

        Ok(())
    }

    pub async fn create_charts(&self, test_path: &str) {
        let connection_stats = &self.client_log_report.connection_stats;
        let audio_send_stats = &self.client_log_report.audio_send_stats;
        let audio_receive_stats = &self.client_log_report.audio_receive_stats_list;
        let video_send_stats = &self.client_log_report.video_send_stats;
        let video_receive_stats = &self.client_log_report.video_receive_stats_list;
        let audio_adaptation = &self.client_log_report.audio_adaptation;

        let mut line_chart_stats = vec![
            &self.docker_stats_report.cpu_usage,
            &self.docker_stats_report.mem_usage,
            &self.docker_stats_report.tx_bitrate,
            &self.docker_stats_report.rx_bitrate,
            &connection_stats.current_round_trip_time,
            &connection_stats.available_outgoing_bitrate,
            &audio_send_stats.packets_per_second,
            &audio_send_stats.average_packet_size,
            &audio_send_stats.bitrate,
            &audio_send_stats.remote_packet_loss,
            &audio_send_stats.remote_jitter,
            &audio_send_stats.remote_round_trip_time,
            &audio_send_stats.audio_energy,
        ];
        if self.show_adaptation {
            line_chart_stats.extend([
                &audio_adaptation.out_bitrate,
                &audio_adaptation.out_packet_length,
                &audio_adaptation.out_loss_floor,
                &audio_adaptation.in_bandwidth,
                &audio_adaptation.in_rtt,
                &audio_adaptation.in_loss,
            ]);
        }
        line_chart_stats.extend(audio_receive_stats.iter().flat_map(|per_ssrc| {
            vec![
                &per_ssrc.packets_per_second,
                &per_ssrc.packet_loss,
                &per_ssrc.bitrate,
                &per_ssrc.jitter,
                &per_ssrc.audio_energy,
                &per_ssrc.jitter_buffer_delay,
                &per_ssrc.jitter_buffer_target_delay,
                &per_ssrc.jitter_buffer_flushes,
                &per_ssrc.concealed_samples_pct,
                &per_ssrc.fec_packets_received,
                &per_ssrc.relative_arrival_delay_per_packet,
            ]
        }));

        if self.show_video {
            line_chart_stats.extend(video_send_stats.iter().flat_map(|per_ssrc| {
                vec![
                    &per_ssrc.packets_per_second,
                    &per_ssrc.average_packet_size,
                    &per_ssrc.bitrate,
                    &per_ssrc.source_framerate,
                    &per_ssrc.framerate,
                    &per_ssrc.key_frames_encoded,
                    &per_ssrc.retransmitted_packets_sent,
                    &per_ssrc.retransmitted_bitrate,
                    &per_ssrc.send_delay_per_packet,
                    &per_ssrc.nack_count,
                    &per_ssrc.pli_count,
                    &per_ssrc.remote_packet_loss,
                    &per_ssrc.remote_jitter,
                    &per_ssrc.remote_round_trip_time,
                    &per_ssrc.source_resolution,
                    &per_ssrc.resolution,
                ]
            }));

            line_chart_stats.extend(video_receive_stats.iter().flat_map(|per_ssrc| {
                vec![
                    &per_ssrc.packets_per_second,
                    &per_ssrc.packet_loss,
                    &per_ssrc.bitrate,
                    &per_ssrc.framerate,
                    &per_ssrc.key_frames_decoded,
                    &per_ssrc.resolution,
                ]
            }));
        }

        if let Some(analysis_report) = &self.analysis_report {
            // Generate charts for audio mos results if they represent a series.
            let audio_reports = [
                &analysis_report.audio_test_results.visqol_mos_speech,
                &analysis_report.audio_test_results.visqol_mos_audio,
                &analysis_report.audio_test_results.pesq_mos,
                &analysis_report.audio_test_results.plc_mos,
                &analysis_report.audio_test_results.mos_average,
            ];
            for report in audio_reports {
                if let AnalysisReportMos::Series(stats) = report {
                    line_chart_stats.push(stats);
                }
            }
        }

        let mut set = JoinSet::new();
        for stats in line_chart_stats.into_iter() {
            let path = test_path.to_string();
            let stats = stats.clone();
            set.spawn_blocking(move || {
                if let Err(err) = Report::create_line_chart(&path, &stats) {
                    error!("create_line_chart() error: {}", err);
                }
            });
        }
        while (set.join_next().await).is_some() {}
    }

    /// Writes the report page for a test case, with a section per client.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_test_case_report(
        reports: &[Report],
        set_name: &str,
        reference_spectrogram: &str,
        network_configs: &Vec<NetworkConfigWithOffset>,
        test_case_config: &TestCaseConfig,
        client_names: &[&str],
    ) -> Result<()> {
        let Some(first) = reports.first() else {
            return Ok(());
        };
        let html = Html::new();

        let mut buf = vec![];
        buf.extend_from_slice(
            html.header(&format!("{}/{} Report", set_name, first.report_name))
                .as_bytes(),
        );
        buf.extend_from_slice(html.network_config_section(network_configs).as_bytes());
        buf.extend_from_slice(
            html.call_config_section(test_case_config, client_names)
                .as_bytes(),
        );

        for report in reports {
            buf.extend_from_slice(&report.client_report_section(
                &html,
                set_name,
                reference_spectrogram,
                test_case_config,
            ));
        }

        buf.extend_from_slice(html.footer().as_bytes());

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&format!("{}/report.html", first.test_path))
            .await?;

        if let Err(err) = file.write_all(buf.as_slice()).await {
            error!("Error writing file! {err}");
        }

        Ok(())
    }

    /// The report sections for a single client. Accordion ids are prefixed with the client name
    /// so that sections from different clients on the same page stay independent.
    fn client_report_section(
        &self,
        html: &Html,
        set_name: &str,
        reference_spectrogram: &str,
        test_case_config: &TestCaseConfig,
    ) -> Vec<u8> {
        let id = |name: &str| format!("{}-{}", self.client_name, name);

        let mut buf = vec![];
        buf.extend_from_slice(
            html.report_heading(
                set_name,
                test_case_config,
                &self.report_name,
                &self.client_name,
                &self.analysis_report,
            )
            .as_bytes(),
        );

        // Add charts for audio mos results if they represent a series to the "Audio Core" section.
        let mut audio_core_stats: Vec<&Stats> = vec![];

        if let Some(analysis_report) = &self.analysis_report {
            if let AnalysisReportMos::Series(stats) =
                &analysis_report.audio_test_results.visqol_mos_speech
            {
                audio_core_stats.push(stats);
            }
            if let AnalysisReportMos::Series(stats) =
                &analysis_report.audio_test_results.visqol_mos_audio
            {
                audio_core_stats.push(stats);
            }
            if let AnalysisReportMos::Series(stats) = &analysis_report.audio_test_results.pesq_mos {
                audio_core_stats.push(stats);
            }
            if let AnalysisReportMos::Series(stats) = &analysis_report.audio_test_results.plc_mos {
                audio_core_stats.push(stats);
            }
            if let AnalysisReportMos::Series(stats) =
                &analysis_report.audio_test_results.mos_average
            {
                audio_core_stats.push(stats);
            }
        }

        if !audio_core_stats.is_empty() {
            let audio_core_stats = Self::build_stats_rows(html, &audio_core_stats);
            buf.extend_from_slice(
                html.accordion_section(
                    &id("audioCore"),
                    vec![HtmlAccordionItem {
                        label: "Call Audio Core".to_string(),
                        body: audio_core_stats,
                        collapsed: true,
                    }],
                )
                .as_bytes(),
            );
        }

        if test_case_config
            .client_b_config()
            .audio
            .generate_spectrogram
        {
            buf.extend_from_slice(
                html.accordion_section(
                    &id("spectrograms"),
                    vec![HtmlAccordionItem {
                        label: "Call Audio Spectrograms".to_string(),
                        body: html.two_image_section(
                            Some("Call Audio Spectrograms"),
                            reference_spectrogram,
                            Some("Original"),
                            &format!("{}.png", self.client_name_wav),
                            Some("Measured"),
                        ),
                        collapsed: true,
                    }],
                )
                .as_bytes(),
            );
        }

        let container_stats = Self::build_stats_rows(
            html,
            &[
                &self.docker_stats_report.cpu_usage,
                &self.docker_stats_report.mem_usage,
                &self.docker_stats_report.tx_bitrate,
                &self.docker_stats_report.rx_bitrate,
            ],
        );
        buf.extend_from_slice(
            html.accordion_section(
                &id("dockerStats"),
                vec![HtmlAccordionItem {
                    label: "Docker Stats".to_string(),
                    body: container_stats,
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        if self.show_adaptation {
            let audio_adaptation = &self.client_log_report.audio_adaptation;
            let audio_adaptation = Self::build_stats_rows(
                html,
                &[
                    &audio_adaptation.out_bitrate,
                    &audio_adaptation.in_bandwidth,
                    &audio_adaptation.out_packet_length,
                    &audio_adaptation.in_rtt,
                    &audio_adaptation.out_loss_floor,
                    &audio_adaptation.in_loss,
                ],
            );
            buf.extend_from_slice(
                html.accordion_section(
                    &id("audioAdaptation"),
                    vec![HtmlAccordionItem {
                        label: "Audio Adaptation".to_string(),
                        body: audio_adaptation,
                        collapsed: true,
                    }],
                )
                .as_bytes(),
            );
        }

        let connection_stats = &self.client_log_report.connection_stats;
        let connection_stats = Self::build_stats_rows(
            html,
            &[
                &connection_stats.current_round_trip_time,
                &connection_stats.available_outgoing_bitrate,
            ],
        );
        buf.extend_from_slice(
            html.accordion_section(
                &id("connectionStats"),
                vec![HtmlAccordionItem {
                    label: "Client Connection Stats".to_string(),
                    body: connection_stats,
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        let audio_send_stats = &self.client_log_report.audio_send_stats;
        let audio_send_stats_body = Self::build_stats_rows(
            html,
            &[
                &audio_send_stats.packets_per_second,
                &audio_send_stats.average_packet_size,
                &audio_send_stats.bitrate,
                &audio_send_stats.remote_packet_loss,
                &audio_send_stats.remote_jitter,
                &audio_send_stats.remote_round_trip_time,
                &audio_send_stats.audio_energy,
            ],
        );
        buf.extend_from_slice(
            html.accordion_section(
                &id("audioSendStats"),
                vec![HtmlAccordionItem {
                    label: format!("Client Audio Send Stats (SSRC={})", audio_send_stats.ssrc),
                    body: audio_send_stats_body,
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        for audio_receive_stats in &self.client_log_report.audio_receive_stats_list {
            let ssrc = &audio_receive_stats.ssrc;
            let sender_name = audio_receive_stats.sender_name.as_deref();
            let audio_receive_stats = Self::build_stats_rows(
                html,
                &[
                    &audio_receive_stats.packets_per_second,
                    &audio_receive_stats.packet_loss,
                    &audio_receive_stats.bitrate,
                    &audio_receive_stats.jitter,
                    &audio_receive_stats.jitter_buffer_delay,
                    &audio_receive_stats.jitter_buffer_target_delay,
                    &audio_receive_stats.jitter_buffer_flushes,
                    &audio_receive_stats.audio_energy,
                    &audio_receive_stats.concealed_samples_pct,
                    &audio_receive_stats.fec_packets_received,
                    &audio_receive_stats.relative_arrival_delay_per_packet,
                ],
            );
            buf.extend_from_slice(
                html.accordion_section(
                    &id(&format!("audioReceiveStats-{ssrc}")),
                    vec![HtmlAccordionItem {
                        label: format!(
                            "Client Audio Receive Stats {}",
                            Self::stream_label(sender_name, ssrc)
                        ),
                        body: audio_receive_stats,
                        collapsed: true,
                    }],
                )
                .as_bytes(),
            );
        }

        if self.show_video {
            for video_send_stats in &self.client_log_report.video_send_stats {
                let ssrc = &video_send_stats.ssrc;
                let video_send_stats = Self::build_stats_rows(
                    html,
                    &[
                        &video_send_stats.packets_per_second,
                        &video_send_stats.average_packet_size,
                        &video_send_stats.bitrate,
                        &video_send_stats.source_framerate,
                        &video_send_stats.framerate,
                        &video_send_stats.key_frames_encoded,
                        &video_send_stats.retransmitted_packets_sent,
                        &video_send_stats.retransmitted_bitrate,
                        &video_send_stats.send_delay_per_packet,
                        &video_send_stats.nack_count,
                        &video_send_stats.pli_count,
                        &video_send_stats.remote_packet_loss,
                        &video_send_stats.remote_jitter,
                        &video_send_stats.remote_round_trip_time,
                    ],
                );

                buf.extend_from_slice(
                    html.accordion_section(
                        &id(&format!("videoSendStats-{ssrc}")),
                        vec![HtmlAccordionItem {
                            label: format!("Client Video Send Stats (ssrc={ssrc})"),
                            body: video_send_stats,
                            collapsed: true,
                        }],
                    )
                    .as_bytes(),
                );
            }

            for video_receive_stats in &self.client_log_report.video_receive_stats_list {
                let ssrc = &video_receive_stats.ssrc;
                let sender_name = video_receive_stats.sender_name.as_deref();
                let video_receive_stats = Self::build_stats_rows(
                    html,
                    &[
                        &video_receive_stats.packets_per_second,
                        &video_receive_stats.packet_loss,
                        &video_receive_stats.bitrate,
                        &video_receive_stats.framerate,
                        &video_receive_stats.key_frames_decoded,
                    ],
                );

                buf.extend_from_slice(
                    html.accordion_section(
                        &id(&format!("videoReceiveStats-{ssrc}")),
                        vec![HtmlAccordionItem {
                            label: format!(
                                "Client Video Receive Stats {}",
                                Self::stream_label(sender_name, ssrc)
                            ),
                            body: video_receive_stats,
                            collapsed: true,
                        }],
                    )
                    .as_bytes(),
                );
            }
        }

        buf
    }

    fn build_stats_rows(html: &Html, stats_charts: &[&Stats]) -> String {
        let mut stats_html = String::new();

        // Put up to two charts per row.
        for stats_chart in stats_charts.chunks(2) {
            match stats_chart {
                [stats_chart_left, stats_chart_right] => {
                    stats_html.push_str(&html.stats_row(
                        Some(stats_chart_left),
                        Some(stats_chart_right),
                        false,
                        true,
                    ));
                }
                [stats_chart_left] => {
                    stats_html.push_str(&html.stats_row(Some(stats_chart_left), None, false, true));
                }
                _ => {}
            }
        }

        stats_html
    }

    /// Return the stats value (the average) for the given dimension.
    fn get_stats_value_for_chart(report: &Report, chart_dimension: &ChartDimension) -> f32 {
        match chart_dimension {
            ChartDimension::MosSpeech => report.analysis_report.as_ref().map_or(0f32, |ar| {
                ar.audio_test_results
                    .visqol_mos_speech
                    .get_mos_for_display()
                    .unwrap_or(0f32)
            }),
            ChartDimension::MosAudio => report.analysis_report.as_ref().map_or(0f32, |ar| {
                ar.audio_test_results
                    .visqol_mos_audio
                    .get_mos_for_display()
                    .unwrap_or(0f32)
            }),
            ChartDimension::ContainerCpuUsage => report.docker_stats_report.cpu_usage.data.ave,
            ChartDimension::ContainerMemUsage => report.docker_stats_report.mem_usage.data.ave,
            ChartDimension::ContainerTxBitrate => report.docker_stats_report.tx_bitrate.data.ave,
            ChartDimension::ContainerRxBitrate => report.docker_stats_report.rx_bitrate.data.ave,
            ChartDimension::ConnectionCurrentRoundTripTime => {
                report
                    .client_log_report
                    .connection_stats
                    .current_round_trip_time
                    .data
                    .ave
            }
            ChartDimension::ConnectionOutgoingBitrate => {
                report
                    .client_log_report
                    .connection_stats
                    .available_outgoing_bitrate
                    .data
                    .ave
            }
            ChartDimension::AudioSendPacketsPerSecond => {
                report
                    .client_log_report
                    .audio_send_stats
                    .packets_per_second
                    .data
                    .ave
            }
            ChartDimension::AudioSendPacketSize => {
                report
                    .client_log_report
                    .audio_send_stats
                    .average_packet_size
                    .data
                    .ave
            }
            ChartDimension::AudioSendBitrate => {
                report.client_log_report.audio_send_stats.bitrate.data.ave
            }
            ChartDimension::AudioSendRemotePacketLoss => {
                report
                    .client_log_report
                    .audio_send_stats
                    .remote_packet_loss
                    .data
                    .ave
            }
            ChartDimension::AudioSendRemoteJitter => {
                report
                    .client_log_report
                    .audio_send_stats
                    .remote_jitter
                    .data
                    .ave
            }
            ChartDimension::AudioSendRemoteRoundTripTime => {
                report
                    .client_log_report
                    .audio_send_stats
                    .remote_round_trip_time
                    .data
                    .ave
            }
            ChartDimension::AudioSendAudioEnergy => {
                report
                    .client_log_report
                    .audio_send_stats
                    .audio_energy
                    .data
                    .ave
            }
            ChartDimension::AudioReceivePacketsPerSecond => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.packets_per_second.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceivePacketLoss => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.packet_loss.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceiveBitrate => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.bitrate.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceiveJitter => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.jitter.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceiveAudioEnergy => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.audio_energy.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceiveJitterBufferDelay => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.jitter_buffer_delay.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceiveJitterBufferTargetDelay => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.jitter_buffer_target_delay.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceiveConcealedSamplesPct => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.concealed_samples_pct.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::AudioReceiveFecPacketsReceived => average(
                &report
                    .client_log_report
                    .audio_receive_stats_list
                    .iter()
                    .map(|stats| stats.fec_packets_received.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendPacketsPerSecond => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.packets_per_second.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendPacketSize => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.average_packet_size.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendBitrate => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.bitrate.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendFramerate => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.framerate.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendKeyFramesEncoded => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.key_frames_encoded.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendRetransmittedPacketsSent => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.retransmitted_packets_sent.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendRetransmittedBitrate => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.retransmitted_bitrate.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendDelayPerPacket => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.send_delay_per_packet.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendNackCount => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.nack_count.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendPliCount => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.pli_count.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendRemotePacketLoss => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.remote_packet_loss.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendRemoteJitter => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.remote_jitter.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoSendRemoteRoundTripTime => average(
                &report
                    .client_log_report
                    .video_send_stats
                    .iter()
                    .map(|stats| stats.remote_round_trip_time.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoReceivePacketsPerSecond => average(
                &report
                    .client_log_report
                    .video_receive_stats_list
                    .iter()
                    .map(|stats| stats.packets_per_second.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoReceivePacketLoss => average(
                &report
                    .client_log_report
                    .video_receive_stats_list
                    .iter()
                    .map(|stats| stats.packet_loss.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoReceiveBitrate => average(
                &report
                    .client_log_report
                    .video_receive_stats_list
                    .iter()
                    .map(|stats| stats.bitrate.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoReceiveFramerate => average(
                &report
                    .client_log_report
                    .video_receive_stats_list
                    .iter()
                    .map(|stats| stats.framerate.data.ave)
                    .collect_vec(),
            ),
            ChartDimension::VideoReceiveKeyFramesDecoded => average(
                &report
                    .client_log_report
                    .video_receive_stats_list
                    .iter()
                    .map(|stats| stats.key_frames_decoded.data.ave)
                    .collect_vec(),
            ),
        }
    }

    pub async fn create_summary_report(
        set_name: &str,
        set_path: &str,
        time_started: &str,
        group_reports: &[GroupRun],
        sounds: &HashMap<String, Sound>,
    ) -> Result<()> {
        info!("Creating summary report for {}", set_name);

        let mut buf = vec![];
        let html = Html::new();

        buf.extend_from_slice(
            html.header(&format!("Summary Report: {}", set_name))
                .as_bytes(),
        );

        buf.extend_from_slice(html.summary_heading(set_name, time_started).as_bytes());

        for (i, report) in group_reports.iter().enumerate() {
            // Add the summary table to the report contents.
            let mut report_contents =
                html.summary_report_section(&report.reports, &report.group_config);

            // Add the call audio core summary table if needed.
            if let Some(call_audio_core_summary) =
                html.summary_call_audio_core_section(&report.reports, &report.group_config)
            {
                report_contents.push_str(&html.accordion_section(
                    &format!("groupCallAudioCoreSummary_{}", i),
                    vec![HtmlAccordionItem {
                        label: "Call Audio Core Summary".to_string(),
                        body: call_audio_core_summary,
                        collapsed: false,
                    }],
                ));
            }

            let mut stats_charts = vec![];

            // Now generate and show any charts configured for the group.
            for chart_dimension in &report.group_config.chart_dimensions {
                let mut domain = vec![];
                let mut data = vec![];

                // Keep our own stats object for all the MOS values we will chart.
                let mut stats = Stats::default();
                // For the summary, we want all the data values to be considered for statistics.
                stats.data.set_filter(0, usize::MAX);

                for (i, test_report) in report.reports.iter().flatten().enumerate() {
                    // Attempt to get the given x_label (if it exists).
                    let x_label = if let Some(label) = report.group_config.x_labels.get(i) {
                        label.to_string()
                    } else {
                        // For now, the default is a combination of the test case name and the
                        // network profile name, since the sound is usually constant for groups
                        // of tests. The client name is included because there is one report per
                        // client, so without it every client of a test case shares a label.
                        format!(
                            "{}/{}@{}",
                            test_report.test_case_name,
                            test_report.client_name,
                            test_report.network_profile.get_name()
                        )
                    };

                    // Get the value to chart.
                    let value = Report::get_stats_value_for_chart(test_report, chart_dimension);

                    // For charting keep the value to 3 decimal places.
                    let rounded_value = (value * 1000.0).round() / 1000.0;

                    domain.push(x_label.to_string());
                    data.push(rounded_value);

                    stats.data.push(rounded_value);
                }

                let (title, y_label) = chart_dimension.get_title_and_y_label();

                stats.config.title = title.to_string();
                stats.config.y_label = y_label.to_string();
                stats.config.y_max = Some(stats.data.max);
                stats.config.x_label = "Test Case".to_string();
                stats.config.chart_name = format!(
                    "{}.{}.chart.svg",
                    report.group_config.group_name,
                    chart_dimension.get_name()
                );

                if let Err(err) = Report::create_bar_chart(set_path, &stats, domain, data) {
                    error!("create_line_chart() error: {}", err);
                }

                stats_charts.push(stats);
            }

            if !stats_charts.is_empty() {
                let mut stats_rows = String::new();

                // Put up to two charts per row.
                for stats_chart in stats_charts.chunks(2) {
                    match stats_chart {
                        [stats_chart_left, stats_chart_right] => {
                            stats_rows.push_str(&html.stats_row(
                                Some(stats_chart_left),
                                Some(stats_chart_right),
                                false,
                                false,
                            ));
                        }
                        [stats_chart_left] => {
                            stats_rows.push_str(&html.stats_row(
                                Some(stats_chart_left),
                                None,
                                false,
                                false,
                            ));
                        }
                        _ => {}
                    }
                }

                // Add the group charts to our report as a 'sub' accordion.
                report_contents.push_str(&html.accordion_section(
                    &format!("groupReport_{}", i),
                    vec![HtmlAccordionItem {
                        label: "Charts".to_string(),
                        body: stats_rows,
                        collapsed: true,
                    }],
                ));
            }

            // Show the report accordion.
            buf.extend_from_slice(
                html.accordion_section(
                    &format!("group_{}", i),
                    vec![HtmlAccordionItem {
                        label: format!("Group: {}", report.group_config.group_name),
                        body: report_contents,
                        collapsed: false,
                    }],
                )
                .as_bytes(),
            );
        }

        buf.extend_from_slice(
            html.accordion_section(
                "mosReference",
                vec![HtmlAccordionItem {
                    label: "Reference Sounds".to_string(),
                    body: html.summary_sounds_item_body(sounds),
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        buf.extend_from_slice(html.footer().as_bytes());

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&format!("{}/summary.html", set_path))
            .await?;

        if let Err(err) = file.write_all(buf.as_slice()).await {
            error!("Error writing file! {err}");
        }

        Ok(())
    }
}

/// A summary row can represent a single record, an aggregate item, or an aggregate average,
/// calculated from 2 or more aggregate items.
#[derive(Clone, Copy, Eq, PartialEq)]
enum SummaryRowType {
    Single,
    Aggregate,
    AggregateItem,
}

/// RTT floor (5th percentile) and inflation (largest sample minus the floor), in ms.
fn rtt_floor_and_inflation(stats: &Stats) -> (f32, f32) {
    let mut samples: Vec<f32> = stats
        .data
        .points
        .iter()
        .enumerate()
        .filter(|(i, _)| *i >= stats.data.filter_min && *i < stats.data.filter_max)
        .map(|(_, &(_, rtt))| rtt)
        // Zero is not reported; above 20 s is a stale estimate.
        .filter(|rtt| *rtt > 0.0 && *rtt <= 20000.0)
        .collect();
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    samples.sort_by(f32::total_cmp);
    let floor = samples[samples.len() / 20];
    let peak = samples[samples.len() - 1];
    (floor, (peak - floor).max(0.0))
}

/// A convenience struct for tracking the averaged values for a row in the summary report. This
/// is particularly useful when aggregating values from several rows.
///
/// There is one row per inbound audio stream, so a client receiving from two senders gets two
/// rows. Loss and concealment are percentages and must not be summed across streams; keeping
/// them separate is what makes them readable.
// Carries owned identity strings, so it cannot be Copy.
#[derive(Clone)]
struct SummaryRow {
    /// The client that sent this inbound stream, when it could be resolved.
    pub sender_name: Option<String>,
    /// The inbound stream's ssrc. None when the client received nothing.
    pub ssrc: Option<String>,
    pub audio_send_packet_size: f32,
    pub audio_send_packet_rate: f32,
    pub audio_send_bitrate: f32,

    pub audio_receive_packet_rate: f32,
    pub audio_receive_bitrate: f32,
    pub audio_receive_loss: f32,
    pub audio_receive_loss_std_dev: f32,
    pub concealed_samples_pct: f32,
    pub fec_packets_received_total: f32,

    pub audio_rtt_floor: f32,
    pub audio_rtt_inflation: f32,
    pub audio_arrival_delay: f32,

    pub container_cpu: f32,
    pub container_memory: f32,
    pub container_tx_bitrate: f32,
    pub container_rx_bitrate: f32,

    pub visqol_mos_speech: Option<f32>,
    pub visqol_mos_audio: Option<f32>,
    pub pesq_mos: Option<f32>,
    pub plc_mos: Option<f32>,
    pub mos_average: Option<f32>,

    pub vmaf: Option<f32>,

    pub row_type: SummaryRowType,
    pub row_index: usize,

    pub video_send_resolution: f32,
    pub video_send_framerate: f32,
    pub video_recv_resolution: f32,
    pub video_recv_framerate: f32,
}

impl SummaryRow {
    /// One row per inbound audio stream. A client that received nothing still gets a single row
    /// with the receive columns zeroed, so it is not dropped from the summary entirely.
    pub fn rows_for(report: &Report) -> Vec<Self> {
        if report.client_log_report.audio_receive_stats_list.is_empty() {
            return vec![Self::new(report, None)];
        }
        // The stats list comes out of a HashMap, so sort for a stable row order - otherwise both
        // the ordering and which row carries the client's MOS would vary between runs.
        let mut streams: Vec<&AudioReceiveStats> = report
            .client_log_report
            .audio_receive_stats_list
            .iter()
            .collect();
        streams.sort_by(|a, b| (&a.sender_name, &a.ssrc).cmp(&(&b.sender_name, &b.ssrc)));
        streams
            .into_iter()
            .enumerate()
            .map(|(index, stats)| {
                let mut row = Self::new(report, Some(stats));
                if index > 0 {
                    // MOS is measured on the client's recording as a whole, not per inbound
                    // stream, so it is shown once - on the client's first row. Repeating it
                    // would read as a per-stream score.
                    row.visqol_mos_speech = None;
                    row.visqol_mos_audio = None;
                    row.pesq_mos = None;
                    row.plc_mos = None;
                    row.mos_average = None;
                    row.vmaf = None;
                }
                row
            })
            .collect()
    }

    fn new(report: &Report, stream: Option<&AudioReceiveStats>) -> Self {
        let (
            audio_receive_packet_rate,
            audio_receive_bitrate,
            audio_receive_loss,
            audio_receive_loss_std_dev,
            concealed_samples_pct,
            fec_packets_received_total,
        ) = stream.map_or((0.0, 0.0, 0.0, 0.0, 0.0, 0.0), |stats| {
            (
                stats.packets_per_second.data.ave,
                stats.bitrate.data.ave,
                stats.packet_loss.data.ave,
                stats.packet_loss.data.std_dev,
                stats.concealed_samples_pct.data.ave,
                stats.fec_packets_received.data.total,
            )
        });
        let (audio_rtt_floor, audio_rtt_inflation) = rtt_floor_and_inflation(
            &report
                .client_log_report
                .audio_send_stats
                .remote_round_trip_time,
        );
        Self {
            sender_name: stream.and_then(|s| s.sender_name.clone()),
            ssrc: stream.map(|s| s.ssrc.clone()),
            audio_send_packet_size: report
                .client_log_report
                .audio_send_stats
                .average_packet_size
                .data
                .ave,
            audio_send_packet_rate: report
                .client_log_report
                .audio_send_stats
                .packets_per_second
                .data
                .ave,
            audio_send_bitrate: report.client_log_report.audio_send_stats.bitrate.data.ave,
            audio_receive_packet_rate,
            audio_receive_bitrate,
            audio_receive_loss,
            audio_receive_loss_std_dev,
            concealed_samples_pct,
            fec_packets_received_total: fec_packets_received_total as f32,
            audio_rtt_floor,
            audio_rtt_inflation,
            audio_arrival_delay: stream.map_or(0.0, |stats| {
                stats.relative_arrival_delay_per_packet.data.ave
            }),
            container_cpu: report.docker_stats_report.cpu_usage.data.ave,
            container_memory: report.docker_stats_report.mem_usage.data.ave,
            container_tx_bitrate: report.docker_stats_report.tx_bitrate.data.ave,
            container_rx_bitrate: report.docker_stats_report.rx_bitrate.data.ave,
            visqol_mos_speech: report.analysis_report.as_ref().and_then(|ar| {
                ar.audio_test_results
                    .visqol_mos_speech
                    .get_mos_for_display()
            }),
            visqol_mos_audio: report
                .analysis_report
                .as_ref()
                .and_then(|ar| ar.audio_test_results.visqol_mos_audio.get_mos_for_display()),
            pesq_mos: report
                .analysis_report
                .as_ref()
                .and_then(|ar| ar.audio_test_results.pesq_mos.get_mos_for_display()),
            plc_mos: report
                .analysis_report
                .as_ref()
                .and_then(|ar| ar.audio_test_results.plc_mos.get_mos_for_display()),
            mos_average: report
                .analysis_report
                .as_ref()
                .and_then(|ar| ar.audio_test_results.mos_average.get_mos_for_display()),
            vmaf: report.analysis_report.as_ref().and_then(|ar| ar.vmaf),
            row_type: SummaryRowType::Single,
            row_index: 0,
            video_send_resolution: report
                .client_log_report
                .video_send_stats
                .first()
                .map(|s| s.resolution.data.ave)
                .unwrap_or(0.0),
            video_send_framerate: report
                .client_log_report
                .video_send_stats
                .first()
                .map(|s| s.framerate.data.ave)
                .unwrap_or(0.0),
            video_recv_resolution: report
                .client_log_report
                .video_receive_stats_list
                .first()
                .map(|s| s.resolution.data.ave)
                .unwrap_or(0.0),
            video_recv_framerate: report
                .client_log_report
                .video_receive_stats_list
                .first()
                .map(|s| s.framerate.data.ave)
                .unwrap_or(0.0),
        }
    }

    /// Seeded from the first sample of a stream, so the aggregate keeps that stream's identity.
    pub fn new_aggregate(row: &Self) -> Self {
        let mut aggregate = row.clone();
        aggregate.row_type = SummaryRowType::Aggregate;
        aggregate
    }

    pub fn set_aggregate_item(&mut self, row_index: usize) {
        self.row_type = SummaryRowType::AggregateItem;
        self.row_index = row_index;
    }

    /// Update the aggregated averages for all values.
    pub fn update(&mut self, new: &Self, count: usize) {
        let new_average = |old_value: f32, new_value: f32| -> f32 {
            (old_value * (count as f32 - 1f32) + new_value) / count as f32
        };

        if count > 1 {
            self.audio_send_packet_size =
                new_average(self.audio_send_packet_size, new.audio_send_packet_size);
            self.audio_send_packet_rate =
                new_average(self.audio_send_packet_rate, new.audio_send_packet_rate);
            self.audio_send_bitrate = new_average(self.audio_send_bitrate, new.audio_send_bitrate);
            self.audio_receive_packet_rate = new_average(
                self.audio_receive_packet_rate,
                new.audio_receive_packet_rate,
            );
            self.audio_receive_bitrate =
                new_average(self.audio_receive_bitrate, new.audio_receive_bitrate);
            self.audio_receive_loss = new_average(self.audio_receive_loss, new.audio_receive_loss);
            self.audio_receive_loss_std_dev = new_average(
                self.audio_receive_loss_std_dev,
                new.audio_receive_loss_std_dev,
            );
            self.concealed_samples_pct =
                new_average(self.concealed_samples_pct, new.concealed_samples_pct);
            self.audio_rtt_floor = new_average(self.audio_rtt_floor, new.audio_rtt_floor);
            self.audio_rtt_inflation =
                new_average(self.audio_rtt_inflation, new.audio_rtt_inflation);
            self.audio_arrival_delay =
                new_average(self.audio_arrival_delay, new.audio_arrival_delay);
            self.fec_packets_received_total = new_average(
                self.fec_packets_received_total,
                new.fec_packets_received_total,
            );
            self.container_cpu = new_average(self.container_cpu, new.container_cpu);
            self.container_memory = new_average(self.container_memory, new.container_memory);
            self.container_tx_bitrate =
                new_average(self.container_tx_bitrate, new.container_tx_bitrate);
            self.container_rx_bitrate =
                new_average(self.container_rx_bitrate, new.container_rx_bitrate);

            // We expect mos and vmaf to always be there or never.
            if let (Some(old), Some(new)) = (self.visqol_mos_speech, new.visqol_mos_speech) {
                self.visqol_mos_speech = Some(new_average(old, new));
            }
            if let (Some(old), Some(new)) = (self.visqol_mos_audio, new.visqol_mos_audio) {
                self.visqol_mos_audio = Some(new_average(old, new));
            }
            if let (Some(old), Some(new)) = (self.pesq_mos, new.pesq_mos) {
                self.pesq_mos = Some(new_average(old, new));
            }
            if let (Some(old), Some(new)) = (self.plc_mos, new.plc_mos) {
                self.plc_mos = Some(new_average(old, new));
            }
            if let (Some(old), Some(new)) = (self.mos_average, new.mos_average) {
                self.mos_average = Some(new_average(old, new));
            }
            if let (Some(vmaf), Some(new_vmaf)) = (self.vmaf, new.vmaf) {
                self.vmaf = Some(new_average(vmaf, new_vmaf));
            }
        }
    }
}

#[derive(Clone)]
struct SummaryCallAudioCoreRow {
    pub mos_over_time: Vec<(i32, f32)>,
    pub mos_average: Option<f32>,
    pub row_type: SummaryRowType,
    pub row_index: usize,
}

impl SummaryCallAudioCoreRow {
    pub fn new(report: &Report) -> Option<Self> {
        if let Some(AnalysisReport {
            audio_test_results:
                AudioTestResults {
                    mos_average: AnalysisReportMos::Series(stats),
                    ..
                },
            ..
        }) = &report.analysis_report
        {
            let mut mos_over_time = stats
                .data
                .points
                .iter()
                .map(|(time, mos)| (((time * 10.0).round() as i32), *mos))
                .collect_vec();
            mos_over_time.sort_by_key(|(time, _)| *time);
            Some(Self {
                mos_over_time,
                mos_average: report
                    .analysis_report
                    .as_ref()
                    .and_then(|ar| ar.audio_test_results.mos_average.get_mos_for_display()),
                row_type: SummaryRowType::Single,
                row_index: 0,
            })
        } else {
            None
        }
    }

    pub fn set_aggregate_item(&mut self, row_index: usize) {
        self.row_type = SummaryRowType::AggregateItem;
        self.row_index = row_index;
    }

    pub fn update(&mut self, new: &Self, count: usize) {
        let new_average = |old_value: f32, new_value: f32| -> f32 {
            (old_value * (count as f32 - 1f32) + new_value) / count as f32
        };

        if count > 1 {
            for (new_time, new_value) in &new.mos_over_time {
                if let Some((_, value)) = self
                    .mos_over_time
                    .iter_mut()
                    .find(|(time, _)| time == new_time)
                {
                    *value = new_average(*value, *new_value);
                } else {
                    self.mos_over_time.push((*new_time, *new_value));
                }
            }
            self.mos_over_time.sort_by_key(|(time, _)| *time);

            if let (Some(old), Some(new)) = (self.mos_average, new.mos_average) {
                self.mos_average = Some(new_average(old, new));
            }
        }
    }
}

pub struct HtmlAccordionItem {
    label: String,
    body: String,
    collapsed: bool,
}

#[derive(Clone, Copy)]
pub struct Html {}

impl Html {
    pub fn new() -> Self {
        Self {}
    }

    /// This creates the HTML header, including opening the body with a bootstrap container.
    pub fn header(self, title: &str) -> String {
        let mut buf = String::new();

        buf.push_str("<!doctype html>\n");
        buf.push_str("<html lang=\"en\">\n");
        buf.push_str("<head>\n");
        buf.push_str("<meta charset=\"utf-8\">\n");
        buf.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");

        let _ = writeln!(buf, "<title>{}</title>", title);

        buf.push_str("<link href=\"https://cdn.jsdelivr.net/npm/bootstrap@5.2.2/dist/css/bootstrap.min.css\" \
                  rel=\"stylesheet\" integrity=\"sha384-Zenh87qX5JnK2Jl0vWa8Ck2rdkQ2Bzep5IDxbcnCeuOxjzrPF/et3URy9Bv1WTRi\" \
                  crossorigin=\"anonymous\">\n");

        buf.push_str("</head>\n");
        buf.push_str("<body>\n");
        buf.push_str("<div class=\"container-fluid\">\n");

        buf
    }

    /// This closes the HTML and terminates the bootstrap container.
    pub fn footer(&self) -> String {
        let mut buf = String::new();

        buf.push_str("</div>\n");
        buf.push_str("<script src=\"https://cdn.jsdelivr.net/npm/bootstrap@5.2.2/dist/js/bootstrap.min.js\" \
                 integrity=\"sha384-IDwe1+LCz02ROU9k972gdyvl+AESN10+x7tBKgc9I5HFtuNz0wWnPclzo6p9vxnk\" \
                 crossorigin=\"anonymous\"></script>\n");
        buf.push_str("</body>\n");
        buf.push_str("</html>\n");

        buf
    }

    pub fn accordion_section(&self, id: &str, items: Vec<HtmlAccordionItem>) -> String {
        let mut buf = String::new();

        let _ = writeln!(buf, "<div class=\"accordion\" id=\"{}\">\n", id);
        buf.push_str("<div class=\"accordion-item\">\n");

        for (i, item) in items.iter().enumerate() {
            let _ = writeln!(
                buf,
                "<h4 class=\"accordion-header\" id=\"{}-heading{}\">\n",
                id, i
            );
            let _ = writeln!(
                buf,
                "<button class=\"accordion-button{}\" type=\"button\" data-bs-toggle=\"collapse\" \
                    data-bs-target=\"#{}-collapse{}\" aria-expanded=\"true\" aria-controls=\"{}-collapse{}\">\n",
                if item.collapsed { " collapsed" } else { "" },
                id,
                i,
                id,
                i
            );

            let _ = writeln!(buf, "<h4>{}</h4>\n", item.label);
            buf.push_str("</button>\n");
            buf.push_str("</h4>\n");

            let _ = writeln!(
                buf,
                "<div id=\"{}-collapse{}\" class=\"accordion-collapse collapse{}\" \
                    aria-labelledby=\"{}-heading{}\">\n",
                id,
                i,
                if item.collapsed { "" } else { " show" },
                id,
                i
            );

            buf.push_str("<div class=\"accordion-body\">\n");

            buf.push_str(&item.body);

            buf.push_str("</div>\n");
            buf.push_str("</div>\n");
        }

        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
    }

    fn get_emphasis_for_mos(mos: Option<f32>) -> &'static str {
        if let Some(weight) = mos {
            if weight >= 3.8 {
                "success"
            } else if weight >= 3.0 {
                "warning"
            } else if weight >= 0.0 {
                "danger"
            } else {
                ""
            }
        } else {
            ""
        }
    }

    pub fn report_heading(
        &self,
        set_name: &str,
        test_case_config: &TestCaseConfig,
        test_name: &str,
        client_name: &str,
        analysis_report: &Option<AnalysisReport>,
    ) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-6\">\n");

        let _ = writeln!(buf, "<h2>{}/{}</h2>", set_name, test_name);
        let _ = writeln!(buf, "<h3 class=\"text-muted\">Client: {}</h3>", client_name);

        buf.push_str("</div>\n");
        buf.push_str("<div class=\"col-md-6\">\n");

        let (visqol_mos_speech, visqol_mos_audio, pesq_mos, plc_mos, mos_average) =
            if let Some(analysis_report) = analysis_report {
                (
                    analysis_report
                        .audio_test_results
                        .visqol_mos_speech
                        .get_mos_for_display(),
                    analysis_report
                        .audio_test_results
                        .visqol_mos_audio
                        .get_mos_for_display(),
                    analysis_report
                        .audio_test_results
                        .pesq_mos
                        .get_mos_for_display(),
                    analysis_report
                        .audio_test_results
                        .plc_mos
                        .get_mos_for_display(),
                    analysis_report
                        .audio_test_results
                        .mos_average
                        .get_mos_for_display(),
                )
            } else {
                (None, None, None, None, None)
            };

        let text_emphasis = Html::get_emphasis_for_mos(mos_average);

        if test_case_config
            .client_b_config()
            .audio
            .visqol_speech_analysis
        {
            let visqol_mos_speech_string = visqol_mos_speech
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());

            let _ = writeln!(
                buf,
                "<h2 class=\"text-right text-{}\">Visqol Speech: {}</h2>",
                text_emphasis, visqol_mos_speech_string,
            );
        }

        if test_case_config
            .client_b_config()
            .audio
            .visqol_audio_analysis
        {
            let visqol_mos_audio_string = visqol_mos_audio
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());

            let _ = writeln!(
                buf,
                "<h2 class=\"text-right text-{}\">Visqol Audio: {}</h2>",
                text_emphasis, visqol_mos_audio_string,
            );
        }

        if test_case_config
            .client_b_config()
            .audio
            .pesq_speech_analysis
        {
            let pesq_mos_string = pesq_mos
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());

            let _ = writeln!(
                buf,
                "<h2 class=\"text-right text-{}\">PESQ: {}</h2>",
                text_emphasis, pesq_mos_string,
            );
        }

        if test_case_config.client_b_config().audio.plc_speech_analysis {
            let plc_mos_string = plc_mos
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());

            let _ = writeln!(
                buf,
                "<h2 class=\"text-right text-{}\">PLC: {}</h2>",
                text_emphasis, plc_mos_string,
            );
        }

        let mos_average_string = mos_average
            .map(|mos| format!("{:.3}", mos))
            .unwrap_or_else(|| "None".to_string());

        let _ = writeln!(
            buf,
            "<h2 class=\"text-right text-{}\">Average MOS: {}</h2>",
            text_emphasis, mos_average_string,
        );

        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
    }

    pub fn network_config_section(&self, network_configs: &Vec<NetworkConfigWithOffset>) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-12\">\n");

        if network_configs.is_empty() {
            buf.push_str("<h3>Network Configurations (None)</h3>\n");
        } else {
            buf.push_str("<h3>Network Configurations</h3>\n");

            buf.push_str("<table class=\"table\">\n");
            buf.push_str("<thead>\n");
            buf.push_str("<tr>\n");
            buf.push_str("<th>Timestamp</th>\n");
            buf.push_str("<th>Values</th>\n");
            buf.push_str("</tr>\n");
            buf.push_str("</thead>\n");
            buf.push_str("<tbody>\n");

            for config in network_configs {
                buf.push_str("<tr>\n");
                let _ = writeln!(buf, "<td>{}</td>", config.offset.as_secs());
                let _ = writeln!(
                    buf,
                    "<td><code><pre>\n{:#?}</pre></code></td>",
                    config.network_config
                );
                buf.push_str("</tr>\n");
            }

            buf.push_str("</tbody>\n");
            buf.push_str("</table>\n");
        }

        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
    }

    /// Renders every client's call config side by side, one column each, with any line that is
    /// not identical across all of them highlighted. Falls back to plain dumps when the configs
    /// do not have the same shape, since `field_trials` and `extra_cli_args` are variable length
    /// and a line-by-line comparison would be misaligned rather than merely unhelpful.
    pub fn call_config_section(
        &self,
        test_case_config: &TestCaseConfig,
        client_names: &[&str],
    ) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-12\">\n");
        buf.push_str("<h3>Call Configuration</h3>\n");

        let configs = test_case_config.usable_client_configs();
        let dumps: Vec<Vec<String>> = configs
            .iter()
            .map(|config| format!("{config:#?}").lines().map(str::to_string).collect())
            .collect();
        let label_for = |index: usize| -> String {
            client_names
                .get(index)
                .map(|name| name.to_string())
                .unwrap_or_else(|| format!("Client {}", index + 1))
        };

        let comparable = dumps
            .first()
            .is_some_and(|first| dumps.iter().all(|lines| lines.len() == first.len()));

        if comparable {
            buf.push_str("<p class=\"text-muted\">Highlighted rows differ between clients.</p>\n");
            buf.push_str("<div class=\"table-responsive\">\n");
            buf.push_str("<table class=\"table table-sm table-bordered font-monospace\">\n");

            buf.push_str("<thead>\n<tr>\n");
            for index in 0..dumps.len() {
                let _ = writeln!(buf, "<th>{}</th>", label_for(index));
            }
            buf.push_str("</tr>\n</thead>\n");

            buf.push_str("<tbody>\n");
            for line_index in 0..dumps[0].len() {
                let line = &dumps[0][line_index];
                let differs = dumps
                    .iter()
                    .any(|lines| lines[line_index].trim() != line.trim());

                if differs {
                    buf.push_str("<tr class=\"table-warning\">\n");
                } else {
                    buf.push_str("<tr>\n");
                }
                for lines in &dumps {
                    let _ = writeln!(
                        buf,
                        "<td style=\"white-space: pre\">{}</td>",
                        lines[line_index]
                    );
                }
                buf.push_str("</tr>\n");
            }
            buf.push_str("</tbody>\n</table>\n</div>\n");
            buf.push_str("</div>\n");
        } else {
            buf.push_str(
                "<p class=\"text-muted\">Configs have different shapes, so they are not \
                 compared line by line.</p>\n",
            );
            buf.push_str("</div>\n");

            for (index, lines) in dumps.iter().enumerate() {
                buf.push_str("<div class=\"col-md-6\">\n");
                let _ = writeln!(buf, "<h4>{}</h4>", label_for(index));
                let _ = writeln!(buf, "<p><code><pre>\n{}</pre></code></p>", lines.join("\n"));
                buf.push_str("</div>\n");
            }
        }

        buf.push_str("</div>\n");

        buf
    }

    fn stats_image_and_data(&self, stats: &Stats, show_title: bool, show_stats: bool) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"col-md-6\">\n");
        if show_title {
            // Note: Most charts already have an embedded title.
            let _ = writeln!(buf, "<h4>{}</h4>", stats.config.title);
        }
        let _ = writeln!(
            buf,
            "<img alt=\"\" class=\"img-fluid\" src=\"{}\" />",
            stats.config.chart_name
        );

        if show_stats {
            buf.push_str("<div class=\"p-3 row justify-content-center\">\n");
            buf.push_str("<div class=\"col-md-2\">\n");
            let _ = writeln!(buf, "min: {:.3}", stats.data.min);
            buf.push_str("</div>\n");
            buf.push_str("<div class=\"col-md-2\">\n");
            let _ = writeln!(buf, "max: {:.3}", stats.data.max);
            buf.push_str("</div>\n");
            buf.push_str("<div class=\"col-md-2\">\n");
            let _ = writeln!(buf, "ave: {:.3}", stats.data.ave);
            buf.push_str("</div>\n");
            buf.push_str("<div class=\"col-md-2\">\n");
            let _ = writeln!(buf, "std_dev: {:.3}", stats.data.std_dev);
            buf.push_str("</div>\n");
            if stats.config.show_total {
                buf.push_str("<div class=\"col-md-2\">\n");
                let _ = writeln!(buf, "tot: {:.0}", stats.data.total);
                buf.push_str("</div>\n");
            }
            buf.push_str("</div>\n");
        }

        buf.push_str("</div>\n");

        buf
    }

    fn stats_row(
        &self,
        stats_left: Option<&Stats>,
        stats_right: Option<&Stats>,
        show_title: bool,
        show_stats: bool,
    ) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");

        if let Some(stats) = stats_left {
            buf.push_str(&self.stats_image_and_data(stats, show_title, show_stats));
        }

        if let Some(stats) = stats_right {
            buf.push_str(&self.stats_image_and_data(stats, show_title, show_stats));
        }

        buf.push_str("</div>\n");

        buf
    }

    fn two_image_detail(
        &self,
        image_left: &str,
        image_left_title: Option<&str>,
        image_right: &str,
        image_right_title: Option<&str>,
    ) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-6\">\n");
        if let Some(title) = image_left_title {
            let _ = writeln!(buf, "<h4>{}</h4>", title);
        }
        let _ = writeln!(
            buf,
            "<img alt=\"\" class=\"img-fluid\" src=\"{}\" />",
            image_left
        );
        buf.push_str("</div>\n");
        buf.push_str("<div class=\"col-md-6\">\n");
        if let Some(title) = image_right_title {
            let _ = writeln!(buf, "<h4>{}</h4>", title);
        }
        let _ = writeln!(
            buf,
            "<img alt=\"\" class=\"img-fluid\" src=\"{}\" />",
            image_right
        );
        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
    }

    pub fn two_image_section(
        &self,
        title: Option<&str>,
        image_left: &str,
        image_left_title: Option<&str>,
        image_right: &str,
        image_right_title: Option<&str>,
    ) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-12\">\n");

        if let Some(title) = title {
            let _ = writeln!(buf, "<h3>{}</h3>", title);
        }
        buf.push_str(&self.two_image_detail(
            image_left,
            image_left_title,
            image_right,
            image_right_title,
        ));

        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
    }

    pub fn summary_heading(&self, set_name: &str, time_started: &str) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-12\">\n");

        let _ = writeln!(buf, "<h2>Test Set: {}</h2>", set_name);
        let _ = writeln!(buf, "<h3 class=\"text-muted\">Date: {}</h3>", time_started);

        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
    }

    fn summary_report_row(
        &self,
        group_config: &GroupConfig,
        report: &Report,
        summary_row: &SummaryRow,
        iteration_count_for_group: usize,
    ) -> String {
        let mut buf = String::new();

        let table_emphasis = Html::get_emphasis_for_mos(summary_row.mos_average);

        match summary_row.row_type {
            SummaryRowType::Single => {
                let _ = writeln!(
                    buf,
                    r#"<tr class="table-{} clickable" onclick="window.location='{}/{}/report.html'">"#,
                    table_emphasis, group_config.group_name, report.report_name
                );
            }
            SummaryRowType::Aggregate => {
                let _ = writeln!(
                    buf,
                    r#"<tr class="table-{}" data-bs-toggle="collapse" data-bs-target=".{}_{}_collapsed">"#,
                    table_emphasis, group_config.group_name, iteration_count_for_group
                );
            }
            SummaryRowType::AggregateItem => {
                let _ = writeln!(
                    buf,
                    r#"<tr class="table-{} clickable w-auto small fw-light collapse {}_{}_collapsed" onclick="window.location='{}/{}_{}/report.html'">"#,
                    table_emphasis,
                    group_config.group_name,
                    iteration_count_for_group,
                    group_config.group_name,
                    report.report_name,
                    summary_row.row_index
                );
            }
        }

        let indent = if summary_row.row_type == SummaryRowType::AggregateItem {
            "&nbsp;&nbsp"
        } else {
            ""
        };

        let _ = writeln!(buf, "<td>{}{}</td>", indent, report.test_case_name);
        let _ = writeln!(buf, "<td>{}{}</td>", indent, report.client_name);
        let _ = writeln!(
            buf,
            "<td>{}{}</td>",
            indent,
            match (&summary_row.sender_name, &summary_row.ssrc) {
                (Some(name), Some(ssrc)) => format!("{name} ({ssrc})"),
                (None, Some(ssrc)) => format!("unknown ({ssrc})"),
                _ => "-".to_string(),
            }
        );
        let _ = writeln!(buf, "<td>{}{}</td>", indent, report.sound_name);
        if group_config.summary_report_columns.show_video {
            let _ = writeln!(buf, "<td>{}{}</td>", indent, report.video_name);
        }
        let _ = writeln!(
            buf,
            "<td>{}{}</td>",
            indent,
            report.network_profile.get_name()
        );

        if group_config.summary_report_columns.show_send_stats {
            let _ = writeln!(
                buf,
                "<td>{}{:.0}</td>",
                indent, summary_row.audio_send_packet_size
            );
            let _ = writeln!(
                buf,
                "<td>{}{:.2}</td>",
                indent, summary_row.audio_send_packet_rate
            );
            let _ = writeln!(
                buf,
                "<td>{}{:.2}</td>",
                indent, summary_row.audio_send_bitrate
            );

            if group_config.summary_report_columns.show_video {
                let _ = writeln!(
                    buf,
                    "<td>{}{:.2}</td>",
                    indent, summary_row.video_send_resolution
                );

                let _ = writeln!(
                    buf,
                    "<td>{}{:.2}</td>",
                    indent, summary_row.video_send_framerate
                );
            }
        }

        if group_config.summary_report_columns.show_receive_stats {
            let _ = writeln!(
                buf,
                "<td>{}{:.2}</td>",
                indent, summary_row.audio_receive_packet_rate
            );
            let _ = writeln!(
                buf,
                "<td>{}{:.2}</td>",
                indent, summary_row.audio_receive_bitrate
            );
            let _ = writeln!(
                buf,
                "<td>{}{:.2}</td>",
                indent, summary_row.audio_receive_loss
            );

            // Coefficient of variation of the loss (stddev / mean). Only meaningful when
            // there is some loss to measure.
            let loss_cv = if summary_row.audio_receive_loss > 0.0 {
                format!(
                    "{:.2}",
                    summary_row.audio_receive_loss_std_dev / summary_row.audio_receive_loss
                )
            } else {
                "N/A".to_string()
            };
            let _ = writeln!(buf, "<td>{}{}</td>", indent, loss_cv);

            if group_config.summary_report_columns.show_video {
                let _ = writeln!(
                    buf,
                    "<td>{}{:.2}</td>",
                    indent, summary_row.video_recv_resolution
                );

                let _ = writeln!(
                    buf,
                    "<td>{}{:.2}</td>",
                    indent, summary_row.video_recv_framerate
                );
            }

            let _ = writeln!(
                buf,
                "<td>{}{:.2}</td>",
                indent, summary_row.concealed_samples_pct
            );

            // Concealment per unit of loss (mean concealment / mean loss).
            let concealment_per_loss = if summary_row.audio_receive_loss >= 0.5 {
                format!(
                    "{:.2}",
                    summary_row.concealed_samples_pct / summary_row.audio_receive_loss
                )
            } else {
                "N/A".to_string()
            };
            let _ = writeln!(buf, "<td>{}{}</td>", indent, concealment_per_loss);

            let _ = writeln!(
                buf,
                "<td>{}{:.0}</td>",
                indent, summary_row.fec_packets_received_total
            );
        }

        if group_config.summary_report_columns.show_delay_stats {
            let _ = writeln!(buf, "<td>{}{:.0}</td>", indent, summary_row.audio_rtt_floor);
            let _ = writeln!(
                buf,
                "<td>{}{:.0}</td>",
                indent, summary_row.audio_rtt_inflation
            );
            let _ = writeln!(
                buf,
                "<td>{}{:.0}</td>",
                indent, summary_row.audio_arrival_delay
            );
        }
        let _ = writeln!(buf, "<td>{}{:.2}</td>", indent, summary_row.container_cpu);
        let _ = writeln!(
            buf,
            "<td>{}{:.2}</td>",
            indent, summary_row.container_memory
        );
        let _ = writeln!(
            buf,
            "<td>{}{:.2}</td>",
            indent, summary_row.container_tx_bitrate
        );
        let _ = writeln!(
            buf,
            "<td>{}{:.2}</td>",
            indent, summary_row.container_rx_bitrate
        );

        // Show the mos average column if any of the mos value columns are shown.
        let mut show_mos_average = false;

        if group_config.summary_report_columns.show_visqol_mos_speech {
            show_mos_average = true;
            if let Some(mos) = summary_row.visqol_mos_speech {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }
        if group_config.summary_report_columns.show_visqol_mos_audio {
            show_mos_average = true;
            if let Some(mos) = summary_row.visqol_mos_audio {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }
        if group_config.summary_report_columns.show_pesq_mos {
            show_mos_average = true;
            if let Some(mos) = summary_row.pesq_mos {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }
        if group_config.summary_report_columns.show_plc_mos {
            show_mos_average = true;
            if let Some(mos) = summary_row.plc_mos {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }

        if show_mos_average {
            if let Some(mos) = summary_row.mos_average {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }

        if group_config.summary_report_columns.show_video {
            if let Some(vmaf) = summary_row.vmaf {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, vmaf);
            } else {
                buf.push_str("<td></td>\n");
            }
        }

        buf.push_str("</tr>\n");

        buf
    }

    pub fn summary_report_section(
        &self,
        reports: &Vec<Result<Report>>,
        group_config: &GroupConfig,
    ) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-12\">\n");

        buf.push_str("<table class=\"table table-hover table-bordered\">\n");
        buf.push_str("<thead>\n");
        buf.push_str("<tr>\n");
        if group_config.summary_report_columns.show_video {
            buf.push_str("<th colspan=\"6\" style=\"width: 33%\">Test Case</th>\n");
        } else {
            buf.push_str("<th colspan=\"5\" style=\"width: 33%\">Test Case</th>\n");
        }
        if group_config.summary_report_columns.show_send_stats {
            let send_colspan = if group_config.summary_report_columns.show_video {
                5
            } else {
                3
            };
            let _ = writeln!(
                buf,
                "<th colspan=\"{send_colspan}\">Client Send Stats (average)</th>"
            );
        }
        if group_config.summary_report_columns.show_receive_stats {
            let recv_colspan = if group_config.summary_report_columns.show_video {
                9
            } else {
                7
            };
            let _ = writeln!(
                buf,
                "<th colspan=\"{recv_colspan}\">Client Receive Stats (average)</th>"
            );
        }
        if group_config.summary_report_columns.show_delay_stats {
            buf.push_str("<th colspan=\"3\">Queuing Delay (ms)</th>\n");
        }
        buf.push_str("<th colspan=\"4\">Container Stats (average)</th>\n");

        let mut visqol_colspan = 0;
        if group_config.summary_report_columns.show_visqol_mos_speech {
            visqol_colspan += 1;
        }
        if group_config.summary_report_columns.show_visqol_mos_audio {
            visqol_colspan += 1;
        }
        if visqol_colspan > 0 {
            let _ = writeln!(buf, "<th colspan=\"{}\">Visqol MOS</th>\n", visqol_colspan);
        }

        let mut other_colspan = 0;
        if group_config.summary_report_columns.show_pesq_mos {
            other_colspan += 1;
        }
        if group_config.summary_report_columns.show_plc_mos {
            other_colspan += 1;
        }
        if other_colspan > 0 {
            let _ = writeln!(buf, "<th colspan=\"{}\">Other MOS</th>\n", other_colspan);
        }

        if visqol_colspan + other_colspan > 0 {
            // Show the MOS average.
            let _ = writeln!(buf, "<th colspan=1>MOS</th>\n");
        }

        if group_config.summary_report_columns.show_video {
            buf.push_str("<th rowspan=\"2\">VMAF</th>\n");
        }
        buf.push_str("</tr>\n");
        buf.push_str("<tr>\n");
        buf.push_str("<th>Name</th>\n");
        buf.push_str("<th>Client</th>\n");
        buf.push_str("<th>From</th>\n");
        buf.push_str("<th>Sound</th>\n");
        if group_config.summary_report_columns.show_video {
            buf.push_str("<th>Video</th>\n");
        }
        buf.push_str("<th>Profile</th>\n");

        if group_config.summary_report_columns.show_send_stats {
            buf.push_str("<th>Packet Size</th>\n");
            buf.push_str("<th>Packet Rate</th>\n");
            buf.push_str("<th>Bitrate</th>\n");
            if group_config.summary_report_columns.show_video {
                buf.push_str("<th>Resolution</th>\n");
                buf.push_str("<th>Framerate</th>\n");
            }
        }
        if group_config.summary_report_columns.show_receive_stats {
            buf.push_str("<th>Packet Rate</th>\n");
            buf.push_str("<th>Bitrate</th>\n");
            buf.push_str("<th>Loss</th>\n");
            buf.push_str("<th>CV</th>\n");
            if group_config.summary_report_columns.show_video {
                buf.push_str("<th>Resolution</th>\n");
                buf.push_str("<th>Framerate</th>\n");
            }
            buf.push_str("<th>Cncl Rate</th>\n");
            buf.push_str("<th>Cncl /Loss</th>\n");
            buf.push_str("<th>FEC</th>\n");
        }
        if group_config.summary_report_columns.show_delay_stats {
            buf.push_str("<th>RTT Floor</th>\n");
            buf.push_str("<th>RTT Infl</th>\n");
            buf.push_str("<th>Arrival</th>\n");
        }

        buf.push_str("<th>CPU</th>\n");
        buf.push_str("<th>Mem</th>\n");
        buf.push_str("<th>TX Bitrate</th>\n");
        buf.push_str("<th>RX Bitrate</th>\n");
        if group_config.summary_report_columns.show_visqol_mos_speech {
            buf.push_str("<th>Speech</th>\n");
        }
        if group_config.summary_report_columns.show_visqol_mos_audio {
            buf.push_str("<th>Audio</th>\n");
        }
        if group_config.summary_report_columns.show_pesq_mos {
            buf.push_str("<th>PESQ</th>\n");
        }
        if group_config.summary_report_columns.show_plc_mos {
            buf.push_str("<th>PLC</th>\n");
        }

        if visqol_colspan + other_colspan > 0 {
            // Show the MOS average.
            buf.push_str("<th>Average</th>\n");
        }

        buf.push_str("</tr>\n");
        buf.push_str("</thead>\n");

        buf.push_str("<tbody>\n");

        // Rows are grouped by identity rather than by position in the list. Reports arrive
        // ordered profile -> iteration -> client, and each client contributes one row per
        // inbound stream, so counting `iterations` reports off the front would group unrelated
        // clients and streams together.
        #[derive(PartialEq, Eq, Hash, Clone)]
        struct RowKey {
            test_case_name: String,
            network_profile: String,
            client_name: String,
            stream: String,
        }

        let mut ordered_keys: Vec<RowKey> = vec![];
        let mut samples_by_key: HashMap<RowKey, Vec<(&Report, SummaryRow)>> = HashMap::new();

        for result in reports {
            match result {
                Ok(report) => {
                    for row in SummaryRow::rows_for(report) {
                        let key = RowKey {
                            test_case_name: report.test_case_name.clone(),
                            network_profile: report.network_profile.get_name().to_string(),
                            client_name: report.client_name.clone(),
                            stream: row.ssrc.clone().unwrap_or_default(),
                        };
                        if !samples_by_key.contains_key(&key) {
                            ordered_keys.push(key.clone());
                        }
                        samples_by_key.entry(key).or_default().push((report, row));
                    }
                }
                Err(err) => {
                    buf.push_str("<tr class=\"table-dark\">\n");
                    let _ = writeln!(buf, "<td>{:?}</td>", err);
                    buf.push_str("</tr>\n");
                }
            }
        }

        // Keeps collapse class names unique per aggregated group.
        let mut iteration_count_for_group = 0;

        for key in &ordered_keys {
            let samples = &samples_by_key[key];
            let (report, _) = samples[0];

            if samples.len() != report.iterations as usize {
                // Aggregation is driven by how many samples actually arrived rather than by the
                // configured count, so a failed iteration still renders - but it is worth
                // saying so, since the aggregate then covers fewer runs than requested.
                info!(
                    "Summary for {}/{} has {} of {} configured iterations",
                    report.test_case_name,
                    report.client_name,
                    samples.len(),
                    report.iterations
                );
            }

            if samples.len() > 1 {
                // Several iterations of the same stream: show the aggregate, then the
                // individual iterations collapsed beneath it.
                let mut aggregate = SummaryRow::new_aggregate(&samples[0].1);
                let mut items = vec![];
                for (index, (_, row)) in samples.iter().enumerate() {
                    let count = index + 1;
                    if count > 1 {
                        aggregate.update(row, count);
                    }
                    let mut item = row.clone();
                    item.set_aggregate_item(count);
                    items.push(item);
                }

                buf.push_str(&self.summary_report_row(
                    group_config,
                    report,
                    &aggregate,
                    iteration_count_for_group,
                ));
                for item in &items {
                    buf.push_str(&self.summary_report_row(
                        group_config,
                        report,
                        item,
                        iteration_count_for_group,
                    ));
                }
                iteration_count_for_group += 1;
            } else {
                buf.push_str(&self.summary_report_row(
                    group_config,
                    report,
                    &samples[0].1,
                    iteration_count_for_group,
                ));
            }
        }

        buf.push_str("</tbody>\n");
        buf.push_str("</table>\n");
        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
    }

    fn summary_call_audio_core_row(
        &self,
        group_config: &GroupConfig,
        report: &Report,
        summary_row: &SummaryCallAudioCoreRow,
        iteration_count_for_group: usize,
        timestamp_columns: &[i32],
    ) -> String {
        let mut buf = String::new();

        let table_emphasis = Html::get_emphasis_for_mos(summary_row.mos_average);

        match summary_row.row_type {
            SummaryRowType::Single => {
                let _ = writeln!(
                    buf,
                    r#"<tr class="table-{} clickable" onclick="window.location='{}/{}/report.html'">"#,
                    table_emphasis, group_config.group_name, report.report_name
                );
            }
            SummaryRowType::Aggregate => {
                let _ = writeln!(
                    buf,
                    r#"<tr class="table-{}" data-bs-toggle="collapse" data-bs-target=".{}_{}_call_audio_core_collapsed">"#,
                    table_emphasis, group_config.group_name, iteration_count_for_group
                );
            }
            SummaryRowType::AggregateItem => {
                let _ = writeln!(
                    buf,
                    r#"<tr class="table-{} clickable w-auto small fw-light collapse {}_{}_call_audio_core_collapsed" onclick="window.location='{}/{}_{}/report.html'">"#,
                    table_emphasis,
                    group_config.group_name,
                    iteration_count_for_group,
                    group_config.group_name,
                    report.report_name,
                    summary_row.row_index
                );
            }
        }

        let indent = if summary_row.row_type == SummaryRowType::AggregateItem {
            "&nbsp;&nbsp"
        } else {
            ""
        };

        let _ = writeln!(buf, "<td>{}{}</td>", indent, report.test_case_name);
        let _ = writeln!(buf, "<td>{}{}</td>", indent, report.client_name);

        for timestamp in timestamp_columns {
            if let Some((_, mos)) = summary_row
                .mos_over_time
                .iter()
                .find(|(time, _)| time == timestamp)
            {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }

        let values = summary_row.mos_over_time.iter().map(|(_, mos)| *mos);
        let min = values.clone().reduce(f32::min);
        let max = values.reduce(f32::max);
        for value in [min, max, summary_row.mos_average] {
            if let Some(mos) = value {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }

        buf.push_str("</tr>\n");

        buf
    }

    pub fn summary_call_audio_core_section(
        &self,
        reports: &Vec<Result<Report>>,
        group_config: &GroupConfig,
    ) -> Option<String> {
        let mut timestamp_columns = vec![];
        for report in reports.iter().flatten() {
            if let Some(summary_row) = SummaryCallAudioCoreRow::new(report) {
                for (timestamp, _) in summary_row.mos_over_time {
                    if !timestamp_columns.contains(&timestamp) {
                        timestamp_columns.push(timestamp);
                    }
                }
            }
        }

        timestamp_columns.sort();
        if timestamp_columns.is_empty() {
            return None;
        }

        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-12\">\n");
        buf.push_str("<h5>Average MOS Over Time</h5>\n");

        buf.push_str("<table class=\"table table-hover table-bordered\">\n");
        buf.push_str("<thead>\n");
        buf.push_str("<tr>\n");
        buf.push_str("<th style=\"width: 16%\">Name</th>\n");
        buf.push_str("<th>Client</th>\n");
        for timestamp in &timestamp_columns {
            let _ = writeln!(buf, "<th>{:.1}</th>", *timestamp as f32 / 10.0);
        }
        buf.push_str("<th>Min</th>\n");
        buf.push_str("<th>Max</th>\n");
        buf.push_str("<th>Ave</th>\n");
        buf.push_str("</tr>\n");
        buf.push_str("</thead>\n");

        buf.push_str("<tbody>\n");

        // Grouped by identity for the same reason as the main summary table: reports arrive
        // ordered profile -> iteration -> client, so counting `iterations` off the front would
        // aggregate different clients together. MOS is per client, so there is no stream
        // component to the key here.
        #[derive(PartialEq, Eq, Hash, Clone)]
        struct CoreKey {
            test_case_name: String,
            network_profile: String,
            client_name: String,
        }

        let mut ordered_keys: Vec<CoreKey> = vec![];
        let mut samples_by_key: HashMap<CoreKey, Vec<(&Report, SummaryCallAudioCoreRow)>> =
            HashMap::new();

        for result in reports {
            match result {
                Ok(report) => {
                    // Only clients with a MOS series appear in this table.
                    if let Some(row) = SummaryCallAudioCoreRow::new(report) {
                        let key = CoreKey {
                            test_case_name: report.test_case_name.clone(),
                            network_profile: report.network_profile.get_name().to_string(),
                            client_name: report.client_name.clone(),
                        };
                        if !samples_by_key.contains_key(&key) {
                            ordered_keys.push(key.clone());
                        }
                        samples_by_key.entry(key).or_default().push((report, row));
                    }
                }
                Err(err) => {
                    let column_count = timestamp_columns.len() + 4;
                    buf.push_str("<tr class=\"table-dark\">\n");
                    let _ = writeln!(buf, "<td colspan=\"{}\">{:?}</td>", column_count, err);
                    buf.push_str("</tr>\n");
                }
            }
        }

        let mut iteration_count_for_group = 0;

        for key in &ordered_keys {
            let samples = &samples_by_key[key];
            let (report, _) = samples[0];

            if samples.len() > 1 {
                let mut aggregate = samples[0].1.clone();
                aggregate.row_type = SummaryRowType::Aggregate;
                let mut items = vec![];
                for (index, (_, row)) in samples.iter().enumerate() {
                    let count = index + 1;
                    if count > 1 {
                        aggregate.update(row, count);
                    }
                    let mut item = row.clone();
                    item.set_aggregate_item(count);
                    items.push(item);
                }

                buf.push_str(&self.summary_call_audio_core_row(
                    group_config,
                    report,
                    &aggregate,
                    iteration_count_for_group,
                    &timestamp_columns,
                ));
                for item in &items {
                    buf.push_str(&self.summary_call_audio_core_row(
                        group_config,
                        report,
                        item,
                        iteration_count_for_group,
                        &timestamp_columns,
                    ));
                }
                iteration_count_for_group += 1;
            } else {
                buf.push_str(&self.summary_call_audio_core_row(
                    group_config,
                    report,
                    &samples[0].1,
                    iteration_count_for_group,
                    &timestamp_columns,
                ));
            }
        }

        buf.push_str("</tbody>\n");
        buf.push_str("</table>\n");
        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        Some(buf)
    }

    pub fn summary_sounds_item_body(&self, sounds: &HashMap<String, Sound>) -> String {
        let mut buf = String::new();

        buf.push_str("<table class=\"table\">\n");
        buf.push_str("<thead>\n");
        buf.push_str("<tr>\n");
        buf.push_str("<th>Sound</th>\n");
        buf.push_str("<th>MOS</th>\n");
        buf.push_str("</tr>\n");
        buf.push_str("</thead>\n");
        buf.push_str("<tbody>\n");

        for (name, sound) in sounds {
            if let Some(mos) = sound.reference_mos {
                buf.push_str("<tr>\n");
                let _ = writeln!(buf, "<td>{}</td>", name);
                let _ = writeln!(buf, "<td>{:.3}</td>", mos);
                buf.push_str("</tr>\n");
            }
        }

        buf.push_str("</tbody>\n");
        buf.push_str("</table>\n");

        buf
    }
}

fn average(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let sum: f32 = values.iter().sum();
    sum / values.len() as f32
}
