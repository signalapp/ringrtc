//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use lazy_static::lazy_static;
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::common::{BandwidthMode, CallId, CallMediaType, DeviceId, FeatureLevel, Result};
use crate::core::call_manager::CallManager;
use crate::core::signaling;
use crate::native::{
    CallState,
    CallStateHandler,
    EndReason,
    NativeCallContext,
    NativePlatform,
    PeerId,
    SignalingSender,
};
use crate::webrtc::media::{AudioTrack, VideoFrame, VideoSink, VideoSource};
use crate::webrtc::peer_connection_factory::{Certificate, IceServer, PeerConnectionFactory};

use neon::prelude::*;

const ENABLE_LOGGING: bool = true;

/// A structure for packing the contents of log messages.
pub struct LogMessage {
    level:   i8,
    file:    String,
    line:    u32,
    message: String,
}

// We store the log messages in a queue to be given to JavaScript when it polls so
// it can show the messages in the console.
// I'd like to use a channel, but they seem difficult to create statically.
static LOG: Log = Log;
lazy_static! {
    static ref LOG_MESSAGES: Mutex<VecDeque<LogMessage>> = Mutex::new(VecDeque::new());
}

struct Log;

impl log::Log for Log {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let message = LogMessage {
                level:   record.level() as i8,
                file:    record.file().unwrap().to_string(),
                line:    record.line().unwrap(),
                message: format!("{}", record.args()),
            };

            let mut messages = LOG_MESSAGES.lock().expect("lock log messages");
            messages.push_back(message);
        }
    }

    fn flush(&self) {}
}

// When JavaScripts polls, we want everything to go through a common queue that
// combines all the things we want to "push" (through polling) to it.
// (Well, everything except log messages.  See above as to why).
pub enum Event {
    // The JavaScript should send the following signaling message to the given
    // PeerId in context of the given CallId.  If the DeviceId is None, then
    // broadcast to all devices of that PeerId.
    SendSignaling(PeerId, Option<DeviceId>, CallId, signaling::Message),
    // The call with the given remote PeerId has changed state.
    // We assume only one call per remote PeerId at a time.
    CallState(PeerId, CallState),
    // The state of the remote video (whether enabled or not)
    // Like call state, we ID the call by PeerId and assume there is only one.
    RemoteVideoState(PeerId, bool),
}

impl SignalingSender for Sender<Event> {
    fn send_signaling(
        &self,
        recipient_id: &PeerId,
        call_id: CallId,
        receiver_device_id: Option<DeviceId>,
        msg: signaling::Message,
    ) -> Result<()> {
        self.send(Event::SendSignaling(
            recipient_id.clone(),
            receiver_device_id,
            call_id,
            msg,
        ))?;
        Ok(())
    }
}

impl CallStateHandler for Sender<Event> {
    fn handle_call_state(&self, remote_peer_id: &PeerId, call_state: CallState) -> Result<()> {
        self.send(Event::CallState(remote_peer_id.clone(), call_state))?;
        Ok(())
    }

    fn handle_remote_video_state(&self, remote_peer_id: &PeerId, enabled: bool) -> Result<()> {
        self.send(Event::RemoteVideoState(remote_peer_id.clone(), enabled))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct OneFrameBuffer {
    state: Arc<Mutex<OneFrameBufferState>>,
}

struct OneFrameBufferState {
    enabled: bool,
    frame:   Option<VideoFrame>,
}

impl VideoSink for OneFrameBuffer {
    fn set_enabled(&self, enabled: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.enabled = enabled;
            if !enabled {
                state.frame = None;
            }
        }
    }

    fn on_video_frame(&self, frame: VideoFrame) {
        if let Ok(mut state) = self.state.lock() {
            if state.enabled {
                state.frame = Some(frame);
            }
        }
    }
}

impl OneFrameBuffer {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(OneFrameBufferState {
                enabled: false,
                frame:   None,
            })),
        }
    }

    fn pop(&self) -> Option<VideoFrame> {
        if let Ok(mut state) = self.state.lock() {
            state.frame.take()
        } else {
            None
        }
    }
}

pub struct CallEndpoint {
    call_manager: CallManager<NativePlatform>,

    events_receiver:  Receiver<Event>,
    // This is what we use to control mute/not.
    // It should probably be per-call, but for now it's easier to have only one.
    outgoing_audio:   AudioTrack,
    // This is what we use to control mute/not.
    // It should probably be per-call, but for now it's easier to have only one.
    outgoing_video:   VideoSource,
    // Pulled out by receiveVideoFrame
    unrendered_frame: OneFrameBuffer,

    peer_connection_factory: PeerConnectionFactory,
}

impl CallEndpoint {
    fn new() -> Result<Self> {
        let (events_sender, events_receiver) = channel::<Event>();

        let use_injectable_network = false;
        let peer_connection_factory = PeerConnectionFactory::new(use_injectable_network)?;
        let outgoing_audio = peer_connection_factory.create_outgoing_audio_track()?;
        let outgoing_video = peer_connection_factory.create_outgoing_video_source()?;
        let unrendered_frame = OneFrameBuffer::new();
        let platform = NativePlatform::new(
            false, // Use async notification from app to send next message.
            peer_connection_factory.clone(),
            // All the things get pumped into the same event channel,
            // but the NativePlatform doesn't know that.
            Box::new(events_sender.clone()),
            Box::new(events_sender.clone()),
            Box::new(unrendered_frame.clone()),
        );
        let call_manager = CallManager::new(platform)?;

        Ok(Self {
            call_manager,
            events_receiver,
            outgoing_audio,
            outgoing_video,
            unrendered_frame,
            peer_connection_factory,
        })
    }
}

fn get_call_id_arg(cx: &mut CallContext<JsCallManager>, i: i32) -> CallId {
    let obj = cx.argument::<JsObject>(i).expect("Get CallId argument");
    let high = obj
        .get(cx, "high")
        .expect("Get CallId.high")
        .downcast::<JsNumber>()
        .expect("CallId.high is a number")
        .value() as u64;
    let low = obj
        .get(cx, "low")
        .expect("Get CallId.low")
        .downcast::<JsNumber>()
        .expect("CallId.low is a number")
        .value() as u64;
    let call_id = CallId::new(((high << 32) & 0xFFFFFFFF00000000) | (low & 0xFFFFFFFF));
    debug!(
        "call_id: {} converted from (high: {} low: {})",
        call_id, high, low
    );
    call_id
}

fn create_call_id_arg<'a>(
    cx: &mut CallContext<'a, JsCallManager>,
    call_id: CallId,
) -> Handle<'a, JsValue> {
    let high = cx.number(((call_id.as_u64() >> 32) & 0xFFFFFFFF) as f64);
    let low = cx.number((call_id.as_u64() & 0xFFFFFFFF) as f64);
    let unsigned = cx.boolean(true);
    let obj = cx.empty_object();
    obj.set(cx, "high", high).expect("set callId.high");
    obj.set(cx, "low", low).expect("set callId.low");
    obj.set(cx, "unsigned", unsigned)
        .expect("set callId.unsigned");
    obj.upcast()
}

declare_types! {
    pub class JsCallManager for CallEndpoint {
        init(mut cx) {
            if ENABLE_LOGGING {
                log::set_logger(&LOG).expect("set logger");
                log::set_max_level(log::LevelFilter::Info);

                // Show WebRTC logs via application Logger while debugging.
                #[cfg(debug_assertions)]
                crate::webrtc::logging::set_logger(log::LevelFilter::Debug);

                #[cfg(not(debug_assertions))]
                crate::webrtc::logging::set_logger(log::LevelFilter::Off);
            }

            debug!("JsCallManager.init()");
            CallEndpoint::new().or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))
        }

        method createOutgoingCall(mut cx) {
            let peer_id = cx.argument::<JsString>(0)?.value() as PeerId;
            let video_enabled = cx.argument::<JsBoolean>(1)?.value();
            let local_device_id = cx.argument::<JsNumber>(2)?.value() as DeviceId;

            let media_type = if video_enabled {
                CallMediaType::Video
            } else {
                CallMediaType::Audio
            };

            debug!("JsCallManager.call({}, {}, {})", peer_id, media_type, local_device_id);
            let mut this = cx.this();

            let call_id = CallId::random();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.create_outgoing_call(peer_id, call_id, media_type, local_device_id)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(create_call_id_arg(&mut cx, call_id))
        }

        method proceed(mut cx) {
            let call_id = get_call_id_arg(&mut cx, 0);
            let ice_server_username = cx.argument::<JsString>(1)?.value();
            let ice_server_password = cx.argument::<JsString>(2)?.value();
            let js_ice_server_urls = cx.argument::<JsArray>(3)?;
            let hide_ip = cx.argument::<JsBoolean>(4)?.value();

            let mut ice_server_urls = Vec::with_capacity(js_ice_server_urls.len() as usize);
            for i in 0..js_ice_server_urls.len() {
                let url: String = js_ice_server_urls.get(&mut cx, i as u32)?.downcast::<JsString>().expect("ICE server URLs are strings").value();
                ice_server_urls.push(url);
            }

            let ice_server = IceServer::new(
                ice_server_username,
                ice_server_password,
                ice_server_urls);
            debug!("JsCallManager.proceed({}, {:?}, {})", call_id, ice_server, hide_ip);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                let certificate = Certificate::generate()?;
                let call_context = NativeCallContext::new(
                    certificate,
                    hide_ip,
                    ice_server,
                    cm.outgoing_audio.clone(),
                    cm.outgoing_video.clone());
                cm.call_manager.proceed(call_id, call_context)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method accept(mut cx) {
            let call_id = get_call_id_arg(&mut cx, 0);
            debug!("JsCallManager.accept({})", call_id);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.accept_call(call_id)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method ignore(mut cx) {
            let call_id = get_call_id_arg(&mut cx, 0);
            debug!("JsCallManager.ignore({})", call_id);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.drop_call(call_id)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method hangup(mut cx) {
            debug!("JsCallManager.hangup()");

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.hangup()?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method signalingMessageSent(mut cx) {
            let call_id = get_call_id_arg(&mut cx, 0);
            debug!("JsCallManager.signalingMessageSent({})", call_id);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.message_sent(call_id)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method signalingMessageSendFailed(mut cx) {
            let call_id = get_call_id_arg(&mut cx, 0);
            debug!("JsCallManager.signalingMessageSendFailed({})", call_id);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.message_send_failure(call_id)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method sendVideoStatus(mut cx) {
            debug!("JsCallManager.sendVideoStatus()");
            let enabled = cx.argument::<JsBoolean>(0)?.value();

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |cm| {
                let mut active_connection = cm.call_manager.active_connection()?;
                active_connection.inject_send_sender_status_via_data_channel(enabled)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method setLowBandwidthMode(mut cx) {
            debug!("JsCallManager.setLowBandwidthMode()");
            let enabled = cx.argument::<JsBoolean>(0)?.value();

            let mode = if enabled {
                BandwidthMode::Low
            } else {
                BandwidthMode::Normal
            };

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |cm| {
                let mut active_connection = cm.call_manager.active_connection()?;
                active_connection.set_bandwidth_mode(mode)?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method receivedOffer(mut cx) {
            let peer_id = cx.argument::<JsString>(0)?.value() as PeerId;
            let sender_device_id = cx.argument::<JsNumber>(1)?.value() as DeviceId;
            let receiver_device_id = cx.argument::<JsNumber>(2)?.value() as DeviceId;
            let age_sec = cx.argument::<JsNumber>(3)?.value() as u64;
            let call_id = get_call_id_arg(&mut cx, 4);
            let offer_type = cx.argument::<JsNumber>(5)?.value() as i32;
            let sender_supports_multi_ring = cx.argument::<JsBoolean>(6)?.value();
            let opaque = cx.argument::<JsValue>(7)?.as_value(&mut cx);
            let sdp = cx.argument::<JsValue>(8)?.as_value(&mut cx);

            let opaque = match opaque.downcast::<JsArrayBuffer>() {
                Ok(handle) => Some(cx.borrow(&handle, |handle| { handle.as_slice().to_vec() })),
                Err(_) => None,
            };
            let sdp = match sdp.downcast::<JsString>() {
                Ok(handle) => Some(handle.value()),
                Err(_) => None,
            };

            let call_media_type = match offer_type {
                1 => CallMediaType::Video,
                _ => CallMediaType::Audio,  // TODO: Do something better.  Default matches are evil.
            };
            let sender_device_feature_level = if sender_supports_multi_ring {
                FeatureLevel::MultiRing
            } else {
                FeatureLevel::Unspecified
            };
            let offer = signaling::Offer::from_opaque_or_sdp(call_media_type, opaque, sdp);
            debug!("JsCallManager.receivedOffer({}, {}, {}, {}, {}, {:?}, {})", peer_id, sender_device_id, call_id, receiver_device_id, call_media_type, sender_device_feature_level, offer.to_redacted_string());

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.received_offer(peer_id, call_id, signaling::ReceivedOffer {
                    offer,
                    age: Duration::from_secs(age_sec),
                    sender_device_id,
                    sender_device_feature_level,
                    receiver_device_id,
                    // An electron client cannot be the primary device.
                    receiver_device_is_primary: false,
                })?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method receivedAnswer(mut cx) {
            let peer_id = cx.argument::<JsString>(0)?.value() as PeerId;
            let sender_device_id = cx.argument::<JsNumber>(1)?.value() as DeviceId;
            let call_id = get_call_id_arg(&mut cx, 2);
            let sender_supports_multi_ring = cx.argument::<JsBoolean>(3)?.value();
            let opaque = cx.argument::<JsValue>(4)?.as_value(&mut cx);
            let sdp = cx.argument::<JsValue>(5)?.as_value(&mut cx);

            let opaque = match opaque.downcast::<JsArrayBuffer>() {
                Ok(handle) => Some(cx.borrow(&handle, |handle| { handle.as_slice().to_vec() })),
                Err(_) => None,
            };
            let sdp = match sdp.downcast::<JsString>() {
                Ok(handle) => Some(handle.value()),
                Err(_) => None,
            };

            let sender_device_feature_level = if sender_supports_multi_ring {
                FeatureLevel::MultiRing
            } else {
                FeatureLevel::Unspecified
            };
            let answer = signaling::Answer::from_opaque_or_sdp(opaque, sdp);
            debug!("JsCallManager.receivedAnswer({}, {}, {}, {})", peer_id, sender_device_id, call_id, answer.to_redacted_string());

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.received_answer(call_id, signaling::ReceivedAnswer {
                    answer,
                    sender_device_id,
                    sender_device_feature_level,
                })?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method receivedIceCandidates(mut cx) {
            let peer_id = cx.argument::<JsString>(0)?.value() as PeerId;
            let sender_device_id = cx.argument::<JsNumber>(1)?.value() as DeviceId;
            let call_id = get_call_id_arg(&mut cx, 2);
            let js_candidates = *cx.argument::<JsArray>(3)?;

            let mut candidates = Vec::with_capacity(js_candidates.len() as usize);
            for i in 0..js_candidates.len() {
                let js_candidate = js_candidates.get(&mut cx, i as u32)?.downcast::<JsObject>().expect("ICE candidates");
                let opaque = match js_candidate.get(&mut cx, "opaque")?.downcast::<JsArrayBuffer>() {
                    Ok(handle) => Some(cx.borrow(&handle, |handle| { handle.as_slice().to_vec() })),
                    Err(_) => None,
                };
                let sdp = match js_candidate.get(&mut cx, "sdp")?.downcast::<JsString>() {
                    Ok(handle) => Some(handle.value()),
                    Err(_) => None,
                };
                candidates.push(signaling::IceCandidate::from_opaque_or_sdp(opaque, sdp));
            }
            debug!("JsCallManager.receivedIceCandidates({}, {}, {}, {})", peer_id, sender_device_id, call_id, candidates.len());

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.received_ice(call_id, signaling::ReceivedIce {
                    ice: signaling::Ice {
                        candidates_added: candidates,
                    },
                    sender_device_id,
                })?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method receivedHangup(mut cx) {
            let peer_id = cx.argument::<JsString>(0)?.value() as PeerId;
            let sender_device_id = cx.argument::<JsNumber>(1)?.value() as DeviceId;
            let call_id = get_call_id_arg(&mut cx, 2);
            let hangup_type = cx.argument::<JsNumber>(3)?.value() as i32;
            let hangup_device_id = cx.argument::<JsValue>(4)?.as_value(&mut cx);

            // TODO: Do something better when we don't know the hangup type
            let hangup_type = signaling::HangupType::from_i32(hangup_type).unwrap_or(signaling::HangupType::Normal);
            let hangup_device_id = if hangup_device_id.is_a::<JsNull>() {
                // This is kind of ugly, but the Android and iOS apps do the same
                // and so from_type_and_device_id assumes it.
                // See signaling.rs for more details.
                0 as DeviceId
            } else {
                hangup_device_id.downcast::<JsNumber>().unwrap().value() as DeviceId
            };
            debug!("JsCallManager.receivedHangup({}, {}, {}, {:?}, {:?})", peer_id, sender_device_id, call_id, hangup_type, hangup_device_id);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                let hangup = signaling::Hangup::from_type_and_device_id(hangup_type, hangup_device_id);

                cm.call_manager.received_hangup(call_id, signaling::ReceivedHangup {
                    hangup,
                    sender_device_id,
                })?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method receivedBusy(mut cx) {
            let peer_id = cx.argument::<JsString>(0)?.value() as PeerId;
            let sender_device_id = cx.argument::<JsNumber>(1)?.value() as DeviceId;
            let call_id = get_call_id_arg(&mut cx, 2);
            debug!("JsCallManager.receivedBusy({}, {}, {})", peer_id, sender_device_id, call_id);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |mut cm| {
                cm.call_manager.received_busy(call_id, signaling::ReceivedBusy{
                    sender_device_id,
                })?;
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method setOutgoingAudioEnabled(mut cx) {
            let enabled = cx.argument::<JsBoolean>(0)?.value();
            debug!("JsCallManager.setOutgoingAudioEnabled({})", enabled);

            let mut this = cx.this();
            cx.borrow_mut(&mut this, |cm| {
                cm.outgoing_audio.set_enabled(enabled);
                // TODO: Should we not send silent audio?
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method sendVideoFrame(mut cx) {
            let width = cx.argument::<JsNumber>(0)?.value() as u32;
            let height = cx.argument::<JsNumber>(1)?.value() as u32;
            let rgba_buffer = cx.argument::<JsArrayBuffer>(2)?;

            let frame = cx.borrow(&rgba_buffer, |handle| {
                VideoFrame::from_rgba(
                    width,
                    height,
                    handle.as_slice(),
                )
            });
            let mut this = cx.this();
            cx.borrow_mut(&mut this, |cm| {
                cm.outgoing_video.push_frame(frame);
                Ok(())
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            Ok(cx.undefined().upcast())
        }

        method receiveVideoFrame(mut cx) {
            let rgba_buffer = cx.argument::<JsArrayBuffer>(0)?;
            let mut this = cx.this();
            let unrendered_frame = cx.borrow_mut(&mut this, |cm| {
                cm.unrendered_frame.pop()
            });
            if let Some(frame) = unrendered_frame {
                let frame = frame.apply_rotation();
                cx.borrow(&rgba_buffer, |handle| {
                    frame.to_rgba(handle.as_mut_slice());
                });
                let js_width = cx.number(frame.width());
                let js_height = cx.number(frame.height());
                let result = JsArray::new(&mut cx, 2);
                result.set(&mut cx, 0, js_width)?;
                result.set(&mut cx, 1, js_height)?;
                Ok(result.upcast())
            } else {
                Ok(cx.undefined().upcast())
            }
        }

        method getAudioInputs(mut cx) {
            let mut this = cx.this();
            let devices = cx.borrow_mut(&mut this, |cm| {
                cm.peer_connection_factory.get_audio_recording_devices()
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;

            let js_devices = JsArray::new(&mut cx, devices.len() as u32);
            for (i, device) in devices.iter().enumerate() {
                let js_device = JsObject::new(&mut cx);
                let name = cx.string(device.name.clone());
                js_device.set(&mut cx, "name", name)?;
                let unique_id = cx.string(device.unique_id.clone());
                js_device.set(&mut cx, "uniqueId", unique_id)?;
                let index = cx.number(i as f64);
                js_device.set(&mut cx, "index", index)?;
                if !device.i18n_key.is_empty() {
                    let i18n_key = cx.string(device.i18n_key.clone());
                    js_device.set(&mut cx, "i18nKey", i18n_key)?;
                }
                js_devices.set(&mut cx, i as u32, js_device)?;
            }
            Ok(js_devices.upcast())
        }

        method setAudioInput(mut cx) {
            let index = cx.argument::<JsNumber>(0)?;
            let mut this = cx.this();
            cx.borrow_mut(&mut this, |cm| {
                cm.peer_connection_factory.set_audio_recording_device(index.value() as u16)
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;

            Ok(cx.undefined().upcast())
        }

        method getAudioOutputs(mut cx) {
            let mut this = cx.this();
            let devices = cx.borrow_mut(&mut this, |cm| {
                cm.peer_connection_factory.get_audio_playout_devices()
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;

            let js_devices = JsArray::new(&mut cx, devices.len() as u32);
            for (i, device) in devices.iter().enumerate() {
                let js_device = JsObject::new(&mut cx);
                let name = cx.string(device.name.clone());
                js_device.set(&mut cx, "name", name)?;
                let unique_id = cx.string(device.unique_id.clone());
                js_device.set(&mut cx, "uniqueId", unique_id)?;
                let index = cx.number(i as f64);
                js_device.set(&mut cx, "index", index)?;
                if !device.i18n_key.is_empty() {
                    let i18n_key = cx.string(device.i18n_key.clone());
                    js_device.set(&mut cx, "i18nKey", i18n_key)?;
                }
                js_devices.set(&mut cx, i as u32, js_device)?;
            }
            Ok(js_devices.upcast())
        }

        method setAudioOutput(mut cx) {
            let index = cx.argument::<JsNumber>(0)?;
            let mut this = cx.this();
            cx.borrow_mut(&mut this, |cm| {
                cm.peer_connection_factory.set_audio_playout_device(index.value() as u16)
            }).or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;

            Ok(cx.undefined().upcast())
        }

        method poll(mut cx) {
            let observer = cx.argument::<JsObject>(0)?;

            let mut log_entries = LOG_MESSAGES.lock().expect("lock log messages");
            for log_entry in log_entries.drain(0..) {
                let method_name = "onLogMessage";
                let args : Vec<Handle<JsValue>> = vec![
                    cx.number(log_entry.level).upcast(),
                    cx.string(log_entry.file).upcast(),
                    cx.number(log_entry.line).upcast(),
                    cx.string(log_entry.message).upcast(),
                ];
                let method = *observer.get(&mut cx, method_name)?.downcast::<JsFunction>().expect("onLogMessage is a function");
                method.call(&mut cx, observer, args)?;
            }

            let mut this = cx.this();
            let events: Vec<Event> = cx.borrow_mut(&mut this, |cm| {
                cm.events_receiver.try_iter().collect()
            });

            for event in events {
                match event {
                    Event::SendSignaling(peer_id, maybe_device_id, call_id, signal) => {
                        let (method_name, data1, data2, data3) : (&str, Handle<JsValue>, Handle<JsValue>, Handle<JsValue>) = match signal {
                            signaling::Message::Offer(offer) => {
                                let opaque = match offer.opaque {
                                    None => cx.undefined().upcast(),
                                    Some(opaque) => {
                                        let mut js_opaque = cx.array_buffer(opaque.len() as u32)?;
                                        cx.borrow_mut(&mut js_opaque, |handle| {
                                            handle.as_mut_slice().copy_from_slice(&opaque);
                                        });
                                        js_opaque.upcast()
                                    }
                                };
                                let sdp = match offer.sdp {
                                    None => cx.undefined().upcast(),
                                    Some(sdp) => cx.string(sdp).upcast(),
                                };
                                ("onSendOffer", cx.number(offer.call_media_type as i32).upcast(), opaque, sdp)
                            },
                            signaling::Message::Answer(answer) => {
                                let opaque = match answer.opaque {
                                    None => cx.undefined().upcast(),
                                    Some(opaque) => {
                                        let mut js_opaque = cx.array_buffer(opaque.len() as u32)?;
                                        cx.borrow_mut(&mut js_opaque, |handle| {
                                            handle.as_mut_slice().copy_from_slice(&opaque);
                                        });
                                        js_opaque.upcast()
                                    }
                                };
                                let sdp = match answer.sdp {
                                    None => cx.undefined().upcast(),
                                    Some(sdp) => cx.string(sdp).upcast(),
                                };

                                ("onSendAnswer", opaque, sdp, cx.undefined().upcast())
                            },
                            signaling::Message::Ice(ice) => {
                                let js_candidates = JsArray::new(&mut cx, ice.candidates_added.len() as u32);
                                for (i, candidate) in ice.candidates_added.iter().enumerate() {
                                    let opaque: neon::handle::Handle<JsValue> = match &candidate.opaque {
                                        None => cx.undefined().upcast(),
                                        Some(opaque) => {
                                            let mut js_opaque = cx.array_buffer(opaque.len() as u32)?;
                                            cx.borrow_mut(&mut js_opaque, |handle| {
                                                handle.as_mut_slice().copy_from_slice(opaque);
                                            });
                                            js_opaque.upcast()
                                        },
                                    };
                                    let sdp: neon::handle::Handle<JsValue> = match &candidate.sdp {
                                        None => cx.undefined().upcast(),
                                        Some(sdp) => cx.string(sdp).upcast(),
                                    };

                                    let js_candidate = cx.empty_object();
                                    js_candidate.set(&mut cx, "opaque", opaque)?;
                                    js_candidate.set(&mut cx, "sdp", sdp)?;
                                    js_candidates.set(&mut cx, i as u32, js_candidate)?;
                                }
                                ("onSendIceCandidates", js_candidates.upcast(), cx.undefined().upcast(), cx.undefined().upcast())
                            }
                            signaling::Message::Hangup(hangup) => {
                                let (hangup_type, hangup_device_id) = hangup.to_type_and_device_id();
                                let hangup_type = cx.number(hangup_type as i32).upcast();
                                let device_id = match hangup_device_id {
                                    Some(device_id) => cx.number(device_id).upcast(),
                                    None => cx.null().upcast(),
                                };
                                ("onSendHangup", hangup_type, device_id, cx.undefined().upcast())
                            }
                            signaling::Message::LegacyHangup(hangup) => {
                                let (hangup_type, hangup_device_id) = hangup.to_type_and_device_id();
                                let hangup_type = cx.number(hangup_type as i32).upcast();
                                let device_id = match hangup_device_id {
                                    Some(device_id) => cx.number(device_id).upcast(),
                                    None => cx.null().upcast(),
                                };
                                ("onSendLegacyHangup", hangup_type, device_id, cx.undefined().upcast())
                            },
                            signaling::Message::Busy => {
                                ("onSendBusy", cx.undefined().upcast(), cx.undefined().upcast(), cx.undefined().upcast())
                            }
                        };
                        let error_message = format!("{} is a function", method_name);
                        let method = *observer.get(&mut cx, method_name)?.downcast::<JsFunction>().expect(&error_message);
                        let args = vec![
                            cx.string(peer_id).upcast(),
                            cx.number(maybe_device_id.unwrap_or(0 as DeviceId) as f64).upcast(),
                            create_call_id_arg(&mut cx, call_id),
                            cx.boolean(maybe_device_id.is_none()).upcast(),
                            data1,
                            data2,
                            data3,
                        ];
                        method.call(&mut cx, observer, args)?;
                        // // TODO: Only call this once it's really sent.  This may be too early.
                        // let mut this = cx.this();
                        // cx.borrow_mut(&mut this, |mut cm| {
                        //   // TODO: handle errors
                        //   let _ = cm.call_manager.message_sent(call_id);
                        // });
                    },
                    Event::CallState(peer_id, CallState::Incoming(call_id, call_media_type)) => {
                        let method_name = "onStartIncomingCall";
                        let args: Vec<Handle<JsValue>> = vec![
                            cx.string(peer_id).upcast(),
                            create_call_id_arg(&mut cx, call_id),
                            cx.boolean(call_media_type == CallMediaType::Video).upcast(),
                        ];
                        let method = *observer.get(&mut cx, method_name)?.downcast::<JsFunction>().expect("onStartIncomingCall is a function");
                        method.call(&mut cx, observer, args)?;
                    },
                    // TODO: Dedup this
                    Event::CallState(peer_id, CallState::Outgoing(call_id, _call_media_type)) => {
                        let method_name = "onStartOutgoingCall";
                        let args: Vec<Handle<JsValue>> = vec![
                            cx.string(peer_id).upcast(),
                            create_call_id_arg(&mut cx, call_id),
                        ];
                        let method = *observer.get(&mut cx, method_name)?.downcast::<JsFunction>().expect("onStartOutgoingCall is a function");
                        method.call(&mut cx, observer, args)?;
                    },
                    Event::CallState(peer_id, CallState::Ended(reason)) => {
                        let method_name = "onCallEnded";
                        let reason_string = match reason {
                            EndReason::LocalHangup => "LocalHangup",
                            EndReason::RemoteHangup => "RemoteHangup",
                            EndReason::RemoteHangupNeedPermission => "RemoteHangupNeedPermission",
                            EndReason::Declined => "Declined",
                            EndReason::Busy => "Busy",
                            EndReason::Glare => "Glare",
                            EndReason::ReceivedOfferExpired => "ReceivedOfferExpired",
                            EndReason::ReceivedOfferWhileActive => "ReceivedOfferWhileActive",
                            EndReason::SignalingFailure => "SignalingFailure",
                            EndReason::ConnectionFailure => "ConnectionFailure",
                            EndReason::InternalFailure => "InternalFailure",
                            EndReason::Timeout => "Timeout",
                            EndReason::AcceptedOnAnotherDevice => "AcceptedOnAnotherDevice",
                            EndReason::DeclinedOnAnotherDevice => "DeclinedOnAnotherDevice",
                            EndReason::BusyOnAnotherDevice => "BusyOnAnotherDevice",
                            EndReason::CallerIsNotMultiring => "CallerIsNotMultiring",
                        };
                        let args = vec![
                            cx.string(peer_id),
                            cx.string(reason_string),
                        ];
                        let method = *observer.get(&mut cx, method_name)?.downcast::<JsFunction>().expect("onCallEnded is a function");
                        method.call(&mut cx, observer, args)?;
                    },
                    Event::CallState(peer_id, state) => {
                        let method_name = "onCallState";
                        let state_string = match state {
                            CallState::Ringing => "ringing",
                            CallState::Connected => "connected",
                            CallState::Connecting => "connecting",
                            CallState::Concluded => "ended",
                            // All covered above.
                            CallState::Incoming(_, _) => "incoming",
                            CallState::Outgoing(_, _) => "outgoing",
                            CallState::Ended(_) => "ended",
                        };
                        let args = vec![
                            cx.string(peer_id),
                            cx.string(state_string),
                        ];
                        let method = *observer.get(&mut cx, method_name)?.downcast::<JsFunction>().expect("onCallState is a function");
                        method.call(&mut cx, observer, args)?;
                    }
                    Event::RemoteVideoState(peer_id, enabled) => {
                        let method_name = "onRemoteVideoEnabled";
                        let args : Vec<Handle<JsValue>> = vec![
                            cx.string(peer_id).upcast(),
                            cx.boolean(enabled).upcast(),
                        ];
                        let method = *observer.get(&mut cx, method_name)?.downcast::<JsFunction>().expect("onRemoteVideoEnabled is a function");
                        method.call(&mut cx, observer, args)?;
                    },
                }
            }
            Ok(cx.undefined().upcast())
        }
    }
}

register_module!(mut cx, {
    cx.export_class::<JsCallManager>("CallManager")?;
    Ok(())
});
