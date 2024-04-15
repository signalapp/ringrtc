//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use anyhow::anyhow;
use bitvec::{
    bits,
    prelude::{BitSlice, LocalBits, Lsb0},
};
use log::*;
use ringrtc::{
    common::{
        actor::{Actor, Stopper},
        CallConfig, CallId, CallMediaType, DeviceId, Result,
    },
    core::{call_manager::CallManager, group_call, signaling},
    lite::{http, sfu::UserId},
    native::{
        CallState, CallStateHandler, GroupUpdate, GroupUpdateHandler, NativeCallContext,
        NativePlatform, PeerId, SignalingSender,
    },
    webrtc::{
        injectable_network,
        injectable_network::InjectableNetwork,
        media::{VideoSink, VideoSource},
        network::NetworkInterfaceType,
        peer_connection::AudioLevel,
        peer_connection_factory::{IceServer, PeerConnectionFactory},
        peer_connection_observer::NetworkRoute,
    },
};
use std::{
    collections::HashSet,
    io,
    iter::{Cycle, StepBy},
    net::{SocketAddr, UdpSocket},
    sync::mpsc::Sender,
    thread,
    time::Duration,
};

use crate::{server::Server, video::LoggingVideoSink, video::VideoInput};

/// Used to optionally sync operations with the user layer.
#[derive(Default)]
pub struct EventSync {
    /// Sends signal when the call state becomes ringing (for a callee).
    pub ringing: Option<Sender<()>>,
    /// Sends signal when the call is connected (from the user's pov, audio is flowing).
    /// This is not the ICE connection. Aka "in-call".
    pub connected: Option<Sender<()>>,
}

#[derive(Clone)]
pub struct CallEndpoint {
    // We keep a copy of these outside of the actor state
    // so we can know them in any thread.
    pub peer_id: PeerId,
    pub device_id: DeviceId,
    // There is probably a way to have a CallEndpoint without a thread,
    // but this is the easiest way to get around the nasty dependency cycle
    // of CallEndpoint -> CallManger -> NativePlatform -> CallEndpoint.
    // And it makes it pretty easy to schedule generation of video frames.
    actor: Actor<CallEndpointState>,
}

struct CallEndpointState {
    peer_id: PeerId,
    device_id: DeviceId,
    call_config: CallConfig,

    // How we send and receive signaling
    signaling_server: Box<dyn Server + Send + 'static>,
    // How we control calls
    call_manager: CallManager<NativePlatform>,
    call_context: NativeCallContext,
    // Events that can be used to signal the user layer (for latches there).
    event_sync: EventSync,

    // Keep a copy around to be able to schedule video frames
    actor: Actor<Self>,
    // Keep a copy around to be able to push out video frames
    outgoing_video_source: VideoSource,

    network: Option<InjectableNetwork>,
    socket: Option<UdpSocket>,
}

#[allow(clippy::too_many_arguments)]
impl CallEndpoint {
    pub fn start(
        peer_id: &str,
        device_id: DeviceId,
        call_config: CallConfig,
        hide_ip: bool,
        ice_server: &IceServer,
        signaling_server: Box<dyn Server + Send + 'static>,
        stopper: &Stopper,
        event_sync: EventSync,
        incoming_video_sink: Option<Box<dyn VideoSink>>,
        use_injectable_network: bool,
    ) -> Result<Self> {
        let peer_id = PeerId::from(peer_id);

        // To send across threads
        let ice_servers = vec![ice_server.clone()];

        Ok(Self::from_actor(
            peer_id.clone(),
            device_id,
            Actor::start(
                format!("endpoint-{peer_id}"),
                stopper.clone(),
                move |actor| {
                    // Constructing this is a funny way of getting a clone of the CallEndpoint
                    // on the actor's thread so we can have it in the actor's state so we can
                    // pass it to the NativePlatform/CallManager.
                    // This is a little weird, but it seems nicer than doing some kind of
                    // Option<CallManager> thing that we have to set later.
                    let endpoint = Self::from_actor(peer_id.clone(), device_id, actor.clone());

                    let mut pcf = PeerConnectionFactory::new(
                        &call_config.audio_config,
                        use_injectable_network,
                    )?;
                    info!(
                        "Audio playout devices: {:?}",
                        pcf.get_audio_playout_devices()
                    );
                    info!(
                        "Audio recording devices: {:?}",
                        pcf.get_audio_recording_devices()
                    );

                    // Set up signaling/state
                    signaling_server.register(&endpoint);
                    let signaling_sender = Box::new(endpoint.clone());
                    let should_assume_messages_sent = true; // cli doesn't support async sending yet.
                    let state_handler = Box::new(endpoint.clone());

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
                        ice_servers,
                        outgoing_audio_track,
                        outgoing_video_track,
                        incoming_video_sink.unwrap_or_else(|| {
                            Box::new(LoggingVideoSink {
                                peer_id: peer_id.clone(),
                            })
                        }),
                    );

                    let network = if use_injectable_network {
                        Some(pcf.injectable_network().expect("get Injectable Network"))
                    } else {
                        None
                    };

                    Ok(CallEndpointState {
                        peer_id,
                        device_id,
                        call_config,

                        signaling_server,
                        call_manager,
                        call_context,
                        event_sync,

                        actor,
                        outgoing_video_source,

                        network,
                        socket: None,
                    })
                },
            )?,
        ))
    }

    fn from_actor(peer_id: PeerId, device_id: DeviceId, actor: Actor<CallEndpointState>) -> Self {
        Self {
            peer_id,
            device_id,
            actor,
        }
    }

    pub fn add_deterministic_loss_network(&self, ip: &str, loss_rate: u8, packet_size_ms: i32) {
        let ip = ip.parse().expect("parse IP address");

        let mut deterministic_loss = DeterministicLoss::new(loss_rate, packet_size_ms, 10)
            .expect("parameters should be valid");

        self.actor.send(move |state| {
            if state.network.is_none() {
                error!("Error: Injectable network not set properly!");
                return;
            }
            let network = state.network.clone().unwrap();

            // The injectable network currently makes an assumption that socket ports will
            // start from 2001. We also make that assumption, and we are only going to give
            // one interface and no external servers for deterministic loss testing, so only
            // one ip/port should end up being used by each client for these tests.
            let local_socket_addr = SocketAddr::new(ip, 2001);
            let socket = UdpSocket::bind(local_socket_addr).expect("bind to address");
            let socket_as_sender = socket.try_clone().expect("clone the socket");
            // Connect the Injectable Network's send function to the UdpSocket.
            network.set_sender(Box::new(move |packet: injectable_network::Packet| {
                if let Err(err) = socket_as_sender.send_to(&packet.data, packet.dest) {
                    error!("Error: Sending packet to {}: {}", packet.dest, err);
                }
            }));

            // Adding it to the network causes the PeerConnections to learn about it through
            // the NetworkMonitor. For our tests, we just assume "wifi" for simplicity.
            network.add_interface("wifi", NetworkInterfaceType::Wifi, ip, 1);

            let socket_as_receiver = socket.try_clone().expect("clone the socket");
            state.socket = Some(socket);

            // Spawn a thread to maintain a receive loop on the socket. This waits for incoming
            // UDP packets until the network is stopped and the socket is set to non-blocking
            // mode. Then it will exit the loop and thread on a "Would Block" error.
            thread::spawn(move || {
                // 2K should be enough for any UDP packet.
                let mut buf = [0; 2048];
                loop {
                    match socket_as_receiver.recv_from(&mut buf) {
                        Ok((number_of_bytes, src_addr)) => {
                            if number_of_bytes > 1200 {
                                warn!(
                                    "Warning: {} bytes received for one packet!",
                                    number_of_bytes
                                );
                            }

                            if !deterministic_loss.next_is_loss() {
                                network.receive_udp(injectable_network::Packet {
                                    source: src_addr,
                                    dest: local_socket_addr,
                                    data: buf[..number_of_bytes].to_vec(),
                                });
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            break;
                        }
                        Err(err) => panic!("recv_from failed: {err}"),
                    }
                }
            });
        });
    }

    pub fn stop_network(&self) {
        self.actor.send(move |state| {
            if let Some(socket) = &state.socket {
                socket
                    .set_nonblocking(true)
                    .expect("set socket to non-blocking");
            }
        });
    }

    pub fn create_outgoing_call(
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
    pub fn receive_signaling(
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

    pub fn send_video<T: VideoInput + Send + 'static>(
        &self,
        input: T,
        interval: Duration,
        initial_delay: Duration,
    ) {
        fn send_one_frame_and_schedule_another<T: VideoInput + Send + 'static>(
            state: &mut CallEndpointState,
            mut input: T,
            interval: Duration,
        ) {
            state.outgoing_video_source.push_frame(input.next_frame());
            state.actor.send_delayed(interval, move |state| {
                send_one_frame_and_schedule_another(state, input, interval);
            });
        }
        self.actor.send_delayed(initial_delay, move |state| {
            send_one_frame_and_schedule_another(state, input, interval);
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
            state
                .signaling_server
                .send(sender_id, sender_device_id, &recipient_id, call_id, msg);
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
        _message: Vec<u8>,
        _urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        unimplemented!()
    }

    fn send_call_message_to_group(
        &self,
        _group_id: group_call::GroupId,
        _message: Vec<u8>,
        _urgency: group_call::SignalingMessageUrgency,
        _recipients_override: HashSet<UserId>,
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
                        state.call_config.clone(),
                        None,
                    )
                    .expect("proceed with call");
            } else if let CallState::Ringing = call_state {
                if let Some(ringing_sender) = &state.event_sync.ringing {
                    let _ = ringing_sender.send(());
                }
            } else if let CallState::Connected = call_state {
                if let Some(connected_sender) = &state.event_sync.connected {
                    let _ = connected_sender.send(());
                }
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

    fn handle_low_bandwidth_for_video(&self, remote_peer_id: &str, recovered: bool) -> Result<()> {
        info!(
            "Not enough bandwidth to send video reliably {} => {}: recovered: {}",
            self.peer_id, remote_peer_id, recovered
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

pub struct DeterministicLoss {
    pre_delay: u8,
    ignore_last_n: u8,
    current_loss_count: u8,
    loss_map_iter: Cycle<StepBy<bitvec::slice::Iter<'static, usize, LocalBits>>>,
}

impl DeterministicLoss {
    pub fn new(loss_rate: u8, packet_size_ms: i32, pre_delay: u8) -> Result<Self> {
        if loss_rate > 50 || loss_rate % 5 != 0 {
            return Err(anyhow!(
                "Loss rate must be less than 50% and a multiple of 5"
            ));
        }

        // This loss map represents 1,024 decisions for a loss rate of 50%. From this, we can
        // support any loss rate < 50% whilst keeping them all _somewhat_ aligned.
        let loss_map: &'static BitSlice = bits![static
            0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1,
            0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 0,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1,
            1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1,
            1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1,
            0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0,
            0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 1,
            0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1,
            0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1,
            1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1,
            1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1,
            1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1,
            0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0,
            1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1,
            0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0,
            0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0,
            1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1,
            1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1,
            0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1,
            0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1,
            1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1,
            1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1,
            1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1,
            1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1,
            0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1,
            0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1,
            1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1,
            1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0,
            0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0,
            0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0,
            0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0,
            1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1,
            0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 1, 1, 1, 1, 1, 0,
        ];

        // For different loss rates (< 50), we will ignore one or more for every 10 loss signals,
        // at the end of every 10 losses.
        let ignore_last_n = 10 - loss_rate / 5;

        // Iterate through the loss map. Based on the packet time, skip every n losses
        // to help keep different packet times _somewhat_ aligned.
        let packet_time_step = (packet_size_ms / 20) as usize;
        let loss_map_iter = loss_map.iter().step_by(packet_time_step).cycle();

        Ok(Self {
            pre_delay,
            ignore_last_n,
            current_loss_count: 0,
            loss_map_iter,
        })
    }

    pub fn next_is_loss(&mut self) -> bool {
        if self.pre_delay > 0 {
            self.pre_delay -= 1;
            return false;
        }

        if *self.loss_map_iter.next().expect("iterator has next()") {
            // The packet should be 'lost' as per the loss map.
            let ignore_loss = (10 - self.current_loss_count) <= self.ignore_last_n;

            self.current_loss_count += 1;
            if self.current_loss_count == 10 {
                self.current_loss_count = 0;
            }

            !ignore_loss
        } else {
            false
        }
    }
}
