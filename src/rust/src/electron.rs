//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use lazy_static::lazy_static;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::common::{
    CallId,
    CallMediaType,
    DeviceId,
    FeatureLevel,
    HttpMethod,
    HttpResponse,
    Result,
};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call_manager::CallManager;
use crate::core::group_call;
use crate::core::group_call::UserId;
use crate::core::signaling;
use crate::native::{
    CallState,
    CallStateHandler,
    EndReason,
    GroupUpdate,
    GroupUpdateHandler,
    HttpClient,
    NativeCallContext,
    NativePlatform,
    PeerId,
    SignalingSender,
};
use crate::webrtc::media::{AudioTrack, VideoFrame, VideoSink, VideoSource, VideoTrack};
use crate::webrtc::peer_connection_factory::{
    AudioDevice,
    Certificate,
    IceServer,
    PeerConnectionFactory,
};

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
    // The JavaScript should send the following opaque call message to the
    // given recipient UUID.
    SendCallMessage(UserId, Vec<u8>),
    // The call with the given remote PeerId has changed state.
    // We assume only one call per remote PeerId at a time.
    CallState(PeerId, CallState),
    // The state of the remote video (whether enabled or not)
    // Like call state, we ID the call by PeerId and assume there is only one.
    RemoteVideoState(PeerId, bool),
    // The group call has an update.
    GroupUpdate(GroupUpdate),
    // JavaScript should initiate an HTTP request.
    SendHttpRequest(
        u32,
        String,
        HttpMethod,
        HashMap<String, String>,
        Option<Vec<u8>>,
    ),
}

impl SignalingSender for Sender<Event> {
    fn send_signaling(
        &self,
        recipient_id: &str,
        call_id: CallId,
        receiver_device_id: Option<DeviceId>,
        msg: signaling::Message,
    ) -> Result<()> {
        self.send(Event::SendSignaling(
            recipient_id.to_string(),
            receiver_device_id,
            call_id,
            msg,
        ))?;
        Ok(())
    }

    fn send_call_message(&self, recipient_uuid: UserId, msg: Vec<u8>) -> Result<()> {
        self.send(Event::SendCallMessage(recipient_uuid, msg))?;
        Ok(())
    }
}

impl CallStateHandler for Sender<Event> {
    fn handle_call_state(&self, remote_peer_id: &str, call_state: CallState) -> Result<()> {
        self.send(Event::CallState(remote_peer_id.to_string(), call_state))?;
        Ok(())
    }

    fn handle_remote_video_state(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        self.send(Event::RemoteVideoState(remote_peer_id.to_string(), enabled))?;
        Ok(())
    }
}

impl HttpClient for Sender<Event> {
    fn send_http_request(
        &self,
        request_id: u32,
        url: String,
        method: HttpMethod,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<()> {
        self.send(Event::SendHttpRequest(
            request_id, url, method, headers, body,
        ))?;
        Ok(())
    }
}

impl GroupUpdateHandler for Sender<Event> {
    fn handle_group_update(&self, update: GroupUpdate) -> Result<()> {
        self.send(Event::GroupUpdate(update))?;
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
    fn new(enabled: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(OneFrameBufferState {
                enabled,
                frame: None,
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

    events_receiver:                          Receiver<Event>,
    // This is what we use to control mute/not.
    // It should probably be per-call, but for now it's easier to have only one.
    outgoing_audio_track:                     AudioTrack,
    // This is what we use to push video frames out.
    outgoing_video_source:                    VideoSource,
    // We only keep this around so we can pass it to PeerConnectionFactory::create_peer_connection
    // via the NativeCallContext.
    outgoing_video_track:                     VideoTrack,
    // Pulled out by receiveVideoFrame for direct/1:1 calls
    incoming_video_buffer:                    OneFrameBuffer,
    // Pulled out by receiveGroupCalLVideoFrame for group calls
    incoming_video_buffer_by_remote_demux_id:
        HashMap<(group_call::ClientId, group_call::DemuxId), Box<OneFrameBuffer>>,

    peer_connection_factory: PeerConnectionFactory,
}

impl CallEndpoint {
    fn new() -> Result<Self> {
        // Relevant for both group calls and 1:1 calls
        let (events_sender, events_receiver) = channel::<Event>();
        let use_injectable_network = false;
        let peer_connection_factory = PeerConnectionFactory::new(use_injectable_network)?;
        let outgoing_audio_track = peer_connection_factory.create_outgoing_audio_track()?;
        outgoing_audio_track.set_enabled(false);
        let outgoing_video_source = peer_connection_factory.create_outgoing_video_source()?;
        let outgoing_video_track =
            peer_connection_factory.create_outgoing_video_track(&outgoing_video_source)?;
        outgoing_video_track.set_enabled(false);

        // Only relevant for 1:1 calls
        let signaling_sender = Box::new(events_sender.clone());
        let should_assume_messages_sent = false; // Use async notification from app to send next message.
        let state_handler = Box::new(events_sender.clone());
        let incoming_video_buffer = OneFrameBuffer::new(false /* enabled */);
        let incoming_video_sink = Box::new(incoming_video_buffer.clone());

        // Only relevant for group calls
        let http_client = Box::new(events_sender.clone());
        let group_handler = Box::new(events_sender);
        let incoming_video_buffer_by_remote_demux_id = HashMap::new();

        let platform = NativePlatform::new(
            peer_connection_factory.clone(),
            signaling_sender,
            should_assume_messages_sent,
            state_handler,
            incoming_video_sink,
            http_client,
            group_handler,
        );
        let call_manager = CallManager::new(platform)?;

        Ok(Self {
            call_manager,
            events_receiver,
            outgoing_audio_track,
            outgoing_video_source,
            outgoing_video_track,
            incoming_video_buffer,
            incoming_video_buffer_by_remote_demux_id,
            peer_connection_factory,
        })
    }
}

fn js_num_to_u64(num: f64) -> u64 {
    // Convert safely from signed.
    num as i32 as u32 as u64
}

fn u64_to_js_num(val: u64) -> f64 {
    // Convert safely to signed.
    val as u32 as i32 as f64
}

fn get_id_arg(cx: &mut FunctionContext, i: i32) -> u64 {
    let obj = cx.argument::<JsObject>(i).expect("Get id argument");
    let high = js_num_to_u64(
        obj.get(cx, "high")
            .expect("Get id.high")
            .downcast::<JsNumber, _>(cx)
            .expect("id.high is a number")
            .value(cx),
    );
    let low = js_num_to_u64(
        obj.get(cx, "low")
            .expect("Get id.low")
            .downcast::<JsNumber, _>(cx)
            .expect("id.low is a number")
            .value(cx),
    );
    let id = ((high << 32) & 0xFFFFFFFF00000000) | (low & 0xFFFFFFFF);
    debug!("id: {} converted from (high: {} low: {})", id, high, low);
    id
}

fn create_id_arg<'a>(cx: &mut FunctionContext<'a>, id: u64) -> Handle<'a, JsValue> {
    let high = cx.number(u64_to_js_num((id >> 32) & 0xFFFFFFFF));
    let low = cx.number(u64_to_js_num(id & 0xFFFFFFFF));
    let unsigned = cx.boolean(true);
    let obj = cx.empty_object();
    obj.set(cx, "high", high).expect("set id.high");
    obj.set(cx, "low", low).expect("set id.low");
    obj.set(cx, "unsigned", unsigned).expect("set id.unsigned");
    obj.upcast()
}

fn to_js_array_buffer<'a>(cx: &mut FunctionContext<'a>, data: &[u8]) -> Handle<'a, JsValue> {
    let mut js_buffer = cx
        .array_buffer(data.len() as u32)
        .expect("create ArrayBuffer");
    cx.borrow_mut(&mut js_buffer, |handle| {
        handle.as_mut_slice().copy_from_slice(data.as_ref());
    });
    js_buffer.upcast()
}

static CALL_ENDPOINT_PROPERTY_KEY: &str = "__call_endpoint_addr";

fn with_call_endpoint<T>(cx: &mut FunctionContext, body: impl FnOnce(&mut CallEndpoint) -> T) -> T {
    let endpoint = cx
        .this()
        .get(cx, CALL_ENDPOINT_PROPERTY_KEY)
        .expect("has endpoint");
    let endpoint = endpoint
        .downcast::<JsBox<RefCell<CallEndpoint>>, _>(cx)
        .expect("has correct type");
    let mut endpoint = endpoint.borrow_mut();
    body(&mut *endpoint)
}

// CallEndpoint doesn't need any custom finalization on the JavaScript side;
// the default implementation (just Drop it) is sufficient.
impl Finalize for CallEndpoint {}

#[allow(non_snake_case)]
fn createCallEndpoint(mut cx: FunctionContext) -> JsResult<JsValue> {
    if ENABLE_LOGGING {
        log::set_logger(&LOG).expect("set logger");

        #[cfg(debug_assertions)]
        log::set_max_level(log::LevelFilter::Debug);

        #[cfg(not(debug_assertions))]
        log::set_max_level(log::LevelFilter::Info);

        // Show WebRTC logs via application Logger while debugging.
        #[cfg(debug_assertions)]
        crate::webrtc::logging::set_logger(log::LevelFilter::Debug);

        #[cfg(not(debug_assertions))]
        crate::webrtc::logging::set_logger(log::LevelFilter::Off);
    }

    debug!("JsCallManager()");
    let endpoint =
        CallEndpoint::new().or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.boxed(RefCell::new(endpoint)).upcast())
}

#[allow(non_snake_case)]
fn createOutgoingCall(mut cx: FunctionContext) -> JsResult<JsValue> {
    let peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let video_enabled = cx.argument::<JsBoolean>(1)?.value(&mut cx);
    let local_device_id = cx.argument::<JsNumber>(2)?.value(&mut cx) as DeviceId;

    let media_type = if video_enabled {
        CallMediaType::Video
    } else {
        CallMediaType::Audio
    };

    debug!(
        "JsCallManager.call({}, {}, {})",
        peer_id, media_type, local_device_id
    );

    let call_id = CallId::random();
    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.create_outgoing_call(
            peer_id,
            call_id,
            media_type,
            local_device_id,
        )?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(create_id_arg(&mut cx, call_id.as_u64()))
}

#[allow(non_snake_case)]
fn proceed(mut cx: FunctionContext) -> JsResult<JsValue> {
    let call_id = CallId::new(get_id_arg(&mut cx, 0));
    let ice_server_username = cx.argument::<JsString>(1)?.value(&mut cx);
    let ice_server_password = cx.argument::<JsString>(2)?.value(&mut cx);
    let js_ice_server_urls = cx.argument::<JsArray>(3)?;
    let hide_ip = cx.argument::<JsBoolean>(4)?.value(&mut cx);
    let bandwidth_mode = cx.argument::<JsNumber>(5)?.value(&mut cx) as i32;

    let mut ice_server_urls = Vec::with_capacity(js_ice_server_urls.len(&mut cx) as usize);
    for i in 0..js_ice_server_urls.len(&mut cx) {
        let url: String = js_ice_server_urls
            .get(&mut cx, i as u32)?
            .downcast::<JsString, _>(&mut cx)
            .expect("ICE server URLs are strings")
            .value(&mut cx);
        ice_server_urls.push(url);
    }

    let ice_server = IceServer::new(ice_server_username, ice_server_password, ice_server_urls);
    debug!(
        "JsCallManager.proceed({}, {:?}, {})",
        call_id, ice_server, hide_ip
    );

    with_call_endpoint(&mut cx, |endpoint| {
        let certificate = Certificate::generate()?;
        let call_context = NativeCallContext::new(
            certificate,
            hide_ip,
            ice_server,
            endpoint.outgoing_audio_track.clone(),
            endpoint.outgoing_video_track.clone(),
        );
        endpoint.call_manager.proceed(
            call_id,
            call_context,
            BandwidthMode::from_i32(bandwidth_mode),
        )?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn accept(mut cx: FunctionContext) -> JsResult<JsValue> {
    let call_id = CallId::new(get_id_arg(&mut cx, 0));
    debug!("JsCallManager.accept({})", call_id);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.accept_call(call_id)?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn ignore(mut cx: FunctionContext) -> JsResult<JsValue> {
    let call_id = CallId::new(get_id_arg(&mut cx, 0));
    debug!("JsCallManager.ignore({})", call_id);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.drop_call(call_id)?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn hangup(mut cx: FunctionContext) -> JsResult<JsValue> {
    debug!("JsCallManager.hangup()");

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.hangup()?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn signalingMessageSent(mut cx: FunctionContext) -> JsResult<JsValue> {
    let call_id = CallId::new(get_id_arg(&mut cx, 0));
    debug!("JsCallManager.signalingMessageSent({})", call_id);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.message_sent(call_id)?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn signalingMessageSendFailed(mut cx: FunctionContext) -> JsResult<JsValue> {
    let call_id = CallId::new(get_id_arg(&mut cx, 0));
    debug!("JsCallManager.signalingMessageSendFailed({})", call_id);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.message_send_failure(call_id)?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn updateBandwidthMode(mut cx: FunctionContext) -> JsResult<JsValue> {
    debug!("JsCallManager.updateBandwidthMode()");
    let bandwidth_mode = cx.argument::<JsNumber>(0)?.value(&mut cx) as i32;

    with_call_endpoint(&mut cx, |endpoint| {
        let active_connection = endpoint.call_manager.active_connection()?;
        active_connection.update_bandwidth_mode(BandwidthMode::from_i32(bandwidth_mode))?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedOffer(mut cx: FunctionContext) -> JsResult<JsValue> {
    let peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let sender_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let receiver_device_id = cx.argument::<JsNumber>(2)?.value(&mut cx) as DeviceId;
    let age_sec = cx.argument::<JsNumber>(3)?.value(&mut cx) as u64;
    let call_id = CallId::new(get_id_arg(&mut cx, 4));
    let offer_type = cx.argument::<JsNumber>(5)?.value(&mut cx) as i32;
    let sender_supports_multi_ring = cx.argument::<JsBoolean>(6)?.value(&mut cx);
    let opaque = cx.argument::<JsArrayBuffer>(7)?;
    let sender_identity_key = cx.argument::<JsArrayBuffer>(8)?;
    let receiver_identity_key = cx.argument::<JsArrayBuffer>(9)?;

    let opaque = cx.borrow(&opaque, |handle| handle.as_slice().to_vec());
    let sender_identity_key = cx.borrow(&sender_identity_key, |handle| handle.as_slice().to_vec());
    let receiver_identity_key =
        cx.borrow(&receiver_identity_key, |handle| handle.as_slice().to_vec());

    let call_media_type = match offer_type {
        1 => CallMediaType::Video,
        _ => CallMediaType::Audio, // TODO: Do something better.  Default matches are evil.
    };
    let sender_device_feature_level = if sender_supports_multi_ring {
        FeatureLevel::MultiRing
    } else {
        FeatureLevel::Unspecified
    };

    with_call_endpoint(&mut cx, |endpoint| {
        let offer = signaling::Offer::new(call_media_type, opaque)?;

        endpoint.call_manager.received_offer(
            peer_id,
            call_id,
            signaling::ReceivedOffer {
                offer,
                age: Duration::from_secs(age_sec),
                sender_device_id,
                sender_device_feature_level,
                receiver_device_id,
                // An electron client cannot be the primary device.
                receiver_device_is_primary: false,
                sender_identity_key,
                receiver_identity_key,
            },
        )?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedAnswer(mut cx: FunctionContext) -> JsResult<JsValue> {
    let _peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let sender_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let call_id = CallId::new(get_id_arg(&mut cx, 2));
    let sender_supports_multi_ring = cx.argument::<JsBoolean>(3)?.value(&mut cx);
    let opaque = cx.argument::<JsArrayBuffer>(4)?;
    let sender_identity_key = cx.argument::<JsArrayBuffer>(5)?;
    let receiver_identity_key = cx.argument::<JsArrayBuffer>(6)?;

    let opaque = cx.borrow(&opaque, |handle| handle.as_slice().to_vec());
    let sender_identity_key = cx.borrow(&sender_identity_key, |handle| handle.as_slice().to_vec());
    let receiver_identity_key =
        cx.borrow(&receiver_identity_key, |handle| handle.as_slice().to_vec());

    let sender_device_feature_level = if sender_supports_multi_ring {
        FeatureLevel::MultiRing
    } else {
        FeatureLevel::Unspecified
    };

    with_call_endpoint(&mut cx, |endpoint| {
        let answer = signaling::Answer::new(opaque)?;
        endpoint.call_manager.received_answer(
            call_id,
            signaling::ReceivedAnswer {
                answer,
                sender_device_id,
                sender_device_feature_level,
                sender_identity_key,
                receiver_identity_key,
            },
        )?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedIceCandidates(mut cx: FunctionContext) -> JsResult<JsValue> {
    let peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let sender_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let call_id = CallId::new(get_id_arg(&mut cx, 2));
    let js_candidates = *cx.argument::<JsArray>(3)?;

    let mut candidates = Vec::with_capacity(js_candidates.len(&mut cx) as usize);
    for i in 0..js_candidates.len(&mut cx) {
        let js_candidate = js_candidates
            .get(&mut cx, i as u32)?
            .downcast::<JsArrayBuffer, _>(&mut cx)
            .expect("ICE candidates");
        let opaque = cx.borrow(&js_candidate, |handle| handle.as_slice().to_vec());
        candidates.push(signaling::IceCandidate::new(opaque));
    }
    debug!(
        "JsCallManager.receivedIceCandidates({}, {}, {}, {})",
        peer_id,
        sender_device_id,
        call_id,
        candidates.len()
    );

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.received_ice(
            call_id,
            signaling::ReceivedIce {
                ice: signaling::Ice {
                    candidates_added: candidates,
                },
                sender_device_id,
            },
        )?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedHangup(mut cx: FunctionContext) -> JsResult<JsValue> {
    let peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let sender_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let call_id = CallId::new(get_id_arg(&mut cx, 2));
    let hangup_type = cx.argument::<JsNumber>(3)?.value(&mut cx) as i32;
    let hangup_device_id = cx.argument::<JsValue>(4)?.as_value(&mut cx);

    // TODO: Do something better when we don't know the hangup type
    let hangup_type =
        signaling::HangupType::from_i32(hangup_type).unwrap_or(signaling::HangupType::Normal);
    let hangup_device_id = if hangup_device_id.is_a::<JsNull, _>(&mut cx) {
        // This is kind of ugly, but the Android and iOS apps do the same
        // and so from_type_and_device_id assumes it.
        // See signaling.rs for more details.
        0
    } else {
        hangup_device_id
            .downcast::<JsNumber, _>(&mut cx)
            .unwrap()
            .value(&mut cx) as DeviceId
    };
    debug!(
        "JsCallManager.receivedHangup({}, {}, {}, {:?}, {:?})",
        peer_id, sender_device_id, call_id, hangup_type, hangup_device_id
    );

    with_call_endpoint(&mut cx, |endpoint| {
        let hangup = signaling::Hangup::from_type_and_device_id(hangup_type, hangup_device_id);

        endpoint.call_manager.received_hangup(
            call_id,
            signaling::ReceivedHangup {
                hangup,
                sender_device_id,
            },
        )?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedBusy(mut cx: FunctionContext) -> JsResult<JsValue> {
    let peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let sender_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let call_id = CallId::new(get_id_arg(&mut cx, 2));
    debug!(
        "JsCallManager.receivedBusy({}, {}, {})",
        peer_id, sender_device_id, call_id
    );

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .received_busy(call_id, signaling::ReceivedBusy { sender_device_id })?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedCallMessage(mut cx: FunctionContext) -> JsResult<JsValue> {
    let remote_user_id = cx.argument::<JsArrayBuffer>(0)?;
    let remote_user_id = cx.borrow(&remote_user_id, |handle| handle.as_slice().to_vec());
    let remote_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let local_device_id = cx.argument::<JsNumber>(2)?.value(&mut cx) as DeviceId;
    let data = cx.argument::<JsArrayBuffer>(3)?;
    let data = cx.borrow(&data, |handle| handle.as_slice().to_vec());
    let message_age_sec = cx.argument::<JsNumber>(4)?.value(&mut cx) as u64;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.received_call_message(
            remote_user_id,
            remote_device_id,
            local_device_id,
            data,
            message_age_sec,
        )?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedHttpResponse(mut cx: FunctionContext) -> JsResult<JsValue> {
    let request_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as u32;
    let status_code = cx.argument::<JsNumber>(1)?.value(&mut cx) as u16;
    let body = cx.argument::<JsArrayBuffer>(2)?;
    let body = cx.borrow(&body, |handle| handle.as_slice().to_vec());
    let response = HttpResponse { status_code, body };

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .received_http_response(request_id, Some(response))?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn httpRequestFailed(mut cx: FunctionContext) -> JsResult<JsValue> {
    let request_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as u32;
    let debug_info = match cx.argument::<JsString>(1) {
        Ok(s) => s.value(&mut cx),
        Err(_) => "<no debug info>".to_string(),
    };
    error!("HTTP request {} failed: {}", request_id, debug_info);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .received_http_response(request_id, None)?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingAudioEnabled(mut cx: FunctionContext) -> JsResult<JsValue> {
    let enabled = cx.argument::<JsBoolean>(0)?.value(&mut cx);
    debug!("JsCallManager.setOutgoingAudioEnabled({})", enabled);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_audio_track.set_enabled(enabled);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingVideoEnabled(mut cx: FunctionContext) -> JsResult<JsValue> {
    let enabled = cx.argument::<JsBoolean>(0)?.value(&mut cx);
    debug!("JsCallManager.setOutgoingVideoEnabled({})", enabled);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_video_track.set_enabled(enabled);
        let mut active_connection = endpoint.call_manager.active_connection()?;
        active_connection.inject_send_sender_status_via_data_channel(enabled)?;
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn sendVideoFrame(mut cx: FunctionContext) -> JsResult<JsValue> {
    let width = cx.argument::<JsNumber>(0)?.value(&mut cx) as u32;
    let height = cx.argument::<JsNumber>(1)?.value(&mut cx) as u32;
    let rgba_buffer = cx.argument::<JsArrayBuffer>(2)?;

    let frame = cx.borrow(&rgba_buffer, |handle| {
        VideoFrame::from_rgba(width, height, handle.as_slice())
    });
    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_video_source.push_frame(frame);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receiveVideoFrame(mut cx: FunctionContext) -> JsResult<JsValue> {
    let rgba_buffer = cx.argument::<JsArrayBuffer>(0)?;
    let frame = with_call_endpoint(&mut cx, |endpoint| endpoint.incoming_video_buffer.pop());
    if let Some(frame) = frame {
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

// Group Calls

#[allow(non_snake_case)]
fn receiveGroupCallVideoFrame(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let remote_demux_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as group_call::DemuxId;
    let rgba_buffer = cx.argument::<JsArrayBuffer>(2)?;

    let frame = with_call_endpoint(&mut cx, |endpoint| {
        if let Some(video_buffer) = endpoint
            .incoming_video_buffer_by_remote_demux_id
            .get(&(client_id, remote_demux_id))
        {
            video_buffer.pop()
        } else {
            None
        }
    });

    if let Some(frame) = frame {
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

#[allow(non_snake_case)]
fn createGroupCallClient(mut cx: FunctionContext) -> JsResult<JsValue> {
    let group_id = cx.argument::<JsValue>(0)?.as_value(&mut cx);
    let sfu_url = cx.argument::<JsString>(1)?.value(&mut cx);

    let mut client_id = group_call::INVALID_CLIENT_ID;

    let group_id: std::vec::Vec<u8> = match group_id.downcast::<JsArrayBuffer, _>(&mut cx) {
        Ok(handle) => cx.borrow(&handle, |handle| handle.as_slice().to_vec()),
        Err(_) => {
            return Ok(cx.number(client_id).upcast());
        }
    };
    with_call_endpoint(&mut cx, |endpoint| {
        let peer_connection_factory = endpoint.peer_connection_factory.clone();
        let outgoing_audio_track = endpoint.outgoing_audio_track.clone();
        let outgoing_video_track = endpoint.outgoing_video_track.clone();
        let result = endpoint.call_manager.create_group_call_client(
            group_id,
            sfu_url,
            Some(peer_connection_factory),
            outgoing_audio_track,
            outgoing_video_track,
        );
        if let Ok(v) = result {
            client_id = v;
        }

        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.number(client_id).upcast())
}

#[allow(non_snake_case)]
fn deleteGroupCallClient(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.delete_group_call_client(client_id);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn connect(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.connect(client_id);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn join(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.join(client_id);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn leave(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        // When leaving, make sure outgoing media is stopped as soon as possible.
        endpoint.outgoing_audio_track.set_enabled(false);
        endpoint.outgoing_video_track.set_enabled(false);
        endpoint.call_manager.leave(client_id);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn disconnect(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        // When disconnecting, make sure outgoing media is stopped as soon as possible.
        endpoint.outgoing_audio_track.set_enabled(false);
        endpoint.outgoing_video_track.set_enabled(false);
        endpoint.call_manager.disconnect(client_id);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingAudioMuted(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let muted = cx.argument::<JsBoolean>(1)?.value(&mut cx);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_audio_track.set_enabled(!muted);
        endpoint
            .call_manager
            .set_outgoing_audio_muted(client_id, muted);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingVideoMuted(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let muted = cx.argument::<JsBoolean>(1)?.value(&mut cx);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_video_track.set_enabled(!muted);
        endpoint
            .call_manager
            .set_outgoing_video_muted(client_id, muted);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn resendMediaKeys(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.resend_media_keys(client_id);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setBandwidthMode(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let bandwidth_mode = cx.argument::<JsNumber>(1)?.value(&mut cx) as i32;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .set_bandwidth_mode(client_id, BandwidthMode::from_i32(bandwidth_mode));
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn requestVideo(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let js_resolutions = *cx.argument::<JsArray>(1)?;

    let mut resolutions = Vec::with_capacity(js_resolutions.len(&mut cx) as usize);
    for i in 0..js_resolutions.len(&mut cx) {
        let js_resolution = js_resolutions
            .get(&mut cx, i as u32)?
            .downcast::<JsObject, _>(&mut cx)
            .expect("VideoRequest");

        let demux_id = match js_resolution
            .get(&mut cx, "demuxId")?
            .downcast::<JsNumber, _>(&mut cx)
        {
            Ok(handle) => Some(handle.value(&mut cx) as group_call::DemuxId),
            Err(_) => None,
        };
        let width = match js_resolution
            .get(&mut cx, "width")?
            .downcast::<JsNumber, _>(&mut cx)
        {
            Ok(handle) => Some(handle.value(&mut cx) as u16),
            Err(_) => None,
        };
        let height = match js_resolution
            .get(&mut cx, "height")?
            .downcast::<JsNumber, _>(&mut cx)
        {
            Ok(handle) => Some(handle.value(&mut cx) as u16),
            Err(_) => None,
        };
        let framerate = match js_resolution
            .get(&mut cx, "framerate")?
            .downcast::<JsNumber, _>(&mut cx)
        {
            Ok(handle) => Some(handle.value(&mut cx) as u16),
            Err(_) => None,
        };

        if let (Some(demux_id), Some(width), Some(height)) = (demux_id, width, height) {
            resolutions.push(group_call::VideoRequest {
                demux_id,
                width,
                height,
                framerate,
            });
        } else {
            warn!("Skipping resolution due to invalid field");
        }
    }

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.request_video(client_id, resolutions);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setGroupMembers(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let js_members = *cx.argument::<JsArray>(1)?;

    let mut members = Vec::with_capacity(js_members.len(&mut cx) as usize);
    for i in 0..js_members.len(&mut cx) {
        let js_member = js_members
            .get(&mut cx, i as u32)?
            .downcast::<JsObject, _>(&mut cx)
            .expect("group_member");
        let user_id = match js_member
            .get(&mut cx, "userId")?
            .downcast::<JsArrayBuffer, _>(&mut cx)
        {
            Ok(handle) => Some(cx.borrow(&handle, |handle| handle.as_slice().to_vec())),
            Err(_) => None,
        };
        let user_id_ciphertext = match js_member
            .get(&mut cx, "userIdCipherText")?
            .downcast::<JsArrayBuffer, _>(&mut cx)
        {
            Ok(handle) => Some(cx.borrow(&handle, |handle| handle.as_slice().to_vec())),
            Err(_) => None,
        };

        match (user_id, user_id_ciphertext) {
            (Some(user_id), Some(user_id_ciphertext)) => {
                members.push(group_call::GroupMemberInfo {
                    user_id,
                    user_id_ciphertext,
                });
            }
            _ => {
                warn!("Ignoring invalid GroupMemberInfo");
            }
        };
    }

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.set_group_members(client_id, members);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setMembershipProof(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let proof = cx.argument::<JsValue>(1)?.as_value(&mut cx);

    let proof: std::vec::Vec<u8> = match proof.downcast::<JsArrayBuffer, _>(&mut cx) {
        Ok(handle) => cx.borrow(&handle, |handle| handle.as_slice().to_vec()),
        Err(_) => {
            return Ok(cx.undefined().upcast());
        }
    };

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.set_membership_proof(client_id, proof);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn peekGroupCall(mut cx: FunctionContext) -> JsResult<JsValue> {
    let request_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as u32;

    let sfu_url = cx.argument::<JsString>(1)?.value(&mut cx) as PeerId;

    let membership_proof = cx.argument::<JsArrayBuffer>(2)?;
    let membership_proof = cx.borrow(&membership_proof, |handle| handle.as_slice().to_vec());

    let js_members = *cx.argument::<JsArray>(3)?;
    let mut members = Vec::with_capacity(js_members.len(&mut cx) as usize);
    for i in 0..js_members.len(&mut cx) {
        let js_member = js_members
            .get(&mut cx, i as u32)?
            .downcast::<JsObject, _>(&mut cx)
            .expect("group_member");
        let user_id = match js_member
            .get(&mut cx, "userId")?
            .downcast::<JsArrayBuffer, _>(&mut cx)
        {
            Ok(handle) => Some(cx.borrow(&handle, |handle| handle.as_slice().to_vec())),
            Err(_) => None,
        };
        let user_id_ciphertext = match js_member
            .get(&mut cx, "userIdCipherText")?
            .downcast::<JsArrayBuffer, _>(&mut cx)
        {
            Ok(handle) => Some(cx.borrow(&handle, |handle| handle.as_slice().to_vec())),
            Err(_) => None,
        };

        match (user_id, user_id_ciphertext) {
            (Some(user_id), Some(user_id_ciphertext)) => {
                members.push(group_call::GroupMemberInfo {
                    user_id,
                    user_id_ciphertext,
                });
            }
            _ => {
                warn!("Ignoring invalid GroupMemberInfo");
            }
        };
    }

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .peek_group_call(request_id, sfu_url, membership_proof, members);
        Ok(())
    })
    .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn getAudioInputs(mut cx: FunctionContext) -> JsResult<JsValue> {
    let devices = with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .peer_connection_factory
            .get_audio_recording_devices()
    })
    .unwrap_or_else(|_| Vec::<AudioDevice>::new());

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

#[allow(non_snake_case)]
fn setAudioInput(mut cx: FunctionContext) -> JsResult<JsValue> {
    let index = cx.argument::<JsNumber>(0)?.value(&mut cx) as u16;
    match with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .peer_connection_factory
            .set_audio_recording_device(index)
    }) {
        Ok(_) => (),
        Err(err) => error!("setAudioInput failed: {}", err),
    };

    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn getAudioOutputs(mut cx: FunctionContext) -> JsResult<JsValue> {
    let devices = with_call_endpoint(&mut cx, |endpoint| {
        endpoint.peer_connection_factory.get_audio_playout_devices()
    })
    .unwrap_or_else(|_| Vec::<AudioDevice>::new());

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

#[allow(non_snake_case)]
fn setAudioOutput(mut cx: FunctionContext) -> JsResult<JsValue> {
    let index = cx.argument::<JsNumber>(0)?.value(&mut cx) as u16;
    match with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .peer_connection_factory
            .set_audio_playout_device(index)
    }) {
        Ok(_) => (),
        Err(err) => error!("setAudioOutput failed: {}", err),
    };

    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn poll(mut cx: FunctionContext) -> JsResult<JsValue> {
    let observer = cx.argument::<JsObject>(0)?;

    let log_entries: Vec<LogMessage> = LOG_MESSAGES
        .lock()
        .expect("lock log messages")
        .drain(0..)
        .collect();
    for log_entry in log_entries.into_iter() {
        let method_name = "onLogMessage";
        let args: Vec<Handle<JsValue>> = vec![
            cx.number(log_entry.level).upcast(),
            cx.string(log_entry.file).upcast(),
            cx.number(log_entry.line).upcast(),
            cx.string(log_entry.message).upcast(),
        ];
        let method = *observer
            .get(&mut cx, method_name)?
            .downcast::<JsFunction, _>(&mut cx)
            .expect("onLogMessage is a function");
        method.call(&mut cx, observer, args)?;
    }

    let events: Vec<Event> = with_call_endpoint(&mut cx, |endpoint| {
        endpoint.events_receiver.try_iter().collect()
    });

    for event in events {
        match event {
            Event::SendSignaling(peer_id, maybe_device_id, call_id, signal) => {
                let (method_name, data1, data2, data3): (
                    &str,
                    Handle<JsValue>,
                    Handle<JsValue>,
                    Handle<JsValue>,
                ) = match signal {
                    signaling::Message::Offer(offer) => {
                        let mut opaque = cx.array_buffer(offer.opaque.len() as u32)?;
                        cx.borrow_mut(&mut opaque, |handle| {
                            handle.as_mut_slice().copy_from_slice(&offer.opaque);
                        });
                        (
                            "onSendOffer",
                            cx.number(offer.call_media_type as i32).upcast(),
                            opaque.upcast(),
                            cx.undefined().upcast(),
                        )
                    }
                    signaling::Message::Answer(answer) => {
                        let mut opaque = cx.array_buffer(answer.opaque.len() as u32)?;
                        cx.borrow_mut(&mut opaque, |handle| {
                            handle.as_mut_slice().copy_from_slice(&answer.opaque);
                        });

                        (
                            "onSendAnswer",
                            opaque.upcast(),
                            cx.undefined().upcast(),
                            cx.undefined().upcast(),
                        )
                    }
                    signaling::Message::Ice(ice) => {
                        let js_candidates =
                            JsArray::new(&mut cx, ice.candidates_added.len() as u32);
                        for (i, candidate) in ice.candidates_added.iter().enumerate() {
                            let opaque: neon::handle::Handle<JsValue> = {
                                let mut js_opaque =
                                    cx.array_buffer(candidate.opaque.len() as u32)?;
                                cx.borrow_mut(&mut js_opaque, |handle| {
                                    handle
                                        .as_mut_slice()
                                        .copy_from_slice(candidate.opaque.as_ref());
                                });
                                js_opaque.upcast()
                            };

                            js_candidates.set(&mut cx, i as u32, opaque)?;
                        }
                        (
                            "onSendIceCandidates",
                            js_candidates.upcast(),
                            cx.undefined().upcast(),
                            cx.undefined().upcast(),
                        )
                    }
                    signaling::Message::Hangup(hangup) => {
                        let (hangup_type, hangup_device_id) = hangup.to_type_and_device_id();
                        let hangup_type = cx.number(hangup_type as i32).upcast();
                        let device_id = match hangup_device_id {
                            Some(device_id) => cx.number(device_id).upcast(),
                            None => cx.null().upcast(),
                        };
                        (
                            "onSendHangup",
                            hangup_type,
                            device_id,
                            cx.undefined().upcast(),
                        )
                    }
                    signaling::Message::LegacyHangup(hangup) => {
                        let (hangup_type, hangup_device_id) = hangup.to_type_and_device_id();
                        let hangup_type = cx.number(hangup_type as i32).upcast();
                        let device_id = match hangup_device_id {
                            Some(device_id) => cx.number(device_id).upcast(),
                            None => cx.null().upcast(),
                        };
                        (
                            "onSendLegacyHangup",
                            hangup_type,
                            device_id,
                            cx.undefined().upcast(),
                        )
                    }
                    signaling::Message::Busy => (
                        "onSendBusy",
                        cx.undefined().upcast(),
                        cx.undefined().upcast(),
                        cx.undefined().upcast(),
                    ),
                };
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                let args = vec![
                    cx.string(peer_id).upcast(),
                    cx.number(maybe_device_id.unwrap_or(0) as f64).upcast(),
                    create_id_arg(&mut cx, call_id.as_u64()),
                    cx.boolean(maybe_device_id.is_none()).upcast(),
                    data1,
                    data2,
                    data3,
                ];
                method.call(&mut cx, observer, args)?;
            }

            Event::CallState(peer_id, CallState::Incoming(call_id, call_media_type)) => {
                let method_name = "onStartIncomingCall";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.string(peer_id).upcast(),
                    create_id_arg(&mut cx, call_id.as_u64()),
                    cx.boolean(call_media_type == CallMediaType::Video).upcast(),
                ];
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect("onStartIncomingCall is a function");
                method.call(&mut cx, observer, args)?;
            }

            // TODO: Dedup this
            Event::CallState(peer_id, CallState::Outgoing(call_id, _call_media_type)) => {
                let method_name = "onStartOutgoingCall";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.string(peer_id).upcast(),
                    create_id_arg(&mut cx, call_id.as_u64()),
                ];
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect("onStartOutgoingCall is a function");
                method.call(&mut cx, observer, args)?;
            }

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
                    EndReason::ReceivedOfferWithGlare => "ReceivedOfferWithGlare",
                    EndReason::SignalingFailure => "SignalingFailure",
                    EndReason::ConnectionFailure => "ConnectionFailure",
                    EndReason::InternalFailure => "InternalFailure",
                    EndReason::Timeout => "Timeout",
                    EndReason::AcceptedOnAnotherDevice => "AcceptedOnAnotherDevice",
                    EndReason::DeclinedOnAnotherDevice => "DeclinedOnAnotherDevice",
                    EndReason::BusyOnAnotherDevice => "BusyOnAnotherDevice",
                    EndReason::CallerIsNotMultiring => "CallerIsNotMultiring",
                };
                let args = vec![cx.string(peer_id), cx.string(reason_string)];
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect("onCallEnded is a function");
                method.call(&mut cx, observer, args)?;
            }

            Event::CallState(peer_id, state) => {
                let method_name = "onCallState";
                let state_string = match state {
                    CallState::Ringing => "ringing",
                    CallState::Connected => "connected",
                    CallState::Connecting => "connecting",
                    // Ignoring Concluded state since application should not treat
                    // it as an 'ending' state transition.
                    CallState::Concluded => return Ok(cx.undefined().upcast()),
                    // All covered above.
                    CallState::Incoming(_, _) => "incoming",
                    CallState::Outgoing(_, _) => "outgoing",
                    CallState::Ended(_) => "ended",
                };
                let args = vec![cx.string(peer_id), cx.string(state_string)];
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect("onCallState is a function");
                method.call(&mut cx, observer, args)?;
            }

            Event::RemoteVideoState(peer_id, enabled) => {
                let method_name = "onRemoteVideoEnabled";
                let args: Vec<Handle<JsValue>> =
                    vec![cx.string(peer_id).upcast(), cx.boolean(enabled).upcast()];
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect("onRemoteVideoEnabled is a function");
                method.call(&mut cx, observer, args)?;
            }

            Event::SendHttpRequest(request_id, url, method, headers, body) => {
                let method_name = "sendHttpRequest";
                // Pass headers as an object with the Fetch API. Only the last value will be sent
                // in case of duplicate headers.
                let js_headers = JsObject::new(&mut cx);
                for (name, value) in headers.iter() {
                    let value = cx.string(value);
                    js_headers.set(&mut cx, name.as_str(), value)?;
                }
                let http_method = method as i32;
                let body = match body {
                    None => cx.undefined().upcast(),
                    Some(body) => {
                        let mut js_body = cx.array_buffer(body.len() as u32)?;
                        cx.borrow_mut(&mut js_body, |handle| {
                            handle.as_mut_slice().copy_from_slice(&body);
                        });
                        js_body.upcast()
                    }
                };
                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(request_id).upcast(),
                    cx.string(url).upcast(),
                    cx.number(http_method).upcast(),
                    js_headers.upcast(),
                    body,
                ];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::SendCallMessage(remote_user_uuid, message) => {
                let method_name = "sendCallMessage";
                let remote_user_uuid = to_js_array_buffer(&mut cx, &remote_user_uuid);
                let message = to_js_array_buffer(&mut cx, &message);
                let args: Vec<Handle<JsValue>> = vec![remote_user_uuid, message];
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect("sendCallMessage is a function");
                method.call(&mut cx, observer, args)?;
            }

            // Group Calls
            Event::GroupUpdate(GroupUpdate::RequestMembershipProof(client_id)) => {
                let method_name = "requestMembershipProof";

                let args: Vec<Handle<JsValue>> = vec![cx.number(client_id).upcast()];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::RequestGroupMembers(client_id)) => {
                let method_name = "requestGroupMembers";

                let args: Vec<Handle<JsValue>> = vec![cx.number(client_id).upcast()];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::ConnectionStateChanged(
                client_id,
                connection_state,
            )) => {
                let method_name = "handleConnectionStateChanged";

                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    cx.number(connection_state as i32).upcast(),
                ];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::JoinStateChanged(client_id, join_state)) => {
                let method_name = "handleJoinStateChanged";

                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    cx.number(match join_state {
                        group_call::JoinState::NotJoined => 0,
                        group_call::JoinState::Joining => 1,
                        group_call::JoinState::Joined(_, _) => 2,
                    })
                    .upcast(),
                ];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::RemoteDeviceStatesChanged(
                client_id,
                remote_device_states,
            )) => {
                let method_name = "handleRemoteDevicesChanged";

                let js_remote_device_states =
                    JsArray::new(&mut cx, remote_device_states.len() as u32);
                for (i, remote_device_state) in remote_device_states.iter().enumerate() {
                    let demux_id = cx.number(remote_device_state.demux_id);
                    let user_id = to_js_array_buffer(&mut cx, &remote_device_state.user_id);
                    let media_keys_received = cx.boolean(remote_device_state.media_keys_received);
                    let audio_muted: neon::handle::Handle<JsValue> =
                        match remote_device_state.audio_muted {
                            None => cx.undefined().upcast(),
                            Some(muted) => cx.boolean(muted).upcast(),
                        };
                    let video_muted: neon::handle::Handle<JsValue> =
                        match remote_device_state.video_muted {
                            None => cx.undefined().upcast(),
                            Some(muted) => cx.boolean(muted).upcast(),
                        };
                    // These are strings because we can't safely convert a u64 to a JavaScript-compatible number. We'll convert them to numeric types on the other side.
                    let added_time: neon::handle::Handle<JsValue> = cx
                        .string(remote_device_state.added_time_as_unix_millis().to_string())
                        .upcast();
                    let speaker_time: neon::handle::Handle<JsValue> = cx
                        .string(
                            remote_device_state
                                .speaker_time_as_unix_millis()
                                .to_string(),
                        )
                        .upcast();

                    let js_remote_device_state = cx.empty_object();
                    js_remote_device_state.set(&mut cx, "demuxId", demux_id)?;
                    js_remote_device_state.set(&mut cx, "userId", user_id)?;
                    js_remote_device_state.set(
                        &mut cx,
                        "mediaKeysReceived",
                        media_keys_received,
                    )?;
                    js_remote_device_state.set(&mut cx, "audioMuted", audio_muted)?;
                    js_remote_device_state.set(&mut cx, "videoMuted", video_muted)?;
                    js_remote_device_state.set(&mut cx, "addedTime", added_time)?;
                    js_remote_device_state.set(&mut cx, "speakerTime", speaker_time)?;

                    js_remote_device_states.set(&mut cx, i as u32, js_remote_device_state)?;
                }

                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    js_remote_device_states.upcast(),
                ];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::IncomingVideoTrack(
                client_id,
                remote_demux_id,
                incoming_video_track,
            )) => {
                with_call_endpoint(&mut cx, |endpoint| {
                    // Warning: this needs to be boxed.  Otherwise, the reference won't work.
                    // TODO: Use the type system to protect against this kind of mistake.
                    let incoming_video_sink =
                        Box::new(OneFrameBuffer::new(true /* enabled */));
                    // TODO: Remove from the map when remote devices no longer have the given demux ID.
                    // It's not a big deal until lots of people leave a group call, which is probably unusual.
                    incoming_video_track.add_sink(incoming_video_sink.as_ref());
                    endpoint
                        .incoming_video_buffer_by_remote_demux_id
                        .insert((client_id, remote_demux_id), incoming_video_sink);
                    Ok(())
                })
                .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
            }

            Event::GroupUpdate(GroupUpdate::PeekChanged(
                client_id,
                members,
                creator,
                era_id,
                max_devices,
                device_count,
            )) => {
                let method_name = "handlePeekChanged";

                let js_members = JsArray::new(&mut cx, members.len() as u32);
                for (i, member) in members.iter().enumerate() {
                    let member: neon::handle::Handle<JsValue> = {
                        let mut js_member = cx.array_buffer(member.len() as u32)?;
                        cx.borrow_mut(&mut js_member, |handle| {
                            handle.as_mut_slice().copy_from_slice(member.as_ref());
                        });
                        js_member.upcast()
                    };
                    js_members.set(&mut cx, i as u32, member)?;
                }
                let js_creator: neon::handle::Handle<JsValue> = match creator {
                    Some(creator) => to_js_array_buffer(&mut cx, &creator).upcast(),
                    None => cx.undefined().upcast(),
                };
                let era_id: neon::handle::Handle<JsValue> = match era_id {
                    None => cx.undefined().upcast(),
                    Some(id) => cx.string(id).upcast(),
                };
                let max_devices: neon::handle::Handle<JsValue> = match max_devices {
                    None => cx.undefined().upcast(),
                    Some(devices) => cx.number(devices).upcast(),
                };
                let device_count: neon::handle::Handle<JsValue> = cx.number(device_count).upcast();

                let js_info = cx.empty_object();
                js_info.set(&mut cx, "joinedMembers", js_members)?;
                js_info.set(&mut cx, "creator", js_creator)?;
                js_info.set(&mut cx, "eraId", era_id)?;
                js_info.set(&mut cx, "maxDevices", max_devices)?;
                js_info.set(&mut cx, "deviceCount", device_count)?;

                let args: Vec<Handle<JsValue>> =
                    vec![cx.number(client_id).upcast(), js_info.upcast()];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::PeekResponse(
                request_id,
                members,
                creator,
                era_id,
                max_devices,
                device_count,
            )) => {
                let method_name = "handlePeekResponse";
                let js_info = cx.empty_object();
                let js_members = JsArray::new(&mut cx, members.len() as u32);
                for (i, member) in members.iter().enumerate() {
                    let member: neon::handle::Handle<JsValue> = {
                        let mut js_member = cx.array_buffer(member.len() as u32)?;
                        cx.borrow_mut(&mut js_member, |handle| {
                            handle.as_mut_slice().copy_from_slice(member.as_ref());
                        });
                        js_member.upcast()
                    };
                    js_members.set(&mut cx, i as u32, member)?;
                }
                let js_creator: neon::handle::Handle<JsValue> = match creator {
                    Some(creator) => to_js_array_buffer(&mut cx, &creator).upcast(),
                    None => cx.undefined().upcast(),
                };
                let era_id: neon::handle::Handle<JsValue> = match era_id {
                    None => cx.undefined().upcast(),
                    Some(id) => cx.string(id).upcast(),
                };
                let max_devices: neon::handle::Handle<JsValue> = match max_devices {
                    None => cx.undefined().upcast(),
                    Some(devices) => cx.number(devices).upcast(),
                };
                let device_count: neon::handle::Handle<JsValue> = cx.number(device_count).upcast();

                js_info.set(&mut cx, "joinedMembers", js_members)?;
                js_info.set(&mut cx, "creator", js_creator)?;
                js_info.set(&mut cx, "eraId", era_id)?;
                js_info.set(&mut cx, "maxDevices", max_devices)?;
                js_info.set(&mut cx, "deviceCount", device_count)?;

                let args: Vec<Handle<JsValue>> =
                    vec![cx.number(request_id).upcast(), js_info.upcast()];
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::Ended(client_id, reason)) => {
                let method_name = "handleEnded";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    cx.number(reason as i32).upcast(),
                ];
                with_call_endpoint(&mut cx, |endpoint| {
                    let ended_client_id = client_id;
                    endpoint.incoming_video_buffer_by_remote_demux_id.retain(
                        |(client_id, _remote_demux_id), _buffer| *client_id != ended_client_id,
                    );
                    Ok(())
                })
                .or_else(|err: failure::Error| cx.throw_error(format!("{}", err)))?;
                let error_message = format!("{} is a function", method_name);
                let method = *observer
                    .get(&mut cx, method_name)?
                    .downcast::<JsFunction, _>(&mut cx)
                    .expect(&error_message);
                method.call(&mut cx, observer, args)?;
            }
        }
    }
    Ok(cx.undefined().upcast())
}

register_module!(mut cx, {
    cx.export_function("createCallEndpoint", createCallEndpoint)?;
    let js_property_key = cx.string(CALL_ENDPOINT_PROPERTY_KEY);
    cx.export_value("callEndpointPropertyKey", js_property_key)?;

    cx.export_function("cm_createOutgoingCall", createOutgoingCall)?;
    cx.export_function("cm_proceed", proceed)?;
    cx.export_function("cm_accept", accept)?;
    cx.export_function("cm_ignore", ignore)?;
    cx.export_function("cm_hangup", hangup)?;
    cx.export_function("cm_signalingMessageSent", signalingMessageSent)?;
    cx.export_function("cm_signalingMessageSendFailed", signalingMessageSendFailed)?;
    cx.export_function("cm_updateBandwidthMode", updateBandwidthMode)?;
    cx.export_function("cm_receivedOffer", receivedOffer)?;
    cx.export_function("cm_receivedAnswer", receivedAnswer)?;
    cx.export_function("cm_receivedIceCandidates", receivedIceCandidates)?;
    cx.export_function("cm_receivedHangup", receivedHangup)?;
    cx.export_function("cm_receivedBusy", receivedBusy)?;
    cx.export_function("cm_receivedCallMessage", receivedCallMessage)?;
    cx.export_function("cm_receivedHttpResponse", receivedHttpResponse)?;
    cx.export_function("cm_httpRequestFailed", httpRequestFailed)?;
    cx.export_function("cm_setOutgoingAudioEnabled", setOutgoingAudioEnabled)?;
    cx.export_function("cm_setOutgoingVideoEnabled", setOutgoingVideoEnabled)?;
    cx.export_function("cm_sendVideoFrame", sendVideoFrame)?;
    cx.export_function("cm_receiveVideoFrame", receiveVideoFrame)?;
    cx.export_function("cm_receiveGroupCallVideoFrame", receiveGroupCallVideoFrame)?;
    cx.export_function("cm_createGroupCallClient", createGroupCallClient)?;
    cx.export_function("cm_deleteGroupCallClient", deleteGroupCallClient)?;
    cx.export_function("cm_connect", connect)?;
    cx.export_function("cm_join", join)?;
    cx.export_function("cm_leave", leave)?;
    cx.export_function("cm_disconnect", disconnect)?;
    cx.export_function("cm_setOutgoingAudioMuted", setOutgoingAudioMuted)?;
    cx.export_function("cm_setOutgoingVideoMuted", setOutgoingVideoMuted)?;
    cx.export_function("cm_resendMediaKeys", resendMediaKeys)?;
    cx.export_function("cm_setBandwidthMode", setBandwidthMode)?;
    cx.export_function("cm_requestVideo", requestVideo)?;
    cx.export_function("cm_setGroupMembers", setGroupMembers)?;
    cx.export_function("cm_setMembershipProof", setMembershipProof)?;
    cx.export_function("cm_peekGroupCall", peekGroupCall)?;
    cx.export_function("cm_getAudioInputs", getAudioInputs)?;
    cx.export_function("cm_setAudioInput", setAudioInput)?;
    cx.export_function("cm_getAudioOutputs", getAudioOutputs)?;
    cx.export_function("cm_setAudioOutput", setAudioOutput)?;
    cx.export_function("cm_poll", poll)?;
    Ok(())
});
