//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use log::{debug, info};

use ringrtc::{
    common::{
        AnswerParameters,
        CallId,
        CallMediaType,
        ConnectionId,
        DeviceId,
        FeatureLevel,
        OfferParameters,
        Result,
    },
    core::call_manager::CallManager,
    native::{
        CallState,
        CallStateHandler,
        NativeCallContext,
        NativePlatform,
        PeerId,
        SignalingMessage,
        SignalingSender,
    },
    simnet::{
        actor::{Actor, Stopper},
        router,
        router::{LinkConfig, Router},
        units::DataRate,
    },
    webrtc::{
        injectable_network,
        injectable_network::{InjectableNetwork, NetworkInterfaceType},
        media::{VideoFrame, VideoSink, VideoSource},
        peer_connection_factory::{Certificate, IceServer, PeerConnectionFactory},
    },
};
use std::{collections::HashMap, thread, time::Duration};

fn main() {
    log::set_logger(&LOG).expect("set logger");
    log::set_max_level(log::LevelFilter::Debug);
    // For detailed logs, uncomment this line.
    ringrtc::webrtc::logging::set_logger(log::LevelFilter::Debug);

    let hide_ip = false;
    // TODO: Real STUN/TURN servers.
    let ice_server = IceServer::new(
        "".to_string(), // username
        "".to_string(), // password
        vec![],         //  vec!["stun:stun.l.google.com".to_string()],
    );
    let enable_forking = true;
    let stopper = Stopper::new();
    let signaling_server = SignalingServer::new(&stopper);
    let router = Router::new(&stopper);
    let good_link = LinkConfig {
        delay_min:                 Duration::from_millis(10),
        delay_max:                 Duration::from_millis(20),
        loss_probabilty:           0.00,
        repeated_loss_probability: 0.00,
        rate:                      DataRate::from_mbps(5),
        queue_size:                DataRate::from_mbps(5) * Duration::from_millis(500),
    };
    let bad_link = LinkConfig {
        delay_min:                 Duration::from_millis(100),
        delay_max:                 Duration::from_millis(200),
        loss_probabilty:           0.005,
        repeated_loss_probability: 0.70,
        rate:                      DataRate::from_kbps(256),
        queue_size:                DataRate::from_kbps(256) * Duration::from_secs(500),
    };

    let caller = CallEndpoint::new(
        "caller",
        1 as DeviceId,
        hide_ip,
        &ice_server,
        enable_forking,
        &signaling_server,
        &router,
        &stopper,
    );
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

    let callee = CallEndpoint::new(
        "callee",
        1 as DeviceId,
        hide_ip,
        &ice_server,
        enable_forking,
        &signaling_server,
        &router,
        &stopper,
    );
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
            let callee = CallEndpoint::new(
                "callee",
                device_id as DeviceId,
                hide_ip,
                &ice_server,
                enable_forking,
                &signaling_server,
                &router,
                &stopper,
            );
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
    peer_id:   PeerId,
    device_id: DeviceId,
    // There is probably a way to have a CallEndpoint without a thread,
    // but this is the easiest way to get around the nasty dependency cycle
    // of CallEndpoint -> CallManger -> NativePlatform -> CallEndpoint.
    // And it makes it pretty easy to schedule generation of video frames.
    actor:     Actor<CallEndpointState>,
}

struct CallEndpointState {
    peer_id:        PeerId,
    device_id:      DeviceId,
    enable_forking: bool,

    // How we send and receive signaling
    signaling_server: SignalingServer,
    // How we tell PeerConnections there are network interfaces and inject
    // packet into them
    network:          InjectableNetwork,
    // How we simulate packets being routed around
    router:           Router,
    // How we control calls
    call_manager:     CallManager<NativePlatform>,
    call_context:     NativeCallContext,

    // Keep a copy around to be able to schedule video frames
    actor:          Actor<Self>,
    // Keep a copy around to be able to push out video frames
    outgoing_video: VideoSource,
}

impl CallEndpoint {
    pub fn new(
        peer_id: &str,
        device_id: DeviceId,
        hide_ip: bool,
        ice_server: &IceServer,
        enable_forking: bool,
        signaling_server: &SignalingServer,
        router: &Router,
        stopper: &Stopper,
    ) -> Self {
        let peer_id = PeerId::from(peer_id);

        // To send across threads
        let ice_server = ice_server.clone();
        let signaling_server: SignalingServer = signaling_server.clone();
        let router = router.clone();

        Self::from_actor(
            peer_id.clone(),
            device_id,
            Actor::new(stopper.clone(), move |actor| {
                // Constructing this is a funny way of getting a clone of the CallEndpoint
                // on the actor's thread so we can have it in the actor's state so we can
                // pass it to the NativePlatform/CallManager.
                // This is a little weird, but it seems nicer than doing some kind of
                // Option<CallManager> thing that we have to set later.
                let endpoint = Self::from_actor(peer_id.clone(), device_id, actor.clone());

                // Set up packet flow
                let use_injectable_network = true;
                let pcf = PeerConnectionFactory::new(use_injectable_network)
                    .expect("create PeerConnectionFactory");
                let network = pcf.injectable_network().expect("get Injectable Network");
                let router_as_sender = router.clone();
                network.set_sender(Box::new(move |packet: injectable_network::Packet| {
                    router_as_sender.send_packet(router::Packet {
                        source: packet.source,
                        dest:   packet.dest,
                        data:   packet.data,
                    });
                }));

                // Set up signaling/state
                signaling_server.add_endpoint(&endpoint);
                let state_handler = Box::new(endpoint.clone());
                let signaling_sender = Box::new(endpoint.clone());
                let incoming_video_sink = Box::new(endpoint.clone());
                let platform = NativePlatform::new(
                    pcf.clone(),
                    state_handler,
                    signaling_sender,
                    incoming_video_sink,
                );
                let call_manager = CallManager::new(platform).expect("create CallManager");

                // And a CallContext.  We'll use the same context for each call.
                let cert = Certificate::generate().expect("generate cert");
                let outgoing_audio = pcf
                    .create_outgoing_audio_track()
                    .expect("create AudioTrack");
                let outgoing_video = pcf
                    .create_outgoing_video_source()
                    .expect("create VideoSource");
                let call_context = NativeCallContext::new(
                    cert,
                    hide_ip,
                    ice_server,
                    outgoing_audio,
                    outgoing_video.clone(),
                );

                CallEndpointState {
                    peer_id,
                    device_id,
                    enable_forking,

                    signaling_server,
                    network,
                    router,
                    call_manager,
                    call_context,

                    actor,
                    outgoing_video,
                }
            }),
        )
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
            state.router.add_interface(
                ip,
                send_config,
                receive_config,
                Box::new(move |packet: router::Packet| {
                    network_as_receiver.receive_udp(injectable_network::Packet {
                        source: packet.source,
                        dest:   packet.dest,
                        data:   packet.data,
                    });
                }),
            );

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
        msg: SignalingMessage,
    ) {
        // To send across threads
        let sender_id = sender_id.clone();

        self.actor.send(move |state| {
            let cm = &mut state.call_manager;
            let connection_id = ConnectionId::new(call_id, sender_device_id);
            match msg {
                SignalingMessage::Offer(media_type, local_device_id, offer) => {
                    let local_is_primary = state.device_id == 1;
                    cm.received_offer(
                        sender_id,
                        connection_id,
                        OfferParameters::new(
                            offer,
                            0,
                            media_type,
                            local_device_id,
                            FeatureLevel::MultiRing,
                            local_is_primary,
                        ),
                    )
                    .expect("receive offer");
                }
                SignalingMessage::Answer(answer) => {
                    cm.received_answer(
                        connection_id,
                        AnswerParameters::new(answer, FeatureLevel::MultiRing),
                    )
                    .expect("received answer");
                }
                SignalingMessage::IceCandidates(ice_candidates) => {
                    cm.received_ice_candidates(connection_id, &ice_candidates)
                        .expect("received ice candidates");
                }
                SignalingMessage::Hangup(description, _use_legacy) => {
                    cm.received_hangup(connection_id, description)
                        .expect("received hangup");
                }
                SignalingMessage::Busy => {
                    cm.received_busy(connection_id).expect("received busy");
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
                .outgoing_video
                .push_frame(VideoFrame::from_rgba(width, height, &rgba_data));
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
        recipient_id: &PeerId,
        connection_id: ConnectionId,
        broadcast: bool,
        msg: SignalingMessage,
    ) -> Result<()> {
        // To send across threads
        let recipient_id = recipient_id.clone();

        self.actor.send(move |state| {
            let sender_id = &state.peer_id;
            let sender_device_id = state.device_id;
            // Note that the DeviceId in the ConnectionId is for the
            // remote device, which isn't relevant here.
            let call_id = connection_id.call_id();
            state.signaling_server.send_signaling(
                &sender_id,
                sender_device_id,
                &recipient_id,
                call_id,
                broadcast,
                msg,
            );
            state
                .call_manager
                .message_sent(connection_id.call_id())
                .expect("signaling message sent");
        });
        Ok(())
    }
}

impl CallStateHandler for CallEndpoint {
    fn handle_call_state(&self, remote_peer_id: &PeerId, call_state: CallState) -> Result<()> {
        info!(
            "State change in call from {}.{} to {}: now {:?}",
            self.peer_id, self.device_id, remote_peer_id, call_state
        );

        self.actor.send(move |state| {
            if let CallState::Incoming(call_id, _call_media_type)
            | CallState::Outgoing(call_id, _call_media_type) = call_state
            {
                // It's easier just to hardcode this, but I suppose we could make it part of the state if we wanted to.
                let remote_device_ids = (1u32..=6u32).collect();
                state
                    .call_manager
                    .proceed(
                        call_id,
                        state.call_context.clone(),
                        remote_device_ids,
                        state.enable_forking,
                    )
                    .expect("proceed with outgoing call");
            }
        });
        Ok(())
    }

    fn handle_remote_video_state(&self, remote_peer_id: &PeerId, enabled: bool) -> Result<()> {
        info!(
            "Video State for {} => {}: {}",
            self.peer_id, remote_peer_id, enabled
        );
        Ok(())
    }
}

impl VideoSink for CallEndpoint {
    fn set_enabled(&self, enabled: bool) {
        if enabled {
            info!("Here comes some video frames")
        } else {
            info!("No more video frames")
        }
    }

    fn on_video_frame(&self, frame: VideoFrame) {
        info!(
            "{:?} received video frame size:{}x{}",
            self.peer_id,
            frame.width(),
            frame.height(),
        );
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
    fn new(stopper: &Stopper) -> Self {
        Self {
            actor: Actor::new(stopper.clone(), move |_actor| SignalingServerState {
                endpoints_by_peer_id: HashMap::new(),
            }),
        }
    }

    fn add_endpoint(&self, endpoint: &CallEndpoint) {
        // To send across threads
        let peer_id = endpoint.peer_id.clone();
        let endpoint = endpoint.clone();

        self.actor.send(move |state| {
            state
                .endpoints_by_peer_id
                .entry(peer_id)
                .or_insert(Vec::with_capacity(1))
                .push(endpoint);
        });
    }

    fn send_signaling(
        &self,
        sender_id: &PeerId,
        sender_device_id: DeviceId,
        recipient_id: &PeerId,
        call_id: CallId,
        _broadcast: bool,
        msg: SignalingMessage,
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
