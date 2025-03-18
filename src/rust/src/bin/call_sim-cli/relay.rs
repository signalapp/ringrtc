//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use log::*;
use ringrtc::{
    common::{
        actor::{Actor, Stopper},
        CallId, CallMediaType, DeviceId, Result,
    },
    core::signaling::{self, HangupType, Ice, IceCandidate, Message},
    native::PeerId,
};
use tokio::runtime::{Builder, Runtime};
use tonic::transport::Channel;
use tower::timeout::Timeout;

use crate::endpoint::CallEndpoint;

// Modules for the calling service, from protobufs compiled by tonic.
pub mod calling {
    #![allow(clippy::derive_partial_eq_without_eq, clippy::enum_variant_names)]
    call_protobuf::include_call_sim_proto!();
}
use calling::{
    call_message, signaling_relay_client::SignalingRelayClient, CallMessage, Registration,
    RelayMessage,
};

/// A 'server' is any server that can relay signaling messages between clients.
pub trait SignalingRelay {
    /// Registers an endpoint with the server.
    fn register(&self, endpoint: &CallEndpoint);
    /// Sends a signaling message to another client. Used for 1:1 calls
    fn send_signaling(
        &self,
        sender_id: &PeerId,
        sender_device_id: DeviceId,
        recipient_id: &PeerId,
        call_id: CallId,
        msg: Message,
    );

    /// Sends a message to another client. Used for group calls
    fn send_call_message(
        &self,
        sender_id: &PeerId,
        sender_device_id: DeviceId,
        recipient_id: Vec<u8>,
        opaque_message: Vec<u8>,
    );
}

#[derive(Clone)]
pub struct CallSimSignalingRelayClient {
    actor: Actor<SignalingRelayClientState>,
}

struct SignalingRelayClientState {
    client: SignalingRelayClient<Timeout<Channel>>,
    rt: Runtime,
    /// Sends signal when the client has been registered.
    pub registered: Option<Sender<i32>>,
}

impl CallSimSignalingRelayClient {
    pub fn new(stopper: &Stopper, registered: Option<Sender<i32>>) -> Result<Self> {
        let rt = Builder::new_multi_thread().enable_all().build()?;

        // Loop forever trying to connect to the server...
        let client = loop {
            let channel = rt.block_on(
                Channel::from_static("http://172.28.0.250:8080")
                    .connect_timeout(Duration::from_millis(500))
                    .connect(),
            );

            if let Ok(channel) = channel {
                // Make sure all requests have a reasonable timeout.
                let timeout_channel = Timeout::new(channel, Duration::from_millis(1000));
                break SignalingRelayClient::new(timeout_channel);
            }

            // Spam the logs if we can't connect to the signaling server.
            warn!("RelayServer: Can't connect: {:?}", channel);
        };

        Ok(Self {
            actor: Actor::start("RelayServer", stopper.clone(), move |_actor| {
                Ok(SignalingRelayClientState {
                    client,
                    rt,
                    registered,
                })
            })?,
        })
    }

    // Retry the request up to 2 times.
    fn send_relay_message(&self, request: RelayMessage) {
        self.actor.send(move |state| {
            for attempt in 1..=3 {
                info!(
                    "Attempting to send relay message attempt {} of 3: {:?}",
                    attempt, request
                );
                let request = tonic::Request::new(request.clone());
                let response = state.rt.block_on(state.client.send(request));
                if response.is_err() {
                    if attempt == 3 {
                        error!(
                            "RelayServer: Could not send send_relay_message() message: {:?}",
                            response
                        );
                    } else {
                        warn!(
                            "RelayServer: Problem sending send_relay_message() message: {:?}",
                            response
                        );
                    }
                } else {
                    info!("Sucessfully sent relay message");
                    return;
                }
            }
        });
    }
}

impl SignalingRelay for CallSimSignalingRelayClient {
    fn register(&self, endpoint: &CallEndpoint) {
        // To send across threads
        let relay_id = endpoint.relay_id();
        let endpoint = endpoint.clone();

        self.actor.send(move |state| {
            let request = tonic::Request::new(Registration {
                client: relay_id.clone(),
            });
            let mut response = state.rt.block_on(state.client.register(request));

            // Retry the request up to 2 times.
            for _ in 1..=2 {
                if response.is_err() {
                    warn!(
                        "RelayServer: Problem sending register() message: {:?}",
                        response
                    );

                    // TODO: Can't clone tonic::Request?
                    // See https://github.com/hyperium/tonic/issues/694#issuecomment-1148598782
                    let request = tonic::Request::new(Registration {
                        client: relay_id.clone(),
                    });
                    response = state.rt.block_on(state.client.register(request));
                } else {
                    info!("RelayServer: Registered successfully as {}!", relay_id);
                    if let Some(registered_sender) = &state.registered {
                        let _ = registered_sender.send(0);
                    }
                    break;
                }
            }

            if let Ok(response) = response {
                let mut stream = response.into_inner();

                state.rt.spawn(async move {
                    loop {
                        // Plus I guess we need a way to get out of this loop?
                        match stream.message().await {
                            Ok(Some(relay_message)) => {
                                info!(
                                    "register(): Message from {}:{}",
                                    relay_message.client, relay_message.device_id
                                );

                                if let Some(call_message) = relay_message.call_message {
                                    info!("register(): {:?}", call_message);
                                    if relay_message.opaque_message.is_some() {
                                        warn!("Received both a call message and signaling message");
                                    }

                                    // Even though the payload can have more, we'll always just
                                    // have one message type at a time in the payload.
                                    let received: Option<(CallId, Message)> = if let Some(offer) =
                                        call_message.offer
                                    {
                                        let call_id = offer.id;
                                        let call_media_type = CallMediaType::from_i32(offer.r#type);
                                        let msg =
                                            signaling::Offer::new(call_media_type, offer.opaque);

                                        match msg {
                                            Ok(offer) => {
                                                Some((CallId::new(call_id), Message::Offer(offer)))
                                            }
                                            Err(err) => {
                                                error!(
                                                    "register(): Error parsing Offer: {:?}",
                                                    err
                                                );
                                                None
                                            }
                                        }
                                    } else if let Some(answer) = call_message.answer {
                                        let call_id = answer.id;
                                        let msg = signaling::Answer::new(answer.opaque);

                                        match msg {
                                            Ok(answer) => Some((
                                                CallId::new(call_id),
                                                Message::Answer(answer),
                                            )),
                                            Err(err) => {
                                                error!(
                                                    "register(): Error parsing Answer: {:?}",
                                                    err
                                                );
                                                None
                                            }
                                        }
                                    } else if !call_message.ice_update.is_empty() {
                                        // Set a dummy, we already know we'll get one in the loop.
                                        let mut call_id = 0;

                                        let candidates = call_message
                                            .ice_update
                                            .iter()
                                            .map(|candidate| {
                                                call_id = candidate.id; // They are all the same.
                                                IceCandidate::new(candidate.opaque.clone())
                                            })
                                            .collect();

                                        let msg = Ice { candidates };

                                        Some((CallId::new(call_id), Message::Ice(msg)))
                                    } else if let Some(busy) = call_message.busy {
                                        let call_id = busy.id;
                                        Some((CallId::new(call_id), Message::Busy))
                                    } else if let Some(hangup) = call_message.hangup {
                                        let call_id = hangup.id;
                                        if let Some(hangup_type) =
                                            HangupType::from_i32(hangup.r#type)
                                        {
                                            let msg = signaling::Hangup::from_type_and_device_id(
                                                hangup_type,
                                                hangup.device_id,
                                            );
                                            Some((CallId::new(call_id), Message::Hangup(msg)))
                                        } else {
                                            error!("register(): Invalid HangupType");
                                            None
                                        }
                                    } else {
                                        None
                                    };

                                    if let Some((call_id, msg)) = received {
                                        endpoint.receive_signaling(
                                            &relay_message.client,
                                            relay_message.device_id,
                                            call_id,
                                            msg,
                                        );
                                    }
                                } else if let Some(opaque_message) = relay_message.opaque_message {
                                    info!("register(): received opaque message");
                                    endpoint.receive_call_message(
                                        &relay_message.client,
                                        relay_message.device_id,
                                        Instant::now(),
                                        opaque_message.clone(),
                                    );
                                }
                            }
                            Ok(None) => {
                                warn!("register(): Received Message: None");
                                // break from here also, right?
                                break;
                            }
                            Err(err) => {
                                error!("register(): {}", err);
                                break;
                            }
                        }
                    }
                });
            } else {
                error!(
                    "RelayServer: Could not send register() message: {:?}",
                    response
                );
            }
        });
    }

    fn send_signaling(
        &self,
        sender_id: &PeerId,
        sender_device_id: DeviceId,
        recipient_id: &PeerId,
        call_id: CallId,
        msg: Message,
    ) {
        info!("send(): Message to {}", recipient_id);

        // To send across threads
        let sender_id = sender_id.clone();

        let call_message = match msg {
            Message::Offer(offer) => CallMessage {
                offer: Some(call_message::Offer {
                    id: call_id.as_u64(),
                    r#type: offer.call_media_type as i32,
                    opaque: offer.opaque,
                }),
                ..Default::default()
            },
            Message::Answer(answer) => CallMessage {
                answer: Some(call_message::Answer {
                    id: call_id.as_u64(),
                    opaque: answer.opaque,
                }),
                ..Default::default()
            },
            Message::Ice(ice) => {
                let ice_update = ice
                    .candidates
                    .iter()
                    .map(|candidate| call_message::IceUpdate {
                        id: call_id.as_u64(),
                        opaque: candidate.clone().opaque,
                    })
                    .collect();
                CallMessage {
                    ice_update,
                    ..Default::default()
                }
            }
            Message::Hangup(hangup) => {
                let (hangup_type, device_id) = hangup.to_type_and_device_id();
                let device_id = device_id.unwrap_or_default();
                CallMessage {
                    hangup: Some(call_message::Hangup {
                        id: call_id.as_u64(),
                        r#type: hangup_type as i32,
                        device_id,
                    }),
                    ..Default::default()
                }
            }
            Message::Busy => CallMessage {
                busy: Some(call_message::Busy {
                    id: call_id.as_u64(),
                }),
                ..Default::default()
            },
        };

        self.send_relay_message(RelayMessage {
            client: sender_id.clone(),
            device_id: sender_device_id,
            call_message: call_message.clone().into(),
            opaque_message: None,
        });
    }

    fn send_call_message(
        &self,
        sender_id: &PeerId,
        sender_device_id: DeviceId,
        recipient_id: Vec<u8>,
        opaque_message: Vec<u8>,
    ) {
        info!("send_opaque_message(): Message to {:?}", recipient_id);

        self.send_relay_message(RelayMessage {
            client: sender_id.clone(),
            device_id: sender_device_id,
            call_message: None,
            opaque_message: Some(opaque_message.clone()),
        });
    }
}
