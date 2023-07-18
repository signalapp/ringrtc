//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use anyhow::Result;
use log::*;

use ringrtc::{
    common::{actor::Stopper, CallConfig, CallId, CallMediaType, DeviceId},
    native::PeerId,
    webrtc::{media::VideoSink, peer_connection_factory::IceServer},
};
use std::{fs::File, path::PathBuf, sync::mpsc::channel, thread, time::Duration};
use tokio::runtime::{Builder, Runtime};
use tonic::transport::Channel;
use tower::timeout::Timeout;

use crate::{
    endpoint::{CallEndpoint, EventSync},
    server::RelayServer,
    video,
};

// Modules for the testing service, from protobufs compiled by tonic.
pub mod calling {
    #![allow(clippy::derive_partial_eq_without_eq, clippy::enum_variant_names)]
    tonic::include_proto!("calling");
}
use calling::test_management_client::TestManagementClient;
use calling::{command_message::Command, Registration};

#[derive(Default)]
pub struct ScenarioConfig {
    pub video_width: u32,
    pub video_height: u32,
    pub video_input: Option<PathBuf>,
    pub output_video_width: u32,
    pub output_video_height: u32,
    pub video_output: Option<PathBuf>,
    pub ice_server: IceServer,
    pub force_relay: bool,
}

pub struct ManagedScenario {
    client: TestManagementClient<Timeout<Channel>>,
    rt: Runtime,
}

impl ManagedScenario {
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

        Ok(ManagedScenario { client, rt })
    }

    pub fn run(&mut self, name: &str, call_config: CallConfig, scenario_config: ScenarioConfig) {
        info!("Starting managed scenario...");

        let stopper = Stopper::new();
        let (registered_tx, registered_rx) = channel();
        let signaling_server =
            RelayServer::new(&stopper, Some(registered_tx)).expect("Start signaling server");
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

        let client = CallEndpoint::start(
            name,
            1 as DeviceId,
            call_config,
            scenario_config.force_relay,
            &scenario_config.ice_server,
            Box::new(signaling_server),
            &stopper,
            event_sync,
            video_sink,
        )
        .expect("Start client");

        // mut so that we can take() it exactly once when the call starts
        let mut video_input: Option<_> = scenario_config.video_input.as_ref().map(|path| {
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
        });

        // Wait to be registered with the relay server.
        info!("Waiting to be registered...");
        let _ = registered_rx.recv();

        // Now let the test server know we are ready ('register' with it too).

        let request = tonic::Request::new(Registration {
            client: name.to_string(),
        });

        let response = self.rt.block_on(self.client.ready(request));

        if let Ok(response) = response {
            let mut stream = response.into_inner();

            let join_handle = self.rt.spawn(async move {
                loop {
                    // Wait forever for commands, until STOP. Most operations are handled
                    // asynchronously by an Actor...
                    match stream.message().await {
                        Ok(Some(message)) => {
                            info!("ready(): Message to us? {}", message.client);
                            match Command::from_i32(message.command) {
                                Some(Command::StartAsCaller) => {
                                    info!("command_message::Command::StartAsCaller");
                                    // Run the call (the callee_id doesn't need to be the actual
                                    // id for testing).
                                    let call_id = CallId::new(0xCA111D);
                                    client.create_outgoing_call(
                                        &PeerId::from("dummy"),
                                        call_id,
                                        CallMediaType::Audio,
                                        client.device_id,
                                    );

                                    info!("Waiting to be connected...");
                                    let _ = connected_rx.recv();
                                    info!("Now in the call...");

                                    if let Some(video_input) = video_input.take() {
                                        client.send_video(
                                            video_input,
                                            video::FRAME_INTERVAL_30FPS,
                                            Duration::from_secs(1),
                                        )
                                    }
                                }
                                Some(Command::StartAsCallee) => {
                                    info!("command_message::Command::StartAsCallee");
                                    // We should know what the incoming call_id is, but for now,
                                    // hardcode it like the original implementation.
                                    let call_id = CallId::new(0xCA111D);

                                    // Wait to be in the ringing state before accepting an
                                    // incoming call.
                                    info!("Waiting to be ringing...");
                                    let _ = ringing_rx.recv();
                                    client.accept_incoming_call(call_id);

                                    info!("Waiting to be connected...");
                                    let _ = connected_rx.recv();
                                    info!("Now in the call...");

                                    if let Some(video_input) = video_input.take() {
                                        client.send_video(
                                            video_input,
                                            video::FRAME_INTERVAL_30FPS,
                                            Duration::from_secs(1),
                                        )
                                    }
                                }
                                Some(Command::Stop) => {
                                    info!("command_message::Command::Stop");
                                    client.hangup();

                                    // Then let the hangup settle.
                                    thread::sleep(Duration::from_millis(500));

                                    stopper.stop_all_and_join();

                                    break;
                                }
                                None => {}
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
            });

            let _ = self.rt.block_on(join_handle);

            let request = tonic::Request::new(Registration {
                client: name.to_string(),
            });

            let _ = self.rt.block_on(self.client.done(request));
        } else {
            error!(
                "ManagedScenario: Could not send ready() message: {:?}",
                response
            );
        }
    }
}
