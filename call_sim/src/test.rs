//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

pub mod calling {
    #![allow(clippy::derive_partial_eq_without_eq, clippy::enum_variant_names)]
    protobuf::include_call_sim_proto!();
}
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Result, anyhow};
use calling::{
    CommandMessage, Empty, command_message::Command, test_management_client::TestManagementClient,
};
use chrono::{DateTime, Local};
use derive_builder::Builder;
use futures_util::future::join_all;
use itertools::{Itertools, izip};
use log::{error, info};
use relative_path::RelativePath;
use tonic::transport::Channel;
use tower::timeout::Timeout;

use crate::{
    audio::{AudioFiles, chop_audio_and_analyze, get_audio_and_analyze},
    common::{
        AToZIterator, AudioAnalysisMode, CallConfig, ClientIpIterator, GroupConfig,
        NetworkConfigWithOffset, NetworkProfile, TestCaseConfig,
    },
    config::DynamicClientProfileFactory,
    docker::{
        self, DockerStats, analyze_video, analyze_visqol_mos, clean_network, clean_up,
        convert_mp4_to_yuv, convert_raw_to_wav, convert_wav_to_16khz_mono, convert_yuv_to_mp4,
        create_network, emulate_network_change, emulate_network_start, finish_perf,
        generate_spectrogram, get_sfu_server_logs, get_signaling_server_logs, get_turn_server_logs,
        start_cli, start_client, start_playout, start_sfu_server, start_signaling_server,
        start_tcpdump, start_turn_server,
    },
    report::{AnalysisReport, AnalysisReportMos, Report},
};

/// How long to wait for a notification from the signaling server about the clients.
const CLIENT_NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Builder, Debug)]
pub struct Client<'a> {
    pub name: String,
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
#[derive(Default, Debug)]
pub struct AudioTestResults {
    /// MOS analysis using visqol with the speech model (wideband).
    pub visqol_mos_speech: AnalysisReportMos,
    /// MOS analysis using visqol with the audio model (fullband).
    pub visqol_mos_audio: AnalysisReportMos,
    /// MOS analysis using pesq (wideband).
    pub pesq_mos: AnalysisReportMos,
    /// MOS analysis using plc.
    pub plc_mos: AnalysisReportMos,
    /// Average MOS across all enabled algorithms.
    pub mos_average: AnalysisReportMos,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SfuConnectionParams {
    Local,
    Remote { sfu_url: String },
}

impl SfuConnectionParams {
    pub fn url(&self) -> &str {
        match self {
            SfuConnectionParams::Local => "http://172.28.0.252:8080",
            SfuConnectionParams::Remote { sfu_url } => sfu_url.as_str(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CallTypeConfig {
    Group {
        sfu_connection_params: SfuConnectionParams,
        group_name: Option<String>,
    },
    Direct,
}

impl CallTypeConfig {
    pub fn is_group_call(&self) -> bool {
        matches!(self, CallTypeConfig::Group { .. })
    }

    pub fn requires_local_sfu(&self) -> bool {
        matches!(
            self,
            CallTypeConfig::Group {
                sfu_connection_params: SfuConnectionParams::Local,
                ..
            }
        )
    }
}

pub struct TestCase<'a> {
    pub report_name: String,
    pub test_path: String,

    pub test_case_name: String,
    pub network_profile: NetworkProfile,

    pub clients: &'a Vec<Client<'a>>,
}

impl<'a> TestCase<'a> {
    pub fn client_a(&self) -> &Client<'a> {
        self.clients.first().unwrap()
    }

    pub fn client_b(&self) -> &Client<'a> {
        self.clients.get(1).unwrap()
    }
}

#[derive(Debug)]
pub struct Sound {
    pub name: String,
    /// Optionally store the mos of the file vs. itself as a theoretical maximum.
    pub reference_mos: Option<f32>,
    pub reference_mos_16khz_mono: Option<f32>,
    pub duration: f64,
}

impl Sound {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            reference_mos: None,
            reference_mos_16khz_mono: None,
            duration: 0.0,
        }
    }

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

    fn spectrogram_extension(&self) -> &str {
        "png"
    }
}

#[derive(Debug)]
pub struct Video {
    pub name: String,
}

impl Video {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

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
    data_path: String,

    group_runs: Vec<GroupRun>,

    client_profile_factory: DynamicClientProfileFactory,
    call_type: CallTypeConfig,

    // Keep track of all reference files used by copying them into the test
    // directory, converting them if necessary (and avoiding duplicates if
    // multiple runs use the same media). This way the test results have full
    // information even if we change the reference media in the future.
    sounds: HashMap<String, Sound>,
    videos: HashMap<String, Video>,

    // Whether to run `perf record` (and report)
    profile: bool,

    // Which clients to analyze and report on.
    // `None` defaults based on call type; see `Test::should_analyze`.
    analyze_clients: Option<HashSet<String>>,
}

pub struct MediaFileIo {
    pub audio_output_file: Option<String>,
    pub video_input_file: Option<String>,
    pub video_output_file: Option<String>,
}

/// How long to wait for every client to report itself ready before giving up on a test case.
/// A client that fails at startup - a missing media or weights file, for instance - exits
/// immediately and never registers, and without a deadline the run would block indefinitely.
const CLIENT_READY_TIMEOUT: Duration = Duration::from_secs(120);

impl Test {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root_path: &PathBuf,
        output_dir: &str,
        media_dir: &str,
        data_dir: &str,
        set_name: &str,
        client_profile_factory: DynamicClientProfileFactory,
        call_type: CallTypeConfig,
        profile: bool,
        analyze_clients: Option<HashSet<String>>,
    ) -> Result<Self> {
        let time_started = chrono::Local::now();

        let output_path = RelativePath::new(output_dir).to_logical_path(root_path);
        let media_path = RelativePath::new(media_dir).to_logical_path(root_path);
        let data_path = RelativePath::new(data_dir).to_logical_path(root_path);

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

        info!("Running test set: {}", set_name);
        info!("Using path: {}", set_path);

        Ok(Self {
            time_started,
            set_path,
            set_name: set_name.to_string(),
            media_path: media_path.display().to_string(),
            data_path: data_path.display().to_string(),

            group_runs: vec![],
            sounds: HashMap::new(),
            videos: HashMap::new(),

            client_profile_factory,
            call_type,
            profile,
            analyze_clients,
        })
    }

    /// Whether to analyze this client's received media and give it a section in the report.
    ///
    /// Test specifies whichclients in [self.analyze_clients] else every client in a
    /// group call is analyzed, while in a 1:1 call client_a is skipped
    fn should_analyze(&self, client_name: &str) -> bool {
        match &self.analyze_clients {
            Some(clients) => clients.contains(client_name),
            None => self.is_group_call() || client_name != "client_a",
        }
    }

    fn is_group_call(&self) -> bool {
        self.call_type.is_group_call()
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

    /// Check that a file exists.
    fn check_file(dir: &str, name: &str) -> Result<()> {
        let path = Path::new(dir).join(name);
        if !path.exists() {
            return Err(anyhow!("Missing file `{}`", path.display()));
        }

        Ok(())
    }

    /// Check that all the files a client needs are available before running any tests.
    fn check_client_files(&self, call_config: &CallConfig) -> Result<()> {
        Self::check_file(
            &self.media_path,
            &Sound::new(&call_config.audio.input_name).raw(),
        )?;

        if let Some(name) = &call_config.video.input_name {
            // Only the mp4 is a reference file, the raw video is generated from it.
            Self::check_file(&self.media_path, &Video::new(name).mp4())?;
        }

        if !call_config.audio.dnn_weights_name.is_empty() {
            Self::check_file(&self.data_path, &call_config.audio.dnn_weights_name)?;
        }

        Ok(())
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
        let sleep_duration = if self.is_group_call() {
            Duration::from_secs(3)
        } else {
            Duration::from_secs(1)
        };

        if test_case_config.needs_turn_server() {
            // We'll assume any relay server configuration should start the test turn server.
            start_turn_server().await?;
        }

        if self.is_group_call() {
            start_sfu_server().await?;
        }

        // Sleep here to allow the server(s) to get running.
        tokio::time::sleep(sleep_duration).await;

        info!("Connecting to test manager...");
        let mut test_manager = self.start_test_manager_client().await?;
        info!("Starting clients...");

        // Sign-up for notifications from the signaling server.
        let request = tonic::Request::new(Empty {});
        let response = test_manager.notification(request).await;

        if let Ok(response) = response {
            let mut stream = response.into_inner();

            let sys_now = SystemTime::now();
            let usable_client_configs = test_case_config.usable_client_configs();
            let client_names = test_case
                .clients
                .iter()
                .map(|c| c.name.clone())
                .collect_vec();
            let client_profiles = self.client_profile_factory.client_profiles_for_group(
                "generated_group",
                &client_names,
                sys_now,
            );
            let clients_and_configs = izip!(
                test_case.clients.iter(),
                usable_client_configs.iter(),
                client_profiles.iter(),
                ClientIpIterator::default()
            );

            // start clients
            for (client, &config, _client_profile, _ip) in clients_and_configs.clone() {
                start_client(
                    &client.name,
                    &test_case.test_path,
                    &self.set_path,
                    &self.data_path,
                )
                .await?;

                if config.tcpdump {
                    start_tcpdump(&client.name, &test_case.test_path).await?;
                }
            }

            // start clis
            info!("\n");
            for (client, &config, client_profile, ip) in clients_and_configs {
                let should_performance_profile =
                    client.name == test_case.client_b().name && self.profile;
                start_cli(
                    &client.name,
                    MediaFileIo {
                        audio_output_file: if test_case_config.save_media_files {
                            Some(client.output_raw.clone())
                        } else {
                            None
                        },
                        video_input_file: client.video.map(|v| v.raw()),
                        video_output_file: if test_case_config.save_media_files {
                            client.output_yuv.clone()
                        } else {
                            None
                        },
                    },
                    config,
                    // TODO: this is needed to configure video height/width
                    None,
                    client_profile,
                    &self.call_type,
                    ip,
                    should_performance_profile,
                )
                .await?;
            }

            info!("Waiting for clients...");

            let mut done = false;
            loop {
                let message = tokio::time::timeout(CLIENT_NOTIFICATION_TIMEOUT, stream.message())
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "Timed out after {:?} waiting for {} clients to report ready. \
                                 Check the client logs in {} - a client that fails at startup \
                                 exits without registering.",
                            CLIENT_READY_TIMEOUT,
                            test_case_config.usable_client_configs().len(),
                            test_case.test_path,
                        )
                    })?;

                match message {
                    Ok(Some(event)) => {
                        // We wait for both clients to indicate that they are ready and already
                        // registered with the relay server.
                        if !done
                            && event.ready_count as usize
                                == test_case_config.usable_client_configs().len()
                        {
                            info!("Running test...");

                            let mut network_configs = network_configs.iter();
                            let mut timed_config_next = network_configs.next();
                            let mut emulation_started = false;

                            if let Some(timed_network_config) = timed_config_next
                                && timed_network_config.offset == Duration::from_secs(0)
                            {
                                info!("  Setting up network emulation.");
                                for client in test_case.clients {
                                    emulate_network_start(
                                        &client.name,
                                        &timed_network_config.network_config,
                                        test_case_config.stun_bypass,
                                    )
                                    .await?;
                                }

                                emulation_started = true;
                                timed_config_next = network_configs.next();
                            }

                            // Start monitoring docker stats. They will end when the associated container stops.
                            let docker_stats = DockerStats::new().await?;
                            for client in test_case.clients {
                                docker_stats.start(&client.name, &test_case.test_path);
                            }

                            // Every client except client_a should be a callee
                            for client in test_case.clients.iter().skip(1) {
                                let request = tonic::Request::new(CommandMessage {
                                    client: client.name.to_string(),
                                    command: Command::StartAsCallee.into(),
                                });

                                test_manager.send_command(request).await?;
                            }

                            for client in test_case.clients {
                                start_playout(
                                    &client.name,
                                    &client.sound.raw(),
                                    client.sound.duration,
                                    test_case_config.length_seconds,
                                )
                                .await?;
                            }

                            // Tell client_a to start as a caller.
                            let request = tonic::Request::new(CommandMessage {
                                client: test_case.client_a().name.to_string(),
                                command: Command::StartAsCaller.into(),
                            });
                            test_manager.send_command(request).await?;

                            info!("Waiting for the test to complete...");

                            // Yield for a moment to let connections be made.
                            thread::sleep(Duration::from_millis(100));

                            let start_time = Instant::now();

                            for i in (1..=(test_case_config.length_seconds)).rev() {
                                eprint!("\r{} seconds remaining...", i);
                                tokio::time::sleep(Duration::from_secs(1)).await;

                                if let Some(timed_network_config) = timed_config_next
                                    && start_time.elapsed() >= timed_network_config.offset
                                {
                                    // Changing the network emulation takes time, so do it concurrently.
                                    let network_config = timed_network_config.network_config;
                                    let stun_bypass = test_case_config.stun_bypass;
                                    // For now we will be ignoring errors when changing the emulation settings.

                                    let client_names = client_names.clone();
                                    tokio::spawn(async move {
                                        eprint!("\n  Applying new emulated network settings...");

                                        // NOTE: We assume this block completes fairly quickly! To avoid issues,
                                        // emulation shouldn't change more than once every 2 seconds!

                                        let handles = client_names
                                            .into_iter()
                                            .map(
                                                |name| -> tokio::task::JoinHandle<
                                                    Result<(), anyhow::Error>,
                                                > {
                                                    tokio::spawn(async move {
                                                        if emulation_started {
                                                            emulate_network_change(
                                                                &name,
                                                                &network_config,
                                                            )
                                                            .await?;
                                                        } else {
                                                            emulate_network_start(
                                                                &name,
                                                                &network_config,
                                                                stun_bypass,
                                                            )
                                                            .await?;
                                                        }
                                                        Ok(())
                                                    })
                                                },
                                            )
                                            .collect_vec();
                                        join_all(handles).await;
                                        info!(" Done.");
                                    });

                                    emulation_started = true;

                                    timed_config_next = network_configs.next();
                                }
                            }

                            // Tell client_a to stop.
                            for client in test_case.clients {
                                let request = tonic::Request::new(CommandMessage {
                                    client: client.name.clone(),
                                    command: Command::Stop.into(),
                                });

                                test_manager.send_command(request).await?;
                            }

                            done = true;

                            info!("Test complete.");
                            info!("Waiting for the clients to terminate...");
                        } else if done && event.ready_count == 0 {
                            info!("  Done.");
                            break;
                        }
                    }
                    Ok(None) => {
                        info!("Received Message: None");
                        break;
                    }
                    Err(err) => {
                        error!("Error: {}", err);
                        break;
                    }
                }
            }
        } else {
            error!("Could not send notification() message: {:?}", response);
        }

        Ok(())
    }

    /// Generates report artifacts by performing analysis on all media outputs. Performs
    /// the necessary conversions to do so.
    ///
    /// A client's recording holds what the *other* clients sent, so that is what it is compared
    /// against - never its own sound. With nobody else talking there is nothing to compare, and
    /// with more than one other talker the recording is a mix that no single reference matches;
    /// analysis is skipped in both cases rather than reporting a meaningless score.
    async fn generate_artifacts(
        &self,
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
    ) -> Result<HashMap<String, AudioTestResults>> {
        let mut audio_test_results = HashMap::new();

        if !test_case_config.save_media_files {
            // Return an empty result if media files are not saved.
            return Ok(audio_test_results);
        }

        let clients_and_configs = || {
            test_case
                .clients
                .iter()
                .zip(test_case_config.usable_client_configs())
        };

        // Perform conversions of audio data.
        for (client, config) in clients_and_configs() {
            convert_raw_to_wav(
                &test_case.test_path,
                &client.output_raw,
                &client.output_wav,
                Some(test_case_config.length_seconds),
            )
            .await?;

            if config.audio.requires_speech() {
                convert_wav_to_16khz_mono(
                    &test_case.test_path,
                    &client.output_wav,
                    &client.output_wav_speech,
                )
                .await?;
            }
        }

        // Clients sending recorded silence are not talkers, so they are never a reference.
        let talkers: Vec<usize> = clients_and_configs()
            .enumerate()
            .filter(|(_, (_, config))| config.audio.input_name != "silence")
            .map(|(index, _)| index)
            .collect();

        for (index, (client, config)) in clients_and_configs().enumerate() {
            if !self.should_analyze(&client.name) {
                info!(
                    "Skipping audio analysis for {}: not in the set of clients to analyze",
                    client.name
                );
                continue;
            }

            let other_talkers: Vec<usize> = talkers
                .iter()
                .copied()
                .filter(|talker| *talker != index)
                .collect();

            let reference = match other_talkers.as_slice() {
                [only] => Some(test_case.clients[*only].sound),
                [] => {
                    info!(
                        "Skipping audio analysis for {}: no other client was talking",
                        client.name
                    );
                    None
                }
                _ => {
                    info!(
                        "Skipping audio analysis for {}: {} other clients were talking, so its \
                         recording is a mix that no single reference matches",
                        client.name,
                        other_talkers.len()
                    );
                    None
                }
            };

            let mut results = AudioTestResults::default();
            if let Some(reference) = reference {
                let audio_files = AudioFiles {
                    degraded_path: &test_case.test_path,
                    degraded_file: &client.output_wav,
                    ref_path: &self.set_path,
                    ref_file: &reference.wav(false),
                };

                let speech_files = AudioFiles {
                    degraded_path: &test_case.test_path,
                    degraded_file: &client.output_wav_speech,
                    ref_path: &self.set_path,
                    ref_file: &reference.wav(true),
                };

                match config.audio.analysis_mode {
                    AudioAnalysisMode::None => {
                        // Do nothing, no analysis is requested.
                    }
                    AudioAnalysisMode::Normal => {
                        get_audio_and_analyze(
                            &audio_files,
                            &speech_files,
                            &client.name,
                            &config.audio,
                            test_case_config.analysis_concurrency,
                            &mut results,
                        )
                        .await?;
                    }
                    AudioAnalysisMode::Chopped => {
                        chop_audio_and_analyze(
                            &audio_files,
                            &speech_files,
                            &client.name,
                            &config.audio,
                            test_case_config.analysis_concurrency,
                            &mut results,
                        )
                        .await?;
                    }
                }
            }
            audio_test_results.insert(client.name.clone(), results);

            if config.audio.generate_spectrogram {
                generate_spectrogram(
                    &test_case.test_path,
                    &client.output_wav,
                    client.sound.spectrogram_extension(),
                )
                .await?;
            }
        }

        // Video sent by client A is analyzed from the point of view of every other client.
        if let (Some(client_a_video), Some(dimensions)) = (
            test_case.client_a().video,
            test_case_config.client_a_config().video.dimensions(),
        ) {
            for client in test_case.clients.iter().skip(1) {
                // A client only records video if it was configured with a video input of its
                // own, so there is nothing to analyze for the ones that were not.
                let (Some(output_yuv), Some(output_mp4)) =
                    (client.output_yuv.as_deref(), client.output_mp4.as_deref())
                else {
                    info!(
                        "Skipping video analysis for {}: it has no video output",
                        client.name
                    );
                    continue;
                };

                convert_yuv_to_mp4(&test_case.test_path, output_yuv, output_mp4, dimensions)
                    .await?;

                analyze_video(
                    &test_case.test_path,
                    output_yuv,
                    &self.set_path,
                    &client_a_video.raw(),
                    dimensions,
                )
                .await?;
            }
        }

        // And client A's view of the video sent to it. Only client B's dimensions are
        // considered, since client A renders a single incoming stream.
        if let Some(dimensions) = test_case_config.client_b_config().video.dimensions() {
            convert_yuv_to_mp4(
                &test_case.test_path,
                test_case
                    .client_a()
                    .output_yuv
                    .as_deref()
                    .expect("missing output"),
                test_case
                    .client_a()
                    .output_mp4
                    .as_deref()
                    .expect("missing output"),
                dimensions,
            )
            .await?;
        }

        Ok(audio_test_results)
    }

    /// Generates reports by parsing/checking artifacts for the test, and returns summary
    /// information about it.
    async fn generate_test_report(
        &self,
        test_case: &TestCase<'_>,
        test_case_config: &TestCaseConfig,
        network_configs: &Vec<NetworkConfigWithOffset>,
        mut test_results: HashMap<String, AudioTestResults>,
    ) -> Result<Vec<Report>> {
        let mut reports = Vec::with_capacity(test_case.clients.len());
        for (client, config) in test_case
            .clients
            .iter()
            .zip(test_case_config.usable_client_configs())
        {
            let results = test_results.remove(&client.name).unwrap_or_default();
            reports
                .push(Report::build(client, config, test_case, test_case_config, results).await?);
        }

        // Every client's outbound SSRCs, so each report can name the client behind each of its
        // inbound streams instead of showing a bare SSRC.
        let sender_name_by_ssrc: HashMap<String, String> = reports
            .iter()
            .flat_map(|report| {
                report
                    .send_ssrcs()
                    .map(|ssrc| (ssrc.to_string(), report.client_name.clone()))
            })
            .collect();

        // Must keep this after populating reports/sender_name_by_ssrc so we can build the SSRC map
        reports.retain(|report| self.should_analyze(&report.client_name));

        for report in &mut reports {
            report.label_senders(&sender_name_by_ssrc);
            if test_case_config.create_charts {
                report.create_charts(&test_case.test_path).await;
            }
        }

        let reference_spectrogram = format!(
            "../../{}.{}",
            test_case.client_a().sound.wav(false),
            test_case.client_a().sound.spectrogram_extension()
        );
        let client_names: Vec<&str> = test_case
            .clients
            .iter()
            .map(|client| client.name.as_str())
            .collect();

        Report::create_test_case_report(
            &reports,
            &self.set_name,
            &reference_spectrogram,
            network_configs,
            test_case_config,
            &client_names,
        )
        .await?;

        Ok(reports)
    }

    /// Process a reference sound by copying to the output directory and converting it to wav.
    /// Optionally, analyze it and store the reference mos value. This function will always
    /// process sounds in full-band (48kHz/two-channel) and wide-band (16kHz/mono).
    async fn process_sound(&mut self, name: &str, analyze: bool) -> Result<()> {
        // Only process each sound once. So if we already have it, don't do anything.
        // Note: This means that mos analysis can only happen when sounds are pre-processed.
        if !self.sounds.contains_key(name) {
            let mut sound = Sound::new(name);

            let raw_name = sound.raw();
            let wav_name = sound.wav(false);
            let wav_name_speech = sound.wav(true);

            // Copy the reference file to our test directory.
            Self::check_file(&self.media_path, &raw_name)?;
            fs::copy(
                format!("{}/{}", self.media_path, raw_name),
                format!("{}/{}", self.set_path, raw_name),
            )?;

            // Make sure there is a wav version of the file available.
            sound.duration = convert_raw_to_wav(&self.set_path, &raw_name, &wav_name, None).await?;
            convert_wav_to_16khz_mono(&self.set_path, &wav_name, &wav_name_speech).await?;

            // And a reference spectrogram. Since the speech wav files have a limited frequency
            // range, we will only generate spectrograms for the full-band audio files.
            generate_spectrogram(&self.set_path, &wav_name, sound.spectrogram_extension()).await?;

            if analyze {
                let extension = "visqol_mos_audio.log".to_string();

                analyze_visqol_mos(
                    &self.set_path,
                    &wav_name,
                    &self.set_path,
                    &wav_name,
                    &extension,
                    false,
                )
                .await?;

                let mos = AnalysisReport::parse_visqol_mos_results(&format!(
                    "{}/{}.{}",
                    self.set_path, wav_name, extension
                ))
                .await?;

                sound.reference_mos = mos;

                let extension = "visqol_mos_speech.log".to_string();

                analyze_visqol_mos(
                    &self.set_path,
                    &wav_name_speech,
                    &self.set_path,
                    &wav_name_speech,
                    &extension,
                    true,
                )
                .await?;

                let mos = AnalysisReport::parse_visqol_mos_results(&format!(
                    "{}/{}.{}",
                    self.set_path, wav_name_speech, extension
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
            let video = Video::new(name);

            let raw_name = video.raw();
            let mp4_name = video.mp4();

            // Copy the *MP4* reference file to our test directory.
            // This is different from sounds, but raw video is much bigger.
            Self::check_file(&self.media_path, &mp4_name)?;
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
    ) -> Result<Vec<Report>> {
        match self
            .run_test(test_case, test_case_config, network_configs)
            .await
        {
            Ok(_) => {
                if let Err(e) = Self::tear_down_virtual_audio(test_case).await {
                    error!("Couldn't tear down audio; continuing. {:?}", e);
                }

                // perf should only ever run on the "second" client
                if self.profile {
                    info!("waiting for perf... ");
                    if let Err(e) = finish_perf(&test_case.client_b().name).await {
                        error!("couldn't wait for perf {:?}", e);
                    }
                    info!("... done");
                }

                // For debugging, dump the signaling_server logs.
                get_signaling_server_logs(&test_case.test_path).await?;

                // Dump the turn server logs if the local one was running.
                if test_case_config.needs_turn_server() {
                    get_turn_server_logs(&test_case.test_path).await?;
                }

                // Dump the turn server logs if the local one was running.
                if self.is_group_call() {
                    get_sfu_server_logs(&test_case.test_path).await?;
                }

                // We are done with the containers.
                clean_up_all_containers().await?;
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
                            Ok(reports) => Ok(reports),
                            Err(err) => {
                                error!("Error generating test report: {}", err);
                                Err(err)
                            }
                        }
                    }
                    Err(err) => {
                        error!("Error generating artifacts: {}", err);
                        Err(err)
                    }
                }
            }
            Err(err) => {
                error!("Error running test: {}", err);
                if let Err(e) = Self::tear_down_virtual_audio(test_case).await {
                    error!("Couldn't tear down audio; continuing. {:?}", e);
                }
                clean_up_all_containers().await?;
                clean_network().await?;

                Err(err)
            }
        }
    }

    async fn tear_down_virtual_audio(test_case: &TestCase<'_>) -> Result<()> {
        docker::tear_down_virtual_audio(
            &test_case
                .clients
                .iter()
                .map(|client| client.name.as_str())
                .collect(),
        )
        .await
    }

    /// Runs the provided test permutations as individual test cases.
    pub async fn run(
        &mut self,
        group_config: GroupConfig,
        tests: Vec<TestCaseConfig>,
        network_profiles: Vec<NetworkProfile>,
    ) -> Result<()> {
        let mut reports: Vec<Result<Report>> = vec![];

        for test in &tests {
            for client_config in &test.usable_client_configs() {
                self.check_client_files(client_config)?;
            }
        }

        for mut test in tests {
            test.is_group_call = self.is_group_call();

            let primary_sound = test
                .usable_client_configs()
                .first()
                .map(|config| config.audio.input_name.as_str())
                .unwrap_or("silence");

            // process media files first
            for client_config in &test.usable_client_configs() {
                self.process_sound(client_config.audio.input_name.as_str(), false)
                    .await?;
                if let Some(input_video) = client_config.video.input_name.as_deref() {
                    self.process_video(input_video).await?;
                }
            }

            let clients: Vec<_> = test
                .usable_client_configs()
                .iter()
                .zip(AToZIterator::default())
                .map(|(client_config, tag)| {
                    let name = format!("client_{tag}");
                    let video = client_config.video.input_name.as_deref();
                    Client {
                        name: name.clone(),
                        // The sound should have been processed.
                        sound: &self.sounds[client_config.audio.input_name.as_str()],
                        video: video.map(|v| &self.videos[v]),
                        output_raw: format!("{name}_output.raw"),
                        output_wav: format!("{name}_a_output.wav"),
                        output_wav_speech: format!("{name}_output.16kHz.mono.wav"),
                        // Note that we check if *B* is sending video to decide if *A* should output video.
                        output_yuv: video.map(|_| format!("{name}_output.yuv")),
                        output_mp4: video.map(|_| format!("{name}_output.mp4")),
                    }
                })
                .collect();

            if clients.len() != test.usable_client_configs().len() {
                return Err(anyhow::anyhow!(
                    "More than 26 clients requested - replace AToZIterator with some other tag generator"
                ));
            }

            for network_profile in &network_profiles {
                for i in 1..=test.iterations {
                    let report_name = format!(
                        "{}-{}-{}",
                        test.test_case_name,
                        primary_sound,
                        network_profile.get_name()
                    );

                    let test_case_path = if test.iterations > 1 {
                        info!("Running test case: {}, iteration: {}", report_name, i);
                        format!(
                            "{}/{}/{}_{}",
                            self.set_path, group_config.group_name, report_name, i
                        )
                    } else {
                        info!("Running test case: {}", report_name);
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
                        clients: &clients,
                    };

                    // One report per client, each becoming its own row in the summary.
                    match self
                        .run_test_case_and_get_report(
                            &test_case,
                            &test,
                            &network_profile.get_config(),
                        )
                        .await
                    {
                        Ok(client_reports) => reports.extend(client_reports.into_iter().map(Ok)),
                        Err(err) => reports.push(Err(err)),
                    }
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

pub async fn clean_up_all_containers() -> Result<()> {
    clean_up(
        vec!["signaling_server", "calling-backend", "turn", "visqol"],
        vec!["client_", "tcpdump_"],
    )
    .await
}
