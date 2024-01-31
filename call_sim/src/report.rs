//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use anyhow::{anyhow, Result};
use plotly::{
    color::NamedColor,
    common::{Font, Line, LineShape, Marker, Mode, Title},
    layout::{Axis, AxisType, BarMode, Margin},
    Bar, ImageFormat, Layout, Plot, Scatter,
};
use regex::Regex;
use std::{collections::HashMap, fmt::Write, str::FromStr};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    task::JoinSet,
};

use crate::common::{
    ChartDimension, GroupConfig, NetworkConfigWithOffset, NetworkProfile, TestCaseConfig,
};
use crate::test::{AudioTestResults, GroupRun, Sound, TestCase};

type ChartPoint = (f32, f32);

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
        }
    }
}

/// Our current standard value for ignoring the first "5 seconds" of garbage data.
const STATS_SKIP_N: usize = 5;

#[derive(Debug, Clone)]
pub struct StatsData {
    /// Internal counter for maintaining the average.
    sum: f64,

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

    /// Calculated statistics within the filtered range.
    pub min: f32,
    pub max: f32,
    pub ave: f32,

    /// Track the max inserted index in case data is aperiodic.
    pub max_index: f32,
}

impl Default for StatsData {
    fn default() -> Self {
        Self {
            sum: 0.0,
            overall_min: f32::MAX,
            overall_max: 0.0,
            filter_min: STATS_SKIP_N,
            filter_max: usize::MAX,
            period: 1.0,
            points: vec![],
            min: f32::MAX,
            max: 0.0,
            ave: 0.0,
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
            self.sum += value as f64;
            self.ave = self.sum as f32 / (self.points.len() - self.filter_min) as f32;
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
        json["aggregate"]["VMAF_score"]
            .as_f64()
            .map(|x| x as f32)
            .ok_or_else(|| anyhow!("invalid vmaf json"))
    }

    // There isn't much to build for audio, now that it is pre-calculated.
    pub async fn build(
        audio_test_results: AudioTestResults,
        video_analysis_file_name: Option<&str>,
    ) -> Result<Self> {
        let vmaf = if let Some(video_analysis_file_name) = video_analysis_file_name {
            Some(Self::parse_video_analysis(video_analysis_file_name).await?)
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

// Structures used only to transfer data that is moved.

#[derive(Debug, Default)]
pub struct ConnectionStatsTransfer {
    pub timestamp_us: Vec<u64>,
    pub current_round_trip_time: StatsData,
    pub available_outgoing_bitrate: StatsData,
}

#[derive(Debug, Default)]
pub struct AudioSendStatsTransfer {
    pub packets_per_second: StatsData,
    pub average_packet_size: StatsData,
    pub bitrate: StatsData,
    pub remote_packet_loss: StatsData,
    pub remote_jitter: StatsData,
    pub remote_round_trip_time: StatsData,
    pub audio_energy: StatsData,
}

#[derive(Debug, Default)]
pub struct VideoSendStatsTransfer {
    pub packets_per_second: StatsData,
    pub average_packet_size: StatsData,
    pub bitrate: StatsData,
    pub framerate: StatsData,
    pub key_frames_encoded: StatsData,
    pub retransmitted_packets_sent: StatsData,
    pub retransmitted_bitrate: StatsData,
    pub send_delay_per_packet: StatsData,
    pub nack_count: StatsData,
    pub pli_count: StatsData,
    pub remote_packet_loss: StatsData,
    pub remote_jitter: StatsData,
    pub remote_round_trip_time: StatsData,
}

#[derive(Debug, Default)]
pub struct AudioReceiveStatsTransfer {
    pub packets_per_second: StatsData,
    pub packet_loss: StatsData,
    pub bitrate: StatsData,
    pub jitter: StatsData,
    pub audio_energy: StatsData,
    pub jitter_buffer_delay: StatsData,
}

#[derive(Debug, Default)]
pub struct VideoReceiveStatsTransfer {
    pub packets_per_second: StatsData,
    pub packet_loss: StatsData,
    pub bitrate: StatsData,
    pub framerate: StatsData,
    pub key_frames_decoded: StatsData,
}

#[derive(Debug, Default)]
pub struct AudioAdaptationTransfer {
    pub bitrate: StatsData,
    pub packet_length: StatsData,
}

#[derive(Debug)]
pub struct ConnectionStats {
    pub timestamp_us: Vec<u64>,
    pub current_round_trip_time_stats: Stats,
    pub available_outgoing_bitrate_stats: Stats,
    pub item_count: usize,
}

#[derive(Debug)]
pub struct AudioSendStats {
    pub packets_per_second_stats: Stats,
    pub average_packet_size_stats: Stats,
    pub bitrate_stats: Stats,
    pub remote_packet_loss_stats: Stats,
    pub remote_jitter_stats: Stats,
    pub remote_round_trip_time_stats: Stats,
    pub audio_energy_stats: Stats,
    pub item_count: usize,
}

#[derive(Debug)]
pub struct VideoSendStats {
    pub packets_per_second_stats: Stats,
    pub average_packet_size_stats: Stats,
    pub bitrate_stats: Stats,
    pub framerate_stats: Stats,
    pub key_frames_encoded_stats: Stats,
    pub retransmitted_packets_sent_stats: Stats,
    pub retransmitted_bitrate_stats: Stats,
    pub send_delay_per_packet_stats: Stats,
    pub nack_count_stats: Stats,
    pub pli_count_stats: Stats,
    pub remote_packet_loss_stats: Stats,
    pub remote_jitter_stats: Stats,
    pub remote_round_trip_time_stats: Stats,
    pub item_count: usize,
}

#[derive(Debug)]
pub struct AudioReceiveStats {
    pub packets_per_second_stats: Stats,
    pub packet_loss_stats: Stats,
    pub bitrate_stats: Stats,
    pub jitter_stats: Stats,
    pub audio_energy_stats: Stats,
    pub jitter_buffer_delay_stats: Stats,
    pub item_count: usize,
}

#[derive(Debug, Default)]
pub struct VideoReceiveStats {
    pub packets_per_second_stats: Stats,
    pub packet_loss_stats: Stats,
    pub bitrate_stats: Stats,
    pub framerate_stats: Stats,
    pub key_frames_decoded_stats: Stats,
}

#[derive(Debug, Default)]
pub struct AudioAdaptation {
    pub bitrate_stats: Stats,
    pub packet_length_stats: Stats,
}

#[derive(Debug)]
pub struct ClientLogReport {
    pub connection_stats: ConnectionStats,
    pub audio_send_stats: AudioSendStats,
    pub video_send_stats: VideoSendStats,
    pub audio_receive_stats: AudioReceiveStats,
    pub video_receive_stats: VideoReceiveStats,
    pub audio_adaptation: AudioAdaptation,
}

impl ClientLogReport {
    async fn parse(
        file_name: &str,
    ) -> Result<(
        ConnectionStatsTransfer,
        AudioSendStatsTransfer,
        VideoSendStatsTransfer,
        AudioReceiveStatsTransfer,
        VideoReceiveStatsTransfer,
        AudioAdaptationTransfer,
    )> {
        // Look through the file and pull out RingRTC logs, particularly the `stats!` details.
        let file = File::open(file_name).await?;
        let reader = BufReader::new(file);

        // Example: ringrtc_stats!,connection,0xca111d,1667611058243536,0ms,100000bps
        let re_connection_line = Regex::new(
            r".*ringrtc_stats!,connection,(?P<call_id>0x[0-9a-fA-F]+),(?P<timestamp_us>\d+),(?P<current_round_trip_time>\d+)ms,(?P<available_outgoing_bitrate>\d+)bps",
        )?;

        // Example: ringrtc_stats!,audio,send,2002,40.0,100.0,32000.0bps,0.0%,0ms,0ms,0.000
        let re_audio_send_line = Regex::new(
            r".*ringrtc_stats!,audio,send,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<average_packet_size>[-+]?[0-9]*\.?[0-9]+),(?P<bitrate>[-+]?[0-9]*\.?[0-9]+)bps,(?P<remote_packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<remote_jitter>\d+)ms,(?P<remote_round_trip_time>\d+)ms,(?P<audio_energy>[-+]?[0-9]*\.?[0-9]+)",
        )?;

        // Example: ringrtc_stats!,video,send,2003,8.0,1052.9,67430bps,2.0fps,0,4.0ms,1280x720,0,0.0bps,162.4ms,0,0,bandwidth,0,0.0%,170.2ms,1.0ms
        let re_video_send_line = Regex::new(
            r".*ringrtc_stats!,video,send,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<average_packet_size>[-+]?[0-9]*\.?[0-9]+),(?P<bitrate>[-+]?[0-9]*\.?[0-9]+)bps,(?P<framerate>[0-9]*\.?[0-9]+)fps,(?P<key_frames_encoded>\d+),(?P<encode_time_per_frame>[0-9]*\.?[0-9]+)ms,(?P<resolution>\d+x\d+),(?P<retransmitted_packets_sent>\d+),(?P<retransmitted_bitrate>[0-9]*\.?[0-9]+)bps,(?P<send_delay_per_packet>[0-9]*\.?[0-9]+)ms,(?P<nack_count>\d+),(?P<pli_count>\d+),(?P<quality_limitation_reason>\w+),(?P<quality_limitation_resolution_changes>\d+),(?P<remote_packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<remote_jitter>[0-9]*\.?[0-9]+)ms,(?P<remote_round_trip_time>[0-9]*\.?[0-9]+)ms",
        )?;

        // Example: ringrtc_stats!,audio,recv,1002,40.0,0.0%,32000.0bps,0ms,0.000,50ms
        let re_audio_receive_line = Regex::new(
            r".*ringrtc_stats!,audio,recv,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<bitrate>[-+]?[0-9]*\.?[0-9]+)bps,(?P<jitter>\d+)ms,(?P<audio_energy>[-+]?[0-9]*\.?[0-9]+),(?P<jitter_buffer_delay>\d+)ms",
        )?;

        // Example: ringrtc_stats!,video,recv,2003,7.0,0.0%,61305bps,1.0fps,1,3.3ms,1280x720
        let re_video_receive_line = Regex::new(
            r".*ringrtc_stats!,video,recv,(?P<ssrc>\d+),(?P<packets_per_second>[-+]?[0-9]*\.?[0-9]+),(?P<packet_loss>[-+]?[0-9]*\.?[0-9]+)%,(?P<bitrate>[0-9]+)bps,(?P<framerate>[0-9]*\.?[0-9]+)fps,(?P<key_frames_decoded>\d+),(?P<decode_time_per_frame>[0-9]*\.?[0-9]+)ms,(?P<resolution>\d+x\d+)",
        )?;

        // Example: ringrtc_adapt!,audio,240,18000,60
        let re_adaptation_line = Regex::new(
            r".*ringrtc_adapt!,audio,(?P<time>\d+),(?P<bitrate>\d+),(?P<packet_length>\d+)",
        )?;

        let mut connection_stats = ConnectionStatsTransfer::default();
        let mut audio_send_stats = AudioSendStatsTransfer::default();
        let mut video_send_stats = VideoSendStatsTransfer::default();
        let mut audio_receive_stats = AudioReceiveStatsTransfer::default();
        let mut video_receive_stats = VideoReceiveStatsTransfer::default();
        let mut audio_adaptation_stats = AudioAdaptationTransfer {
            bitrate: StatsData::new_skip_n(0),
            packet_length: StatsData::new_skip_n(0),
        };

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(cap) = re_connection_line.captures(&line) {
                connection_stats
                    .timestamp_us
                    .push(u64::from_str(&cap["timestamp_us"])?);
                connection_stats
                    .current_round_trip_time
                    .push(f32::from_str(&cap["current_round_trip_time"])?);
                connection_stats
                    .available_outgoing_bitrate
                    .push(f32::from_str(&cap["available_outgoing_bitrate"])? / 1000.0);
                continue;
            }

            if let Some(cap) = re_audio_send_line.captures(&line) {
                audio_send_stats
                    .packets_per_second
                    .push(f32::from_str(&cap["packets_per_second"])?);
                audio_send_stats
                    .average_packet_size
                    .push(f32::from_str(&cap["average_packet_size"])?);
                audio_send_stats
                    .bitrate
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                audio_send_stats
                    .remote_packet_loss
                    .push(f32::from_str(&cap["remote_packet_loss"])?);
                audio_send_stats
                    .remote_jitter
                    .push(f32::from_str(&cap["remote_jitter"])?);
                audio_send_stats
                    .remote_round_trip_time
                    .push(f32::from_str(&cap["remote_round_trip_time"])?);
                audio_send_stats
                    .audio_energy
                    .push(f32::from_str(&cap["audio_energy"])?);
                continue;
            }

            if let Some(cap) = re_video_send_line.captures(&line) {
                video_send_stats
                    .packets_per_second
                    .push(f32::from_str(&cap["packets_per_second"])?);
                video_send_stats
                    .average_packet_size
                    .push(f32::from_str(&cap["average_packet_size"])?);
                video_send_stats
                    .bitrate
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                video_send_stats
                    .framerate
                    .push(f32::from_str(&cap["framerate"])?);
                video_send_stats
                    .key_frames_encoded
                    .push(f32::from_str(&cap["key_frames_encoded"])?);
                video_send_stats
                    .retransmitted_packets_sent
                    .push(f32::from_str(&cap["retransmitted_packets_sent"])?);
                video_send_stats
                    .retransmitted_bitrate
                    .push(f32::from_str(&cap["retransmitted_bitrate"])?);
                video_send_stats
                    .send_delay_per_packet
                    .push(f32::from_str(&cap["send_delay_per_packet"])?);
                video_send_stats
                    .nack_count
                    .push(f32::from_str(&cap["nack_count"])?);
                video_send_stats
                    .pli_count
                    .push(f32::from_str(&cap["pli_count"])?);
                video_send_stats
                    .remote_packet_loss
                    .push(f32::from_str(&cap["remote_packet_loss"])?);
                video_send_stats
                    .remote_jitter
                    .push(f32::from_str(&cap["remote_jitter"])?);
                video_send_stats
                    .remote_round_trip_time
                    .push(f32::from_str(&cap["remote_round_trip_time"])?);
                continue;
            }

            if let Some(cap) = re_audio_receive_line.captures(&line) {
                audio_receive_stats
                    .packets_per_second
                    .push(f32::from_str(&cap["packets_per_second"])?);
                audio_receive_stats
                    .bitrate
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                audio_receive_stats
                    .audio_energy
                    .push(f32::from_str(&cap["audio_energy"])?);
                audio_receive_stats
                    .packet_loss
                    .push(f32::from_str(&cap["packet_loss"])?);
                audio_receive_stats
                    .jitter
                    .push(f32::from_str(&cap["jitter"])?);
                audio_receive_stats
                    .jitter_buffer_delay
                    .push(f32::from_str(&cap["jitter_buffer_delay"])?);
                continue;
            }

            if let Some(cap) = re_video_receive_line.captures(&line) {
                video_receive_stats
                    .packets_per_second
                    .push(f32::from_str(&cap["packets_per_second"])?);
                video_receive_stats
                    .bitrate
                    .push(f32::from_str(&cap["bitrate"])? / 1000.0);
                video_receive_stats
                    .packet_loss
                    .push(f32::from_str(&cap["packet_loss"])?);
                video_receive_stats
                    .framerate
                    .push(f32::from_str(&cap["framerate"])?);
                video_receive_stats
                    .key_frames_decoded
                    .push(f32::from_str(&cap["key_frames_decoded"])?);
                continue;
            }

            if let Some(cap) = re_adaptation_line.captures(&line) {
                let time_index = f32::from_str(&cap["time"])?;
                audio_adaptation_stats
                    .bitrate
                    .push_with_index(time_index, f32::from_str(&cap["bitrate"])? / 1000.0);
                audio_adaptation_stats
                    .packet_length
                    .push_with_index(time_index, f32::from_str(&cap["packet_length"])?);
                continue;
            }
        }

        Ok((
            connection_stats,
            audio_send_stats,
            video_send_stats,
            audio_receive_stats,
            video_receive_stats,
            audio_adaptation_stats,
        ))
    }

    pub async fn build(file_name: &str, client_name: &str) -> Result<Self> {
        let (
            connection_stats,
            audio_send_stats,
            video_send_stats,
            audio_receive_stats,
            video_receive_stats,
            audio_adaptation,
        ) = ClientLogReport::parse(file_name).await?;

        // We assume that all entries in the stats vectors are in sync.
        if (connection_stats.timestamp_us.len() != audio_send_stats.bitrate.points.len())
            || (connection_stats.timestamp_us.len() != video_send_stats.bitrate.points.len())
            || (connection_stats.timestamp_us.len() != audio_receive_stats.bitrate.points.len())
            || (connection_stats.timestamp_us.len() != video_receive_stats.bitrate.points.len())
        {
            return Err(anyhow!("RingRTC stats were not in sync!"));
        }

        let item_count = connection_stats.timestamp_us.len();

        let current_round_trip_time_stats = Stats {
            config: StatsConfig {
                title: "Current Round Trip Time".to_string(),
                chart_name: format!("{}.log.connection.rtt.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: connection_stats.current_round_trip_time,
        };

        let available_outgoing_bitrate_stats = Stats {
            config: StatsConfig {
                title: "Available Outgoing Bitrate".to_string(),
                chart_name: format!("{}.log.connection.outgoing_bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: connection_stats.available_outgoing_bitrate,
        };

        let connection_stats = ConnectionStats {
            timestamp_us: connection_stats.timestamp_us,
            current_round_trip_time_stats,
            available_outgoing_bitrate_stats,
            item_count,
        };

        let packets_per_second_stats = Stats {
            config: StatsConfig {
                title: "Audio Send Packet Rate".to_string(),
                chart_name: format!("{}.log.audio.send.packet_rate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Packets/Second".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.packets_per_second,
        };

        let average_packet_size_stats = Stats {
            config: StatsConfig {
                title: "Audio Send Packet Size".to_string(),
                chart_name: format!("{}.log.audio.send.packet_size.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Average Size Per Period".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.average_packet_size,
        };

        let bitrate_stats = Stats {
            config: StatsConfig {
                title: "Audio Send Bitrate".to_string(),
                chart_name: format!("{}.log.audio.send.bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.bitrate,
        };

        let remote_packet_loss_stats = Stats {
            config: StatsConfig {
                title: "Audio Send Remote Packet Loss".to_string(),
                chart_name: format!("{}.log.audio.send.remote_loss.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "%".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.remote_packet_loss,
        };

        let remote_jitter_stats = Stats {
            config: StatsConfig {
                title: "Audio Send Remote Jitter".to_string(),
                chart_name: format!("{}.log.audio.send.remote_jitter.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.remote_jitter,
        };

        let remote_round_trip_time_stats = Stats {
            config: StatsConfig {
                title: "Audio Send Remote Round Trip Time".to_string(),
                chart_name: format!("{}.log.audio.send.remote_rtt.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: audio_send_stats.remote_round_trip_time,
        };

        let audio_energy_stats = Stats {
            config: StatsConfig {
                title: "Audio Send Audio Energy".to_string(),
                chart_name: format!("{}.log.audio.send.audio_energy.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Energy".to_string(),
                y_max: Some(1.0),
                ..Default::default()
            },
            data: audio_send_stats.audio_energy,
        };

        let audio_send_stats = AudioSendStats {
            packets_per_second_stats,
            average_packet_size_stats,
            bitrate_stats,
            remote_packet_loss_stats,
            remote_jitter_stats,
            remote_round_trip_time_stats,
            audio_energy_stats,
            item_count,
        };

        let packets_per_second_stats = Stats {
            config: StatsConfig {
                title: "Video Send Packet Rate".to_string(),
                chart_name: format!("{}.log.video.send.packet_rate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Packets/Second".to_string(),
                ..Default::default()
            },
            data: video_send_stats.packets_per_second,
        };

        let average_packet_size_stats = Stats {
            config: StatsConfig {
                title: "Video Send Packet Size".to_string(),
                chart_name: format!("{}.log.video.send.packet_size.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Average Size Per Period".to_string(),
                ..Default::default()
            },
            data: video_send_stats.average_packet_size,
        };

        let bitrate_stats = Stats {
            config: StatsConfig {
                title: "Video Send Bitrate".to_string(),
                chart_name: format!("{}.log.video.send.bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: video_send_stats.bitrate,
        };

        let framerate_stats = Stats {
            config: StatsConfig {
                title: "Video Send Framerate".to_string(),
                chart_name: format!("{}.log.video.send.framerate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "fps".to_string(),
                y_max: Some(32.0),
                ..Default::default()
            },
            data: video_send_stats.framerate,
        };

        let key_frames_encoded_stats = Stats {
            config: StatsConfig {
                title: "Video Key Frames Encoded".to_string(),
                chart_name: format!("{}.log.video.send.key_frames_encoded.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "# frames".to_string(),
                ..Default::default()
            },
            data: video_send_stats.key_frames_encoded,
        };

        let retransmitted_packets_sent_stats = Stats {
            config: StatsConfig {
                title: "Video Retransmitted Packets".to_string(),
                chart_name: format!("{}.log.video.send.retransmitted_packets.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "# Packets".to_string(),
                ..Default::default()
            },
            data: video_send_stats.retransmitted_packets_sent,
        };

        let retransmitted_bitrate_stats = Stats {
            config: StatsConfig {
                title: "Video Send Retransmitted Bitrate".to_string(),
                chart_name: format!("{}.log.video.send.retransmitted_bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: video_send_stats.retransmitted_bitrate,
        };

        let send_delay_per_packet_stats = Stats {
            config: StatsConfig {
                title: "Video Send Send Delay Per Packet".to_string(),
                chart_name: format!("{}.log.video.send.delay_per_packet.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "ms".to_string(),
                ..Default::default()
            },
            data: video_send_stats.send_delay_per_packet,
        };

        let nack_count_stats = Stats {
            config: StatsConfig {
                title: "Video Recieved NACK Count".to_string(),
                chart_name: format!("{}.log.video.send.nack_count.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "# NACKs".to_string(),
                ..Default::default()
            },
            data: video_send_stats.nack_count,
        };

        let pli_count_stats = Stats {
            config: StatsConfig {
                title: "Video Received PLI Count".to_string(),
                chart_name: format!("{}.log.video.send.pli_count.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "# PLIs".to_string(),
                ..Default::default()
            },
            data: video_send_stats.pli_count,
        };

        let remote_packet_loss_stats = Stats {
            config: StatsConfig {
                title: "Video Send Remote Packet Loss".to_string(),
                chart_name: format!("{}.log.video.send.remote_loss.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "%".to_string(),
                ..Default::default()
            },
            data: video_send_stats.remote_packet_loss,
        };

        let remote_jitter_stats = Stats {
            config: StatsConfig {
                title: "Video Send Remote Jitter".to_string(),
                chart_name: format!("{}.log.video.send.remote_jitter.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: video_send_stats.remote_jitter,
        };

        let remote_round_trip_time_stats = Stats {
            config: StatsConfig {
                title: "Video Send Remote Round Trip Time".to_string(),
                chart_name: format!("{}.log.video.send.remote_rtt.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: video_send_stats.remote_round_trip_time,
        };

        let video_send_stats = VideoSendStats {
            packets_per_second_stats,
            average_packet_size_stats,
            bitrate_stats,
            framerate_stats,
            key_frames_encoded_stats,
            retransmitted_packets_sent_stats,
            retransmitted_bitrate_stats,
            send_delay_per_packet_stats,
            nack_count_stats,
            pli_count_stats,
            remote_packet_loss_stats,
            remote_jitter_stats,
            remote_round_trip_time_stats,
            item_count,
        };

        let packets_per_second_stats = Stats {
            config: StatsConfig {
                title: "Audio Receive Packet Rate".to_string(),
                chart_name: format!("{}.log.audio.receive.packet_rate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Packets/Second".to_string(),
                ..Default::default()
            },
            data: audio_receive_stats.packets_per_second,
        };

        let packet_loss_stats = Stats {
            config: StatsConfig {
                title: "Audio Receive Packet Loss".to_string(),
                chart_name: format!("{}.log.audio.receive.loss.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "%".to_string(),
                ..Default::default()
            },
            data: audio_receive_stats.packet_loss,
        };

        let bitrate_stats = Stats {
            config: StatsConfig {
                title: "Audio Receive Bitrate".to_string(),
                chart_name: format!("{}.log.audio.receive.bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: audio_receive_stats.bitrate,
        };

        let jitter_stats = Stats {
            config: StatsConfig {
                title: "Audio Receive Jitter".to_string(),
                chart_name: format!("{}.log.audio.receive.jitter.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: audio_receive_stats.jitter,
        };

        let audio_energy_stats = Stats {
            config: StatsConfig {
                title: "Audio Receive Audio Energy".to_string(),
                chart_name: format!("{}.log.audio.receive.audio_energy.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "dB".to_string(),
                y_max: Some(1.0),
                ..Default::default()
            },
            data: audio_receive_stats.audio_energy,
        };

        let jitter_buffer_delay_stats = Stats {
            config: StatsConfig {
                title: "Audio Receive Jitter Buffer Delay".to_string(),
                chart_name: format!("{}.log.audio.receive.jitter_buffer_delay.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                ..Default::default()
            },
            data: audio_receive_stats.jitter_buffer_delay,
        };

        let audio_receive_stats = AudioReceiveStats {
            packets_per_second_stats,
            packet_loss_stats,
            bitrate_stats,
            jitter_stats,
            audio_energy_stats,
            jitter_buffer_delay_stats,
            item_count,
        };

        let packets_per_second_stats = Stats {
            config: StatsConfig {
                title: "Video Receive Packet Rate".to_string(),
                chart_name: format!("{}.log.video.receive.packet_rate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Packets/Second".to_string(),
                ..Default::default()
            },
            data: video_receive_stats.packets_per_second,
        };

        let packet_loss_stats = Stats {
            config: StatsConfig {
                title: "Video Receive Packet Loss".to_string(),
                chart_name: format!("{}.log.video.receive.loss.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "%".to_string(),
                ..Default::default()
            },
            data: video_receive_stats.packet_loss,
        };

        let bitrate_stats = Stats {
            config: StatsConfig {
                title: "Video Receive Bitrate".to_string(),
                chart_name: format!("{}.log.video.receive.bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                ..Default::default()
            },
            data: video_receive_stats.bitrate,
        };

        let framerate_stats = Stats {
            config: StatsConfig {
                title: "Video Receive Framerate".to_string(),
                chart_name: format!("{}.log.video.receive.framerate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "fps".to_string(),
                y_max: Some(32.0),
                ..Default::default()
            },
            data: video_receive_stats.framerate,
        };

        let key_frames_decoded_stats = Stats {
            config: StatsConfig {
                title: "Video Key Frames Decoded".to_string(),
                chart_name: format!("{}.log.video.receive.key_frames_decoded.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "# frames".to_string(),
                ..Default::default()
            },
            data: video_receive_stats.key_frames_decoded,
        };

        let video_receive_stats = VideoReceiveStats {
            packets_per_second_stats,
            packet_loss_stats,
            bitrate_stats,
            framerate_stats,
            key_frames_decoded_stats,
        };

        let bitrate_stats = Stats {
            config: StatsConfig {
                title: "Adaptation Bitrate Changes".to_string(),
                chart_name: format!("{}.audio.adaptation.bitrate.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "Kbps".to_string(),
                x_max: Some(audio_adaptation.bitrate.max_index + 5.0),
                line_shape: LineShape::Hv,
                ..Default::default()
            },
            data: audio_adaptation.bitrate,
        };

        let packet_length_stats = Stats {
            config: StatsConfig {
                title: "Adaptation Packet Length Changes".to_string(),
                chart_name: format!("{}.audio.adaptation.packet_length.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "milliseconds".to_string(),
                x_max: Some(audio_adaptation.packet_length.max_index + 5.0),
                line_shape: LineShape::Hv,
                ..Default::default()
            },
            data: audio_adaptation.packet_length,
        };

        let audio_adaptation = AudioAdaptation {
            bitrate_stats,
            packet_length_stats,
        };

        Ok(Self {
            connection_stats,
            audio_send_stats,
            video_send_stats,
            audio_receive_stats,
            video_receive_stats,
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

    pub analysis_report: AnalysisReport,
    pub docker_stats_report: DockerStatsReport,
    pub client_log_report: ClientLogReport,

    /// If there is no video being tested, don't generate charts or columns for it.
    pub show_video: bool,
    /// Keep track of how many iterations were assigned for the test case.
    pub iterations: u16,
}

impl Report {
    /// Build a report from client_b's perspective.
    pub async fn build_b(
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
        audio_test_results: AudioTestResults,
    ) -> Result<Self> {
        let analysis_report = AnalysisReport::build(
            // Note: _Move_ the audio_test_results to the report.
            audio_test_results,
            test_case
                .client_b
                .output_yuv
                .as_ref()
                .map(|output_yuv| format!("{}/{}.json", test_case.test_path, output_yuv))
                .as_deref(),
        )
        .await?;
        let docker_stats_report = DockerStatsReport::build(
            &format!(
                "{}/{}_stats.log",
                test_case.test_path, test_case.client_b.name
            ),
            test_case.client_b.name,
        )
        .await?;
        let client_log_report = ClientLogReport::build(
            &format!("{}/{}.log", test_case.test_path, test_case.client_b.name),
            test_case.client_b.name,
        )
        .await?;

        let test_report = Report {
            report_name: test_case.report_name.to_string(),
            test_path: test_case.test_path.to_string(),
            test_case_name: test_case.test_case_name.to_string(),
            sound_name: test_case.client_a.sound.name.to_string(),
            video_name: test_case
                .client_a
                .video
                .map(|v| v.name.clone())
                .unwrap_or_default(),
            network_profile: test_case.network_profile.clone(),
            client_name: test_case.client_b.name.to_string(),
            client_name_wav: test_case.client_b.output_wav.to_string(),
            analysis_report,
            docker_stats_report,
            client_log_report,
            show_video: test_case_config.client_a_config.video.input_name.is_some()
                || test_case_config.client_b_config.video.input_name.is_some(),
            iterations: test_case_config.iterations,
        };

        if test_case_config.create_charts {
            test_report.create_charts(&test_case.test_path).await;
        }

        Ok(test_report)
    }

    fn create_bar_chart(test_path: &str, stats: &Stats, domain: Vec<String>, data: Vec<f32>) {
        let width = 800;
        let height = 600;
        let margin = Margin::default().left(60).right(40).top(70).bottom(60);

        let trace = Bar::new(domain, data);

        let x_axis = Axis::default()
            .color(NamedColor::DimGray)
            .show_line(true)
            .title(Title::from(&*stats.config.x_label))
            .type_(AxisType::Category);

        let y_min = stats.config.y_min.unwrap_or(0.0);
        let y_max = stats.config.y_max.unwrap_or({
            // Default to 10% more than the overall max value.
            stats.data.overall_max.mul_add(0.1, stats.data.overall_max)
        });

        let y_axis = Axis::default()
            .color(NamedColor::DimGray)
            .show_line(true)
            .title(Title::from(&*stats.config.y_label))
            .range(vec![y_min, y_max]);

        let layout = Layout::new()
            .bar_mode(BarMode::Group)
            .title(
                Title::from(&*stats.config.title)
                    .font(Font::new().size(24).color(NamedColor::DimGray)),
            )
            .x_axis(x_axis)
            .y_axis(y_axis)
            .width(width)
            .height(height)
            .margin(margin);

        let mut plot = Plot::new();
        plot.add_trace(trace);
        plot.set_layout(layout);

        plot.write_image(
            format!("{}/{}", test_path, stats.config.chart_name),
            ImageFormat::SVG,
            width,
            height,
            1.0,
        );
    }

    fn create_line_chart(test_path: &str, stats: &Stats) {
        let width = 800;
        let height = 600;
        let margin = Margin::default().left(60).right(40).top(70).bottom(60);

        let (x_trace, y_trace) = stats.data.points.iter().cloned().unzip();

        let marker_size = if stats.data.points.len() > 60 {
            // If the length is more than 60, we'll squelch markers.
            2
        } else {
            // Use a reasonably sized circle to mark the plotted points.
            10
        };

        let trace = Scatter::new(x_trace, y_trace)
            .mode(Mode::LinesMarkers)
            .marker(Marker::new().size(marker_size))
            .line(
                Line::new()
                    .color(NamedColor::SteelBlue)
                    .width(2.0)
                    .shape(stats.config.line_shape.clone()),
            );

        let x_min = stats.config.x_min.unwrap_or(0.0);
        let x_max = stats.config.x_max.unwrap_or({
            // Default to the actual length of the data + 5 to avoid cut-off.
            stats.data.points.len() as f32 + 5.0
        });

        let x_axis = Axis::default()
            .color(NamedColor::DimGray)
            .show_line(true)
            .title(Title::from(&*stats.config.x_label))
            .range(vec![x_min, x_max]);

        let y_min = stats.config.y_min.unwrap_or(0.0);
        let y_max = stats.config.y_max.unwrap_or({
            // Default to 10% more than the overall max value.
            stats.data.overall_max.mul_add(0.1, stats.data.overall_max)
        });

        let y_axis = Axis::default()
            .color(NamedColor::DimGray)
            .show_line(true)
            .title(Title::from(&*stats.config.y_label))
            .range(vec![y_min, y_max]);

        let layout = Layout::new()
            .title(
                Title::new(&stats.config.title)
                    .font(Font::new().size(24).color(NamedColor::DimGray)),
            )
            .x_axis(x_axis)
            .y_axis(y_axis)
            .width(width)
            .height(height)
            .margin(margin);

        let mut plot = Plot::new();
        plot.add_trace(trace);
        plot.set_layout(layout);

        plot.write_image(
            format!("{}/{}", test_path, stats.config.chart_name),
            ImageFormat::SVG,
            width,
            height,
            1.0,
        );
    }

    pub async fn create_charts(&self, test_path: &str) {
        let connection_stats = &self.client_log_report.connection_stats;
        let audio_send_stats = &self.client_log_report.audio_send_stats;
        let audio_receive_stats = &self.client_log_report.audio_receive_stats;
        let video_send_stats = &self.client_log_report.video_send_stats;
        let video_receive_stats = &self.client_log_report.video_receive_stats;
        let audio_adaptation = &self.client_log_report.audio_adaptation;

        let mut line_chart_stats = vec![
            &self.docker_stats_report.cpu_usage,
            &self.docker_stats_report.mem_usage,
            &self.docker_stats_report.tx_bitrate,
            &self.docker_stats_report.rx_bitrate,
            &connection_stats.current_round_trip_time_stats,
            &connection_stats.available_outgoing_bitrate_stats,
            &audio_send_stats.packets_per_second_stats,
            &audio_send_stats.average_packet_size_stats,
            &audio_send_stats.bitrate_stats,
            &audio_send_stats.remote_packet_loss_stats,
            &audio_send_stats.remote_jitter_stats,
            &audio_send_stats.remote_round_trip_time_stats,
            &audio_send_stats.audio_energy_stats,
            &audio_receive_stats.packets_per_second_stats,
            &audio_receive_stats.packet_loss_stats,
            &audio_receive_stats.bitrate_stats,
            &audio_receive_stats.jitter_stats,
            &audio_receive_stats.audio_energy_stats,
            &audio_receive_stats.jitter_buffer_delay_stats,
            &audio_adaptation.bitrate_stats,
            &audio_adaptation.packet_length_stats,
        ];

        if self.show_video {
            line_chart_stats.append(&mut vec![
                &video_send_stats.packets_per_second_stats,
                &video_send_stats.average_packet_size_stats,
                &video_send_stats.bitrate_stats,
                &video_send_stats.framerate_stats,
                &video_send_stats.key_frames_encoded_stats,
                &video_send_stats.retransmitted_packets_sent_stats,
                &video_send_stats.retransmitted_bitrate_stats,
                &video_send_stats.send_delay_per_packet_stats,
                &video_send_stats.nack_count_stats,
                &video_send_stats.pli_count_stats,
                &video_send_stats.remote_packet_loss_stats,
                &video_send_stats.remote_jitter_stats,
                &video_send_stats.remote_round_trip_time_stats,
                &video_receive_stats.packets_per_second_stats,
                &video_receive_stats.packet_loss_stats,
                &video_receive_stats.bitrate_stats,
                &video_receive_stats.framerate_stats,
                &video_receive_stats.key_frames_decoded_stats,
            ]);
        }

        // Generate charts for audio mos results if they represent a series.
        let audio_reports = [
            &self.analysis_report.audio_test_results.visqol_mos_speech,
            &self.analysis_report.audio_test_results.visqol_mos_audio,
            &self.analysis_report.audio_test_results.visqol_mos_average,
            &self.analysis_report.audio_test_results.pesq_mos,
            &self.analysis_report.audio_test_results.plc_mos,
        ];
        for report in audio_reports {
            if let AnalysisReportMos::Series(stats) = report {
                line_chart_stats.push(stats);
            }
        }

        let mut set = JoinSet::new();
        for stats in line_chart_stats.into_iter() {
            let path = test_path.to_string();
            let stats = stats.clone();
            set.spawn_blocking(move || {
                Report::create_line_chart(&path, &stats);
            });
        }
        while (set.join_next().await).is_some() {}
    }

    pub async fn create_test_case_report(
        &self,
        set_name: &str,
        reference_spectrogram: &str,
        network_configs: &Vec<NetworkConfigWithOffset>,
        test_case_config: &TestCaseConfig,
    ) -> Result<()> {
        let mut buf = vec![];
        let html = Html::new();

        buf.extend_from_slice(
            html.header(&format!("{}/{} Report", set_name, self.report_name))
                .as_bytes(),
        );

        buf.extend_from_slice(
            html.report_heading(
                set_name,
                test_case_config,
                &self.report_name,
                &self.client_name,
                &self.analysis_report.audio_test_results,
            )
            .as_bytes(),
        );
        buf.extend_from_slice(html.network_config_section(network_configs).as_bytes());
        buf.extend_from_slice(html.call_config_section(test_case_config).as_bytes());

        // Add charts for audio mos results if they represent a series to the "Audio Core" section.
        let mut audio_core_stats: Vec<&Stats> = vec![];

        if let AnalysisReportMos::Series(stats) =
            &self.analysis_report.audio_test_results.visqol_mos_speech
        {
            audio_core_stats.push(stats);
        }
        if let AnalysisReportMos::Series(stats) =
            &self.analysis_report.audio_test_results.visqol_mos_audio
        {
            audio_core_stats.push(stats);
        }
        if let AnalysisReportMos::Series(stats) =
            &self.analysis_report.audio_test_results.visqol_mos_average
        {
            audio_core_stats.push(stats);
        }
        if let AnalysisReportMos::Series(stats) = &self.analysis_report.audio_test_results.pesq_mos
        {
            audio_core_stats.push(stats);
        }
        if let AnalysisReportMos::Series(stats) = &self.analysis_report.audio_test_results.plc_mos {
            audio_core_stats.push(stats);
        }

        if !audio_core_stats.is_empty() {
            let audio_core_stats = Self::build_stats_rows(&html, &audio_core_stats);
            buf.extend_from_slice(
                html.accordion_section(
                    "audioCore",
                    vec![HtmlAccordionItem {
                        label: "Call Audio Core".to_string(),
                        body: audio_core_stats,
                        collapsed: true,
                    }],
                )
                .as_bytes(),
            );
        }

        if test_case_config.client_b_config.audio.generate_spectrogram {
            buf.extend_from_slice(
                html.accordion_section(
                    "spectrograms",
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
            &html,
            &[
                &self.docker_stats_report.cpu_usage,
                &self.docker_stats_report.mem_usage,
                &self.docker_stats_report.tx_bitrate,
                &self.docker_stats_report.rx_bitrate,
            ],
        );
        buf.extend_from_slice(
            html.accordion_section(
                "dockerStats",
                vec![HtmlAccordionItem {
                    label: "Docker Stats".to_string(),
                    body: container_stats,
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        if test_case_config.client_b_config.audio.adaptation > 0 {
            let audio_adaptation = &self.client_log_report.audio_adaptation;
            let audio_adaptation = Self::build_stats_rows(
                &html,
                &[
                    &audio_adaptation.bitrate_stats,
                    &audio_adaptation.packet_length_stats,
                ],
            );
            buf.extend_from_slice(
                html.accordion_section(
                    "audioAdaptation",
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
            &html,
            &[
                &connection_stats.current_round_trip_time_stats,
                &connection_stats.available_outgoing_bitrate_stats,
            ],
        );
        buf.extend_from_slice(
            html.accordion_section(
                "connectionStats",
                vec![HtmlAccordionItem {
                    label: "Client Connection Stats".to_string(),
                    body: connection_stats,
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        let audio_send_stats = &self.client_log_report.audio_send_stats;
        let audio_send_stats = Self::build_stats_rows(
            &html,
            &[
                &audio_send_stats.packets_per_second_stats,
                &audio_send_stats.average_packet_size_stats,
                &audio_send_stats.bitrate_stats,
                &audio_send_stats.remote_packet_loss_stats,
                &audio_send_stats.remote_jitter_stats,
                &audio_send_stats.remote_round_trip_time_stats,
                &audio_send_stats.audio_energy_stats,
            ],
        );
        buf.extend_from_slice(
            html.accordion_section(
                "audioSendStats",
                vec![HtmlAccordionItem {
                    label: "Client Audio Send Stats".to_string(),
                    body: audio_send_stats,
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        let audio_receive_stats = &self.client_log_report.audio_receive_stats;
        let audio_receive_stats = Self::build_stats_rows(
            &html,
            &[
                &audio_receive_stats.packets_per_second_stats,
                &audio_receive_stats.packet_loss_stats,
                &audio_receive_stats.bitrate_stats,
                &audio_receive_stats.jitter_stats,
                &audio_receive_stats.jitter_buffer_delay_stats,
                &audio_receive_stats.audio_energy_stats,
            ],
        );
        buf.extend_from_slice(
            html.accordion_section(
                "audioReceiveStats",
                vec![HtmlAccordionItem {
                    label: "Client Audio Receive Stats".to_string(),
                    body: audio_receive_stats,
                    collapsed: true,
                }],
            )
            .as_bytes(),
        );

        if self.show_video {
            let video_send_stats = &self.client_log_report.video_send_stats;
            let video_send_stats = Self::build_stats_rows(
                &html,
                &[
                    &video_send_stats.packets_per_second_stats,
                    &video_send_stats.average_packet_size_stats,
                    &video_send_stats.bitrate_stats,
                    &video_send_stats.framerate_stats,
                    &video_send_stats.key_frames_encoded_stats,
                    &video_send_stats.retransmitted_packets_sent_stats,
                    &video_send_stats.retransmitted_bitrate_stats,
                    &video_send_stats.send_delay_per_packet_stats,
                    &video_send_stats.nack_count_stats,
                    &video_send_stats.pli_count_stats,
                    &video_send_stats.remote_packet_loss_stats,
                    &video_send_stats.remote_jitter_stats,
                    &video_send_stats.remote_round_trip_time_stats,
                ],
            );

            buf.extend_from_slice(
                html.accordion_section(
                    "videoSendStats",
                    vec![HtmlAccordionItem {
                        label: "Client Video Send Stats".to_string(),
                        body: video_send_stats,
                        collapsed: true,
                    }],
                )
                .as_bytes(),
            );

            let video_receive_stats = &self.client_log_report.video_receive_stats;
            let video_receive_stats = Self::build_stats_rows(
                &html,
                &[
                    &video_receive_stats.packets_per_second_stats,
                    &video_receive_stats.packet_loss_stats,
                    &video_receive_stats.bitrate_stats,
                    &video_receive_stats.framerate_stats,
                    &video_receive_stats.key_frames_decoded_stats,
                ],
            );

            buf.extend_from_slice(
                html.accordion_section(
                    "videoReceiveStats",
                    vec![HtmlAccordionItem {
                        label: "Client Video Receive Stats".to_string(),
                        body: video_receive_stats,
                        collapsed: true,
                    }],
                )
                .as_bytes(),
            );
        }

        buf.extend_from_slice(html.footer().as_bytes());

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&format!("{}/report.html", self.test_path))
            .await?;

        if let Err(err) = file.write_all(buf.as_slice()).await {
            println!("Error writing file! {err}");
        }

        Ok(())
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
            ChartDimension::MosSpeech => report
                .analysis_report
                .audio_test_results
                .visqol_mos_speech
                .get_mos_for_display()
                .unwrap_or(0f32),
            ChartDimension::MosAudio => report
                .analysis_report
                .audio_test_results
                .visqol_mos_audio
                .get_mos_for_display()
                .unwrap_or(0f32),
            ChartDimension::ContainerCpuUsage => report.docker_stats_report.cpu_usage.data.ave,
            ChartDimension::ContainerMemUsage => report.docker_stats_report.mem_usage.data.ave,
            ChartDimension::ContainerTxBitrate => report.docker_stats_report.tx_bitrate.data.ave,
            ChartDimension::ContainerRxBitrate => report.docker_stats_report.rx_bitrate.data.ave,
            ChartDimension::ConnectionCurrentRoundTripTime => {
                report
                    .client_log_report
                    .connection_stats
                    .current_round_trip_time_stats
                    .data
                    .ave
            }
            ChartDimension::ConnectionOutgoingBitrate => {
                report
                    .client_log_report
                    .connection_stats
                    .available_outgoing_bitrate_stats
                    .data
                    .ave
            }
            ChartDimension::AudioSendPacketsPerSecond => {
                report
                    .client_log_report
                    .audio_send_stats
                    .packets_per_second_stats
                    .data
                    .ave
            }
            ChartDimension::AudioSendPacketSize => {
                report
                    .client_log_report
                    .audio_send_stats
                    .average_packet_size_stats
                    .data
                    .ave
            }
            ChartDimension::AudioSendBitrate => {
                report
                    .client_log_report
                    .audio_send_stats
                    .bitrate_stats
                    .data
                    .ave
            }
            ChartDimension::AudioSendRemotePacketLoss => {
                report
                    .client_log_report
                    .audio_send_stats
                    .remote_packet_loss_stats
                    .data
                    .ave
            }
            ChartDimension::AudioSendRemoteJitter => {
                report
                    .client_log_report
                    .audio_send_stats
                    .remote_jitter_stats
                    .data
                    .ave
            }
            ChartDimension::AudioSendRemoteRoundTripTime => {
                report
                    .client_log_report
                    .audio_send_stats
                    .remote_round_trip_time_stats
                    .data
                    .ave
            }
            ChartDimension::AudioSendAudioEnergy => {
                report
                    .client_log_report
                    .audio_send_stats
                    .audio_energy_stats
                    .data
                    .ave
            }
            ChartDimension::AudioReceivePacketsPerSecond => {
                report
                    .client_log_report
                    .audio_receive_stats
                    .packets_per_second_stats
                    .data
                    .ave
            }
            ChartDimension::AudioReceivePacketLoss => {
                report
                    .client_log_report
                    .audio_receive_stats
                    .packet_loss_stats
                    .data
                    .ave
            }
            ChartDimension::AudioReceiveBitrate => {
                report
                    .client_log_report
                    .audio_receive_stats
                    .bitrate_stats
                    .data
                    .ave
            }
            ChartDimension::AudioReceiveJitter => {
                report
                    .client_log_report
                    .audio_receive_stats
                    .jitter_stats
                    .data
                    .ave
            }
            ChartDimension::AudioReceiveAudioEnergy => {
                report
                    .client_log_report
                    .audio_receive_stats
                    .audio_energy_stats
                    .data
                    .ave
            }
            ChartDimension::AudioReceiveJitterBufferDelay => {
                report
                    .client_log_report
                    .audio_receive_stats
                    .jitter_buffer_delay_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendPacketsPerSecond => {
                report
                    .client_log_report
                    .video_send_stats
                    .packets_per_second_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendPacketSize => {
                report
                    .client_log_report
                    .video_send_stats
                    .average_packet_size_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendBitrate => {
                report
                    .client_log_report
                    .video_send_stats
                    .bitrate_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendFramerate => {
                report
                    .client_log_report
                    .video_send_stats
                    .framerate_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendKeyFramesEncoded => {
                report
                    .client_log_report
                    .video_send_stats
                    .key_frames_encoded_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendRetransmittedPacketsSent => {
                report
                    .client_log_report
                    .video_send_stats
                    .retransmitted_packets_sent_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendRetransmittedBitrate => {
                report
                    .client_log_report
                    .video_send_stats
                    .retransmitted_bitrate_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendDelayPerPacket => {
                report
                    .client_log_report
                    .video_send_stats
                    .send_delay_per_packet_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendNackCount => {
                report
                    .client_log_report
                    .video_send_stats
                    .nack_count_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendPliCount => {
                report
                    .client_log_report
                    .video_send_stats
                    .pli_count_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendRemotePacketLoss => {
                report
                    .client_log_report
                    .video_send_stats
                    .remote_packet_loss_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendRemoteJitter => {
                report
                    .client_log_report
                    .video_send_stats
                    .remote_jitter_stats
                    .data
                    .ave
            }
            ChartDimension::VideoSendRemoteRoundTripTime => {
                report
                    .client_log_report
                    .video_send_stats
                    .remote_round_trip_time_stats
                    .data
                    .ave
            }
            ChartDimension::VideoReceivePacketsPerSecond => {
                report
                    .client_log_report
                    .video_receive_stats
                    .packets_per_second_stats
                    .data
                    .ave
            }
            ChartDimension::VideoReceivePacketLoss => {
                report
                    .client_log_report
                    .video_receive_stats
                    .packet_loss_stats
                    .data
                    .ave
            }
            ChartDimension::VideoReceiveBitrate => {
                report
                    .client_log_report
                    .video_receive_stats
                    .bitrate_stats
                    .data
                    .ave
            }
            ChartDimension::VideoReceiveFramerate => {
                report
                    .client_log_report
                    .video_receive_stats
                    .framerate_stats
                    .data
                    .ave
            }
            ChartDimension::VideoReceiveKeyFramesDecoded => {
                report
                    .client_log_report
                    .video_receive_stats
                    .key_frames_decoded_stats
                    .data
                    .ave
            }
        }
    }

    pub async fn create_summary_report(
        set_name: &str,
        set_path: &str,
        time_started: &str,
        group_reports: &[GroupRun],
        sounds: &HashMap<String, Sound>,
    ) -> Result<()> {
        println!("\nCreating summary report for {}", set_name);

        let mut buf = vec![];
        let html = Html::new();

        buf.extend_from_slice(
            html.header(&format!("Summary Report: {}", set_name))
                .as_bytes(),
        );

        buf.extend_from_slice(html.summary_heading(set_name, time_started).as_bytes());

        for (i, report) in group_reports.iter().enumerate() {
            // Add the report table to a report contents.
            let mut report_contents =
                html.summary_report_section(&report.reports, &report.group_config);

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
                        // of tests.
                        format!(
                            "{}@{}",
                            test_report.test_case_name,
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

                Report::create_bar_chart(set_path, &stats, domain, data);

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
            println!("Error writing file! {err}");
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

/// A convenience struct for tracking the averaged values for a row in the summary report. This
/// is particularly useful when aggregating values from several rows.
#[derive(Clone, Copy)]
struct SummaryRow {
    pub audio_send_packet_size: f32,
    pub audio_send_packet_rate: f32,
    pub audio_send_bitrate: f32,

    pub audio_receive_packet_rate: f32,
    pub audio_receive_bitrate: f32,
    pub audio_receive_loss: f32,

    pub container_cpu: f32,
    pub container_memory: f32,
    pub container_tx_bitrate: f32,
    pub container_rx_bitrate: f32,

    pub visqol_mos_speech: Option<f32>,
    pub visqol_mos_audio: Option<f32>,
    pub visqol_mos_average: Option<f32>,
    pub pesq_mos: Option<f32>,
    pub plc_mos: Option<f32>,

    pub vmaf: Option<f32>,

    pub row_type: SummaryRowType,
    pub row_index: usize,
}

impl SummaryRow {
    pub fn new(report: &Report) -> Self {
        Self {
            audio_send_packet_size: report
                .client_log_report
                .audio_send_stats
                .average_packet_size_stats
                .data
                .ave,
            audio_send_packet_rate: report
                .client_log_report
                .audio_send_stats
                .packets_per_second_stats
                .data
                .ave,
            audio_send_bitrate: report
                .client_log_report
                .audio_send_stats
                .bitrate_stats
                .data
                .ave,
            audio_receive_packet_rate: report
                .client_log_report
                .audio_receive_stats
                .packets_per_second_stats
                .data
                .ave,
            audio_receive_bitrate: report
                .client_log_report
                .audio_receive_stats
                .bitrate_stats
                .data
                .ave,
            audio_receive_loss: report
                .client_log_report
                .audio_receive_stats
                .packet_loss_stats
                .data
                .ave,
            container_cpu: report.docker_stats_report.cpu_usage.data.ave,
            container_memory: report.docker_stats_report.mem_usage.data.ave,
            container_tx_bitrate: report.docker_stats_report.tx_bitrate.data.ave,
            container_rx_bitrate: report.docker_stats_report.rx_bitrate.data.ave,
            visqol_mos_speech: report
                .analysis_report
                .audio_test_results
                .visqol_mos_speech
                .get_mos_for_display(),
            visqol_mos_audio: report
                .analysis_report
                .audio_test_results
                .visqol_mos_audio
                .get_mos_for_display(),
            visqol_mos_average: report
                .analysis_report
                .audio_test_results
                .visqol_mos_average
                .get_mos_for_display(),
            pesq_mos: report
                .analysis_report
                .audio_test_results
                .pesq_mos
                .get_mos_for_display(),
            plc_mos: report
                .analysis_report
                .audio_test_results
                .plc_mos
                .get_mos_for_display(),
            vmaf: report.analysis_report.vmaf,
            row_type: SummaryRowType::Single,
            row_index: 0,
        }
    }

    pub fn new_aggregate(report: &Report) -> Self {
        let mut aggregate = Self::new(report);
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
            if let (Some(old), Some(new)) = (self.visqol_mos_average, new.visqol_mos_average) {
                self.visqol_mos_average = Some(new_average(old, new));
            }
            if let (Some(old), Some(new)) = (self.pesq_mos, new.pesq_mos) {
                self.pesq_mos = Some(new_average(old, new));
            }
            if let (Some(old), Some(new)) = (self.plc_mos, new.plc_mos) {
                self.plc_mos = Some(new_average(old, new));
            }
            if let (Some(vmaf), Some(new_vmaf)) = (self.vmaf, new.vmaf) {
                self.vmaf = Some(new_average(vmaf, new_vmaf));
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
            let _ = writeln!(buf, "<button class=\"accordion-button{}\" type=\"button\" data-bs-toggle=\"collapse\" \
                    data-bs-target=\"#{}-collapse{}\" aria-expanded=\"true\" aria-controls=\"{}-collapse{}\">\n",
                    if item.collapsed { " collapsed" } else { "" }, id, i, id, i);

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

    fn get_emphasis_for_mos(
        visqol_mos_speech: Option<f32>,
        visqol_mos_audio: Option<f32>,
        pesq_mos: Option<f32>,
        plc_mos: Option<f32>,
    ) -> &'static str {
        let weight = match (visqol_mos_speech, visqol_mos_audio, pesq_mos, plc_mos) {
            (Some(mos_s), Some(mos_a), _, _) => (mos_s + mos_a) / 2.0,
            (Some(mos_s), None, _, _) => mos_s,
            (None, Some(mos_a), _, _) => mos_a,
            (None, None, Some(pesq_mos), _) => pesq_mos,
            (None, None, None, Some(plc_mos)) => plc_mos,
            (None, None, None, None) => 0.0,
        };

        if weight > 4.0 {
            "success"
        } else if weight > 3.5 {
            "warning"
        } else if weight > 0.0 {
            "danger"
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
        audio_test_results: &AudioTestResults,
    ) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");
        buf.push_str("<div class=\"col-md-6\">\n");

        let _ = writeln!(buf, "<h2>{}/{}</h2>", set_name, test_name);
        let _ = writeln!(buf, "<h3 class=\"text-muted\">Client: {}</h3>", client_name);

        buf.push_str("</div>\n");
        buf.push_str("<div class=\"col-md-6\">\n");

        let visqol_mos_speech = audio_test_results.visqol_mos_speech.get_mos_for_display();
        let visqol_mos_audio = audio_test_results.visqol_mos_audio.get_mos_for_display();
        let visqol_mos_average = audio_test_results.visqol_mos_average.get_mos_for_display();
        let pesq_mos = audio_test_results.pesq_mos.get_mos_for_display();
        let plc_mos = audio_test_results.plc_mos.get_mos_for_display();

        let text_emphasis =
            Html::get_emphasis_for_mos(visqol_mos_speech, visqol_mos_audio, pesq_mos, plc_mos);

        if test_case_config
            .client_b_config
            .audio
            .visqol_speech_analysis
            || test_case_config.client_b_config.audio.visqol_audio_analysis
        {
            let visqol_mos_speech_string = visqol_mos_speech
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());
            let visqol_mos_audio_string = visqol_mos_audio
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());
            let visqol_mos_average_string = visqol_mos_average
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());

            let _ = writeln!(
                buf,
                "<h2 class=\"text-right text-{}\">Visqol Speech: {} Audio: {} Average: {}</h2>",
                text_emphasis,
                visqol_mos_speech_string,
                visqol_mos_audio_string,
                visqol_mos_average_string,
            );
        }

        if test_case_config.client_b_config.audio.pesq_speech_analysis {
            let pesq_mos_string = pesq_mos
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());

            let _ = writeln!(
                buf,
                "<h2 class=\"text-right text-{}\">PESQ MOS: {}</h2>",
                text_emphasis, pesq_mos_string,
            );
        }

        if test_case_config.client_b_config.audio.plc_speech_analysis {
            let plc_mos_string = plc_mos
                .map(|mos| format!("{:.3}", mos))
                .unwrap_or_else(|| "None".to_string());

            let _ = writeln!(
                buf,
                "<h2 class=\"text-right text-{}\">PLC MOS: {}</h2>",
                text_emphasis, plc_mos_string,
            );
        }

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

    pub fn call_config_section(&self, test_case_config: &TestCaseConfig) -> String {
        let mut buf = String::new();

        buf.push_str("<div class=\"p-3 row\">\n");

        buf.push_str("<div class=\"col-md-12\">\n");
        buf.push_str("<h3>Call Configuration</h3>\n");
        buf.push_str("</div>\n");

        buf.push_str("<div class=\"col-md-6\">\n");
        buf.push_str("<h4>Client A</h4>\n");
        let _ = writeln!(
            buf,
            "<p><code><pre>\n{:#?}</pre></code></p>",
            &test_case_config.client_a_config
        );
        buf.push_str("</div>\n");

        buf.push_str("<div class=\"col-md-6\">\n");
        buf.push_str("<h4>Client B</h4>\n");
        let _ = writeln!(
            buf,
            "<p><code><pre>\n{:#?}</pre></code></p>",
            &test_case_config.client_b_config
        );
        buf.push_str("</div>\n");

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

        let table_emphasis = Html::get_emphasis_for_mos(
            summary_row.visqol_mos_speech,
            summary_row.visqol_mos_audio,
            summary_row.pesq_mos,
            summary_row.plc_mos,
        );

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

        if group_config.summary_report_columns.show_visqol_mos_speech {
            if let Some(mos) = summary_row.visqol_mos_speech {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }
        if group_config.summary_report_columns.show_visqol_mos_audio {
            if let Some(mos) = summary_row.visqol_mos_audio {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }
        if group_config.summary_report_columns.show_visqol_mos_average {
            if let Some(mos) = summary_row.visqol_mos_average {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }
        if group_config.summary_report_columns.show_pesq_mos {
            if let Some(mos) = summary_row.pesq_mos {
                let _ = writeln!(buf, "<td>{}{:.3}</td>", indent, mos);
            } else {
                buf.push_str("<td></td>\n");
            }
        }
        if group_config.summary_report_columns.show_plc_mos {
            if let Some(mos) = summary_row.plc_mos {
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
            buf.push_str("<th colspan=\"4\" style=\"width: 33%\">Test Case</th>\n");
        } else {
            buf.push_str("<th colspan=\"3\" style=\"width: 33%\">Test Case</th>\n");
        }
        buf.push_str("<th colspan=\"3\">Client Send Stats (average)</th>\n");
        buf.push_str("<th colspan=\"3\">Client Receive Stats (average)</th>\n");
        buf.push_str("<th colspan=\"4\">Container Stats (average)</th>\n");

        let mut colspan = 0;
        if group_config.summary_report_columns.show_visqol_mos_speech {
            colspan += 1;
        }
        if group_config.summary_report_columns.show_visqol_mos_audio {
            colspan += 1;
        }
        if group_config.summary_report_columns.show_visqol_mos_average {
            colspan += 1;
        }
        if colspan > 0 {
            let _ = writeln!(buf, "<th colspan=\"{}\">Visqol MOS</th>\n", colspan);
        }

        let mut colspan = 0;
        if group_config.summary_report_columns.show_pesq_mos {
            colspan += 1;
        }
        if group_config.summary_report_columns.show_plc_mos {
            colspan += 1;
        }
        if colspan > 0 {
            let _ = writeln!(buf, "<th colspan=\"{}\">MOS</th>\n", colspan);
        }

        if group_config.summary_report_columns.show_video {
            buf.push_str("<th rowspan=\"2\">VMAF</th>\n");
        }
        buf.push_str("</tr>\n");
        buf.push_str("<tr>\n");
        buf.push_str("<th>Name</th>\n");
        buf.push_str("<th>Sound</th>\n");
        if group_config.summary_report_columns.show_video {
            buf.push_str("<th>Video</th>\n");
        }
        buf.push_str("<th>Profile</th>\n");
        buf.push_str("<th>Packet Size</th>\n");
        buf.push_str("<th>Packet Rate</th>\n");
        buf.push_str("<th>Bitrate</th>\n");
        buf.push_str("<th>Packet Rate</th>\n");
        buf.push_str("<th>Bitrate</th>\n");
        buf.push_str("<th>Loss</th>\n");
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
        if group_config.summary_report_columns.show_visqol_mos_average {
            buf.push_str("<th>Average</th>\n");
        }
        if group_config.summary_report_columns.show_pesq_mos {
            buf.push_str("<th>PESQ</th>\n");
        }
        if group_config.summary_report_columns.show_plc_mos {
            buf.push_str("<th>PLC</th>\n");
        }
        buf.push_str("</tr>\n");
        buf.push_str("</thead>\n");

        buf.push_str("<tbody>\n");

        let mut summary_rows: Vec<SummaryRow> = vec![];
        let mut aggregate_summary_row: Option<SummaryRow> = None;

        // Keep track of the number of iterable items there are for the group so that we
        // can make sure class names are unique.
        let mut iteration_count_for_group = 0;

        for result in reports {
            // Each report will result in a row in the summary. Each row can be either for
            // a specific test case or an aggregate of several iterations, and then the
            // actual aggregated items themselves. The aggregated items are hidden by default.
            match result {
                Ok(report) => {
                    let mut current_summary_row = SummaryRow::new(report);

                    if report.iterations > 1 {
                        if !summary_rows.is_empty() {
                            // We are already aggregating the test iterations.
                            if let Some(aggregate) = &mut aggregate_summary_row {
                                aggregate.update(&current_summary_row, summary_rows.len() + 1);
                                current_summary_row.set_aggregate_item(summary_rows.len() + 1);
                                summary_rows.push(current_summary_row);

                                if summary_rows.len() == report.iterations as usize {
                                    // This is the end. Show the aggregate summary row first. Use
                                    // the current report for naming.
                                    buf.push_str(&self.summary_report_row(
                                        group_config,
                                        report,
                                        aggregate,
                                        iteration_count_for_group,
                                    ));

                                    // Show all the iterations.
                                    summary_rows.iter().for_each(|summary_line| {
                                        buf.push_str(&self.summary_report_row(
                                            group_config,
                                            report,
                                            summary_line,
                                            iteration_count_for_group,
                                        ));
                                    });

                                    summary_rows.clear();
                                    aggregate_summary_row = None;
                                    iteration_count_for_group += 1;
                                }
                            } else {
                                // This would be a bad state, warn and reset.
                                println!(
                                    "There are summary_lines but averaged_summary_line is None!"
                                );
                                summary_rows.clear();
                                aggregate_summary_row = None;
                                iteration_count_for_group += 1;
                            }
                        } else {
                            // This is the first of N iterations to track.

                            // Make a new aggregate for all rows in the test iteration.
                            aggregate_summary_row = Some(SummaryRow::new_aggregate(report));

                            // Set the current row as the first iteration item.
                            current_summary_row.set_aggregate_item(1);

                            // Add the current row to our list for display once all rows in the
                            // test iterations are aggregated.
                            summary_rows.push(current_summary_row);
                        }
                    } else {
                        // Display the report normally, one measurement for the line.
                        buf.push_str(&self.summary_report_row(
                            group_config,
                            report,
                            &current_summary_row,
                            iteration_count_for_group,
                        ));
                    }
                }
                Err(err) => {
                    buf.push_str("<tr class=\"table-dark\">\n");
                    let _ = writeln!(buf, "<td>{:?}</td>", err);
                    buf.push_str("</tr>\n");
                }
            }
        }

        buf.push_str("</tbody>\n");
        buf.push_str("</table>\n");
        buf.push_str("</div>\n");
        buf.push_str("</div>\n");

        buf
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
