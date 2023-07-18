//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

mod endpoint;
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
            AudioConfig, FileBasedAdmConfig, IceServer, RffiAudioDeviceModuleType,
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

    /// The target bitrate to encode audio at. When tcc is enabled, this is the initial bitrate.
    #[arg(long, default_value = "40000", value_parser = clap::value_parser!(u16).range(500..))]
    default_bitrate_bps: u16,

    /// The minimum bitrate to encode audio at. This is only used when tcc is enabled.
    #[arg(long, default_value = "20000", value_parser = clap::value_parser!(u16).range(500..))]
    min_bitrate_bps: u16,

    /// The maximum bitrate to encode audio at. This is only used when tcc is enabled.
    #[arg(long, default_value = "40000", value_parser = clap::value_parser!(u16).range(500..))]
    max_bitrate_bps: u16,

    /// The encoding complexity for audio.
    #[arg(long, default_value = "9", value_parser = clap::value_parser!(u16).range(0..=10))]
    complexity: u16,

    /// The length of an audio frame size (ptime).
    #[arg(long, default_value = "20", value_parser = clap::builder::PossibleValuesParser::new(["20", "40", "60", "80", "100", "120"]))]
    packet_size_ms: String,

    /// Whether to use CBR for encoding audio. False means VBR.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    cbr: bool,

    /// Whether to use DTX when encoding audio.
    #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
    dtx: bool,

    /// Whether to use FEC when encoding audio.
    #[arg(long, action = clap::ArgAction::Set, default_value = "true")]
    fec: bool,

    /// Whether to enable transport-cc feedback for audio. This will allow the bitrate to vary
    /// between `min_bitrate_bps` and `max_bitrate_bps` when using CBR.
    #[arg(long, action = clap::ArgAction::Set, default_value = "false")]
    tcc: bool,

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

    #[arg(long, default_value = "200")]
    audio_jitter_buffer_max_packets: u16,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let fern_logger = Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.target(),
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

        IceServer::new(args.relay_username, args.relay_password, args.relay_servers)
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
            packet_size_ms: args.packet_size_ms.parse().expect("validated by clap"),
            bandwidth: AudioBandwidth::Auto,
            start_bitrate_bps: args.default_bitrate_bps,
            min_bitrate_bps: args.min_bitrate_bps,
            max_bitrate_bps: args.max_bitrate_bps,
            complexity: args.complexity,
            enable_cbr: args.cbr,
            enable_dtx: args.dtx,
            enable_fec: args.fec,
        },
        enable_tcc_audio: args.tcc,
        audio_jitter_buffer_max_packets: args.audio_jitter_buffer_max_packets as isize,
        enable_vp9: args.vp9,
    };

    let mut scenario = ManagedScenario::new()?;
    scenario.run(
        &args.name,
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
    );

    Ok(())
}
