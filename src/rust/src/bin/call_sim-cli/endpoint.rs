//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

mod direct_call_sim;
mod group_call_sim;

use direct_call_sim::DirectCall;
use log::*;
use ringrtc::{
    common::{
        actor::{Actor, Stopper},
        CallConfig, CallId, CallMediaType, DeviceId, Result,
    },
    core::{call_manager::CallManager, group_call, signaling},
    lite::{http, sfu::UserId},
    native::{NativeCallContext, NativePlatform, PeerId, SignalingSender},
    webrtc::{
        media::{AudioTrack, VideoSink, VideoSource, VideoTrack},
        peer_connection_factory::{AudioConfig, IceServer, PeerConnectionFactory},
    },
};
use std::{collections::HashSet, sync::mpsc::Sender, time::Duration};

use crate::{
    network::DeterministicLossNetwork,
    server::Server,
    video::{LoggingVideoSink, VideoInput},
};
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

    // How we send and receive signaling
    signaling_server: Box<dyn Server + Send + 'static>,
    // How we control calls
    call_manager: CallManager<NativePlatform>,
    // Events that can be used to signal the user layer (for latches there).
    event_sync: EventSync,

    // Keep a copy around to be able to schedule video frames
    actor: Actor<Self>,
    // Keep a copy around to be able to push out video frames
    outgoing_video_source: VideoSource,
    outgoing_video_track: VideoTrack,
    outgoing_audio_track: AudioTrack,
    incoming_video_sink: Box<dyn VideoSink>,

    network: Option<DeterministicLossNetwork>,

    direct_call: Option<DirectCall>,
}

#[allow(clippy::too_many_arguments)]
impl CallEndpoint {
    pub fn new(
        peer_id: &str,
        device_id: DeviceId,
        audio_config: &AudioConfig,
        signaling_server: Box<dyn Server + Send + 'static>,
        stopper: &Stopper,
        event_sync: EventSync,
        incoming_video_sink: Option<Box<dyn VideoSink>>,
        use_injectable_network: bool,
    ) -> Result<Self> {
        let peer_id = PeerId::from(peer_id);
        let audio_config = audio_config.clone();

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

                    let mut pcf =
                        PeerConnectionFactory::new(&audio_config, use_injectable_network)?;
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

                    // Initialize media. We'll use the same for each call.
                    let outgoing_audio_track = pcf.create_outgoing_audio_track()?;
                    let outgoing_video_source = pcf.create_outgoing_video_source()?;
                    let outgoing_video_track =
                        pcf.create_outgoing_video_track(&outgoing_video_source)?;
                    let incoming_video_sink = incoming_video_sink.unwrap_or_else(|| {
                        Box::new(LoggingVideoSink {
                            peer_id: peer_id.clone(),
                        })
                    });

                    let network = if use_injectable_network {
                        Some(DeterministicLossNetwork::new(
                            pcf.injectable_network().expect("get Injectable Network"),
                        ))
                    } else {
                        None
                    };

                    Ok(CallEndpointState {
                        peer_id,
                        device_id,

                        signaling_server,
                        call_manager,
                        event_sync,

                        actor,

                        outgoing_video_source,
                        outgoing_video_track,
                        outgoing_audio_track,
                        incoming_video_sink,

                        network,

                        direct_call: None,
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

    /// Initializes state used in direct calls
    pub fn init_direct_settings(
        &mut self,
        hide_ip: bool,
        ice_server: &IceServer,
        call_config: CallConfig,
    ) {
        // To send across threads
        let ice_servers = vec![ice_server.clone()];

        self.actor.send(move |state| {
            let call_context = NativeCallContext::new(
                hide_ip,
                ice_servers,
                state.outgoing_audio_track.clone(),
                state.outgoing_video_track.clone(),
                state.incoming_video_sink.clone(),
            );

            state.direct_call = Some(DirectCall::new(call_context, call_config));
        });
    }

    pub fn add_deterministic_loss_network(&self, ip: &str, loss_rate: u8, packet_size_ms: i32) {
        let ip = ip.to_owned();
        self.actor.send(move |state| {
            if let Some(ref mut network) = state.network {
                network.add_deterministic_loss(ip, loss_rate, packet_size_ms);
            } else {
                error!("Error: Injectable network not set properly!");
            }
        });
    }

    pub fn stop_network(&self) {
        self.actor.send(move |state| {
            if let Some(ref network) = state.network {
                network.stop_network();
            }
        });
    }

    pub fn create_outgoing_direct_call(
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

    pub fn accept_incoming_direct_call(&self, call_id: CallId) {
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

impl http::Delegate for CallEndpoint {
    fn send_request(&self, _request_id: u32, _request: http::Request) {
        unimplemented!()
    }
}
