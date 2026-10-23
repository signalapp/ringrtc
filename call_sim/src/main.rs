//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

mod audio;
mod common;
mod config;
mod docker;
mod report;
mod test;

use std::{collections::HashSet, env, io::Write, time::Duration};

use anyhow::Result;
use clap::Parser;
use hex::FromHex;
use itertools::iproduct;
use log::info;

use crate::{
    common::{
        AudioAnalysisMode, AudioConfig, BurstLength, CallConfig, CallProfile::DeterministicLoss,
        ChartDimension, DelayVariationStrategy, Distribution, GeLossModel, GroupConfig, Loss,
        NetworkConfig, NetworkConfigWithOffset, NetworkProfile, RelayServerConfig,
        SummaryReportColumns, TestCaseConfig, VideoConfig,
    },
    config::DynamicClientProfileFactory,
    docker::{build_images, clean_network},
    test::{CallTypeConfig, Test, clean_up_all_containers},
};

fn compile_time_root_directory() -> &'static std::ffi::OsStr {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent() // ringrtc
        .unwrap()
        .as_os_str()
}

#[derive(Parser, Debug)]
struct Args {
    /// Specifies which tests to run.
    test_sets: Vec<String>,

    /// Specifies path to the root of the ringrtc directory.
    /// Usually relative when provided on the command line.
    #[arg(long, default_value = compile_time_root_directory())]
    root: String,

    /// If set, specifies the output directory for artifacts, relative to the root. Otherwise
    /// a `call_sim/test_results` directory will be created on the provided root.
    #[arg(long, default_value = "call_sim/test_results")]
    output_dir: String,

    /// If set, specifies the directory where reference media can be found, relative to the
    /// root directory. Otherwise the `call_sim/media` directory will be assumed.
    #[arg(long, default_value = "call_sim/media")]
    media_dir: String,

    /// If set, specifies the directory where misc data can be found, relative to the
    /// root directory. Otherwise the `call_sim/data` directory will be assumed.
    #[arg(long, default_value = "call_sim/data")]
    data_dir: String,

    /// Builds all dependent docker images.
    #[arg(short, long)]
    build: bool,

    /// Cleans up containers and networks before running (in case prior runs failed to do so).
    #[arg(short, long)]
    clean: bool,

    #[arg(long)]
    client_profile_dir: Option<String>,

    /// Specify a group from the group list in the client_profile file
    /// If None, then uses the first group in the list
    #[arg(long)]
    group_name: Option<String>,

    /// Run `perf record` on the clients
    #[arg(long)]
    profile: bool,

    /// Skip building the visqol_mos container. This build can be slow, and it's only needed for
    /// certain analyses.
    #[arg(long)]
    skip_visqol_mos_build: bool,

    /// Which clients to analyze and include a report section for, as a comma separated list.
    /// For 1:1 calls, you must specify both a & b to get both sides of analysis
    ///
    /// Clients may be named either by their suffix or in full, so `--analyze-clients=a,b` and
    /// `--analyze-clients=client_a,client_b` are equivalent.
    ///
    /// When unset, group calls analyze every client, and 1:1 calls analyze every client except
    /// client_a, whose received media is usually not interesting.
    #[arg(long, value_delimiter = ',')]
    analyze_clients: Option<Vec<String>>,
}

/// If set to None, the tests will default to a local SFU.
/// Set the url and auth key when running call sim group calls on a remote sfu.
/// The Auth Key is used to generate profiles that will be accepted by the SFUs
/// authentication
const SFU_URL: Option<&str> = None; // Some("https://sfu.test.voip.signal.org");
fn group_auth_key_gen() -> [u8; 32] {
    <[u8; 32]>::from_hex("deaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddead")
        .unwrap()
}

/// This is a minimal example of a test.
async fn run_minimal_example(test: &mut Test) -> Result<()> {
    // Optional: Pre-process sounds that you will use. This will generate a spectrogram
    // and calculate a reference MOS for each sound. Normally, this might be useful,
    // but sometimes you just want to run a test and don't need this information.
    // Here we are leaving out the `silence` sound since there is no point to get a
    // MOS value for it.
    test.preprocess_sounds(vec!["normal_phrasing"]).await?;

    // Run a test with a `default` config. Here, we will use 30-second calls and specify
    // a default call configuration. The test will actually run one or more test cases,
    // each a permutation of the call configuration, sound pairs, and network profiles.
    test.run(
        GroupConfig {
            group_name: "minimal_example".to_string(),
            ..Default::default()
        },
        vec![TestCaseConfig {
            test_case_name: "default".to_string(),
            client_configs: vec![
                // client_a is sending a set of spoken phrases, which will be analyzed from
                // client_b's perspective.
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        // We don't look at analysis from client_a's point of view, so there
                        // is no need to generate anything for it.
                        generate_spectrogram: false,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                // In this case, client_b is sending recorded silence (the default).
                CallConfig::default(),
            ],
            ..Default::default()
        }],
        // Finally, the network profiles to test against can be specified. The `None`
        // profile won't try to emulate anything, which is useful when establishing
        // baseline measurements.
        vec![NetworkProfile::None],
    )
    .await?;

    Ok(())
}

/// This is a test set to test the "normal phrasing" audio (and video) over various network
/// profiles. This sets packet time to 60ms to align with our production configuration.
async fn run_baseline(test: &mut Test, with_video: bool) -> Result<()> {
    let video = if with_video {
        VideoConfig {
            input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
            ..Default::default()
        }
    } else {
        Default::default()
    };

    test.run(
        GroupConfig {
            group_name: match with_video {
                false => "baseline".to_string(),
                true => "baseline_with_video".to_string(),
            },
            // Show all the different measurements in the summary columns. Hide video.
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: with_video,
                ..Default::default()
            },
            ..Default::default()
        },
        vec![TestCaseConfig {
            test_case_name: "ptime-60".to_string(),
            client_configs: vec![
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        generate_spectrogram: false,
                        ..Default::default()
                    },
                    video: video.clone(),
                    ..Default::default()
                },
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        // Calculate all mos values for these audio tests.
                        visqol_speech_analysis: true,
                        visqol_audio_analysis: true,
                        pesq_speech_analysis: true,
                        plc_speech_analysis: true,
                        ..Default::default()
                    },
                    video,
                    ..Default::default()
                },
            ],
            // Run 3 iterations of each test to get an average to help contain the
            // non-deterministic behavior of the tests.
            iterations: 3,
            ..Default::default()
        }],
        vec![
            NetworkProfile::Default,
            NetworkProfile::Moderate,
            NetworkProfile::International,
            NetworkProfile::SpikyLoss,
            NetworkProfile::LimitedBandwidth(300),
            NetworkProfile::LimitedBandwidth(100),
            NetworkProfile::LimitedBandwidth(50),
            NetworkProfile::LimitedBandwidth(25),
            NetworkProfile::SimpleLoss(10),
            NetworkProfile::SimpleLoss(20),
            NetworkProfile::SimpleLoss(30),
            NetworkProfile::SimpleLoss(40),
            NetworkProfile::SimpleLoss(50),
        ],
    )
    .await?;

    Ok(())
}

/// Test 60ms ptime over a range of loss using various bursty loss profiles.
async fn run_bursty_loss_test(test: &mut Test, with_video: bool) -> Result<()> {
    let video = if with_video {
        VideoConfig {
            input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
            ..Default::default()
        }
    } else {
        Default::default()
    };

    test.run(
        GroupConfig {
            group_name: match with_video {
                false => "bursty_loss_test".to_string(),
                true => "bursty_loss_test_with_video".to_string(),
            },
            // Show all the different measurements in the summary columns. Hide video.
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: with_video,
                ..Default::default()
            },
            ..Default::default()
        },
        vec![TestCaseConfig {
            test_case_name: "ptime-60".to_string(),
            client_configs: vec![
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        generate_spectrogram: false,
                        ..Default::default()
                    },
                    video: video.clone(),
                    ..Default::default()
                },
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        // Calculate all mos values for these audio tests.
                        visqol_speech_analysis: true,
                        visqol_audio_analysis: true,
                        pesq_speech_analysis: true,
                        plc_speech_analysis: true,
                        ..Default::default()
                    },
                    video,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        vec![
            NetworkProfile::Default,
            NetworkProfile::BurstyLoss(10, BurstLength::Short),
            NetworkProfile::BurstyLoss(10, BurstLength::Medium),
            NetworkProfile::BurstyLoss(10, BurstLength::Long),
            NetworkProfile::BurstyLoss(10, BurstLength::VeryLong),
            NetworkProfile::BurstyLoss(20, BurstLength::Short),
            NetworkProfile::BurstyLoss(20, BurstLength::Medium),
            NetworkProfile::BurstyLoss(20, BurstLength::Long),
            NetworkProfile::BurstyLoss(20, BurstLength::VeryLong),
            NetworkProfile::BurstyLoss(30, BurstLength::Short),
            NetworkProfile::BurstyLoss(30, BurstLength::Medium),
            NetworkProfile::BurstyLoss(30, BurstLength::Long),
            NetworkProfile::BurstyLoss(30, BurstLength::VeryLong),
            NetworkProfile::BurstyLoss(40, BurstLength::Short),
            NetworkProfile::BurstyLoss(40, BurstLength::Medium),
            NetworkProfile::BurstyLoss(40, BurstLength::Long),
            NetworkProfile::BurstyLoss(40, BurstLength::VeryLong),
            NetworkProfile::BurstyLoss(50, BurstLength::Short),
            NetworkProfile::BurstyLoss(50, BurstLength::Medium),
            NetworkProfile::BurstyLoss(50, BurstLength::Long),
            NetworkProfile::BurstyLoss(50, BurstLength::VeryLong),
        ],
    )
    .await?;

    Ok(())
}

/// Test 20ms and 60ms ptime over a range of deterministic loss, with and without dtx.
/// Note that deterministic loss is better than the SimpleLoss network profile, but
/// it is still not completely reliable.
async fn run_deterministic_loss_test(test: &mut Test, with_video: bool) -> Result<()> {
    let video = if with_video {
        VideoConfig {
            input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
            ..Default::default()
        }
    } else {
        Default::default()
    };
    let ptime_values = [20, 60];
    let dtx_values = [false, true];
    let loss_values = (0..=50).step_by(10);

    let test_cases = iproduct!(dtx_values, ptime_values, loss_values)
        .map(
            |(enable_dtx, initial_packet_size_ms, loss)| TestCaseConfig {
                test_case_name: format!("ptime-{initial_packet_size_ms}_dtx-{enable_dtx}_{loss}"),
                length_seconds: 30,
                client_configs: vec![
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            initial_packet_size_ms,
                            enable_dtx,
                            generate_spectrogram: false,
                            ..Default::default()
                        },
                        video: video.clone(),
                        profile: DeterministicLoss(loss),
                        ..Default::default()
                    },
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            initial_packet_size_ms,
                            enable_dtx,
                            visqol_speech_analysis: true,
                            visqol_audio_analysis: true,
                            pesq_speech_analysis: true,
                            plc_speech_analysis: true,
                            ..Default::default()
                        },
                        video: video.clone(),
                        profile: DeterministicLoss(loss),
                        ..Default::default()
                    },
                ],
                iterations: 1,
                ..Default::default()
            },
        )
        .collect::<Vec<_>>();

    test.run(
        GroupConfig {
            group_name: match with_video {
                false => "deterministic_loss_test".to_string(),
                true => "deterministic_loss_test_with_video".to_string(),
            },
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: with_video,
                ..Default::default()
            },
            ..Default::default()
        },
        test_cases,
        vec![NetworkProfile::None],
    )
    .await?;

    Ok(())
}

/// Run as a group call (`group_multi_deterministic_loss_test`).
///
/// Differs from `deterministic_loss_test` in three ways, all of which are needed to get a
/// meaningful MOS out of a call with more than two participants:
///
///  - Only client A talks. If more than one client sends audio the listeners record a mix and
///    score badly no matter how clean the network is.
///  - Client A uses `constant_phrasing` instead of `normal_phrasing`. The SFU stops forwarding
///    silent packets, typically when DTX has kicked in. The SFU forwarder preserves sequence \
///    number offsets, those drops show up at the receivers as packet loss. `constant_phrasing`
///    silences are short enough to avoid packets getting dropped.
///  - Loss is applied only to the listeners. This stops client A from losing RTCP reports.
async fn run_multi_deterministic_loss_test(test: &mut Test, client_count: usize) -> Result<()> {
    assert!(
        client_count >= 2,
        "need at least one talker and one listener"
    );

    let loss_values = (0..=20).step_by(10);

    let test_cases = loss_values
        .map(|loss| {
            // Client A is the only talker, and is not measured.
            let talker = CallConfig {
                audio: AudioConfig {
                    input_name: "constant_phrasing".to_string(),
                    initial_packet_size_ms: 60,
                    generate_spectrogram: false,
                    ..Default::default()
                },
                profile: DeterministicLoss(0),
                ..Default::default()
            };

            // Everyone else stays silent and is measured against the talker's audio.
            let listener = CallConfig {
                audio: AudioConfig {
                    input_name: "silence".to_string(),
                    initial_packet_size_ms: 60,
                    visqol_speech_analysis: true,
                    visqol_audio_analysis: true,
                    pesq_speech_analysis: true,
                    plc_speech_analysis: true,
                    ..Default::default()
                },
                profile: DeterministicLoss(loss),
                ..Default::default()
            };

            let mut client_configs = vec![talker];
            client_configs.extend(std::iter::repeat_n(listener, client_count - 1));

            TestCaseConfig {
                test_case_name: format!("clients-{client_count}_loss-{loss}"),
                length_seconds: 30,
                client_configs,
                iterations: 1,
                ..Default::default()
            }
        })
        .collect::<Vec<_>>();

    test.run(
        GroupConfig {
            group_name: "multi_deterministic_loss_test".to_string(),
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                ..Default::default()
            },
            ..Default::default()
        },
        test_cases,
        vec![NetworkProfile::None],
    )
    .await?;

    Ok(())
}

/// Here is a test to run without a TURN server, with a TURN server, and forcing the use of a
/// TURN server over UDP, and then forcing the use of a TURN server over TCP.
/// Notes:
///  - The default username and password are already set by default
///  - Both clients will use the TURN server (in this test)
///  - The `turn` domain name should resolve by Docker to the container with the name `turn`
async fn run_relay_tests(test: &mut Test) -> Result<()> {
    test.run(
        GroupConfig {
            group_name: "relay_tests".to_string(),
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_audio: false,
                show_video: false,
                ..Default::default()
            },
            chart_dimensions: vec![ChartDimension::MosSpeech],
            ..Default::default()
        },
        vec![
            TestCaseConfig {
                test_case_name: "no_relay".to_string(),
                client_configs: vec![
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            TestCaseConfig {
                test_case_name: "with_relay".to_string(),
                client_configs: vec![
                    CallConfig {
                        relay_servers: RelayServerConfig {
                            urls: vec![
                                "stun:turn".to_string(),
                                "turn:turn".to_string(),
                                "turn:turn:80?transport=tcp".to_string(),
                            ],
                            ..Default::default()
                        },
                        start_turn_server: true,
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    CallConfig {
                        relay_servers: RelayServerConfig {
                            urls: vec![
                                "stun:turn".to_string(),
                                "turn:turn".to_string(),
                                "turn:turn:80?transport=tcp".to_string(),
                            ],
                            ..Default::default()
                        },
                        start_turn_server: true,
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            TestCaseConfig {
                test_case_name: "force_udp_relay".to_string(),
                client_configs: vec![
                    CallConfig {
                        relay_servers: RelayServerConfig {
                            urls: vec!["turn:turn".to_string()],
                            ..Default::default()
                        },
                        start_turn_server: true,
                        force_relay: true,
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    CallConfig {
                        relay_servers: RelayServerConfig {
                            urls: vec!["turn:turn".to_string()],
                            ..Default::default()
                        },
                        start_turn_server: true,
                        force_relay: true,
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            TestCaseConfig {
                test_case_name: "force_tcp_relay".to_string(),
                client_configs: vec![
                    CallConfig {
                        relay_servers: RelayServerConfig {
                            urls: vec!["turn:turn:80?transport=tcp".to_string()],
                            ..Default::default()
                        },
                        start_turn_server: true,
                        force_relay: true,
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    CallConfig {
                        relay_servers: RelayServerConfig {
                            urls: vec!["turn:turn:80?transport=tcp".to_string()],
                            ..Default::default()
                        },
                        start_turn_server: true,
                        force_relay: true,
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        ],
        vec![NetworkProfile::None],
    )
    .await?;

    Ok(())
}

/// Exercise TURN relay servers with a long (30-minute) test. Specify the relay server
/// credentials to use and adjust the length and iterations to desired values. This will
/// send both audio and video but will not save the received data nor perform any
/// analysis on the media.
async fn run_turn_long_tests(test: &mut Test) -> Result<()> {
    // Define the relay server configuration asymmetrically for both clients.
    let relay_urls = vec!["<add one or more relay servers here>".to_string()];
    let relay_urls_with_ips = vec!["<add one or more relay servers here>".to_string()];
    let relay_username = "<add username here>".to_string();
    let relay_password = "<add password here>".to_string();
    let relay_hostname = "<add hostname here>".to_string();

    test.run(
        GroupConfig {
            group_name: "turn_long_tests".to_string(),
            summary_report_columns: SummaryReportColumns::none(),
            ..Default::default()
        },
        vec![TestCaseConfig {
            test_case_name: "force_relay_with_video".to_string(),
            length_seconds: 1800,
            client_configs: vec![
                CallConfig {
                    relay_servers: RelayServerConfig {
                        username: relay_username.clone(),
                        password: relay_password.clone(),
                        urls: relay_urls.clone(),
                        urls_with_ips: relay_urls_with_ips.clone(),
                        hostname: Some(relay_hostname.clone()),
                    },
                    force_relay: true,
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        generate_spectrogram: false,
                        visqol_speech_analysis: false,
                        visqol_audio_analysis: false,
                        pesq_speech_analysis: false,
                        plc_speech_analysis: false,
                        ..Default::default()
                    },
                    video: VideoConfig {
                        input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                CallConfig {
                    relay_servers: RelayServerConfig {
                        username: relay_username,
                        password: relay_password,
                        urls: relay_urls,
                        urls_with_ips: relay_urls_with_ips,
                        hostname: Some(relay_hostname),
                    },
                    force_relay: true,
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        generate_spectrogram: false,
                        visqol_speech_analysis: false,
                        visqol_audio_analysis: false,
                        pesq_speech_analysis: false,
                        plc_speech_analysis: false,
                        ..Default::default()
                    },
                    video: VideoConfig {
                        input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            save_media_files: false,
            iterations: 4,
            ..Default::default()
        }],
        vec![NetworkProfile::None],
    )
    .await?;

    Ok(())
}

/// A test that sends video.
async fn run_video_send_over_bandwidth(test: &mut Test) -> Result<()> {
    test.preprocess_sounds(vec!["normal_phrasing"]).await?;

    test.run(
        GroupConfig {
            group_name: "video_send_over_bandwidth".to_string(),
            chart_dimensions: vec![ChartDimension::MosSpeech],
            ..Default::default()
        },
        vec![TestCaseConfig {
            test_case_name: "video".to_string(),
            client_configs: vec![
                CallConfig {
                    video: VideoConfig {
                        // This will expect a file named "ConferenceMotion_50fps@1280x720.mp4" in the media directory.
                        // The dimensions are important, because the converted video only contains raw frame data.
                        // (This particular video *is* 50fps, but the CLI hardcodes 30fps for both send and receive.)
                        input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                        ..Default::default()
                    },
                    ..CallConfig::default()
                }
                .with_audio_input_name("normal_phrasing"),
                CallConfig::default().with_audio_input_name("normal_phrasing"),
            ],
            ..Default::default()
        }],
        vec![
            NetworkProfile::None,
            NetworkProfile::LimitedBandwidth(2000),
            NetworkProfile::LimitedBandwidth(1500),
            NetworkProfile::LimitedBandwidth(1250),
            NetworkProfile::LimitedBandwidth(1000),
            NetworkProfile::LimitedBandwidth(750),
            NetworkProfile::LimitedBandwidth(500),
            NetworkProfile::LimitedBandwidth(250),
            NetworkProfile::LimitedBandwidth(125),
            NetworkProfile::LimitedBandwidth(100),
            NetworkProfile::LimitedBandwidth(75),
            NetworkProfile::LimitedBandwidth(50),
        ],
    )
    .await?;

    Ok(())
}

/// Bidirectional video test comparing the vp8 and vp9 video codecs.
async fn run_video_compare_vp8_vs_vp9(test: &mut Test, bitrate_values: &Vec<u16>) -> Result<()> {
    let mut test_cases = Vec::new();

    for bitrate in bitrate_values {
        test_cases.push(TestCaseConfig {
            test_case_name: format!("vp8_{}", bitrate),
            client_configs: vec![
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        ..Default::default()
                    },
                    video: VideoConfig {
                        input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                        ..Default::default()
                    },
                    allowed_bitrate_kbps: *bitrate,
                    ..Default::default()
                },
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        ..Default::default()
                    },
                    video: VideoConfig {
                        input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                        ..Default::default()
                    },
                    allowed_bitrate_kbps: *bitrate,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        test_cases.push(TestCaseConfig {
            test_case_name: format!("vp9_{}", bitrate),
            client_configs: vec![
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        ..Default::default()
                    },
                    video: VideoConfig {
                        input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                        enable_vp9: true,
                    },
                    allowed_bitrate_kbps: *bitrate,
                    ..Default::default()
                },
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        ..Default::default()
                    },
                    video: VideoConfig {
                        input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                        enable_vp9: true,
                    },
                    allowed_bitrate_kbps: *bitrate,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
    }

    test.run(
        GroupConfig {
            group_name: "video_compare_vp8_vs_vp9".to_string(),
            chart_dimensions: vec![ChartDimension::MosSpeech],
            ..Default::default()
        },
        test_cases,
        vec![NetworkProfile::Default],
    )
    .await?;

    Ok(())
}

/// Test the scenario with changing bandwidth over one minute intervals:
/// 1 minute unlimited -> 1 minute 50kbps -> 1 minute 25kbps -> 1 minute unlimited
///
/// Uses a 12-second reference audio file so that the resulting 240-second session recording
/// can be chopped evenly and MOS calculated for each 12-second audio segment.
async fn run_changing_bandwidth_audio_test(test: &mut Test) -> Result<()> {
    let test_cases = [20, 60, 120].map(|initial_packet_size_ms| TestCaseConfig {
        test_case_name: format!("ptime_{initial_packet_size_ms}"),
        length_seconds: 240,
        client_configs: vec![
            CallConfig {
                audio: AudioConfig {
                    input_name: "normal_12s".to_string(),
                    initial_packet_size_ms,
                    generate_spectrogram: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            CallConfig {
                audio: AudioConfig {
                    input_name: "normal_12s".to_string(),
                    initial_packet_size_ms,
                    analysis_mode: AudioAnalysisMode::Chopped,
                    generate_spectrogram: false,
                    visqol_speech_analysis: true,
                    visqol_audio_analysis: true,
                    pesq_speech_analysis: true,
                    plc_speech_analysis: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    test.run(
        GroupConfig {
            group_name: "changing_bandwidth_audio_test".to_string(),
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: false,
                ..Default::default()
            },
            ..Default::default()
        },
        test_cases.into(),
        vec![NetworkProfile::Custom(
            "limit_default".to_string(),
            vec![
                NetworkConfigWithOffset {
                    offset: Duration::from_secs(0),
                    network_config: NetworkConfig {
                        ..Default::default()
                    },
                },
                NetworkConfigWithOffset {
                    offset: Duration::from_secs(60),
                    network_config: NetworkConfig {
                        rate: 50,
                        ..Default::default()
                    },
                },
                NetworkConfigWithOffset {
                    offset: Duration::from_secs(120),
                    network_config: NetworkConfig {
                        rate: 25,
                        ..Default::default()
                    },
                },
                NetworkConfigWithOffset {
                    offset: Duration::from_secs(180),
                    network_config: NetworkConfig {
                        ..Default::default()
                    },
                },
            ],
        )],
    )
    .await?;

    Ok(())
}

async fn run_perf_test(test: &mut Test) -> Result<()> {
    test.run(
        GroupConfig {
            group_name: "perf_test".to_string(),
            // Show all the different measurements in the summary columns. Hide video.
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: false,
                show_visqol_mos_audio: false,
                show_pesq_mos: false,
                show_plc_mos: false,
                show_video: false,
                ..Default::default()
            },
            ..Default::default()
        },
        vec![
            TestCaseConfig {
                test_case_name: "audio".to_string(),
                length_seconds: 60,
                save_media_files: false,
                analysis_concurrency: 1,
                client_configs: vec![
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "speaker_b".to_string(),
                            generate_spectrogram: false,
                            visqol_speech_analysis: false,
                            visqol_audio_analysis: false,
                            pesq_speech_analysis: false,
                            plc_speech_analysis: false,
                            analysis_mode: AudioAnalysisMode::None,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            enable_aec: true,
                            generate_spectrogram: false,
                            visqol_speech_analysis: false,
                            visqol_audio_analysis: false,
                            pesq_speech_analysis: false,
                            plc_speech_analysis: false,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                iterations: 1,
                ..Default::default()
            },
            TestCaseConfig {
                test_case_name: "video".to_string(),
                length_seconds: 60,
                save_media_files: false,
                analysis_concurrency: 1,
                client_configs: vec![
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "speaker_b".to_string(),
                            generate_spectrogram: false,
                            visqol_speech_analysis: false,
                            visqol_audio_analysis: false,
                            pesq_speech_analysis: false,
                            plc_speech_analysis: false,
                            analysis_mode: AudioAnalysisMode::None,
                            ..Default::default()
                        },
                        video: VideoConfig {
                            input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    CallConfig {
                        audio: AudioConfig {
                            input_name: "normal_phrasing".to_string(),
                            enable_aec: true,
                            generate_spectrogram: false,
                            visqol_speech_analysis: false,
                            visqol_audio_analysis: false,
                            pesq_speech_analysis: false,
                            plc_speech_analysis: false,
                            ..Default::default()
                        },
                        video: VideoConfig {
                            input_name: Some("ConferenceMotion_50fps@1280x720".to_string()),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ],
                iterations: 1,
                ..Default::default()
            },
        ],
        vec![NetworkProfile::None],
    )
    .await?;

    Ok(())
}

/// Test the different PLC modes: NetEq, Opus, Opus Deep.
async fn run_plc_tests(test: &mut Test) -> Result<()> {
    let test_cases = [None, Some(0), Some(5)].map(|decoder_complexity| TestCaseConfig {
        test_case_name: format!(
            "plc_{}",
            decoder_complexity.map_or("None".to_string(), |c| c.to_string())
        ),
        client_configs: vec![
            CallConfig {
                audio: AudioConfig {
                    input_name: "normal_phrasing".to_string(),
                    generate_spectrogram: false,
                    decoder_complexity,
                    ..Default::default()
                },
                ..Default::default()
            },
            CallConfig {
                audio: AudioConfig {
                    input_name: "normal_phrasing".to_string(),
                    visqol_speech_analysis: true,
                    visqol_audio_analysis: true,
                    pesq_speech_analysis: true,
                    plc_speech_analysis: true,
                    decoder_complexity,
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
        iterations: 1,
        ..Default::default()
    });

    test.run(
        GroupConfig {
            group_name: "plc_tests".to_string(),
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: false,
                ..Default::default()
            },
            ..Default::default()
        },
        test_cases.into(),
        vec![
            NetworkProfile::None,
            NetworkProfile::SimpleLoss(10),
            NetworkProfile::SimpleLoss(20),
            NetworkProfile::SimpleLoss(30),
            NetworkProfile::SimpleLoss(40),
            NetworkProfile::SimpleLoss(50),
        ],
    )
    .await?;

    Ok(())
}

async fn run_dred_tests(test: &mut Test) -> Result<()> {
    let configs = [
        (60, true, 0, Some(0)),    // Before DRED with FEC and Opus PLC
        (60, false, 6, Some(0)),   // DRED 60ms
        (60, false, 12, Some(0)),  // DRED 120ms
        (60, false, 25, Some(0)),  // DRED 250ms
        (60, false, 50, Some(0)),  // DRED 500ms
        (60, false, 100, Some(0)), // DRED 1s
    ];

    let losses = [0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50];

    let test_cases: Vec<_> = configs
        .iter()
        .flat_map(|&(initial_packet_size_ms, enable_fec, dred_duration, decoder_complexity)| {
            losses.iter().map(move |&loss| TestCaseConfig {
                test_case_name: format!(
                    "ptime_{initial_packet_size_ms}-fec_{enable_fec}-dred_duration_{dred_duration}-plc_{}-loss_{loss}",
                    decoder_complexity.map_or("None".to_string(), |c| c.to_string())
                ),
                length_seconds: 30,
                client_configs: vec![ CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        generate_spectrogram: false,
                        visqol_speech_analysis: false,
                        visqol_audio_analysis: false,
                        pesq_speech_analysis: false,
                        plc_speech_analysis: false,
                        initial_packet_size_ms,
                        enable_fec,
                        dred_duration,
                        decoder_complexity,
                        // Force DRED to be encoded with the expected loss rate. The
                        // simulation will assume steady loss conditions.
                        min_packet_loss_percent: if dred_duration > 0 { loss } else { 0 },
                        ..Default::default()
                    },
                    profile: DeterministicLoss(loss),
                    ..Default::default()
                },
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_phrasing".to_string(),
                        visqol_speech_analysis: true,
                        visqol_audio_analysis: true,
                        pesq_speech_analysis: true,
                        plc_speech_analysis: true,
                        initial_packet_size_ms,
                        enable_fec,
                        dred_duration,
                        decoder_complexity,
                        // Force DRED to be encoded with the expected loss rate. The
                        // simulation will assume steady loss conditions.
                        min_packet_loss_percent: if dred_duration > 0 { loss } else { 0 },
                        ..Default::default()
                    },
                    profile: DeterministicLoss(loss),
                    ..Default::default()
                }
                ],
                iterations: 3,
                ..Default::default()
            })
        })
        .collect();

    test.run(
        GroupConfig {
            group_name: "dred_tests".to_string(),
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: false,
                ..Default::default()
            },
            ..Default::default()
        },
        test_cases,
        vec![NetworkProfile::None],
    )
    .await?;

    Ok(())
}

/// Smart flushing with and without DRED, over stall-and-burst queueing, bursty loss and a deep
/// jitter buffer. STUN bypasses the emulation so that ICE stays writable during the stalls.
async fn run_dred_flush_tests(test: &mut Test) -> Result<()> {
    let configs = [
        (0, "off", None),
        (
            0,
            "smart_default",
            Some("WebRTC-Audio-NetEqSmartFlushing/enabled:true"),
        ),
        (100, "off", None),
        (
            100,
            "smart",
            Some(
                "WebRTC-Audio-NetEqSmartFlushing/enabled:true,target_level_threshold_ms:600,target_level_multiplier:2",
            ),
        ),
        (
            100,
            "smart_default",
            Some("WebRTC-Audio-NetEqSmartFlushing/enabled:true"),
        ),
    ];

    let test_cases = configs.map(|(dred_duration, smart_flushing, field_trial)| {
        let field_trials: Vec<String> = field_trial.iter().map(|t| t.to_string()).collect();
        TestCaseConfig {
            test_case_name: format!(
                "dred_duration_{dred_duration}-smart_flushing_{smart_flushing}"
            ),
            length_seconds: 120,
            client_configs: vec![
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_12s".to_string(),
                        generate_spectrogram: false,
                        visqol_speech_analysis: false,
                        dred_duration,
                        // Force DRED to be encoded with the expected loss rate.
                        min_packet_loss_percent: if dred_duration > 0 { 20 } else { 0 },
                        ..Default::default()
                    },
                    ..Default::default()
                }
                .with_field_trials(&field_trials),
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_12s".to_string(),
                        analysis_mode: AudioAnalysisMode::Chopped,
                        generate_spectrogram: false,
                        visqol_speech_analysis: true,
                        visqol_audio_analysis: true,
                        pesq_speech_analysis: true,
                        plc_speech_analysis: true,
                        dred_duration,
                        // Force DRED to be encoded with the expected loss rate.
                        min_packet_loss_percent: if dred_duration > 0 { 20 } else { 0 },
                        ..Default::default()
                    },
                    ..Default::default()
                }
                .with_field_trials(&field_trials),
            ],
            iterations: 3,
            stun_bypass: true,
            ..Default::default()
        }
    });

    let loss = Some(Loss::GeModel(GeLossModel::SimpleGilbert { p: 3, r: 12 }));

    test.run(
        GroupConfig {
            group_name: "dred_flush_tests".to_string(),
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: false,
                ..Default::default()
            },
            ..Default::default()
        },
        test_cases.into(),
        vec![
            // 5 s stalls at 1 kbps (0 is unlimited) released at 240 kbps, one per 12 s segment from
            // segment 1 to 8. Segment 0 is clean (the control) and the call ends with a recovery.
            NetworkProfile::Custom(
                "stall_5s_x8".to_string(),
                vec![
                    NetworkConfigWithOffset::loss_and_rate(0, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(14, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(19, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(26, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(31, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(38, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(43, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(50, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(55, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(62, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(67, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(74, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(79, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(86, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(91, None, 240),
                    NetworkConfigWithOffset::loss_and_rate(98, None, 1),
                    NetworkConfigWithOffset::loss_and_rate(103, None, 240),
                ],
            ),
            // 20% Gilbert loss with 8 s (full flush) and 2 s (mid level) stalls at 4 kbps, each in
            // its own 12 s segment. 4 kbps keeps HTB's token debt, and so the release lag, small.
            NetworkProfile::Custom(
                "loss_20_stall_8s_2s_x2".to_string(),
                vec![
                    NetworkConfigWithOffset::loss_and_rate(0, loss, 240),
                    NetworkConfigWithOffset::loss_and_rate(26, loss, 4),
                    NetworkConfigWithOffset::loss_and_rate(34, loss, 240),
                    NetworkConfigWithOffset::loss_and_rate(50, loss, 4),
                    NetworkConfigWithOffset::loss_and_rate(52, loss, 240),
                    NetworkConfigWithOffset::loss_and_rate(74, loss, 4),
                    NetworkConfigWithOffset::loss_and_rate(82, loss, 240),
                    NetworkConfigWithOffset::loss_and_rate(98, loss, 4),
                    NetworkConfigWithOffset::loss_and_rate(100, loss, 240),
                ],
            ),
            // Jitter holds a deep buffer, and 25% loss in ~1 s bursts gives large DRED recoveries.
            NetworkProfile::Custom(
                "jitter_150_loss_25_burst_1s".to_string(),
                vec![NetworkConfigWithOffset {
                    offset: Duration::from_secs(0),
                    network_config: NetworkConfig {
                        delay: 200,
                        delay_variability: 150,
                        delay_variation_strategy: Some(DelayVariationStrategy::Distribution(
                            Distribution::Normal,
                        )),
                        loss: Some(Loss::GeModel(GeLossModel::SimpleGilbert { p: 2, r: 6 })),
                        ..Default::default()
                    },
                }],
            ),
        ],
    )
    .await?;

    Ok(())
}

async fn run_tcc_tests(test: &mut Test) -> Result<()> {
    let tcc_values = [false, true];
    let adaptation_values = 0..=1;
    let min_bitrate_values = [32000, 24000, 16000];

    let test_cases = iproduct!(tcc_values, adaptation_values, min_bitrate_values)
        .map(|(enable_tcc, adaptation, min_bitrate_bps)| TestCaseConfig {
            test_case_name: format!(
                "tcc-{enable_tcc}_adaptation-{adaptation}_min-{min_bitrate_bps}"
            ),
            length_seconds: 60,
            client_configs: vec![
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_12s".to_string(),
                        min_bitrate_bps,
                        adaptation,
                        enable_tcc,
                        generate_spectrogram: false,
                        visqol_speech_analysis: false,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                CallConfig {
                    audio: AudioConfig {
                        input_name: "normal_12s".to_string(),
                        min_bitrate_bps,
                        adaptation,
                        enable_tcc,
                        analysis_mode: AudioAnalysisMode::Chopped,
                        generate_spectrogram: false,
                        visqol_speech_analysis: true,
                        visqol_audio_analysis: true,
                        pesq_speech_analysis: true,
                        plc_speech_analysis: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
        .collect::<Vec<_>>();

    test.run(
        GroupConfig {
            group_name: "tcc_tests".to_string(),
            summary_report_columns: SummaryReportColumns {
                show_visqol_mos_speech: true,
                show_visqol_mos_audio: true,
                show_pesq_mos: true,
                show_plc_mos: true,
                show_video: false,
                ..Default::default()
            },
            ..Default::default()
        },
        test_cases,
        vec![NetworkProfile::None, NetworkProfile::LimitedBandwidth(30)],
    )
    .await?;

    Ok(())
}

pub fn format_log_line(
    buf: &mut env_logger::fmt::Formatter,
    record: &log::Record<'_>,
) -> std::io::Result<()> {
    write!(buf, "[{} {:<5} ", buf.timestamp_millis(), record.level())?;
    match record.file() {
        // Log the filename for crates in this workspace.
        Some(file) if !file.starts_with(['\\', '/']) => {
            write!(buf, "{}:{}", file, record.line().unwrap_or(0))?;
        }
        // Log the target (usually the module path) for anything else.
        _ => {
            write!(buf, "{}", record.target())?;
        }
    }
    writeln!(buf, "] {}", record.args())?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    unsafe {
        std::env::set_var("RUST_BACKTRACE", "full");
        std::env::set_var("RUST_LIB_BACKTRACE", "0");
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(format_log_line)
        .target(env_logger::Target::Stdout)
        .init();
    let args = Args::parse();

    info!("Starting the call simulator...");

    let mut root_path = env::current_dir()?;
    root_path.push(args.root);
    info!("  Using root path: {}", root_path.display());
    env::set_current_dir(&root_path)?;

    let client_profile_factory = DynamicClientProfileFactory::new_with_key(group_auth_key_gen());
    let mut test_sets = args.test_sets;
    if test_sets.is_empty() {
        // For quick testing, change this to the name of your test case.
        test_sets.push("baseline".to_string());
    }

    let direct_call_config = CallTypeConfig::Direct;
    let group_call_config = CallTypeConfig::Group {
        sfu_connection_params: SFU_URL.map_or(test::SfuConnectionParams::Local, |url| {
            test::SfuConnectionParams::Remote {
                sfu_url: url.to_string(),
            }
        }),
        group_name: args.group_name,
    };
    let contains_group_call = test_sets.iter().any(|name| name.starts_with("group_"));
    let build_local_sfu = contains_group_call && group_call_config.requires_local_sfu();
    // Normalize `a` or `client_a`
    let analyze_clients: Option<HashSet<String>> = args.analyze_clients.map(|clients| {
        clients
            .iter()
            .map(|client| {
                let client = client.trim();
                if client.starts_with("client_") {
                    client.to_string()
                } else {
                    format!("client_{}", client)
                }
            })
            .collect()
    });

    if args.build {
        build_images(!args.skip_visqol_mos_build, build_local_sfu).await?;
    }

    if args.clean {
        clean_up_all_containers().await?;
        clean_network().await?;
    }

    for test_set_name in test_sets {
        let (call_type_config, test_set_name) =
            if let Some(name) = test_set_name.strip_prefix("group_") {
                (group_call_config.clone(), name.to_owned())
            } else {
                (direct_call_config.clone(), test_set_name)
            };
        info!(
            "Running test set {} as call type {:?}",
            test_set_name, call_type_config,
        );
        let test = &mut Test::new(
            &root_path,
            &args.output_dir,
            &args.media_dir,
            &args.data_dir,
            &test_set_name,
            client_profile_factory.clone(),
            call_type_config,
            args.profile,
            analyze_clients.clone(),
        )?;
        match test_set_name.as_str() {
            "minimal_example" => run_minimal_example(test).await?,
            "baseline" => run_baseline(test, false).await?,
            "baseline_with_video" => run_baseline(test, true).await?,
            "bursty_loss_test" => run_bursty_loss_test(test, false).await?,
            "bursty_loss_test_with_video" => run_bursty_loss_test(test, true).await?,
            "deterministic_loss_test" => run_deterministic_loss_test(test, false).await?,
            "deterministic_loss_test_with_video" => run_deterministic_loss_test(test, true).await?,
            "multi_deterministic_loss_test" => run_multi_deterministic_loss_test(test, 3).await?,
            "relay_tests" => run_relay_tests(test).await?,
            "turn_long_tests" => run_turn_long_tests(test).await?,
            "video_send_over_bandwidth" => run_video_send_over_bandwidth(test).await?,
            "video_compare_vp8_vs_vp9" => {
                run_video_compare_vp8_vs_vp9(test, &vec![1000, 2000]).await?
            }
            "changing_bandwidth_audio_test" => run_changing_bandwidth_audio_test(test).await?,
            "profiling_suite" => run_perf_test(test).await?,
            "plc_tests" => run_plc_tests(test).await?,
            "dred_tests" => run_dred_tests(test).await?,
            "dred_flush_tests" => run_dred_flush_tests(test).await?,
            "tcc_tests" => run_tcc_tests(test).await?,
            _ => panic!("unknown test set \"{test_set_name}\""),
        }
        test.report().await?;
    }

    Ok(())
}
