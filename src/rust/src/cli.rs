//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use log::{debug, info};

use ringrtc::{
    common::{
        actor::{Actor, Stopper},
        units::DataRate,
        CallId, CallMediaType, DeviceId, Result,
    },
    core::{bandwidth_mode::BandwidthMode, call_manager::CallManager, group_call, signaling},
    lite::{http, sfu::UserId},
    native::{
        CallState, CallStateHandler, GroupUpdate, GroupUpdateHandler, NativeCallContext,
        NativePlatform, PeerId, SignalingSender,
    },
    simnet::{
        router,
        router::{LinkConfig, Router},
    },
    webrtc::{
        injectable_network,
        injectable_network::InjectableNetwork,
        media::{VideoFrame, VideoPixelFormat, VideoSink, VideoSource},
        network::NetworkInterfaceType,
        peer_connection::AudioLevel,
        peer_connection_factory::{self as pcf, IceServer, PeerConnectionFactory},
        peer_connection_observer::NetworkRoute,
    },
};
use std::{collections::HashMap, thread, time::Duration};

fn main() {
    log::set_logger(&LOG).expect("set logger");
    log::set_max_level(log::LevelFilter::Debug);

    // Show WebRTC logs via application Logger while debugging.
    #[cfg(debug_assertions)]
    ringrtc::webrtc::logging::set_logger(log::LevelFilter::Debug);

    #[cfg(not(debug_assertions))]
    ringrtc::webrtc::logging::set_logger(log::LevelFilter::Warn);

    let hide_ip = false;
    // TODO: Real STUN/TURN servers.
    let ice_server = IceServer::none();
    let stopper = Stopper::new();
    let signaling_server = SignalingServer::start(&stopper).expect("Start signaling server");
    let router = Router::start(&stopper).expect("Start router");
    let good_link = LinkConfig {
        delay_min: Duration::from_millis(10),
        delay_max: Duration::from_millis(20),
        loss_probabilty: 0.00,
        repeated_loss_probability: 0.00,
        rate: DataRate::from_mbps(5),
        queue_size: DataRate::from_mbps(5) * Duration::from_millis(500),
    };
    let bad_link = LinkConfig {
        delay_min: Duration::from_millis(100),
        delay_max: Duration::from_millis(200),
        loss_probabilty: 0.005,
        repeated_loss_probability: 0.70,
        rate: DataRate::from_kbps(256),
        queue_size: DataRate::from_kbps(256) * Duration::from_secs(500),
    };

    let caller = CallEndpoint::start(
        "caller",
        1 as DeviceId,
        hide_ip,
        &ice_server,
        &signaling_server,
        &router,
        &stopper,
    )
    .expect("Start caller");
    caller.add_network_interface(
        "cell",
        NetworkInterfaceType::Cellular,
        "1.1.0.1",
        1,
        &bad_link,
        &bad_link,
    );
    // caller.add_network_interface(
    //     "wifi",
    //     NetworkInterfaceType::Cellular,
    //     "1.1.0.2",
    //     2,
    //     &good_link,
    //     &good_link,
    // );

    let callee = CallEndpoint::start(
        "callee",
        1 as DeviceId,
        hide_ip,
        &ice_server,
        &signaling_server,
        &router,
        &stopper,
    )
    .expect("Start callee");
    callee.add_network_interface(
        "cell",
        NetworkInterfaceType::Cellular,
        "2.1.0.1",
        1,
        &good_link,
        &good_link,
    );
    callee.add_network_interface(
        "wifi",
        NetworkInterfaceType::Wifi,
        "2.1.0.2",
        2,
        &good_link,
        &good_link,
    );

    // Callee devices that won't answer but will still ring.
    let _ignored_callees: Vec<CallEndpoint> = (2..=6)
        .map(|device_id| {
            let callee = CallEndpoint::start(
                "callee",
                device_id as DeviceId,
                hide_ip,
                &ice_server,
                &signaling_server,
                &router,
                &stopper,
            )
            .expect("Start ignored callee");
            callee.add_network_interface(
                "cell",
                NetworkInterfaceType::Cellular,
                &format!("2.{}.0.1", device_id),
                1,
                &good_link,
                &good_link,
            );
            callee.add_network_interface(
                "wifi",
                NetworkInterfaceType::Wifi,
                &format!("2.{}.0.2", device_id),
                2,
                &good_link,
                &good_link,
            );
            callee
        })
        .collect();

    // Run the call
    let call_id = CallId::new(0xCA111D);
    caller.create_outgoing_call(
        &callee.peer_id,
        call_id,
        CallMediaType::Audio,
        caller.device_id,
    );

    // Let it connect and ring before accepting the call.
    thread::sleep(Duration::from_secs(5));
    callee.accept_incoming_call(call_id);
    caller.send_generated_video(640, 480, Duration::from_millis(33));

    // Let this go for a while before hanging up
    thread::sleep(Duration::from_secs(5));

    caller.hangup();
    callee.hangup();
    // Then let that settle before ending.
    thread::sleep(Duration::from_secs(1));

    stopper.stop_all_and_join();
}

#[derive(Clone)]
struct CallEndpoint {
    // We keep a copy of these outside of the actor state
    // so we can know them in any thread.
    peer_id: PeerId,
    device_id: DeviceId,
    // There is probably a way to have a CallEndpoint without a thread,
    // but this is the easiest way to get around the nasty dependency cycle
    // of CallEndpoint -> CallManger -> NativePlatform -> CallEndpoint.
    // And it makes it pretty easy to schedule generation of video frames.
    actor: Actor<CallEndpointState>,
}

struct CallEndpointState {
    peer_id: PeerId,
    device_id: DeviceId,

    // How we send and receive signaling
    signaling_server: SignalingServer,
    // How we tell PeerConnections there are network interfaces and inject
    // packet into them
    network: InjectableNetwork,
    // How we simulate packets being routed around
    router: Router,
    // How we control calls
    call_manager: CallManager<NativePlatform>,
    call_context: NativeCallContext,

    // Keep a copy around to be able to schedule video frames
    actor: Actor<Self>,
    // Keep a copy around to be able to push out video frames
    outgoing_video_source: VideoSource,
}

impl CallEndpoint {
    pub fn start(
        peer_id: &str,
        device_id: DeviceId,
        hide_ip: bool,
        ice_server: &IceServer,
        signaling_server: &SignalingServer,
        router: &Router,
        stopper: &Stopper,
    ) -> Result<Self> {
        let peer_id = PeerId::from(peer_id);

        // To send across threads
        let ice_server = ice_server.clone();
        let signaling_server: SignalingServer = signaling_server.clone();
        let router = router.clone();

        Ok(Self::from_actor(
            peer_id.clone(),
            device_id,
            Actor::start(stopper.clone(), move |actor| {
                // Constructing this is a funny way of getting a clone of the CallEndpoint
                // on the actor's thread so we can have it in the actor's state so we can
                // pass it to the NativePlatform/CallManager.
                // This is a little weird, but it seems nicer than doing some kind of
                // Option<CallManager> thing that we have to set later.
                let endpoint = Self::from_actor(peer_id.clone(), device_id, actor.clone());

                let mut pcf = PeerConnectionFactory::new(pcf::Config {
                    use_injectable_network: true, // Set up packet flow
                    ..Default::default()
                })?;
                info!(
                    "Audio playout devices: {:?}",
                    pcf.get_audio_playout_devices()
                );
                info!(
                    "Audio recording devices: {:?}",
                    pcf.get_audio_recording_devices()
                );

                let network = pcf.injectable_network().expect("get Injectable Network");
                let router_as_sender = router.clone();
                network.set_sender(Box::new(move |packet: injectable_network::Packet| {
                    router_as_sender.send_packet(router::Packet {
                        source: packet.source,
                        dest: packet.dest,
                        data: packet.data,
                    });
                }));

                // Set up signaling/state
                signaling_server.add_endpoint(&endpoint);
                let signaling_sender = Box::new(endpoint.clone());
                let should_assume_messages_sent = true; // cli doesn't support async sending yet.
                let state_handler = Box::new(endpoint.clone());
                let incoming_video_sink = Box::new(LoggingVideoSink {
                    peer_id: peer_id.clone(),
                });

                // Fill in fake group call things
                let http_client = http::DelegatingClient::new(endpoint.clone());
                let group_handler = Box::new(endpoint);

                let platform = NativePlatform::new(
                    pcf.clone(),
                    signaling_sender,
                    should_assume_messages_sent,
                    state_handler,
                    group_handler,
                );
                let call_manager = CallManager::new(platform, http_client)?;

                // And a CallContext.  We'll use the same context for each call.
                let outgoing_audio_track = pcf.create_outgoing_audio_track()?;
                let outgoing_video_source = pcf.create_outgoing_video_source()?;
                let outgoing_video_track =
                    pcf.create_outgoing_video_track(&outgoing_video_source)?;
                let call_context = NativeCallContext::new(
                    hide_ip,
                    ice_server,
                    outgoing_audio_track,
                    outgoing_video_track,
                    incoming_video_sink,
                );

                Ok(CallEndpointState {
                    peer_id,
                    device_id,

                    signaling_server,
                    network,
                    router,
                    call_manager,
                    call_context,

                    actor,
                    outgoing_video_source,
                })
            })?,
        ))
    }

    fn from_actor(peer_id: PeerId, device_id: DeviceId, actor: Actor<CallEndpointState>) -> Self {
        Self {
            peer_id,
            device_id,
            actor,
        }
    }

    pub fn add_network_interface(
        &self,
        name: &'static str,
        typ: NetworkInterfaceType,
        ip: &str,
        preference: u16,
        send_config: &LinkConfig,
        receive_config: &LinkConfig,
    ) {
        let ip = ip.parse().expect("parse IP address");

        // To send across threads
        let send_config = send_config.clone();
        let receive_config = receive_config.clone();

        self.actor.send(move |state| {
            // Adding it to the network causes the PeerConnections to learn about it through
            // the NetworkMonitor.
            state.network.add_interface(name, typ, ip, preference);
            // Adding it to the router applies the config to the up and down links
            // and allow routing packets to and from other endpoints.alloc
            // Passing in network.get_receiver() causes packets from the PeerConnections
            // to be routed through the router.
            let network_as_receiver = state.network.clone();
            state
                .router
                .add_interface(
                    ip,
                    send_config,
                    receive_config,
                    Box::new(move |packet: router::Packet| {
                        network_as_receiver.receive_udp(injectable_network::Packet {
                            source: packet.source,
                            dest: packet.dest,
                            data: packet.data,
                        });
                    }),
                )
                .expect("Add router interface");

            debug!(
                "Added an interface for {:?} to {:?}.{:?}",
                ip, state.peer_id, state.device_id
            );
        });
    }

    fn create_outgoing_call(
        &self,
        callee_id: &PeerId,
        call_id: CallId,
        media_type: CallMediaType,
        local_device_id: DeviceId,
    ) {
        // To send across threads
        let callee_id = callee_id.clone();

        self.actor.send(move |state| {
            state
                .call_manager
                .create_outgoing_call(callee_id, call_id, media_type, local_device_id)
                .expect("start outgoing call");
        });
    }

    pub fn accept_incoming_call(&self, call_id: CallId) {
        self.actor.send(move |state| {
            state
                .call_manager
                .accept_call(call_id)
                .expect("accept incoming call");
        });
    }

    pub fn hangup(&self) {
        self.actor.send(move |state| {
            state.call_manager.hangup().expect("hangup");
        });
    }

    // A callback from SignalingServer.
    fn receive_signaling(
        &self,
        sender_id: &PeerId,
        sender_device_id: DeviceId,
        call_id: CallId,
        msg: signaling::Message,
    ) {
        // To send across threads
        let sender_id = sender_id.clone();

        let sender_identity_key = sender_id.as_bytes().to_vec();
        let receiver_identity_key = self.peer_id.as_bytes().to_vec();
        self.actor.send(move |state| {
            let cm = &mut state.call_manager;
            match msg {
                signaling::Message::Offer(offer) => {
                    cm.received_offer(
                        sender_id,
                        call_id,
                        signaling::ReceivedOffer {
                            offer,
                            age: Duration::from_secs(0),
                            sender_device_id,
                            receiver_device_id: state.device_id,
                            receiver_device_is_primary: (state.device_id == 1),
                            sender_identity_key,
                            receiver_identity_key,
                        },
                    )
                    .expect("receive offer");
                }
                signaling::Message::Answer(answer) => {
                    cm.received_answer(
                        call_id,
                        signaling::ReceivedAnswer {
                            answer,
                            sender_device_id,
                            sender_identity_key,
                            receiver_identity_key,
                        },
                    )
                    .expect("received answer");
                }
                signaling::Message::Ice(ice) => {
                    cm.received_ice(
                        call_id,
                        signaling::ReceivedIce {
                            ice,
                            sender_device_id,
                        },
                    )
                    .expect("received ice candidates");
                }
                signaling::Message::Hangup(hangup) => {
                    cm.received_hangup(
                        call_id,
                        signaling::ReceivedHangup {
                            hangup,
                            sender_device_id,
                        },
                    )
                    .expect("received hangup");
                }
                signaling::Message::Busy => {
                    cm.received_busy(call_id, signaling::ReceivedBusy { sender_device_id })
                        .expect("received busy");
                }
            }
        });
    }

    fn send_generated_video(&self, width: u32, height: u32, duration: Duration) {
        fn send_one_frame_and_schedule_another(
            state: &mut CallEndpointState,
            width: u32,
            height: u32,
            duration: Duration,
        ) {
            let rgba_data: Vec<u8> = (0..(width * height * 4)).map(|i: u32| i as u8).collect();
            state
                .outgoing_video_source
                .push_frame(VideoFrame::copy_from_slice(
                    width,
                    height,
                    VideoPixelFormat::Rgba,
                    &rgba_data,
                ));
            state.actor.send_delayed(duration, move |state| {
                send_one_frame_and_schedule_another(state, width, height, duration);
            });
        }
        self.actor.send(move |state| {
            send_one_frame_and_schedule_another(state, width, height, duration);
        });
    }
}

impl SignalingSender for CallEndpoint {
    fn send_signaling(
        &self,
        recipient_id: &str,
        call_id: CallId,
        _receiver_device_id: Option<DeviceId>,
        msg: signaling::Message,
    ) -> Result<()> {
        // To send across threads
        let recipient_id = recipient_id.to_string();

        self.actor.send(move |state| {
            let sender_id = &state.peer_id;
            let sender_device_id = state.device_id;
            state.signaling_server.send_signaling(
                sender_id,
                sender_device_id,
                &recipient_id,
                call_id,
                msg,
            );
            state
                .call_manager
                .message_sent(call_id)
                .expect("signaling message sent");
        });
        Ok(())
    }

    fn send_call_message(
        &self,
        _recipient_id: UserId,
        _msg: Vec<u8>,
        _urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        unimplemented!()
    }

    fn send_call_message_to_group(
        &self,
        _group_id: group_call::GroupId,
        _msg: Vec<u8>,
        _urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        unimplemented!()
    }
}

impl CallStateHandler for CallEndpoint {
    fn handle_call_state(
        &self,
        remote_peer_id: &str,
        call_id: CallId,
        call_state: CallState,
    ) -> Result<()> {
        info!(
            "State change in call from {}.{} to {}: now {:?}",
            self.peer_id, self.device_id, remote_peer_id, call_state
        );

        self.actor.send(move |state| {
            if let CallState::Incoming(_call_media_type) | CallState::Outgoing(_call_media_type) =
                call_state
            {
                state
                    .call_manager
                    .proceed(
                        call_id,
                        state.call_context.clone(),
                        BandwidthMode::VeryLow,
                        None,
                    )
                    .expect("proceed with call");
            }
        });
        Ok(())
    }

    fn handle_network_route(
        &self,
        remote_peer_id: &str,
        network_route: NetworkRoute,
    ) -> Result<()> {
        info!(
            "Network route changed for {} => {}: {:?}",
            self.peer_id, remote_peer_id, network_route
        );
        Ok(())
    }

    fn handle_audio_levels(
        &self,
        remote_peer_id: &str,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        debug!(
            "Audio Levels captured for {} => {}: captured: {}; received: {}",
            self.peer_id, remote_peer_id, captured_level, received_level
        );
        Ok(())
    }

    fn handle_remote_video_state(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        info!(
            "Video State for {} => {}: {}",
            self.peer_id, remote_peer_id, enabled
        );
        Ok(())
    }

    fn handle_remote_sharing_screen(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        info!(
            "Sharing Screen for {} => {}: {}",
            self.peer_id, remote_peer_id, enabled
        );
        Ok(())
    }
}

impl GroupUpdateHandler for CallEndpoint {
    fn handle_group_update(&self, update: GroupUpdate) -> Result<()> {
        info!("Group Update {}", update);
        Ok(())
    }
}

impl http::Delegate for CallEndpoint {
    fn send_request(&self, _request_id: u32, _request: http::Request) {
        unimplemented!()
    }
}

#[derive(Clone)]
struct LoggingVideoSink {
    peer_id: PeerId,
}

impl VideoSink for LoggingVideoSink {
    fn on_video_frame(&self, track_id: u32, frame: VideoFrame) {
        info!(
            "{:?}.{} received video frame size:{}x{}",
            self.peer_id,
            track_id,
            frame.width(),
            frame.height(),
        );
    }

    fn box_clone(&self) -> Box<dyn VideoSink> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct SignalingServer {
    actor: Actor<SignalingServerState>,
}

struct SignalingServerState {
    endpoints_by_peer_id: HashMap<PeerId, Vec<CallEndpoint>>,
}

impl SignalingServer {
    fn start(stopper: &Stopper) -> Result<Self> {
        Ok(Self {
            actor: Actor::start(stopper.clone(), move |_actor| {
                Ok(SignalingServerState {
                    endpoints_by_peer_id: HashMap::new(),
                })
            })?,
        })
    }

    fn add_endpoint(&self, endpoint: &CallEndpoint) {
        // To send across threads
        let peer_id = endpoint.peer_id.clone();
        let endpoint = endpoint.clone();

        self.actor.send(move |state| {
            state
                .endpoints_by_peer_id
                .entry(peer_id)
                .or_insert_with(|| Vec::with_capacity(1))
                .push(endpoint);
        });
    }

    fn send_signaling(
        &self,
        sender_id: &PeerId,
        sender_device_id: DeviceId,
        recipient_id: &PeerId,
        call_id: CallId,
        msg: signaling::Message,
    ) {
        // To send across threads
        let sender_id = sender_id.clone();
        let recipient_id = recipient_id.clone();

        // TODO: Get a better simulation by having the signaling put traffic in the Router.
        self.actor.send(move |state| {
            if let Some(endpoints) = state.endpoints_by_peer_id.get(&recipient_id) {
                for endpoint in endpoints {
                    endpoint.receive_signaling(&sender_id, sender_device_id, call_id, msg.clone());
                }
            } else {
                info!(
                    "Dropping signaling message because of unknown PeerId {:?}",
                    recipient_id
                );
            }
        });
    }
}

struct Log;

static LOG: Log = Log;

impl log::Log for Log {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!("{} - {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}
