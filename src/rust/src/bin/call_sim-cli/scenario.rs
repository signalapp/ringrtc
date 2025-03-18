//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    collections::HashMap,
    fs::File,
    path::PathBuf,
    sync::mpsc::{channel, Receiver},
    thread,
    time::Duration,
};

use anyhow::Result;
use log::*;
use ringrtc::{
    common::{actor::Stopper, CallConfig, CallId, CallMediaType, DeviceId},
    core::group_call::GroupId,
    lite::sfu::{GroupMember, MembershipProof, UserId},
    native::PeerId,
    webrtc::{media::VideoSink, peer_connection_factory::IceServer},
};
use tokio::runtime::{Builder, Runtime};
use tonic::transport::Channel;
use tower::timeout::Timeout;

use crate::{
    endpoint::{CallEndpoint, EventSync},
    relay::CallSimSignalingRelayClient,
    video::{self, I420Source},
};

// Modules for the testing service, from protobufs compiled by tonic.
pub mod calling {
    #![allow(clippy::derive_partial_eq_without_eq, clippy::enum_variant_names)]
    call_protobuf::include_call_sim_proto!();
}
use calling::{
    command_message::Command, test_management_client::TestManagementClient, CommandMessage,
    Registration,
};

struct ClientSync {
    registered: Receiver<i32>,
    connected: Receiver<()>,
    ringing: Receiver<()>,
    stopper: Stopper,
}

#[derive(Default)]
pub struct ScenarioConfig {
    pub video_width: u32,
    pub video_height: u32,
    pub video_input: Option<PathBuf>,
    pub output_video_width: u32,
    pub output_video_height: u32,
    pub video_output: Option<PathBuf>,
    pub deterministic_loss: Option<u8>,
    pub call_type_config: ScenarioCallTypeConfig,
}

#[derive(Clone)]
pub enum ScenarioCallTypeConfig {
    DirectCallConfig {
        ice_server: IceServer,
        force_relay: bool,
    },

    GroupCallConfig {
        sfu_url: String,
        group_id: GroupId,
        membership_proof: MembershipProof,
        group_member_info: Vec<GroupMember>,
    },
}

impl Default for ScenarioCallTypeConfig {
    fn default() -> Self {
        Self::DirectCallConfig {
            ice_server: Default::default(),
            force_relay: true,
        }
    }
}

pub struct ScenarioManager {
    client: TestManagementClient<Timeout<Channel>>,
    rt: Runtime,
}

impl ScenarioManager {
    pub fn new() -> Result<Self> {
        let rt = Builder::new_multi_thread().enable_all().build()?;

        info!("Connecting to the test manager...");

        let channel = rt.block_on(
            Channel::from_static("http://172.28.0.250:8080")
                .connect_timeout(Duration::from_millis(500))
                .connect(),
        )?;

        // Make sure all requests have a reasonable timeout.
        let timeout_channel = Timeout::new(channel, Duration::from_millis(1000));
        let client = TestManagementClient::new(timeout_channel);

        Ok(ScenarioManager { client, rt })
    }

    fn initialize_client(
        name: &str,
        ip: &str,
        user_id: Option<UserId>,
        device_id: DeviceId,
        call_config: CallConfig,
        scenario_config: &ScenarioConfig,
    ) -> (CallEndpoint, ClientSync) {
        let stopper = Stopper::new();
        let (registered_tx, registered_rx) = channel();
        let signaling_server = CallSimSignalingRelayClient::new(&stopper, Some(registered_tx))
            .expect("Start signaling server");
        let (ringing_tx, ringing_rx) = channel();
        let (connected_tx, connected_rx) = channel();
        let event_sync = EventSync {
            ringing: Some(ringing_tx),
            connected: Some(connected_tx),
        };

        let video_sink = scenario_config.video_output.as_ref().map(|path| {
            Box::new(video::WriterVideoSink::new(
                File::create(path).expect("open video output"),
                scenario_config.output_video_width,
                scenario_config.output_video_height,
            )) as Box<dyn VideoSink>
        });

        let packet_size_ms = call_config.audio_encoder_config.initial_packet_size_ms;

        let mut client = CallEndpoint::new(
            name,
            device_id,
            user_id,
            &call_config.audio_config,
            Box::new(signaling_server),
            &stopper,
            event_sync,
            video_sink,
            scenario_config.deterministic_loss.is_some(),
        )
        .expect("Start client");

        if let Some(loss_rate) = scenario_config.deterministic_loss {
            client.add_deterministic_loss_network(ip, loss_rate, packet_size_ms);
        }

        let client_sync = ClientSync {
            ringing: ringing_rx,
            registered: registered_rx,
            connected: connected_rx,
            stopper,
        };

        match &scenario_config.call_type_config {
            ScenarioCallTypeConfig::DirectCallConfig {
                ice_server,
                force_relay,
            } => {
                client.init_direct_settings(*force_relay, ice_server, call_config);
            }
            ScenarioCallTypeConfig::GroupCallConfig {
                sfu_url: _,
                group_id,
                membership_proof: _,
                group_member_info,
            } => {
                client.init_group_settings(HashMap::from([(
                    group_id.clone(),
                    group_member_info.clone(),
                )]));
            }
        }

        (client, client_sync)
    }

    fn initialize_video_input(scenario_config: &ScenarioConfig) -> Option<I420Source<File>> {
        scenario_config.video_input.as_ref().map(|path| {
            let ScenarioConfig {
                mut video_width,
                mut video_height,
                ..
            } = scenario_config;
            if video_width == 0 || video_height == 0 {
                let basename = path.file_stem().expect("not a valid file path");
                let basename = basename.to_str().expect("filenames must be UTF-8");
                let (_, dimensions) = basename
                    .rsplit_once('@')
                    .expect("cannot infer video dimensions from filename");
                let (width_str, height_str) = dimensions
                    .split_once('x')
                    .expect("cannot infer video dimensions from filename");
                video_width = width_str
                    .parse()
                    .expect("cannot parse video width from filename");
                video_height = height_str
                    .parse()
                    .expect("cannot parse video height from filename");
            }
            video::I420Source::new(
                video_width,
                video_height,
                File::open(path).expect("open video input"),
            )
        })
    }

    pub fn run(
        &mut self,
        name: &str,
        ip: &str,
        user_id: Option<UserId>,
        device_id: DeviceId,
        call_config: CallConfig,
        scenario_config: ScenarioConfig,
    ) {
        info!("Starting managed scenario...");

        let video_input = Self::initialize_video_input(&scenario_config);
        let (client, client_sync) =
            Self::initialize_client(name, ip, user_id, device_id, call_config, &scenario_config);

        // Wait to be registered with the relay server.
        info!("Waiting to be registered...");
        let _ = client_sync.registered.recv();

        // Now let the test server know we are ready ('register' with it too).
        let request = tonic::Request::new(Registration {
            client: name.to_string(),
        });

        let response = self.rt.block_on(self.client.ready(request));

        if let Ok(response) = response {
            let stream = response.into_inner();

            let join_handle = self.rt.spawn(async move {
                match scenario_config.call_type_config {
                    ScenarioCallTypeConfig::GroupCallConfig { .. } => {
                        Self::run_group(
                            stream,
                            client,
                            client_sync,
                            video_input,
                            scenario_config.call_type_config,
                        )
                        .await
                    }
                    ScenarioCallTypeConfig::DirectCallConfig { .. } => {
                        Self::run_direct(stream, client, client_sync, video_input).await
                    }
                }
            });

            let _ = self.rt.block_on(join_handle);

            // tell the test server we are done
            let request = tonic::Request::new(Registration {
                client: name.to_string(),
            });

            let _ = self.rt.block_on(self.client.done(request));

            info!("Done.");
        } else {
            error!(
                "ManagedScenario: Could not send ready() message: {:?}",
                response
            );
        }
    }

    async fn run_direct(
        mut command_stream: tonic::Streaming<CommandMessage>,
        client: CallEndpoint,
        client_sync: ClientSync,
        mut video_input: Option<I420Source<File>>,
    ) {
        info!("run_direct(): starting loop");
        loop {
            // Wait forever for commands, until STOP. Most operations are handled
            // asynchronously by an Actor...
            match command_stream.message().await {
                Ok(Some(message)) => {
                    info!("ready(): Message to us? {}", message.client);
                    match Command::try_from(message.command) {
                        Ok(Command::StartAsCaller) => {
                            info!("command_message::Command::StartAsCaller");
                            // Run the call (the callee_id doesn't need to be the actual
                            // id for testing).
                            let call_id = CallId::new(0xCA111D);
                            client.create_outgoing_direct_call(
                                &PeerId::from("dummy"),
                                call_id,
                                CallMediaType::Audio,
                                client.device_id,
                            );

                            info!("Waiting to be connected...");
                            let _ = client_sync.connected.recv();
                            info!("Now in the call...");

                            if let Some(video_input) = video_input.take() {
                                client.send_video(
                                    video_input,
                                    video::FRAME_INTERVAL_30FPS,
                                    Duration::from_secs(1),
                                )
                            }
                        }
                        Ok(Command::StartAsCallee) => {
                            info!("command_message::Command::StartAsCallee");
                            // We should know what the incoming call_id is, but for now,
                            // hardcode it like the original implementation.
                            let call_id = CallId::new(0xCA111D);

                            // Wait to be in the ringing state before accepting an
                            // incoming call.
                            info!("Waiting to be ringing...");
                            let _ = client_sync.ringing.recv();
                            client.accept_incoming_direct_call(call_id);

                            info!("Waiting to be connected...");
                            let _ = client_sync.connected.recv();
                            info!("Now in the call...");

                            if let Some(video_input) = video_input.take() {
                                client.send_video(
                                    video_input,
                                    video::FRAME_INTERVAL_30FPS,
                                    Duration::from_secs(1),
                                )
                            }
                        }
                        Ok(Command::Stop) => {
                            info!("command_message::Command::Stop");
                            client.hangup();
                            client.stop_network();

                            // Then let the hangup settle.
                            thread::sleep(Duration::from_millis(100));

                            client_sync.stopper.stop_all_and_join();

                            break;
                        }
                        Err(_) => {}
                    }
                }
                Ok(None) => {
                    warn!("ready(): Received Message: None");
                    break;
                }
                Err(err) => {
                    error!("ready(): {}", err);
                    break;
                }
            }
        }
        info!("Done with scenario.");
    }

    async fn run_group(
        mut command_stream: tonic::Streaming<CommandMessage>,
        client: CallEndpoint,
        client_sync: ClientSync,
        mut video_input: Option<I420Source<File>>,
        group_scenario_config: ScenarioCallTypeConfig,
    ) {
        info!("run_group(): starting loop");

        let ScenarioCallTypeConfig::GroupCallConfig {
            sfu_url,
            group_id,
            membership_proof,
            group_member_info: _,
        } = group_scenario_config
        else {
            panic!("expected a group call config")
        };

        loop {
            // Wait forever for commands, until STOP. Most operations are handled
            // asynchronously by an Actor...
            match command_stream.message().await {
                Ok(Some(message)) => {
                    info!("ready(): Message to us? {}", message.client);
                    match Command::try_from(message.command) {
                        Ok(Command::StartAsCaller) => {
                            info!("command_message::Command::StartAsCaller");
                            client.join_group_call(
                                sfu_url.clone(),
                                group_id.clone(),
                                membership_proof.clone(),
                            );

                            info!("Waiting to be connected...");
                            let _ = client_sync.connected.recv();
                            info!("Now in the group call...");

                            if let Some(video_input) = video_input.take() {
                                client.send_video(
                                    video_input,
                                    video::FRAME_INTERVAL_30FPS,
                                    Duration::from_secs(1),
                                )
                            }

                            info!("finished command_message::Command::StartAsCaller");
                        }
                        Ok(Command::StartAsCallee) => {
                            info!("command_message::Command::StartAsCallee");
                            // TODO: implement wait for group ring
                            client.join_group_call(
                                sfu_url.clone(),
                                group_id.clone(),
                                membership_proof.clone(),
                            );

                            info!("Waiting to be connected...");
                            let _ = client_sync.connected.recv();
                            info!("Now in the call...");

                            if let Some(video_input) = video_input.take() {
                                client.send_video(
                                    video_input,
                                    video::FRAME_INTERVAL_30FPS,
                                    Duration::from_secs(1),
                                )
                            }
                            info!("finished command_message::Command::StartAsCallee");
                        }
                        Ok(Command::Stop) => {
                            info!("command_message::Command::Stop");
                            client.hangup_group_call();
                            client.stop_network();

                            // Then let the hangup settle.
                            thread::sleep(Duration::from_millis(100));

                            client_sync.stopper.stop_all_and_join();

                            info!("finished command_message::Command::Stop");
                            break;
                        }
                        Err(_) => {}
                    }
                }
                Ok(None) => {
                    warn!("ready(): Received Message: None");
                    break;
                }
                Err(err) => {
                    error!("ready(): {}", err);
                    break;
                }
            }
        }
        info!("Done with scenario.");
    }
}
