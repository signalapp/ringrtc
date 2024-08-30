//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{fmt, path::Path, time::Duration};

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
    pub show_visqol_mos_average: bool,
    pub show_pesq_mos: bool,
    pub show_plc_mos: bool,
    /// A general flag to control video columns.
    pub show_video: bool,
}

impl Default for SummaryReportColumns {
    fn default() -> Self {
        Self {
            show_visqol_mos_speech: true,
            show_visqol_mos_audio: true,
            show_visqol_mos_average: false,
            show_pesq_mos: false,
            show_plc_mos: false,
            show_video: true,
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
    /// The overall configuration specific to client A.
    pub client_a_config: CallConfig,
    /// The overall configuration specific to client B.
    pub client_b_config: CallConfig,
    /// A flag to control recording of packet capture. Enabling this results in a `tcpdump.pcap`
    /// file among the generated artifacts for the test.
    pub tcp_dump: bool,
    /// The number of times to run the test case.
    pub iterations: u16,
    /// Whether to create charts for reports. This takes time and is sometimes not needed
    /// when running large test sets.
    pub create_charts: bool,
}

impl Default for TestCaseConfig {
    fn default() -> Self {
        Self {
            test_case_name: "default".to_string(),
            length_seconds: 30,
            client_a_config: Default::default(),
            client_b_config: Default::default(),
            tcp_dump: false,
            iterations: 1,
            create_charts: true,
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

/// General structure for configuration settings to send to the cli.
#[derive(Debug, Clone)]
pub struct CallConfig {
    /// The maximum bitrate allowed for the call (audio and video).
    pub allowed_bitrate_kbps: u16,
    /// The audio-specific configuration.
    pub audio: AudioConfig,
    /// The video-specific configuration.
    pub video: VideoConfig,
    /// Relay server configuration, a vector of STUN/TURN server(s) that the client can use.
    /// If this is empty, the test TURN server will not even be started.
    pub relay_servers: Vec<String>,
    /// Relay server username. Applies to all relay servers.
    pub relay_username: String,
    /// Relay server password. Applies to all relay servers.
    pub relay_password: String,
    /// If using relay servers, whether or not to force their use.
    pub force_relay: bool,
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
            relay_servers: vec![],
            // Set our basic credentials, but they aren't used if there are no relay servers.
            relay_username: "test".to_string(),
            relay_password: "test".to_string(),
            force_relay: false,
            // By default, all tests will disable the ANY port allocator setting.
            field_trials: vec![
                "RingRTC-AnyAddressPortsKillSwitch/Enabled".to_string(),
                "RingRTC-PruneTurnPorts/Enabled".to_string(),
            ],
            extra_cli_args: vec![],
            stats_interval_secs: 1,
            stats_initial_offset_secs: 0,
            profile: CallProfile::None,
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
    pub complexity: i32,
    /// The adaptation method to use. 0 means no adaptation (the default).
    pub adaptation: i32,
    /// Flag to enable the Opus constant bitrate mode.
    pub enable_cbr: bool,
    /// Flag to enable the Opus DTX.
    pub enable_dtx: bool,
    /// Flag to enable the Opus in-band FEC.
    pub enable_fec: bool,
    /// Flag to enable transport-wide congestion control for audio.
    pub enable_tcc: bool,
    /// Flag to enabled redundant packets to be sent for audio.
    pub enable_red: bool,
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
            initial_packet_size_ms: 20,
            min_packet_size_ms: 20,
            max_packet_size_ms: 20,
            initial_bitrate_bps: 32000,
            min_bitrate_bps: 16000,
            max_bitrate_bps: 32000,
            bandwidth: AudioBandwidth::Auto,
            complexity: 9,
            adaptation: 0,
            enable_cbr: true,
            enable_dtx: true,
            enable_fec: true,
            enable_tcc: false,
            enable_red: false,
            enable_high_pass_filter: true,
            // Default tests now disable AEC in order to prevent random timing delays
            // from causing double-talk and thus attenuating valid audio.
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

/// General structure for network emulation settings.
/// (see https://manpages.ubuntu.com/manpages/jammy/man8/tc-netem.8.html)
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
    Bernoulli {
        p: u8,
    },
    SimpleGilbert {
        p: u8,
        r: u8,
    },
    Gilbert {
        p: u8,
        r: u8,
        one_minus_h: u8,
    },
    GilbertElliot {
        /// Transition probability from the good state to the bad state.
        p: u8,
        /// Transition probability from the bad state to the good state.
        r: u8,
        /// 1-h, the loss probability while in the bad state. (default: 1)
        one_minus_h: u8,
        /// 1-k, the loss probability while in the good state. (default: 0)
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

/// This struct can be used to form Simple Gilbert loss models based on "Mean Loss Burst Size"
/// from here: https://ntnuopen.ntnu.no/ntnu-xmlui/bitstream/handle/11250/2409900/15147_FULLTEXT.pdf
#[allow(dead_code)]
struct MlbsData {
    loss: u8,
    mlbs: f32,
    r: u8,
    p: u8,
}

#[allow(dead_code)]
#[rustfmt::skip]
const MLBS_DATA: [MlbsData; 24] = [
    MlbsData { loss: 5, mlbs: 1.5, r: 65, p: 3 },
    MlbsData { loss: 5, mlbs: 2.0, r: 50, p: 3 },
    MlbsData { loss: 5, mlbs: 3.0, r: 35, p: 2 },
    MlbsData { loss: 5, mlbs: 4.0, r: 25, p: 1 },
    MlbsData { loss: 10, mlbs: 1.5, r: 65, p: 7 },
    MlbsData { loss: 10, mlbs: 2.0, r: 50, p: 6 },
    MlbsData { loss: 10, mlbs: 3.0, r: 35, p: 4 },
    MlbsData { loss: 10, mlbs: 4.0, r: 25, p: 3 },
    MlbsData { loss: 20, mlbs: 1.5, r: 65, p: 16 },
    MlbsData { loss: 20, mlbs: 2.0, r: 50, p: 13 },
    MlbsData { loss: 20, mlbs: 3.0, r: 35, p: 9 },
    MlbsData { loss: 20, mlbs: 4.0, r: 25, p: 6 },
    MlbsData { loss: 30, mlbs: 1.5, r: 65, p: 28 },
    MlbsData { loss: 30, mlbs: 2.0, r: 50, p: 21 },
    MlbsData { loss: 30, mlbs: 3.0, r: 35, p: 15 },
    MlbsData { loss: 30, mlbs: 4.0, r: 25, p: 11 },
    MlbsData { loss: 40, mlbs: 1.5, r: 65, p: 43 },
    MlbsData { loss: 40, mlbs: 2.0, r: 50, p: 33 },
    MlbsData { loss: 40, mlbs: 3.0, r: 35, p: 23 },
    MlbsData { loss: 40, mlbs: 4.0, r: 25, p: 17 },
    MlbsData { loss: 50, mlbs: 1.5, r: 65, p: 65 },
    MlbsData { loss: 50, mlbs: 2.0, r: 50, p: 50 },
    MlbsData { loss: 50, mlbs: 3.0, r: 35, p: 35 },
    MlbsData { loss: 50, mlbs: 4.0, r: 25, p: 25 },
];

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
    /// Bursty loss model using pre-calculated mean loss burst size (may fail test if no match).
    //BurstyLoss { loss: u8, mlbs: u8 },
    /// Sets a simple uniform loss percentage.
    SimpleLoss(u8),
    /// Sets a bandwidth limitation (kbps).
    LimitedBandwidth(u32),
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
            NetworkProfile::SimpleLoss(loss) => {
                format!("simple_loss_{}", loss)
            }
            NetworkProfile::LimitedBandwidth(rate) => {
                format!("limited_bandwidth_{}", rate)
            }
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
                        loss: Some(Loss::GeModel(GeLossModel::SimpleGilbert { p: 3, r: 25 })),
                        duplication: 2,
                        corruption: 0,
                        reorder: 5,
                        reorder_correlation: 50,
                        reorder_gap: 0,
                        rate: 300,
                        limit: 250,
                        slot: 0,
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
                        limit: 16,
                        ..Default::default()
                    },
                }]
            }
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientProfile {
    pub user_id: String,
    pub device_id: String,
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    // friendly name for group, expected to be unique in config file
    pub name: String,
    // Base64 encoded
    pub id: String,
    pub membership_proof: String,
    pub members: Vec<GroupMember>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    pub user_id: String,
    pub member_id: String,
}
