//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

mod endpoint;
mod network;
mod scenario;
mod server;
mod video;

use anyhow::Result;
use clap::Parser;
use fern::Dispatch;
use log::*;
use ringrtc::{
    common::{units, CallConfig, DataMode},
    webrtc::{
        media::AudioBandwidth,
        media::AudioEncoderConfig,
        peer_connection_factory::{
            AudioConfig, AudioJitterBufferConfig, FileBasedAdmConfig, IceServer,
            RffiAudioDeviceModuleType,
        },
    },
};
use std::ffi::CString;

use crate::scenario::ManagedScenario;

#[derive(Parser, Debug)]
struct Args {
    /// Sets the name of the client.
    #[arg(long, default_value = "default")]
    name: String,

    /// If set, specifies the file to use for logging.
    #[arg(long)]
    log_file: Option<String>,

    /// How often to post stats to the log file.
    #[arg(long, default_value = "10")]
    stats_interval_secs: u16,

    /// How soon to post stats to the log file before the first interval.
    #[arg(long, default_value = "2")]
    stats_initial_offset_secs: u16,

    /// Specifies the file (including path) to use for audio input.
    #[arg(long, default_value = "")]
    input_file: String,

    /// Specifies the file (including path) to use for audio output.
    #[arg(long, default_value = "")]
    output_file: String,

    /// Specifies the file (including path) to use for video input.
    ///
    /// Only supported in managed scenario mode at this time.
    /// The video file must be in raw YUV I420 format, and will be played at 30fps.
    ///
    /// If the file stem ends with a resolution in the format "@640x480",
    /// the width and height arguments can be omitted.
    #[arg(long)]
    input_video_file: Option<String>,

    #[arg(long, default_value_t = 0)]
    input_video_width: u32,

    #[arg(long, default_value_t = 0)]
    input_video_height: u32,

    #[arg(long, default_value_t = 0)]
    output_video_width: u32,

    #[arg(long, default_value_t = 0)]
    output_video_height: u32,

    #[arg(long)]
    output_video_file: Option<String>,

    /// The allowed bitrate for all media.
    #[arg(long, default_value = "2000", value_parser = clap::value_parser!(u16).range(30..))]
    allowed_bitrate_kbps: u16,

    /// The initial bitrate for encoding audio.
    #[arg(long, default_value = "32000", value_parser = clap::value_parser!(i32).range(500..))]
    initial_bitrate_bps: i32,

    /// The minimum bitrate for encoding audio.
    #[arg(long, default_value = "16000", value_parser = clap::value_parser!(i32).range(500..))]
    min_bitrate_bps: i32,

    /// The maximum bitrate for encoding audio.
    #[arg(long, default_value = "32000", value_parser = clap::value_parser!(i32).range(500..))]
    max_bitrate_bps: i32,

    /// The encoding bandwidth for audio.
    #[arg(long, default_value_t = AudioBandwidth::Auto, value_enum)]
    bandwidth: AudioBandwidth,

    /// The encoding complexity for audio.
    #[arg(long, default_value = "9", value_parser = clap::value_parser!(i32).range(0..=10))]
    complexity: i32,

    /// The size of an audio frame (ptime).
    #[arg(long, default_value = "20", value_parser = clap::builder::PossibleValuesParser::new(["20", "40", "60", "80", "100", "120"]))]
    initial_packet_size_ms: String,

    /// The minimum size of an audio frame (ptime).
    #[arg(long, default_value = "20", value_parser = clap::builder::PossibleValuesParser::new(["20", "40", "60", "80", "100", "120"]))]
    min_packet_size_ms: String,

    /// The maximum size of an audio frame (ptime).
    #[arg(long, default_value = "20", value_parser = clap::builder::PossibleValuesParser::new(["20", "40", "60", "80", "100", "120"]))]
    max_packet_size_ms: String,

    /// Whether to use CBR for encoding audio. False means VBR.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    cbr: bool,

    /// Whether to use DTX when encoding audio.
    #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
    dtx: bool,

    /// Whether to use FEC when encoding audio.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    fec: bool,

    /// Whether to use adaptation when encoding audio. Set to 0 to disable (default).
    #[arg(long, default_value_t = 0)]
    adaptation: i32,

    /// Whether to enable transport-cc feedback for audio. This will allow the bitrate to vary
    /// between `min_bitrate_bps` and `max_bitrate_bps` when using CBR.
    #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
    tcc: bool,

    /// Whether to enable redundant packets for audio.
    #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
    red: bool,

    /// Whether to enable the VP9 codec for video.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    vp9: bool,

    /// Whether to enable a high pass filter on audio input.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    high_pass_filter: bool,

    /// Whether to enable Acoustic Echo Cancellation (AEC) on audio input.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    aec: bool,

    /// Whether to enable Noise Suppression (NS) on audio input.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    ns: bool,

    /// Whether to enable Automatic Gain Control (AGC) on audio input.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    agc: bool,

    /// Specifies the field trials to configure in WebRTC. The format of the string is
    /// "$FIELD_TRIAL_NAME/$FIELD_TRIAL_VALUE/" (concatenated for each field trial to configure).
    /// Note that if any field trials are configured, the string must end in a "/".
    #[arg(long, default_value = "")]
    field_trials: String,

    /// A list of relay servers to provide to WebRTC for connectivity options.
    #[arg(long)]
    relay_servers: Vec<String>,

    #[arg(long, default_value = "")]
    relay_username: String,

    #[arg(long, default_value = "")]
    relay_password: String,

    /// Whether to force the use of relay servers or not.
    #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
    force_relay: bool,

    #[arg(long, default_value = "50")]
    audio_jitter_buffer_max_packets: i32,

    #[arg(long, default_value = "0")]
    audio_jitter_buffer_min_delay_ms: i32,

    #[arg(long, default_value = "500")]
    audio_jitter_buffer_max_target_delay_ms: i32,

    #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
    audio_jitter_buffer_fast_accelerate: bool,

    #[arg(long, default_value = "5000")]
    audio_rtcp_report_interval_ms: i32,

    /// The IP address of the client (the main interface to test with).
    #[arg(long, default_value = "")]
    ip: String,

    /// Deterministic loss percent to use to determine when to drop packets. This will
    /// turn on the injectable network using a UDP socket.
    #[arg(long)]
    deterministic_loss: Option<u8>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let fern_logger = Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}:{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.file().unwrap(),
                record.line().unwrap(),
                message
            ))
        })
        .level(LevelFilter::Debug);

    if let Some(log_file) = args.log_file {
        fern_logger.chain(fern::log_file(log_file)?).apply()?;
    } else {
        fern_logger.chain(std::io::stdout()).apply()?;
    }

    // Show WebRTC logs via application Logger while debugging.
    ringrtc::webrtc::logging::set_logger(log::LevelFilter::Debug);

    info!("Setting field trials to {}", &args.field_trials);
    ringrtc::webrtc::field_trial::init(&args.field_trials).expect("no null characters");

    let ice_server = if args.relay_servers.is_empty() {
        IceServer::none()
    } else {
        info!("Setting relay servers: {:?}", args.relay_servers);
        info!("  username: {}", args.relay_username);
        info!("  password: {}", args.relay_password);
        info!("     force: {}", args.force_relay);

        IceServer::new(
            args.relay_username,
            args.relay_password,
            // TODO: Add support for hostname when TLS TURN is supported with the call sim
            "".to_string(),
            args.relay_servers,
        )
    };

    // Create a call configuration that should be used for the call.
    let call_config = CallConfig {
        // This configuration is currently the same as `Normal`.
        data_mode: DataMode::Custom {
            max_bitrate: units::DataRate::from_kbps(args.allowed_bitrate_kbps as u64),
            max_group_call_receive_rate: units::DataRate::default(),
        },
        stats_interval_secs: args.stats_interval_secs,
        stats_initial_offset_secs: args.stats_initial_offset_secs,
        audio_config: AudioConfig {
            audio_device_module_type: RffiAudioDeviceModuleType::File,
            file_based_adm_config: Some(FileBasedAdmConfig {
                input_file: CString::new(args.input_file).expect("CString::new failed"),
                output_file: CString::new(args.output_file).expect("CString::new failed"),
            }),
            high_pass_filter_enabled: args.high_pass_filter,
            aec_enabled: args.aec,
            ns_enabled: args.ns,
            agc_enabled: args.agc,
        },
        audio_encoder_config: AudioEncoderConfig {
            initial_packet_size_ms: args
                .initial_packet_size_ms
                .parse()
                .expect("validated by clap"),
            min_packet_size_ms: args.min_packet_size_ms.parse().expect("validated by clap"),
            max_packet_size_ms: args.max_packet_size_ms.parse().expect("validated by clap"),
            initial_bitrate_bps: args.initial_bitrate_bps,
            min_bitrate_bps: args.min_bitrate_bps,
            max_bitrate_bps: args.max_bitrate_bps,
            bandwidth: args.bandwidth,
            complexity: args.complexity,
            adaptation: args.adaptation,
            enable_cbr: args.cbr,
            enable_dtx: args.dtx,
            enable_fec: args.fec,
        },
        enable_tcc_audio: args.tcc,
        enable_red_audio: args.red,
        audio_jitter_buffer_config: AudioJitterBufferConfig {
            max_packets: args.audio_jitter_buffer_max_packets,
            min_delay_ms: args.audio_jitter_buffer_min_delay_ms,
            max_target_delay_ms: args.audio_jitter_buffer_max_target_delay_ms,
            fast_accelerate: args.audio_jitter_buffer_fast_accelerate,
        },
        audio_rtcp_report_interval_ms: args.audio_rtcp_report_interval_ms,
        enable_vp9: args.vp9,
    };

    let mut scenario = ManagedScenario::new()?;
    scenario.run(
        &args.name,
        &args.ip,
        call_config,
        scenario::ScenarioConfig {
            video_width: args.input_video_width,
            video_height: args.input_video_height,
            video_input: args.input_video_file.map(Into::into),
            output_video_width: args.output_video_width,
            output_video_height: args.output_video_height,
            video_output: args.output_video_file.map(Into::into),
            ice_server,
            force_relay: args.force_relay,
        },
        args.deterministic_loss,
    );

    Ok(())
}
