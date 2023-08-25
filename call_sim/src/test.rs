//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// Modules for the testing service, from protobufs compiled by tonic.
pub mod calling {
    #![allow(clippy::derive_partial_eq_without_eq, clippy::enum_variant_names)]
    tonic::include_proto!("calling");
}

use anyhow::Result;
use calling::{
    command_message::Command, test_management_client::TestManagementClient, CommandMessage, Empty,
};
use chrono::{DateTime, Local};
use relative_path::RelativePath;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use tonic::transport::Channel;
use tower::timeout::Timeout;

use crate::audio::{chop_audio_and_analyze, get_audio_and_analyze};
use crate::common::{
    AudioAnalysisMode, GroupConfig, NetworkConfigWithOffset, NetworkProfile, TestCaseConfig,
};
use crate::docker::{
    analyze_audio, analyze_video, clean_network, clean_up, convert_mp4_to_yuv, convert_raw_to_wav,
    convert_wav_to_16khz_mono, convert_yuv_to_mp4, create_network, emulate_network_change,
    emulate_network_start, generate_spectrogram, get_signaling_server_logs, get_turn_server_logs,
    start_cli, start_client, start_signaling_server, start_tcp_dump, start_turn_server,
    DockerStats,
};
use crate::report::{AnalysisReport, AnalysisReportMos, Report};

pub struct Client<'a> {
    pub name: &'a str,
    pub sound: &'a Sound,
    pub video: Option<&'a Video>,

    pub output_raw: String,
    pub output_wav: String,
    pub output_wav_speech: String,
    pub output_yuv: Option<String>,
    pub output_mp4: Option<String>,
}

/// A property bag used to attach results and artifacts to tests. Normally, artifacts are
/// saved to the file system and processed when reporting, but it is more efficient to
/// record and pass some things along as we create them.
#[derive(Default)]
pub struct TestResults {
    /// MOS analysis using the speech model (wideband).
    pub mos_s: AnalysisReportMos,
    /// MOS analysis using the audio model (fullband).
    pub mos_a: AnalysisReportMos,
}

pub struct TestCase<'a> {
    pub report_name: String,
    pub test_path: String,

    pub test_case_name: String,
    pub network_profile: NetworkProfile,

    pub client_a: &'a Client<'a>,
    pub client_b: &'a Client<'a>,
}

pub struct Sound {
    pub name: String,
    /// Optionally store the mos of the file vs. itself as a theoretical maximum.
    pub reference_mos: Option<f32>,
    pub reference_mos_16khz_mono: Option<f32>,
}

impl Sound {
    fn raw(&self) -> String {
        format!("{}.raw", self.name)
    }

    fn wav(&self, speech: bool) -> String {
        if speech {
            format!("{}.16kHz.mono.wav", self.name)
        } else {
            format!("{}.wav", self.name)
        }
    }

    fn analysis_extension(&self) -> &str {
        "analysis.log"
    }

    fn spectrogram_extension(&self) -> &str {
        "png"
    }
}

pub struct Video {
    pub name: String,
}

impl Video {
    fn raw(&self) -> String {
        format!("{}.yuv", self.name)
    }

    fn mp4(&self) -> String {
        format!("{}.mp4", self.name)
    }
}

pub struct GroupRun {
    pub group_config: GroupConfig,
    pub reports: Vec<Result<Report>>,
}

#[allow(dead_code)]
pub struct Test {
    time_started: DateTime<Local>,

    set_path: String,
    set_name: String,

    media_path: String,

    group_runs: Vec<GroupRun>,

    // Keep track of all reference files used by copying them into the test
    // directory, converting them if necessary (and avoiding duplicates if
    // multiple runs use the same media). This way the test results have full
    // information even if we change the reference media in the future.
    sounds: HashMap<String, Sound>,
    videos: HashMap<String, Video>,
}

impl Test {
    pub fn new(
        root_path: &PathBuf,
        output_dir: &str,
        media_dir: &str,
        set_name: &str,
    ) -> Result<Self> {
        let time_started = chrono::Local::now();

        let output_path = RelativePath::new(output_dir).to_logical_path(root_path);
        let media_path = RelativePath::new(media_dir).to_logical_path(root_path);

        // All output must go to a unique directory, generate one using the current datetime.
        let set_path = RelativePath::new(&format!(
            "{}-{}",
            set_name,
            time_started.format("%Y-%m-%d-%H-%M-%S")
        ))
        .to_logical_path(output_path);

        // Make sure the directory to store the set of all tests is created.
        fs::create_dir_all(set_path.clone())?;

        let set_path = set_path.display().to_string();

        println!("\nRunning test set: {}", set_name);
        println!("  Using path: {}", set_path);

        Ok(Self {
            time_started,
            set_path,
            set_name: set_name.to_string(),
            media_path: media_path.display().to_string(),

            group_runs: vec![],
            sounds: HashMap::new(),
            videos: HashMap::new(),
        })
    }

    async fn start_test_manager_client(&self) -> Result<TestManagementClient<Timeout<Channel>>> {
        let channel = Channel::from_static("http://localhost:9090")
            .connect_timeout(Duration::from_millis(500))
            .connect()
            .await?;

        // Make sure all requests have a reasonable timeout.
        Ok(TestManagementClient::new(Timeout::new(
            channel,
            Duration::from_millis(1000),
        )))
    }

    /// The fundamental test block that orchestrates various docker functions in order
    /// to achieve test execution of the RingRTC clients.
    async fn run_test(
        &self,
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
        network_configs: &[NetworkConfigWithOffset],
    ) -> Result<()> {
        create_network().await?;
        start_signaling_server().await?;

        if !test_case_config.client_a_config.relay_servers.is_empty()
            || !test_case_config.client_a_config.relay_servers.is_empty()
        {
            // We'll assume any relay server configuration should start the test turn server.
            start_turn_server().await?;
        }

        if test_case_config.tcp_dump {
            start_tcp_dump(&test_case.test_path).await?;
        }

        // Sleep here to allow the server(s) to get running.
        tokio::time::sleep(Duration::from_secs(1)).await;

        println!("Connecting to test manager...");
        let mut test_manager = self.start_test_manager_client().await?;
        println!("Starting clients...");

        // Sign-up for notifications from the signaling server.
        let request = tonic::Request::new(Empty {});
        let response = test_manager.notification(request).await;

        if let Ok(response) = response {
            let mut stream = response.into_inner();

            start_client(
                test_case.client_a.name,
                &test_case.test_path,
                &self.set_path,
            )
            .await?;
            start_client(
                test_case.client_b.name,
                &test_case.test_path,
                &self.set_path,
            )
            .await?;

            println!();

            start_cli(
                test_case.client_a.name,
                &test_case.client_a.sound.raw(),
                &test_case.client_a.output_raw,
                test_case.client_a.video.map(|v| v.raw()).as_deref(),
                test_case.client_a.output_yuv.as_deref(),
                &test_case_config.client_a_config,
                &test_case_config.client_b_config,
            )
            .await?;

            start_cli(
                test_case.client_b.name,
                &test_case.client_b.sound.raw(),
                &test_case.client_b.output_raw,
                test_case.client_b.video.map(|v| v.raw()).as_deref(),
                test_case.client_b.output_yuv.as_deref(),
                &test_case_config.client_b_config,
                &test_case_config.client_a_config,
            )
            .await?;

            println!("Waiting for clients...");

            let mut done = false;
            loop {
                match stream.message().await {
                    Ok(Some(event)) => {
                        // We wait for both clients to indicate that they are ready and already
                        // registered with the relay server.
                        if !done && event.ready_count == 2 {
                            println!("\nRunning test...");

                            let mut network_configs = network_configs.iter();
                            let mut timed_config_next = network_configs.next();
                            let mut emulation_started = false;

                            if let Some(timed_network_config) = timed_config_next {
                                if timed_network_config.offset == Duration::from_secs(0) {
                                    println!("  Setting up network emulation.");
                                    emulate_network_start(
                                        test_case.client_a.name,
                                        &timed_network_config.network_config,
                                    )
                                    .await?;
                                    emulate_network_start(
                                        test_case.client_b.name,
                                        &timed_network_config.network_config,
                                    )
                                    .await?;
                                    emulation_started = true;
                                    timed_config_next = network_configs.next();
                                }
                            }

                            // Start monitoring docker stats. They will end when the associated container stops.
                            let docker_stats = DockerStats::new().await?;
                            docker_stats.start(test_case.client_a.name, &test_case.test_path)?;
                            docker_stats.start(test_case.client_b.name, &test_case.test_path)?;

                            // Tell client_b to start as a callee.
                            let request = tonic::Request::new(CommandMessage {
                                client: test_case.client_b.name.to_string(),
                                command: Command::StartAsCallee.into(),
                            });

                            test_manager.send_command(request).await?;

                            // Tell client_a to start as a caller.
                            let request = tonic::Request::new(CommandMessage {
                                client: test_case.client_a.name.to_string(),
                                command: Command::StartAsCaller.into(),
                            });

                            test_manager.send_command(request).await?;

                            println!("\nWaiting for the test to complete...");

                            // Yield for a moment to let connections be made.
                            thread::sleep(Duration::from_millis(100));

                            let start_time = Instant::now();

                            for i in (1..=(test_case_config.length_seconds)).rev() {
                                eprint!("\r{} seconds remaining...", i);
                                tokio::time::sleep(Duration::from_secs(1)).await;

                                if let Some(timed_network_config) = timed_config_next {
                                    if start_time.elapsed() >= timed_network_config.offset {
                                        // Changing the network emulation takes time, so do it concurrently.
                                        let network_config = timed_network_config.network_config;
                                        let client_name_a = test_case.client_a.name.to_string();
                                        let client_name_b = test_case.client_b.name.to_string();

                                        // For now we will be ignoring errors when changing the emulation settings.
                                        tokio::spawn(async move {
                                            eprint!(
                                                "\n  Applying new emulated network settings..."
                                            );

                                            let join_handle_a: tokio::task::JoinHandle<
                                                Result<(), anyhow::Error>,
                                            > = tokio::spawn(async move {
                                                if emulation_started {
                                                    emulate_network_change(
                                                        &client_name_a,
                                                        &network_config,
                                                    )
                                                    .await?;
                                                } else {
                                                    emulate_network_start(
                                                        &client_name_a,
                                                        &network_config,
                                                    )
                                                    .await?;
                                                }
                                                Ok(())
                                            });
                                            let join_handle_b: tokio::task::JoinHandle<
                                                Result<(), anyhow::Error>,
                                            > = tokio::spawn(async move {
                                                if emulation_started {
                                                    emulate_network_change(
                                                        &client_name_b,
                                                        &network_config,
                                                    )
                                                    .await?;
                                                } else {
                                                    emulate_network_start(
                                                        &client_name_b,
                                                        &network_config,
                                                    )
                                                    .await?;
                                                }
                                                Ok(())
                                            });

                                            // NOTE: We assume this block completes fairly quickly! To avoid issues,
                                            // emulation shouldn't change more than once every 2 seconds!

                                            let _ = tokio::join!(join_handle_a, join_handle_b);
                                            eprintln!(" Done.");
                                        });

                                        emulation_started = true;

                                        timed_config_next = network_configs.next();
                                    }
                                }
                            }

                            // Tell client_a to stop.
                            let request = tonic::Request::new(CommandMessage {
                                client: test_case.client_a.name.to_string(),
                                command: Command::Stop.into(),
                            });

                            test_manager.send_command(request).await?;

                            // Tell client_b to stop.
                            let request = tonic::Request::new(CommandMessage {
                                client: test_case.client_b.name.to_string(),
                                command: Command::Stop.into(),
                            });

                            test_manager.send_command(request).await?;

                            done = true;

                            println!("\r  Test complete.");
                            println!("\nWaiting for the clients to terminate...");
                        } else if done && event.ready_count == 0 {
                            println!("  Done.");
                            break;
                        }
                    }
                    Ok(None) => {
                        println!("Received Message: None");
                        break;
                    }
                    Err(err) => {
                        println!("Error: {}", err);
                        break;
                    }
                }
            }
        } else {
            println!("Could not send notification() message: {:?}", response);
        }

        Ok(())
    }

    /// Generates report artifacts by performing analysis on all media outputs. Performs
    /// the necessary conversions to do so.
    async fn generate_artifacts(
        &self,
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
    ) -> Result<TestResults> {
        let mut test_results = TestResults::default();

        // Perform conversions of audio data.
        convert_raw_to_wav(
            &test_case.test_path,
            &test_case.client_a.output_raw,
            &test_case.client_a.output_wav,
            Some(test_case_config.length_seconds),
        )
        .await?;

        if test_case_config.client_a_config.audio.speech_analysis {
            convert_wav_to_16khz_mono(
                &test_case.test_path,
                &test_case.client_a.output_wav,
                &test_case.client_a.output_wav_speech,
            )
            .await?;
        }

        convert_raw_to_wav(
            &test_case.test_path,
            &test_case.client_b.output_raw,
            &test_case.client_b.output_wav,
            Some(test_case_config.length_seconds),
        )
        .await?;

        if test_case_config.client_b_config.audio.speech_analysis {
            convert_wav_to_16khz_mono(
                &test_case.test_path,
                &test_case.client_b.output_wav,
                &test_case.client_b.output_wav_speech,
            )
            .await?;
        }

        match test_case_config.client_b_config.audio.analysis_mode {
            AudioAnalysisMode::None => {
                // Do nothing, no analysis is requested.
            }
            AudioAnalysisMode::Normal => {
                if test_case_config.client_b_config.audio.speech_analysis {
                    get_audio_and_analyze(
                        &test_case.test_path,
                        &test_case.client_b.output_wav_speech,
                        &self.set_path,
                        &test_case.client_a.sound.wav(true),
                        test_case.client_b.sound.analysis_extension(),
                        true,
                        &mut test_results,
                    )
                    .await?;
                }
                if test_case_config.client_b_config.audio.audio_analysis {
                    get_audio_and_analyze(
                        &test_case.test_path,
                        &test_case.client_b.output_wav,
                        &self.set_path,
                        &test_case.client_a.sound.wav(false),
                        test_case.client_b.sound.analysis_extension(),
                        false,
                        &mut test_results,
                    )
                    .await?;
                }
            }
            AudioAnalysisMode::Chopped => {
                if test_case_config.client_b_config.audio.speech_analysis {
                    chop_audio_and_analyze(
                        &test_case.test_path,
                        &test_case.client_b.output_wav_speech,
                        &self.set_path,
                        &test_case.client_a.sound.wav(true),
                        test_case.client_b.sound.analysis_extension(),
                        test_case.client_b.name,
                        true,
                        &mut test_results,
                    )
                    .await?;
                }
                if test_case_config.client_b_config.audio.audio_analysis {
                    chop_audio_and_analyze(
                        &test_case.test_path,
                        &test_case.client_b.output_wav,
                        &self.set_path,
                        &test_case.client_a.sound.wav(false),
                        test_case.client_b.sound.analysis_extension(),
                        test_case.client_b.name,
                        false,
                        &mut test_results,
                    )
                    .await?;
                }
            }
        }

        if test_case_config.client_b_config.audio.generate_spectrogram {
            generate_spectrogram(
                &test_case.test_path,
                &test_case.client_b.output_wav,
                test_case.client_b.sound.spectrogram_extension(),
            )
            .await?;
        }

        if let (Some(client_a_video), Some(dimensions)) = (
            test_case.client_a.video,
            test_case_config.client_a_config.video.dimensions(),
        ) {
            convert_yuv_to_mp4(
                &test_case.test_path,
                test_case
                    .client_b
                    .output_yuv
                    .as_deref()
                    .expect("missing output"),
                test_case
                    .client_b
                    .output_mp4
                    .as_deref()
                    .expect("missing output"),
                dimensions,
            )
            .await?;

            analyze_video(
                &test_case.test_path,
                test_case
                    .client_b
                    .output_yuv
                    .as_deref()
                    .expect("missing output"),
                &self.set_path,
                &client_a_video.raw(),
                dimensions,
            )
            .await?;
        }

        if let Some(dimensions) = test_case_config.client_b_config.video.dimensions() {
            convert_yuv_to_mp4(
                &test_case.test_path,
                test_case
                    .client_a
                    .output_yuv
                    .as_deref()
                    .expect("missing output"),
                test_case
                    .client_a
                    .output_mp4
                    .as_deref()
                    .expect("missing output"),
                dimensions,
            )
            .await?;
        }

        Ok(test_results)
    }

    /// Generates reports by parsing/checking artifacts for the test, and returns summary
    /// information about it.
    async fn generate_test_report(
        &self,
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
        network_configs: &Vec<NetworkConfigWithOffset>,
        test_results: TestResults,
    ) -> Result<Report> {
        let report = Report::build_b(test_case, test_case_config, test_results).await?;

        report
            .create_test_case_report(
                &self.set_name,
                &format!(
                    "../../{}.{}",
                    test_case.client_a.sound.wav(false),
                    test_case.client_a.sound.spectrogram_extension()
                ),
                network_configs,
                test_case_config,
            )
            .await?;

        Ok(report)
    }

    /// Process a reference sound by copying to the output directory and converting it to wav.
    /// Optionally, analyze it and store the reference mos value. This function will always
    /// process sounds in full-band (48kHz/two-channel) and wide-band (16kHz/mono).
    async fn process_sound(&mut self, name: &str, analyze: bool) -> Result<()> {
        // Only process each sound once. So if we already have it, don't do anything.
        // Note: This means that mos analysis can only happen when sounds are pre-processed.
        if !self.sounds.contains_key(name) {
            let mut sound = Sound {
                name: name.to_string(),
                reference_mos: None,
                reference_mos_16khz_mono: None,
            };

            let raw_name = sound.raw();
            let wav_name = sound.wav(false);
            let wav_name_speech = sound.wav(true);

            // Copy the reference file to our test directory.
            fs::copy(
                format!("{}/{}", self.media_path, raw_name),
                format!("{}/{}", self.set_path, raw_name),
            )?;

            // Make sure there is a wav version of the file available.
            convert_raw_to_wav(&self.set_path, &raw_name, &wav_name, None).await?;
            convert_wav_to_16khz_mono(&self.set_path, &wav_name, &wav_name_speech).await?;

            // And a reference spectrogram. Since the speech wav files have a limited frequency
            // range, we will only generate spectrograms for the full-band audio files.
            generate_spectrogram(&self.set_path, &wav_name, sound.spectrogram_extension()).await?;

            if analyze {
                analyze_audio(
                    &self.set_path,
                    &wav_name,
                    &self.set_path,
                    &wav_name,
                    sound.analysis_extension(),
                    false,
                )
                .await?;

                let mos = AnalysisReport::parse_audio_analysis(&format!(
                    "{}/{}.{}",
                    self.set_path,
                    wav_name,
                    sound.analysis_extension()
                ))
                .await?;

                sound.reference_mos = mos;

                analyze_audio(
                    &self.set_path,
                    &wav_name_speech,
                    &self.set_path,
                    &wav_name_speech,
                    sound.analysis_extension(),
                    true,
                )
                .await?;

                let mos = AnalysisReport::parse_audio_analysis(&format!(
                    "{}/{}.{}",
                    self.set_path,
                    wav_name_speech,
                    sound.analysis_extension()
                ))
                .await?;

                sound.reference_mos_16khz_mono = mos;
            }

            self.sounds.insert(name.to_string(), sound);
        }

        Ok(())
    }

    /// An optional function to generate a spectrogram and analyze each sound with itself
    /// in order to get reference values for it. Will copy the sound and create an associated
    /// wav file if not done so already.
    pub async fn preprocess_sounds(&mut self, sounds: Vec<&str>) -> Result<()> {
        for sound in sounds {
            // Process the reference sound and analyze it.
            self.process_sound(sound, true).await?;
        }

        Ok(())
    }

    /// Process a reference video by copying to the output directory and converting it to YUV frames.
    async fn process_video(&mut self, name: &str) -> Result<()> {
        // Only process each video once. So if we already have it, don't do anything.
        if !self.videos.contains_key(name) {
            let video = Video {
                name: name.to_string(),
            };

            let raw_name = video.raw();
            let mp4_name = video.mp4();

            // Copy the *MP4* reference file to our test directory.
            // This is different from sounds, but raw video is much bigger.
            fs::copy(
                format!("{}/{}", self.media_path, mp4_name),
                format!("{}/{}", self.set_path, mp4_name),
            )?;

            // Make sure there is a raw version of the file available.
            convert_mp4_to_yuv(&self.set_path, &mp4_name, &raw_name).await?;

            self.videos.insert(name.to_string(), video);
        }

        Ok(())
    }

    /// An optional function to convert video to YUV frames.
    #[allow(dead_code)]
    pub async fn preprocess_video(&mut self, videos: &[&str]) -> Result<()> {
        for video in videos {
            self.process_video(video).await?;
        }

        Ok(())
    }

    async fn run_test_case_and_get_report(
        &self,
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
        network_configs: &Vec<NetworkConfigWithOffset>,
    ) -> Result<Report> {
        match self
            .run_test(test_case, test_case_config, network_configs)
            .await
        {
            Ok(_) => {
                // For debugging, dump the signaling_server logs.
                get_signaling_server_logs(&test_case.test_path).await?;

                if !test_case_config.client_a_config.relay_servers.is_empty()
                    || !test_case_config.client_a_config.relay_servers.is_empty()
                {
                    // Also dump the turn server logs if it was used.
                    get_turn_server_logs(&test_case.test_path).await?;
                }

                // We are done with the containers.
                clean_up(vec![
                    test_case.client_a.name,
                    test_case.client_b.name,
                    "signaling_server",
                    "turn",
                    "tcpdump",
                ])
                .await?;
                clean_network().await?;

                match self.generate_artifacts(test_case, test_case_config).await {
                    Ok(test_results) => {
                        match self
                            .generate_test_report(
                                test_case,
                                test_case_config,
                                network_configs,
                                test_results,
                            )
                            .await
                        {
                            Ok(report) => Ok(report),
                            Err(err) => {
                                println!("Error generating test report: {}", err);
                                Err(err)
                            }
                        }
                    }
                    Err(err) => {
                        println!("Error generating artifacts: {}", err);
                        Err(err)
                    }
                }
            }
            Err(err) => {
                println!("Error running test: {}", err);
                clean_up(vec![
                    test_case.client_a.name,
                    test_case.client_b.name,
                    "signaling_server",
                    "turn",
                    "tcpdump",
                ])
                .await?;
                clean_network().await?;

                Err(err)
            }
        }
    }

    /// Runs the provided test permutations as individual test cases.
    pub async fn run(
        &mut self,
        group_config: GroupConfig,
        tests: Vec<TestCaseConfig>,
        profiles: Vec<NetworkProfile>,
    ) -> Result<()> {
        let mut reports: Vec<Result<Report>> = vec![];

        for test in tests {
            let a_to_b_sound = test.client_a_config.audio.input_name.as_str();
            let b_to_a_sound = test.client_b_config.audio.input_name.as_str();

            // Make sure the sounds are copied and converted, but they don't need to be
            // analyzed if not already.
            self.process_sound(a_to_b_sound, false).await?;
            self.process_sound(b_to_a_sound, false).await?;

            let a_to_b_video = test.client_a_config.video.input_name.as_deref();
            let b_to_a_video = test.client_b_config.video.input_name.as_deref();

            if let Some(a_to_b_video) = a_to_b_video {
                self.process_video(a_to_b_video).await?;
            }
            if let Some(b_to_a_video) = b_to_a_video {
                self.process_video(b_to_a_video).await?;
            }

            for network_profile in &profiles {
                for i in 1..=test.iterations {
                    let report_name = format!(
                        "{}-{}-{}",
                        test.test_case_name,
                        a_to_b_sound,
                        network_profile.get_name()
                    );

                    let test_case_path = if test.iterations > 1 {
                        println!("\nRunning test case: {}, iteration: {}", report_name, i);
                        format!(
                            "{}/{}/{}_{}",
                            self.set_path, group_config.group_name, report_name, i
                        )
                    } else {
                        println!("\nRunning test case: {}", report_name);
                        format!(
                            "{}/{}/{}",
                            self.set_path, group_config.group_name, report_name
                        )
                    };
                    fs::create_dir_all(test_case_path.clone())?;

                    let test_case = TestCase {
                        report_name,
                        test_path: test_case_path,
                        test_case_name: test.test_case_name.to_string(),
                        network_profile: network_profile.clone(),
                        client_a: &Client {
                            name: "client_a",
                            // The sound should have been processed.
                            sound: &self.sounds[a_to_b_sound],
                            video: a_to_b_video.map(|v| &self.videos[v]),
                            output_raw: "client_a_output.raw".to_string(),
                            output_wav: "client_a_output.wav".to_string(),
                            output_wav_speech: "client_a_output.16kHz.mono.wav".to_string(),
                            // Note that we check if *B* is sending video to decide if *A* should output video.
                            output_yuv: b_to_a_video.map(|_| "client_a_output.yuv".to_string()),
                            output_mp4: b_to_a_video.map(|_| "client_a_output.mp4".to_string()),
                        },
                        client_b: &Client {
                            name: "client_b",
                            sound: &self.sounds[b_to_a_sound],
                            video: b_to_a_video.map(|v| &self.videos[v]),
                            output_raw: "client_b_output.raw".to_string(),
                            output_wav: "client_b_output.wav".to_string(),
                            output_wav_speech: "client_b_output.16kHz.mono.wav".to_string(),
                            output_yuv: a_to_b_video.map(|_| "client_b_output.yuv".to_string()),
                            output_mp4: a_to_b_video.map(|_| "client_b_output.mp4".to_string()),
                        },
                    };

                    reports.push(
                        self.run_test_case_and_get_report(
                            &test_case,
                            &test,
                            &network_profile.get_config(),
                        )
                        .await,
                    );
                }
            }
        }

        // Push the group of test case reports in with the test config itself for reporting.
        self.group_runs.push(GroupRun {
            group_config,
            reports,
        });

        Ok(())
    }

    // Publish a report and clear history.
    pub async fn report(&mut self) -> Result<()> {
        Report::create_summary_report(
            &self.set_name,
            &self.set_path,
            &self.time_started.format("%Y-%m-%d %H:%M:%S").to_string(),
            &self.group_runs,
            &self.sounds,
        )
        .await?;

        self.group_runs.clear();

        Ok(())
    }
}
