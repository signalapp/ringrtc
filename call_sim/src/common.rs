//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{collections::HashSet, fmt, path::Path, time::Duration};

use derive_builder::Builder;

/// ChartDimension is used for summary reports, to help automate the summary charting and
/// display of most tracked `dimensions` that are available.
#[allow(dead_code)]
pub enum ChartDimension {
    MosSpeech,
    MosAudio,

    ContainerCpuUsage,
    ContainerMemUsage,
    ContainerTxBitrate,
    ContainerRxBitrate,

    ConnectionCurrentRoundTripTime,
    ConnectionOutgoingBitrate,

    AudioSendPacketsPerSecond,
    AudioSendPacketSize,
    AudioSendBitrate,
    AudioSendRemotePacketLoss,
    AudioSendRemoteJitter,
    AudioSendRemoteRoundTripTime,
    AudioSendAudioEnergy,

    AudioReceivePacketsPerSecond,
    AudioReceivePacketLoss,
    AudioReceiveBitrate,
    AudioReceiveJitter,
    AudioReceiveAudioEnergy,
    AudioReceiveJitterBufferDelay,
    AudioReceiveJitterBufferTargetDelay,
    AudioReceiveConcealedSamplesPct,
    AudioReceiveFecPacketsReceived,

    VideoSendPacketsPerSecond,
    VideoSendPacketSize,
    VideoSendBitrate,
    VideoSendFramerate,
    VideoSendKeyFramesEncoded,
    VideoSendRetransmittedPacketsSent,
    VideoSendRetransmittedBitrate,
    VideoSendDelayPerPacket,
    VideoSendNackCount,
    VideoSendPliCount,
    VideoSendRemotePacketLoss,
    VideoSendRemoteJitter,
    VideoSendRemoteRoundTripTime,

    VideoReceivePacketsPerSecond,
    VideoReceivePacketLoss,
    VideoReceiveBitrate,
    VideoReceiveFramerate,
    VideoReceiveKeyFramesDecoded,
}

impl ChartDimension {
    pub fn get_title_and_y_label(&self) -> (&'static str, &'static str) {
        match self {
            ChartDimension::MosSpeech => ("MOS Speech", "MOS"),
            ChartDimension::MosAudio => ("MOS Audio", "MOS"),
            ChartDimension::ContainerCpuUsage => ("CPU Usage", "%"),
            ChartDimension::ContainerMemUsage => ("Memory Usage", "MiB"),
            ChartDimension::ContainerTxBitrate => ("TX Bitrate", "kbps"),
            ChartDimension::ContainerRxBitrate => ("RX Bitrate", "kbps"),
            ChartDimension::ConnectionCurrentRoundTripTime => ("RTT", "milliseconds"),
            ChartDimension::ConnectionOutgoingBitrate => ("Outgoing Bitrate", "kbps"),
            ChartDimension::AudioSendPacketsPerSecond => {
                ("Audio Sent Packet Rate", "Packets/Second")
            }
            ChartDimension::AudioSendPacketSize => ("Audio Sent Packet Size", "Bytes"),
            ChartDimension::AudioSendBitrate => ("Audio Sent Bitrate", "kbps"),
            ChartDimension::AudioSendRemotePacketLoss => ("Audio Remote Packet Loss", "%"),
            ChartDimension::AudioSendRemoteJitter => ("Audio Remote Jitter", "milliseconds"),
            ChartDimension::AudioSendRemoteRoundTripTime => ("Audio Remote RTT", "milliseconds"),
            ChartDimension::AudioSendAudioEnergy => ("Audio Sent Energy", "Energy"),
            ChartDimension::AudioReceivePacketsPerSecond => {
                ("Audio Received Packet Rate", "Packets/Second")
            }
            ChartDimension::AudioReceivePacketLoss => ("Audio Received Packet Loss", "%"),
            ChartDimension::AudioReceiveBitrate => ("Audio Received Bitrate", "kbps"),
            ChartDimension::AudioReceiveJitter => ("Audio Received Jitter", "milliseconds"),
            ChartDimension::AudioReceiveAudioEnergy => ("Audio Received Energy", "Energy"),
            ChartDimension::AudioReceiveJitterBufferDelay => {
                ("Audio Received Jitter Buffer Delay", "milliseconds")
            }
            ChartDimension::AudioReceiveJitterBufferTargetDelay => {
                ("Audio Received Jitter Buffer Target Delay", "milliseconds")
            }
            ChartDimension::AudioReceiveConcealedSamplesPct => {
                ("Audio Received Concealed Samples", "%")
            }
            ChartDimension::AudioReceiveFecPacketsReceived => {
                ("Audio Received FEC Packets", "Packets")
            }
            ChartDimension::VideoSendPacketsPerSecond => {
                ("Video Sent Packet Rate", "Packets/Second")
            }
            ChartDimension::VideoSendPacketSize => ("Video Sent Packet Size", "Bytes"),
            ChartDimension::VideoSendBitrate => ("Video Sent Bitrate", "kbps"),
            ChartDimension::VideoSendFramerate => ("Sent Framerate", "fps"),
            ChartDimension::VideoSendKeyFramesEncoded => ("Key Frames Encoded", "frames"),
            ChartDimension::VideoSendRetransmittedPacketsSent => {
                ("Video Retransmitted Packet Rate", "Packets/Second")
            }
            ChartDimension::VideoSendRetransmittedBitrate => ("Video Retransmitted Bitrate", "bps"),
            ChartDimension::VideoSendDelayPerPacket => {
                ("Video Send Delay Per Packet", "milliseconds")
            }
            ChartDimension::VideoSendNackCount => ("Received Nack Count", "NACKs"),
            ChartDimension::VideoSendPliCount => ("Received PLI Count", "PLIs"),
            ChartDimension::VideoSendRemotePacketLoss => ("Video Remote Packet Loss", "%"),
            ChartDimension::VideoSendRemoteJitter => ("Video Remote Jitter", "milliseconds"),
            ChartDimension::VideoSendRemoteRoundTripTime => ("Video Remote RTT", "milliseconds"),
            ChartDimension::VideoReceivePacketsPerSecond => {
                ("Video Received Packet Rate", "Packets/Second")
            }
            ChartDimension::VideoReceivePacketLoss => ("Video Received Packet Loss", "%"),
            ChartDimension::VideoReceiveBitrate => ("Video Received Bitrate", "kbps"),
            ChartDimension::VideoReceiveFramerate => ("Received Framerate", "fps"),
            ChartDimension::VideoReceiveKeyFramesDecoded => ("Key Frames Decoded", "frames"),
        }
    }

    pub fn get_name(&self) -> &'static str {
        match self {
            ChartDimension::MosSpeech => "mos_speech",
            ChartDimension::MosAudio => "mos_audio",
            ChartDimension::ContainerCpuUsage => "container_cpu_usage",
            ChartDimension::ContainerMemUsage => "container_mem_usage",
            ChartDimension::ContainerTxBitrate => "container_tx_bitrate",
            ChartDimension::ContainerRxBitrate => "container_rx_bitrate",
            ChartDimension::ConnectionCurrentRoundTripTime => "connection_current_rtt",
            ChartDimension::ConnectionOutgoingBitrate => "connection_outgoing_bitrate",
            ChartDimension::AudioSendPacketsPerSecond => "audio_send_pps",
            ChartDimension::AudioSendPacketSize => "audio_send_packet_size",
            ChartDimension::AudioSendBitrate => "audio_send_bitrate",
            ChartDimension::AudioSendRemotePacketLoss => "audio_send_remote_packet_loss",
            ChartDimension::AudioSendRemoteJitter => "audio_send_remote_jitter",
            ChartDimension::AudioSendRemoteRoundTripTime => "audio_send_remote_rtt",
            ChartDimension::AudioSendAudioEnergy => "audio_send_audio_energy",
            ChartDimension::AudioReceivePacketsPerSecond => "audio_receive_pps",
            ChartDimension::AudioReceivePacketLoss => "audio_receive_packet_loss",
            ChartDimension::AudioReceiveBitrate => "audio_receive_bitrate",
            ChartDimension::AudioReceiveJitter => "audio_receive_jitter",
            ChartDimension::AudioReceiveAudioEnergy => "audio_receive_audio_energy",
            ChartDimension::AudioReceiveJitterBufferDelay => "audio_receive_jitter_buffer_delay",
            ChartDimension::AudioReceiveJitterBufferTargetDelay => {
                "audio_receive_jitter_buffer_target_delay"
            }
            ChartDimension::AudioReceiveConcealedSamplesPct => {
                "audio_receive_concealed_samples_pct"
            }
            ChartDimension::AudioReceiveFecPacketsReceived => "audio_receive_fec_packets_received",
            ChartDimension::VideoSendPacketsPerSecond => "video_send_pps",
            ChartDimension::VideoSendPacketSize => "video_send_packet_size",
            ChartDimension::VideoSendBitrate => "video_send_bitrate",
            ChartDimension::VideoSendFramerate => "video_send_framerate",
            ChartDimension::VideoSendKeyFramesEncoded => "video_key_frames_encoded",
            ChartDimension::VideoSendRetransmittedPacketsSent => "video_retransmitted_pps",
            ChartDimension::VideoSendRetransmittedBitrate => "video_retransmitted_bitrate",
            ChartDimension::VideoSendDelayPerPacket => "video_send_delay_per_packet",
            ChartDimension::VideoSendNackCount => "video_nack_count_received",
            ChartDimension::VideoSendPliCount => "video_pli_count_received",
            ChartDimension::VideoSendRemotePacketLoss => "video_send_remote_packet_loss",
            ChartDimension::VideoSendRemoteJitter => "video_send_remote_jitter",
            ChartDimension::VideoSendRemoteRoundTripTime => "video_send_remote_rtt",
            ChartDimension::VideoReceivePacketsPerSecond => "video_receive_pps",
            ChartDimension::VideoReceivePacketLoss => "video_receive_packet_loss",
            ChartDimension::VideoReceiveBitrate => "video_receive_bitrate",
            ChartDimension::VideoReceiveFramerate => "video_receive_framerate",
            ChartDimension::VideoReceiveKeyFramesDecoded => "video_key_frames_decoded",
        }
    }
}

#[derive(Debug)]
pub struct SummaryReportColumns {
    pub show_visqol_mos_speech: bool,
    pub show_visqol_mos_audio: bool,
    pub show_pesq_mos: bool,
    pub show_plc_mos: bool,
    /// A general flag to control video columns.
    pub show_video: bool,
    /// Show client send stats columns in the summary.
    pub show_send_stats: bool,
    /// Show client receive stats columns in the summary.
    pub show_receive_stats: bool,
    /// Show queuing delay columns in the summary.
    pub show_delay_stats: bool,
}

impl Default for SummaryReportColumns {
    fn default() -> Self {
        Self {
            show_visqol_mos_speech: true,
            show_visqol_mos_audio: true,
            show_pesq_mos: false,
            show_plc_mos: false,
            show_video: true,
            show_send_stats: true,
            show_receive_stats: true,
            show_delay_stats: false,
        }
    }
}

impl SummaryReportColumns {
    pub fn none() -> Self {
        Self {
            show_visqol_mos_speech: false,
            show_visqol_mos_audio: false,
            show_pesq_mos: false,
            show_plc_mos: false,
            show_video: false,
            show_send_stats: true,
            show_receive_stats: true,
            show_delay_stats: false,
        }
    }
}

#[derive(Default)]
pub struct GroupConfig {
    /// A name to distinguish this group from others.
    pub group_name: String,
    /// Specify the charts to be displayed in the summary report for the group.
    pub chart_dimensions: Vec<ChartDimension>,
    /// The labels to use for the charts on the x-axis.
    pub x_labels: &'static [&'static str],
    /// Columns to show in summary reports.
    pub summary_report_columns: SummaryReportColumns,
}

#[derive(Debug, Clone)]
pub struct TestCaseConfig {
    /// A name to give the test case uniqueness among others.
    pub test_case_name: String,
    /// The amount of time that the test should consume (once client instances have started).
    pub length_seconds: u16,
    /// The overall configuration of different clients
    /// Requires a minimum of 1 client for group calls, 2 for 1:1 calls
    /// Only uses the first 2 configs for 1:1
    pub client_configs: Vec<CallConfig>,
    /// The number of times to run the test case.
    pub iterations: u16,
    /// Whether to create charts for reports. This takes time and is sometimes not needed
    /// when running large test sets.
    pub create_charts: bool,
    /// Whether to save media files in the output. This takes time and disk space and is sometimes
    /// not needed.
    pub save_media_files: bool,
    /// The number of analysis operations to run at once. Use 1 to keep analysis serial.
    pub analysis_concurrency: u16,
    /// Whether or not this test case should be a group call
    pub is_group_call: bool,
    /// If true, STUN packets bypass the network emulation, keeping ICE writable during stalls.
    pub stun_bypass: bool,
}

impl TestCaseConfig {
    /// Gets the first config in `client_configs`
    /// Panics if does not exist
    pub fn client_a_config(&self) -> &CallConfig {
        self.client_configs.first().unwrap()
    }

    /// Gets the first config in `client_configs`
    /// Panics if does not exist
    pub fn client_b_config(&self) -> &CallConfig {
        self.client_configs.get(1).unwrap()
    }

    pub fn usable_client_configs(&self) -> Vec<&CallConfig> {
        if self.is_group_call {
            self.client_configs.iter().collect()
        } else {
            self.client_configs.iter().take(2).collect()
        }
    }

    pub fn needs_turn_server(&self) -> bool {
        self.usable_client_configs()
            .iter()
            .any(|config| config.start_turn_server)
    }
}

impl Default for TestCaseConfig {
    fn default() -> Self {
        Self {
            test_case_name: "default".to_string(),
            length_seconds: 30,
            client_configs: vec![Default::default(), Default::default()],
            iterations: 1,
            create_charts: true,
            save_media_files: true,
            analysis_concurrency: 16,
            is_group_call: false,
            stun_bypass: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum CallProfile {
    /// Don't set any special profile for the call.
    None,
    /// Sets loss percentage using a pre-determined loss map (via a client's injectable network).
    /// Should allow for _almost_ reproducible measurements. Note that all packets for WebRTC
    /// will flow through the lossy stream.
    DeterministicLoss(u8),
}

#[derive(Clone, Debug)]
pub struct RelayServerConfig {
    /// Relay server username. Applies to all relay servers.
    pub username: String,
    /// Relay server password. Applies to all relay servers.
    pub password: String,
    /// FQDNs for STUN and TURN servers.
    pub urls: Vec<String>,
    /// IP addresses of STUN and TURN servers.
    pub urls_with_ips: Vec<String>,
    /// Relay server hostname. Applies to all relay servers to resolve certs when using IPs.
    pub hostname: Option<String>,
}

impl Default for RelayServerConfig {
    fn default() -> Self {
        Self {
            // Set our basic credentials, they aren't used if there are no relay servers.
            username: "".to_string(),
            password: "".to_string(),
            urls: vec![],
            urls_with_ips: vec![],
            hostname: None,
        }
    }
}

/// General structure for configuration settings to send to the cli.
#[derive(Debug, Clone)]
pub struct CallConfig {
    /// The maximum bitrate allowed for the call (audio and video).
    pub allowed_bitrate_kbps: u16,
    /// The audio-specific configuration.
    pub audio: AudioConfig,
    /// The video-specific configuration.
    pub video: VideoConfig,
    /// The configuration to use if testing with relay servers.
    pub relay_servers: RelayServerConfig,
    /// If using relay servers, whether or not to force their use.
    pub force_relay: bool,
    /// If true, starts a local TURN server to support STUN/TURN requests.
    pub start_turn_server: bool,
    /// The WebRTC field trial string and associated settings (i.e. "WebRTC-Something/Enabled").
    pub field_trials: Vec<String>,
    /// For quick-and-dirty testing.
    pub extra_cli_args: Vec<String>,
    /// How often to post stats to the log file.
    pub stats_interval_secs: u16,
    /// How soon to post stats to the log file before the first interval.
    pub stats_initial_offset_secs: u16,
    /// Application of a profile for the call, to be set in the client.
    pub profile: CallProfile,
    /// A flag to control packet capture. Enabling results in a `client_x.pcap` file
    /// among the generated artifacts for the test.
    pub tcpdump: bool,
}

impl CallConfig {
    pub fn with_audio_input_name(mut self, input: &str) -> Self {
        self.audio = self.audio.with_input_name(input);
        self
    }

    #[allow(dead_code)]
    pub fn with_field_trials(mut self, input: &[String]) -> Self {
        self.field_trials.extend_from_slice(input);
        self
    }
}

impl Default for CallConfig {
    fn default() -> Self {
        Self {
            allowed_bitrate_kbps: 2000,
            audio: AudioConfig::default(),
            video: VideoConfig::default(),
            relay_servers: Default::default(),
            force_relay: false,
            start_turn_server: false,
            // By default, all tests will disable the ANY port allocator setting.
            field_trials: vec![
                "RingRTC-AnyAddressPortsKillSwitch/Enabled".to_string(),
                "RingRTC-PruneTurnPorts/Enabled".to_string(),
            ],
            extra_cli_args: vec![],
            stats_interval_secs: 1,
            stats_initial_offset_secs: 0,
            profile: CallProfile::None,
            tcpdump: false,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum AudioBandwidth {
    // Constants in libopus.
    Auto = -1000,
    Full = 1105,
    SuperWide = 1104,
    Wide = 1103,
    Medium = 1102,
    Narrow = 1101,
}

impl fmt::Display for AudioBandwidth {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AudioBandwidth::Auto => write!(f, "auto"),
            AudioBandwidth::Full => write!(f, "full"),
            AudioBandwidth::SuperWide => write!(f, "super-wide"),
            AudioBandwidth::Wide => write!(f, "wide"),
            AudioBandwidth::Medium => write!(f, "medium"),
            AudioBandwidth::Narrow => write!(f, "narrow"),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum AudioAnalysisMode {
    /// Skip audio analysis. Shows up as None in reports.
    None,
    /// Perform normal analysis using visqol (reference time _should_ equal the degraded time).
    Normal,
    /// Chop the degraded file into N files with time equal to the reference file time. The
    /// reference time should be shorter than the degraded time and the degraded file time
    /// _should_ be a multiple of the reference file time. Analyze each chopped file against
    /// the reference using visqol and keep a record of the values over time.
    Chopped,
}

/// The configuration to use for all things related to audio. Note that the only audio/speech
/// codec used is Opus, and most settings are specific for it.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// The name (without path or extension) of the audio file to use as source material.
    pub input_name: String,
    /// The initial desired packet size, the amount of time in each packet.
    pub initial_packet_size_ms: i32,
    /// The minimum packet size. Used only in adaptive scenarios.
    pub min_packet_size_ms: i32,
    /// The maximum packet size. Used only in adaptive scenarios.
    pub max_packet_size_ms: i32,
    /// The initial audio encoding bitrate.
    pub initial_bitrate_bps: i32,
    /// The minimum audio encoding bitrate. Used only in adaptive scenarios.
    pub min_bitrate_bps: i32,
    /// The maximum encoding bitrate. Used only in adaptive scenarios.
    pub max_bitrate_bps: i32,
    /// The Opus bandwidth value to use (Auto is the default).
    pub bandwidth: AudioBandwidth,
    /// The Opus complexity value to use.
    pub complexity: u8,
    /// The adaptation method to use. 0 means no adaptation (the default).
    pub adaptation: i32,
    /// Flag to enable the Opus constant bitrate mode.
    pub enable_cbr: bool,
    /// Flag to enable the Opus DTX.
    pub enable_dtx: bool,
    /// Flag to enable the Opus in-band FEC.
    pub enable_fec: bool,
    /// The duration of DRED to use in 10ms units. Set to 0 to disable.
    pub dred_duration: u8,
    /// Minimum packet loss percentage reported to the encoder (0-100).
    pub min_packet_loss_percent: u8,
    /// None when using NetEq PLC, 0 use Opus PLC, 5 use Opus Deep PLC (if compiled),
    /// 6 use Opus Deep PLC + LACE (if compiled), 7 use Opus Deep PLC + NoLACE (if compiled).
    pub decoder_complexity: Option<u8>,
    /// The name of the Opus DNN weights file to load from the data directory.
    pub dnn_weights_name: String,
    /// Flag to enable transport-wide congestion control for audio.
    pub enable_tcc: bool,
    /// Flag to enable WebRTC's high pass filter.
    pub enable_high_pass_filter: bool,
    /// Flag to enable WebRTC's acoustic echo cancellation.
    pub enable_aec: bool,
    /// Flag to enable WebRTC's noise suppression.
    pub enable_ns: bool,
    /// Flag to enable WebRTC's automatic gain control.
    pub enable_agc: bool,
    /// The maximum number of packets the jitter buffer can hold.
    pub jitter_buffer_max_packets: i32,
    /// The minimum amount of delay to allow in the jitter buffer.
    pub jitter_buffer_min_delay_ms: i32,
    /// The maximum amount of delay to target in the jitter buffer.
    pub jitter_buffer_max_target_delay_ms: i32,
    /// Whether or not to turn on fast accelerate mode of the jitter buffer.
    pub jitter_buffer_fast_accelerate: bool,
    /// How often RTCP reports should be sent. Subject to jitter applied by WebRTC.
    pub rtcp_report_interval_ms: i32,
    /// Flag to enable visqol speech (wideband) analysis.
    pub visqol_speech_analysis: bool,
    /// Flag to enable visqol audio (fullband) analysis.
    pub visqol_audio_analysis: bool,
    /// Flag to enable pesq speech analysis.
    pub pesq_speech_analysis: bool,
    /// Flag to enable plc speech analysis.
    pub plc_speech_analysis: bool,
    /// The mechanism to use when analyzing speech/audio.
    pub analysis_mode: AudioAnalysisMode,
    /// Sometimes spectrogram generation takes too long, so we might want to disable it.
    pub generate_spectrogram: bool,
}

impl AudioConfig {
    pub fn with_input_name(mut self, input: &str) -> Self {
        self.input_name.clear();
        self.input_name.push_str(input);
        self
    }

    pub fn requires_speech(&self) -> bool {
        self.visqol_speech_analysis || self.pesq_speech_analysis || self.plc_speech_analysis
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            input_name: "silence".to_string(),
            initial_packet_size_ms: 60,
            min_packet_size_ms: 60,
            max_packet_size_ms: 60,
            initial_bitrate_bps: 32000,
            min_bitrate_bps: 32000,
            max_bitrate_bps: 32000,
            bandwidth: AudioBandwidth::Auto,
            complexity: 9,
            adaptation: 0,
            enable_cbr: true,
            enable_dtx: true,
            enable_fec: false,
            dred_duration: 100,
            min_packet_loss_percent: 0,
            decoder_complexity: Some(0),
            dnn_weights_name: "deep_plc-dred-weights.bin".to_string(),
            enable_tcc: false,
            enable_high_pass_filter: true,
            // Disable AEC by default to prevent random timing delays from
            // causing double-talk and thus attenuating valid audio.
            enable_aec: false,
            enable_ns: true,
            enable_agc: true,
            jitter_buffer_max_packets: 50,
            jitter_buffer_min_delay_ms: 0,
            jitter_buffer_max_target_delay_ms: 500,
            jitter_buffer_fast_accelerate: false,
            rtcp_report_interval_ms: 5000,
            visqol_speech_analysis: true,
            visqol_audio_analysis: false,
            pesq_speech_analysis: false,
            plc_speech_analysis: false,
            analysis_mode: AudioAnalysisMode::Normal,
            generate_spectrogram: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VideoConfig {
    /// The name (without path or extension) of the video file to use as source material.
    pub input_name: Option<String>,
    /// Flag to use the VP9 video codec, otherwise VP8 will be used (the default).
    pub enable_vp9: bool,
}

impl VideoConfig {
    pub fn dimensions(&self) -> Option<(u16, u16)> {
        // FIXME: Duplicated from the CLI.
        let basename = &self
            .input_name
            .as_ref()
            .map(Path::new)?
            .file_stem()
            .expect("not a valid file path");
        let basename = basename.to_str().expect("filenames must be UTF-8");
        let (_, dimensions) = basename
            .rsplit_once('@')
            .expect("cannot infer video dimensions from filename");
        let (width_str, height_str) = dimensions
            .split_once('x')
            .expect("cannot infer video dimensions from filename");
        let video_width: u16 = width_str
            .parse()
            .expect("cannot parse video width from filename");
        let video_height: u16 = height_str
            .parse()
            .expect("cannot parse video height from filename");
        Some((video_width, video_height))
    }
}

/// A NetworkConfig item to be applied at a particular time offset.
#[derive(Copy, Clone, Debug)]
pub struct NetworkConfigWithOffset {
    /// The offset is a duration, but in practice it will be quantized to 1 second.
    pub offset: Duration,
    /// The network configuration to apply at the given time.
    pub network_config: NetworkConfig,
}

impl NetworkConfigWithOffset {
    pub fn loss_and_rate(offset_secs: u64, loss: Option<Loss>, rate: u32) -> Self {
        Self {
            offset: Duration::from_secs(offset_secs),
            network_config: NetworkConfig {
                loss,
                rate,
                ..Default::default()
            },
        }
    }
}

/// General structure for network emulation settings.
/// (see https://manpages.ubuntu.com/manpages/resolute/man8/tc-netem.8.html and
/// https://man7.org/linux/man-pages/man8/tc-htb.8.html for more information)
#[derive(Copy, Clone, Default, Debug)]
pub struct NetworkConfig {
    /// ms (if 0, won't be used)
    pub delay: u32,
    /// ms (if 0, won't be used)
    pub delay_variability: u32,
    /// How to apply `delay_variability` to `delay`. None will sample from a normal distribution
    /// each time.
    pub delay_variation_strategy: Option<DelayVariationStrategy>,
    /// See [Loss]
    pub loss: Option<Loss>,
    /// % (if 0, won't be used)
    pub duplication: u8,
    /// % (if 0, won't be used)
    pub corruption: u8,
    /// % (if 0, won't be used). Use this with `delay`. This is the percentage of packets which
    /// won't be delayed.
    pub reorder: u8,
    /// % (if 0, won't be used)
    pub reorder_correlation: u8,
    /// count (if 0, won't be used)
    pub reorder_gap: u8,
    /// kbps (if 0, won't be used)
    pub rate: u32,
    /// bytes, the HTB burst and cburst for `rate` (if 0, 3000 is used)
    pub burst: u32,
    /// packets (if 0, won't be used)
    pub limit: u32,
    /// ms to accumulate in a slot before delivering packets (if 0, won't be used)
    pub slot: u32,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub enum DelayVariationStrategy {
    /// %
    Correlation(u8),
    /// The distribution to sample when determining the delay variability.
    Distribution(Distribution),
}

/// A probability distribution which can be sampled from.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub enum Distribution {
    Uniform,
    Normal,
    Pareto,
    ParetoNormal,
}

impl fmt::Display for Distribution {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Distribution::Uniform => "uniform",
                Distribution::Normal => "normal",
                Distribution::Pareto => "pareto",
                Distribution::ParetoNormal => "paretonormal",
            }
        )
    }
}

/// The Gilbert-Elliot model of packet loss and its special cases.
///
/// This models packet loss as varying depending on which of two states the model is currently
/// in. Generally one state (the "bad" state) will have higher packet loss. The probability of
/// transitioning out of the bad state can be kept low to simulate bursty packet loss.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub enum GeLossModel {
    /// Each packet is lost independently with probability p.
    Bernoulli { p: u8 },
    /// Every packet is lost in the bad state and none in the good state.
    /// p starts a burst and r ends it: loss = p/(p+r), mean burst = 100/r packets.
    SimpleGilbert { p: u8, r: u8 },
    /// SimpleGilbert, but a packet in the bad state is only lost with probability 1-h.
    /// loss = (1-h) * p/(p+r), so bursts can have gaps.
    Gilbert { p: u8, r: u8, one_minus_h: u8 },
    /// Gilbert, but a packet in the good state is also lost with probability 1-k.
    /// loss = ((1-h) * p + (1-k) * r)/(p+r), adding isolated losses between bursts.
    GilbertElliot {
        p: u8,
        r: u8,
        one_minus_h: u8,
        one_minus_k: u8,
    },
}

/// A state function using Markov models with transition probabilities.
///
/// State 1 corresponds to good reception.
/// State 2 to good reception within a burst.
/// State 3 to to burst losses.
/// State 4 to independent losses.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub enum MarkovLossModel {
    Bernoulli {
        p13: u8,
    },
    TwoState {
        p13: u8,
        p31: u8,
    },
    ThreeState {
        p13: u8,
        p31: u8,
        p32: u8,
        p23: u8,
    },
    FourState {
        p13: u8,
        p31: u8,
        p32: u8,
        p23: u8,
        p14: u8,
    },
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub enum Loss {
    /// % of packets to drop.
    Percentage(u8),
    /// The loss model to use.
    GeModel(GeLossModel),
    /// The state function (using Markov models).
    State(MarkovLossModel),
}

/// Mean Loss Burst Size (MLBS) describes the correlation of packet loss by explaining
/// how many consecutive packets are lost on average each time one is lost.
/// Based on "Quality of Experience of WebRTC-based Video Communication" from:
/// https://ntnuopen.ntnu.no/ntnu-xmlui/bitstream/handle/11250/2409900/15147_FULLTEXT.pdf
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum BurstLength {
    /// Short bursts: ~1.5 consecutive packets lost per loss event
    Short,
    /// Medium bursts: ~2 consecutive packets lost per loss event
    Medium,
    /// Long bursts: ~3 consecutive packets lost per loss event
    Long,
    /// Very long bursts: ~4 consecutive packets lost per loss event
    VeryLong,
}

impl BurstLength {
    /// Computes SimpleGilbert model parameters (p, r) for the given loss percentage.
    /// Returns (p, r) where:
    /// - r: Bad to Good transition probability (based on MLBS)
    /// - p: Good to Bad transition probability (based on loss percentage and r)
    pub fn compute_simple_gilbert_params(&self, loss_percent: u8) -> (u8, u8) {
        // Use r values from the paper (with slight adjustments for stability).
        let r = match self {
            BurstLength::Short => 65,
            BurstLength::Medium => 50,
            BurstLength::Long => 35,
            BurstLength::VeryLong => 30, // 25
        };
        let loss = loss_percent.min(99); // Make sure loss is < 100.
        let p = (((loss as f32) * (r as f32)) / (100.0 - (loss as f32))).round() as u8;
        (p, r)
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum NetworkProfile {
    /// Don't set any network emulation.
    None,
    /// Provide a network configuration but it has no impediments and a high rate.
    Default,
    /// Provide your own timed configuration(s) along with a name.
    Custom(String, Vec<NetworkConfigWithOffset>),
    /// Some delay (100ms), jitter (25ms), Loss (5% normal), constant.
    Moderate,
    /// Lots of delay (250ms), jitter (100ms), Loss (10% bursty), and other constant impediments.
    International,
    /// Good network for 10 seconds, bad loss (30%) for 10 seconds, good for 10 seconds.
    SpikyLoss,
    /// Bursty loss using SimpleGilbert model with specified loss percentage and burst length.
    BurstyLoss(u8, BurstLength),
    /// Sets a simple uniform loss percentage.
    SimpleLoss(u8),
    /// Sets a bandwidth limitation (kbps).
    LimitedBandwidth(u32),

    // Calibrated to field measurements.
    /// A healthy access link: 150 kbps, 200 ms queue, 70 ms RTT, no capacity drops.
    NominalAccess,
    /// 120 kbps dropping to 28 kbps for 20% of the call, 500 ms queue, 80 ms RTT.
    ChokedAccess,
    /// 90 kbps dropping to 16 kbps for 40% of the call, 1500 ms queue, 70 ms RTT.
    ChokedAccessSevere,
    /// [NetworkProfile::ChokedAccessSevere] with a 250 ms RTT.
    ChokedAccessSevereIntl,
    /// [NetworkProfile::ChokedAccessSevere] with a 4000 ms queue.
    ChokedAccessSevereDeep,
    /// [NetworkProfile::ChokedAccess] with a 60 ms queue, so drops cause loss instead of delay.
    ShallowBufferChoke,
    /// [NetworkProfile::NominalAccess] with bursty loss (~14%).
    BurstLossOnly,
    /// [NetworkProfile::ChokedAccess] with seven 10 s drops.
    BriefSpikes,
    /// 28 kbps for the whole call, 500 ms queue, 80 ms RTT.
    SustainedChoke,
}

/// A bottleneck link whose capacity drops at scheduled times.
struct Bottleneck {
    /// Round-trip time without queuing, for the whole call (ms).
    rtt_ms: u32,
    /// Capacity outside of drops (kbps).
    capacity_kbps: u32,
    /// Queue depth, as round-trip queuing delay at the current capacity (ms).
    queue_ms: u32,
    /// Capacity during drops (kbps).
    reduced_capacity_kbps: u32,
    /// Each drop as (start, duration), with start measured from the start of the call.
    drops: &'static [(Duration, Duration)],
    /// Random loss, for the whole call.
    loss: Option<Loss>,
}

const NOMINAL_ACCESS: Bottleneck = Bottleneck {
    rtt_ms: 70,
    capacity_kbps: 150,
    queue_ms: 200,
    reduced_capacity_kbps: 0,
    drops: &[],
    loss: None,
};

const CHOKED_ACCESS: Bottleneck = Bottleneck {
    rtt_ms: 80,
    capacity_kbps: 120,
    queue_ms: 500,
    reduced_capacity_kbps: 28,
    drops: &[
        (Duration::from_secs(45), Duration::from_secs(10)),
        (Duration::from_secs(120), Duration::from_secs(10)),
        (Duration::from_secs(200), Duration::from_secs(40)),
    ],
    loss: None,
};

const CHOKED_ACCESS_SEVERE: Bottleneck = Bottleneck {
    rtt_ms: 70,
    capacity_kbps: 90,
    queue_ms: 1500,
    reduced_capacity_kbps: 16,
    drops: &[
        (Duration::from_secs(20), Duration::from_secs(10)),
        (Duration::from_secs(75), Duration::from_secs(10)),
        (Duration::from_secs(100), Duration::from_secs(40)),
        (Duration::from_secs(165), Duration::from_secs(10)),
        (Duration::from_secs(195), Duration::from_secs(40)),
        (Duration::from_secs(250), Duration::from_secs(10)),
    ],
    loss: None,
};

impl Bottleneck {
    fn network_config(&self, capacity_kbps: u32) -> NetworkConfig {
        // Emulation is applied on each client's egress, so round-trip values are halved.
        let delay = self.rtt_ms / 2;
        let queue_ms = self.queue_ms / 2;
        NetworkConfig {
            delay,
            delay_variability: 5,
            delay_variation_strategy: Some(DelayVariationStrategy::Correlation(50)),
            loss: self.loss,
            rate: capacity_kbps,
            // Near one MTU, so a capacity drop takes effect at once.
            burst: 1600,
            // The netem queue also holds the packets in its delay stage.
            limit: (packets(capacity_kbps, queue_ms) + packets(capacity_kbps, delay)).max(2),
            ..Default::default()
        }
    }

    fn config(&self) -> Vec<NetworkConfigWithOffset> {
        let mut configs = vec![NetworkConfigWithOffset {
            offset: Duration::from_secs(2),
            network_config: self.network_config(self.capacity_kbps),
        }];

        for &(start, duration) in self.drops {
            configs.push(NetworkConfigWithOffset {
                offset: start,
                network_config: self.network_config(self.reduced_capacity_kbps),
            });
            configs.push(NetworkConfigWithOffset {
                offset: start + duration,
                network_config: self.network_config(self.capacity_kbps),
            });
        }

        configs
    }
}

/// Packets that hold `ms` of traffic at `capacity_kbps`, at 300 bytes (32 kbps Opus at 60 ms).
fn packets(capacity_kbps: u32, ms: u32) -> u32 {
    (capacity_kbps * ms).div_ceil(300 * 8)
}

impl NetworkProfile {
    pub fn get_name(&self) -> String {
        match self {
            NetworkProfile::None => "none".to_string(),
            NetworkProfile::Default => "default".to_string(),
            NetworkProfile::Custom(name, _) => name.to_string(),
            NetworkProfile::Moderate => "moderate".to_string(),
            NetworkProfile::International => "international".to_string(),
            NetworkProfile::SpikyLoss => "spiky_loss".to_string(),
            NetworkProfile::BurstyLoss(loss, burst) => {
                format!("bursty_loss_{}_{:?}", loss, burst)
            }
            NetworkProfile::SimpleLoss(loss) => {
                format!("simple_loss_{}", loss)
            }
            NetworkProfile::LimitedBandwidth(rate) => {
                format!("limited_bandwidth_{}", rate)
            }
            NetworkProfile::NominalAccess => "nominal_access".to_string(),
            NetworkProfile::ChokedAccess => "choked_access".to_string(),
            NetworkProfile::ChokedAccessSevere => "choked_access_severe".to_string(),
            NetworkProfile::ChokedAccessSevereIntl => "choked_access_severe_intl".to_string(),
            NetworkProfile::ChokedAccessSevereDeep => "choked_access_severe_deep".to_string(),
            NetworkProfile::ShallowBufferChoke => "shallow_buffer_choke".to_string(),
            NetworkProfile::BurstLossOnly => "burst_loss_only".to_string(),
            NetworkProfile::BriefSpikes => "brief_spikes".to_string(),
            NetworkProfile::SustainedChoke => "sustained_choke".to_string(),
        }
    }

    pub fn get_config(&self) -> Vec<NetworkConfigWithOffset> {
        match self {
            NetworkProfile::None => vec![],
            NetworkProfile::Default => {
                vec![NetworkConfigWithOffset {
                    offset: Duration::from_secs(2),
                    network_config: Default::default(),
                }]
            }
            NetworkProfile::Custom(_, config) => config.to_vec(),
            NetworkProfile::Moderate => {
                vec![NetworkConfigWithOffset {
                    offset: Duration::from_secs(2),
                    network_config: NetworkConfig {
                        delay: 100,
                        delay_variability: 25,
                        delay_variation_strategy: Some(DelayVariationStrategy::Distribution(
                            Distribution::Pareto,
                        )),
                        // 5% independent loss.
                        loss: Some(Loss::State(MarkovLossModel::Bernoulli { p13: 5 })),
                        ..Default::default()
                    },
                }]
            }
            NetworkProfile::International => {
                vec![NetworkConfigWithOffset {
                    offset: Duration::from_secs(2),
                    network_config: NetworkConfig {
                        delay: 250,
                        delay_variability: 100,
                        delay_variation_strategy: Some(DelayVariationStrategy::Distribution(
                            Distribution::Pareto,
                        )),
                        // Emulate a ~11% loss with a mean burst of 4 packets.
                        loss: Some(Loss::GeModel(GeLossModel::SimpleGilbert { p: 3, r: 25 })),
                        duplication: 2,
                        reorder: 5,
                        reorder_correlation: 50,
                        rate: 300,
                        limit: 30,
                        ..Default::default()
                    },
                }]
            }
            NetworkProfile::SpikyLoss => {
                vec![
                    NetworkConfigWithOffset {
                        offset: Duration::from_secs(2),
                        network_config: NetworkConfig {
                            delay: 50,
                            delay_variability: 10,
                            delay_variation_strategy: Some(DelayVariationStrategy::Correlation(50)),
                            loss: None,
                            ..Default::default()
                        },
                    },
                    NetworkConfigWithOffset {
                        offset: Duration::from_secs(10),
                        network_config: NetworkConfig {
                            delay: 100,
                            delay_variability: 20,
                            delay_variation_strategy: Some(DelayVariationStrategy::Correlation(50)),
                            // Emulate a ~31% loss with a mean burst of 4 packets.
                            loss: Some(Loss::GeModel(GeLossModel::SimpleGilbert { p: 11, r: 25 })),
                            ..Default::default()
                        },
                    },
                    NetworkConfigWithOffset {
                        offset: Duration::from_secs(20),
                        network_config: NetworkConfig {
                            delay: 50,
                            delay_variability: 10,
                            delay_variation_strategy: Some(DelayVariationStrategy::Correlation(50)),
                            loss: None,
                            ..Default::default()
                        },
                    },
                ]
            }
            NetworkProfile::BurstyLoss(loss, burst) => {
                let (p, r) = burst.compute_simple_gilbert_params(*loss);

                vec![NetworkConfigWithOffset {
                    offset: Duration::from_secs(2),
                    network_config: NetworkConfig {
                        loss: Some(Loss::GeModel(GeLossModel::SimpleGilbert { p, r })),
                        ..Default::default()
                    },
                }]
            }
            NetworkProfile::SimpleLoss(loss) => {
                vec![NetworkConfigWithOffset {
                    offset: Duration::from_secs(2),
                    network_config: NetworkConfig {
                        loss: Some(Loss::Percentage(*loss)),
                        ..Default::default()
                    },
                }]
            }
            NetworkProfile::LimitedBandwidth(rate) => {
                vec![NetworkConfigWithOffset {
                    offset: Duration::from_secs(2),
                    network_config: NetworkConfig {
                        rate: *rate,
                        limit: ((*rate * 100) / 12000).max(4), // ~100ms buffering (typical consumer network)
                        ..Default::default()
                    },
                }]
            }
            NetworkProfile::NominalAccess => NOMINAL_ACCESS.config(),
            NetworkProfile::ChokedAccess => CHOKED_ACCESS.config(),
            NetworkProfile::ChokedAccessSevere => CHOKED_ACCESS_SEVERE.config(),
            NetworkProfile::ChokedAccessSevereIntl => Bottleneck {
                rtt_ms: 250,
                ..CHOKED_ACCESS_SEVERE
            }
            .config(),
            NetworkProfile::ChokedAccessSevereDeep => Bottleneck {
                queue_ms: 4000,
                ..CHOKED_ACCESS_SEVERE
            }
            .config(),
            NetworkProfile::ShallowBufferChoke => Bottleneck {
                queue_ms: 60,
                ..CHOKED_ACCESS
            }
            .config(),
            NetworkProfile::BurstLossOnly => Bottleneck {
                // Emulate a ~14% loss with a mean burst of 4 packets.
                loss: Some(Loss::GeModel(GeLossModel::SimpleGilbert { p: 4, r: 25 })),
                ..NOMINAL_ACCESS
            }
            .config(),
            NetworkProfile::BriefSpikes => Bottleneck {
                drops: const {
                    &[
                        (Duration::from_secs(30), Duration::from_secs(10)),
                        (Duration::from_secs(70), Duration::from_secs(10)),
                        (Duration::from_secs(110), Duration::from_secs(10)),
                        (Duration::from_secs(150), Duration::from_secs(10)),
                        (Duration::from_secs(190), Duration::from_secs(10)),
                        (Duration::from_secs(230), Duration::from_secs(10)),
                        (Duration::from_secs(270), Duration::from_secs(10)),
                    ]
                },
                ..CHOKED_ACCESS
            }
            .config(),
            NetworkProfile::SustainedChoke => Bottleneck {
                rtt_ms: 80,
                capacity_kbps: 28,
                queue_ms: 500,
                ..NOMINAL_ACCESS
            }
            .config(),
        }
    }
}

#[derive(Clone, Builder, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[builder(derive(Debug), setter(into))]
pub struct ClientProfile {
    pub user_id: String,
    pub device_id: String,
    pub groups: Vec<Group>,
}

#[derive(Clone, Builder, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[builder(derive(Debug), setter(into))]
pub struct GroupMetadata {
    // friendly name for group, expected to be unique
    pub name: String,
    pub id_base64: String,
    pub members: HashSet<GroupMember>,
}

#[derive(Clone, Builder, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[builder(derive(Debug), setter(into))]
pub struct Group {
    pub metadata: GroupMetadata,
    pub membership_proof: String,
}

#[derive(Clone, Builder, Debug, Hash, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[builder(derive(Debug), setter(into))]
pub struct GroupMember {
    pub user_id: String,
    pub member_id: String,
}

#[derive(Default)]
pub struct AToZIterator {
    idx: u8,
}

impl Iterator for AToZIterator {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx > 25 {
            return None;
        }

        let result = Some((b'a' + self.idx) as char);
        self.idx += 1;
        result
    }
}

/// Starts the iterator at 172.28.0.2 and increments by up to 172.28.0.130
#[derive(Clone, Default)]
pub struct ClientIpIterator {
    idx: u8,
}

impl Iterator for ClientIpIterator {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == 128 {
            return None;
        }

        let result = Some(format!("172.28.0.{}", 2 + self.idx));
        self.idx += 1;
        result
    }
}
