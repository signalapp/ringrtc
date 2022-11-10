//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use lazy_static::lazy_static;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::common::{CallId, CallMediaType, DeviceId, Result};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call_manager::CallManager;
use crate::core::group_call;
use crate::core::group_call::{GroupId, SignalingMessageUrgency};
use crate::core::signaling;
use crate::lite::{
    http,
    sfu::{DemuxId, GroupMember, PeekInfo, UserId},
};
use crate::native::{
    CallState, CallStateHandler, EndReason, GroupUpdate, GroupUpdateHandler, NativeCallContext,
    NativePlatform, PeerId, SignalingSender,
};
use crate::webrtc::media::{
    AudioTrack, VideoFrame, VideoPixelFormat, VideoSink, VideoSource, VideoTrack,
};
use crate::webrtc::peer_connection::AudioLevel;
use crate::webrtc::peer_connection_factory::{
    self as pcf, AudioDevice, IceServer, PeerConnectionFactory,
};
use crate::webrtc::peer_connection_observer::NetworkRoute;
use neon::types::buffer::TypedArray;

use neon::prelude::*;

const ENABLE_LOGGING: bool = true;

/// A structure for packing the contents of log messages.
pub struct LogMessage {
    level: i8,
    file: String,
    line: u32,
    message: String,
}

// We store the log messages in a queue to be given to JavaScript when it processes events so
// it can show the messages in the console.
// We could report these as Events, but then logging during event processing would cause
// the event handler to be rescheduled over and over.
static LOG: Log = Log;
lazy_static! {
    static ref LOG_MESSAGES: Mutex<Vec<LogMessage>> = Mutex::new(Vec::new());
    static ref CURRENT_EVENT_REPORTER: Mutex<Option<EventReporter>> = Mutex::new(None);
}

struct Log;

impl log::Log for Log {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let message = LogMessage {
                level: record.level() as i8,
                file: record.file().unwrap().to_string(),
                line: record.line().unwrap(),
                message: record.args().to_string(),
            };

            match CURRENT_EVENT_REPORTER.lock() {
                Ok(reporter) => {
                    if let Some(ref reporter) = *reporter {
                        {
                            let mut messages = LOG_MESSAGES.lock().expect("lock log messages");
                            messages.push(message);
                        }
                        reporter.report()
                    }
                }
                Err(e) => {
                    // The reporter panicked previously. At this point it might not be safe to log.
                    eprintln!("error: could not log to JavaScript: {}", e);
                    eprintln!(
                        "note: message contents: {}:{}: {}",
                        message.file, message.line, message.message
                    );
                }
            }
        }
    }

    fn flush(&self) {}
}

// When JavaScript processes events, we want everything to go through a common queue that
// combines all the things we want to "push" to it.
// (Well, everything except log messages.  See above as to why).
pub enum Event {
    // The JavaScript should send the following signaling message to the given
    // PeerId in context of the given CallId.  If the DeviceId is None, then
    // broadcast to all devices of that PeerId.
    SendSignaling(PeerId, Option<DeviceId>, CallId, signaling::Message),
    // The JavaScript should send the following opaque call message to the
    // given recipient UUID.
    SendCallMessage {
        recipient_uuid: UserId,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    },
    // The JavaScript should send the following opaque call message to all
    // other members of the given group
    SendCallMessageToGroup {
        group_id: GroupId,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    },
    // The call with the given remote PeerId has changed state.
    // We assume only one call per remote PeerId at a time.
    CallState(PeerId, CallId, CallState),
    // The state of the remote video (whether enabled or not) changed.
    // Like call state, we ID the call by PeerId and assume there is only one.
    RemoteVideoStateChange(PeerId, bool),
    // Whether the remote is sharing its screen or not changed.
    // Like call state, we ID the call by PeerId and assume there is only one.
    RemoteSharingScreenChange(PeerId, bool),
    // The group call has an update.
    GroupUpdate(GroupUpdate),
    // JavaScript should initiate an HTTP request.
    SendHttpRequest {
        request_id: u32,
        request: http::Request,
    },
    // The network route changed for a 1:1 call
    NetworkRouteChange(PeerId, NetworkRoute),
    AudioLevels {
        peer_id: PeerId,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    },
}

/// Wraps a [`std::sync::mpsc::Sender`] with a callback to report new events.
#[derive(Clone)]
struct EventReporter {
    sender: Sender<Event>,
    report: Arc<dyn Fn() + Send + Sync>,
}

impl EventReporter {
    fn new(sender: Sender<Event>, report: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            sender,
            report: Arc::new(report),
        }
    }

    fn send(&self, event: Event) -> Result<()> {
        self.sender.send(event)?;
        self.report();
        Ok(())
    }

    fn report(&self) {
        (self.report)();
    }
}

impl SignalingSender for EventReporter {
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

    fn send_call_message(
        &self,
        recipient_uuid: UserId,
        message: Vec<u8>,
        urgency: SignalingMessageUrgency,
    ) -> Result<()> {
        self.send(Event::SendCallMessage {
            recipient_uuid,
            message,
            urgency,
        })?;
        Ok(())
    }

    fn send_call_message_to_group(
        &self,
        group_id: GroupId,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        self.send(Event::SendCallMessageToGroup {
            group_id,
            message,
            urgency,
        })?;
        Ok(())
    }
}

impl CallStateHandler for EventReporter {
    fn handle_call_state(
        &self,
        remote_peer_id: &str,
        call_id: CallId,
        call_state: CallState,
    ) -> Result<()> {
        self.send(Event::CallState(
            remote_peer_id.to_string(),
            call_id,
            call_state,
        ))?;
        Ok(())
    }

    fn handle_network_route(
        &self,
        remote_peer_id: &str,
        network_route: NetworkRoute,
    ) -> Result<()> {
        self.send(Event::NetworkRouteChange(
            remote_peer_id.to_string(),
            network_route,
        ))?;
        Ok(())
    }

    fn handle_remote_video_state(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        self.send(Event::RemoteVideoStateChange(
            remote_peer_id.to_string(),
            enabled,
        ))?;
        Ok(())
    }

    fn handle_remote_sharing_screen(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        self.send(Event::RemoteSharingScreenChange(
            remote_peer_id.to_string(),
            enabled,
        ))?;
        Ok(())
    }

    fn handle_audio_levels(
        &self,
        remote_peer_id: &str,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        self.send(Event::AudioLevels {
            peer_id: remote_peer_id.to_string(),
            captured_level,
            received_level,
        })?;
        Ok(())
    }
}

impl http::Delegate for EventReporter {
    fn send_request(&self, request_id: u32, request: http::Request) {
        let _ = self.send(Event::SendHttpRequest {
            request_id,
            request,
        });
    }
}

impl GroupUpdateHandler for EventReporter {
    fn handle_group_update(&self, update: GroupUpdate) -> Result<()> {
        self.send(Event::GroupUpdate(update))?;
        Ok(())
    }
}

pub struct CallEndpoint {
    call_manager: CallManager<NativePlatform>,

    events_receiver: Receiver<Event>,
    // This is what we use to control mute/not.
    // It should probably be per-call, but for now it's easier to have only one.
    outgoing_audio_track: AudioTrack,
    // This is what we use to push video frames out.
    outgoing_video_source: VideoSource,
    // We only keep this around so we can pass it to PeerConnectionFactory::create_peer_connection
    // via the NativeCallContext.
    outgoing_video_track: VideoTrack,
    // Boxed so we can pass it as a Box<dyn VideoSink>
    incoming_video_sink: Box<LastFramesVideoSink>,

    peer_connection_factory: PeerConnectionFactory,

    // NOTE: This creates a reference cycle, since the JS-side NativeCallManager has a reference
    // to the CallEndpoint box. Since we use the NativeCallManager as a singleton, though, this
    // isn't a problem in practice (except maybe for tests).
    // If Neon ever adds a Weak type, we should use that instead.
    // See https://github.com/neon-bindings/neon/issues/674.
    js_object: Arc<Root<JsObject>>,
}

impl CallEndpoint {
    fn new<'a>(
        cx: &mut impl Context<'a>,
        js_object: Handle<'a, JsObject>,
        use_new_audio_device_module: bool,
    ) -> Result<Self> {
        // Relevant for both group calls and 1:1 calls
        let (events_sender, events_receiver) = channel::<Event>();
        let peer_connection_factory = PeerConnectionFactory::new(pcf::Config {
            use_new_audio_device_module,
            ..Default::default()
        })?;
        let outgoing_audio_track = peer_connection_factory.create_outgoing_audio_track()?;
        outgoing_audio_track.set_enabled(false);
        let outgoing_video_source = peer_connection_factory.create_outgoing_video_source()?;
        let outgoing_video_track =
            peer_connection_factory.create_outgoing_video_track(&outgoing_video_source)?;
        outgoing_video_track.set_enabled(false);
        let incoming_video_sink = Box::<LastFramesVideoSink>::default();

        let event_reported = Arc::new(AtomicBool::new(false));
        let js_object = Arc::new(Root::new(cx, &*js_object));
        let js_object_weak = Arc::downgrade(&js_object);
        let mut js_channel = cx.channel();
        js_channel.unref(cx); // Don't keep Node alive just for this channel.
        let event_reporter = EventReporter::new(events_sender, move || {
            // First check to see if an event has been reported recently.
            // We aren't using this for synchronizing any other memory state,
            // so Relaxed is good enough.
            if event_reported.swap(true, std::sync::atomic::Ordering::Relaxed) {
                return;
            }

            // Then signal the event through the JavaScript channel.
            // Ignore any failures; maybe we're resetting the CallEndpoint,
            // or in the process of quitting the app.
            if let Some(js_object) = js_object_weak.upgrade() {
                let event_reported_for_callback = event_reported.clone();
                let _ = js_channel.try_send(move |mut cx| {
                    // We aren't using this for synchronizing any other memory state,
                    // so Relaxed is good enough.
                    // But we have to do it before the items are actually processed,
                    // because otherwise a new event could come in *during* the processing.
                    event_reported_for_callback.store(false, std::sync::atomic::Ordering::Relaxed);

                    let observer = js_object.as_ref().to_inner(&mut cx);
                    let method_name = "processEvents";
                    let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                    method.call(&mut cx, observer, Vec::<Handle<JsValue>>::new())?;
                    Ok(())
                });
            }
        });

        {
            let event_reporter_for_logging = &mut *CURRENT_EVENT_REPORTER
                .lock()
                .expect("lock event reporter for logging");
            *event_reporter_for_logging = Some(event_reporter.clone());
        }

        // Only relevant for 1:1 calls
        let signaling_sender = Box::new(event_reporter.clone());
        let should_assume_messages_sent = false; // Use async notification from app to send next message.
        let state_handler = Box::new(event_reporter.clone());

        // Only relevant for group calls
        let http_client = http::DelegatingClient::new(event_reporter.clone());
        let group_handler = Box::new(event_reporter);

        let platform = NativePlatform::new(
            peer_connection_factory.clone(),
            signaling_sender,
            should_assume_messages_sent,
            state_handler,
            group_handler,
        );
        let call_manager = CallManager::new(platform, http_client)?;

        Ok(Self {
            call_manager,
            events_receiver,
            outgoing_audio_track,
            outgoing_video_source,
            outgoing_video_track,
            incoming_video_sink,
            peer_connection_factory,
            js_object,
        })
    }
}

#[derive(Clone, Default)]
struct LastFramesVideoSink {
    last_frame_by_track_id: Arc<Mutex<HashMap<u32, VideoFrame>>>,
}

impl VideoSink for LastFramesVideoSink {
    fn on_video_frame(&self, track_id: u32, frame: VideoFrame) {
        self.last_frame_by_track_id
            .lock()
            .unwrap()
            .insert(track_id, frame);
    }

    fn box_clone(&self) -> Box<dyn VideoSink> {
        Box::new(self.clone())
    }
}

impl LastFramesVideoSink {
    fn pop(&self, track_id: u32) -> Option<VideoFrame> {
        self.last_frame_by_track_id
            .lock()
            .unwrap()
            .remove(&track_id)
    }

    fn clear(&self) {
        self.last_frame_by_track_id.lock().unwrap().clear();
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
        obj.get::<JsNumber, _, _>(cx, "high")
            .expect("Get id.high")
            .value(cx),
    );
    let low = js_num_to_u64(
        obj.get::<JsNumber, _, _>(cx, "low")
            .expect("Get id.low")
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

fn to_js_buffer<'a>(cx: &mut FunctionContext<'a>, data: &[u8]) -> Handle<'a, JsValue> {
    let mut js_buffer = cx.buffer(data.len()).expect("create Buffer");
    js_buffer.as_mut_slice(cx).copy_from_slice(data.as_ref());

    js_buffer.upcast()
}

static CALL_ENDPOINT_PROPERTY_KEY: &str = "__call_endpoint_addr";

fn with_call_endpoint<T>(cx: &mut FunctionContext, body: impl FnOnce(&mut CallEndpoint) -> T) -> T {
    let endpoint = cx
        .this()
        .get::<JsBox<RefCell<CallEndpoint>>, _, _>(cx, CALL_ENDPOINT_PROPERTY_KEY)
        .expect("has endpoint");
    let mut endpoint = endpoint.borrow_mut();
    body(&mut endpoint)
}

impl Finalize for CallEndpoint {
    fn finalize<'a, C: Context<'a>>(self, cx: &mut C) {
        self.js_object.finalize(cx)
    }
}

#[allow(non_snake_case)]
fn createCallEndpoint(mut cx: FunctionContext) -> JsResult<JsValue> {
    let js_call_manager = cx.argument::<JsObject>(0)?;
    let use_new_audio_device_module = cx.argument::<JsBoolean>(1)?.value(&mut cx);

    if ENABLE_LOGGING {
        let is_first_time_initializing_logger = log::set_logger(&LOG).is_ok();
        if is_first_time_initializing_logger {
            #[cfg(debug_assertions)]
            log::set_max_level(log::LevelFilter::Debug);

            #[cfg(not(debug_assertions))]
            log::set_max_level(log::LevelFilter::Info);

            // Show WebRTC logs via application Logger while debugging.
            #[cfg(debug_assertions)]
            crate::webrtc::logging::set_logger(log::LevelFilter::Debug);

            #[cfg(not(debug_assertions))]
            crate::webrtc::logging::set_logger(log::LevelFilter::Warn);
        }
    }

    debug!("JsCallManager()");
    let endpoint = CallEndpoint::new(&mut cx, js_call_manager, use_new_audio_device_module)
        .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.boxed(RefCell::new(endpoint)).upcast())
}

#[allow(non_snake_case)]
fn setSelfUuid(mut cx: FunctionContext) -> JsResult<JsValue> {
    debug!("JsCallManager.setSelfUuid()");

    let uuid = cx.argument::<JsBuffer>(0)?;
    let uuid = uuid.as_slice(&cx).to_vec();

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.set_self_uuid(uuid)?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(create_id_arg(&mut cx, call_id.as_u64()))
}

#[allow(non_snake_case)]
fn cancelGroupRing(mut cx: FunctionContext) -> JsResult<JsValue> {
    debug!("JsCallManager.cancelGroupRing()");

    let group_id = cx.argument::<JsBuffer>(0)?;
    let group_id = group_id.as_slice(&cx).to_vec();
    let ring_id = cx
        .argument::<JsString>(1)?
        .value(&mut cx)
        .parse::<i64>()
        .or_else(|_| cx.throw_error("invalid serial number"))?;
    let reason_or_null = cx.argument::<JsValue>(2)?;
    let reason = match reason_or_null.downcast::<JsNull, _>(&mut cx) {
        Ok(_) => None,
        Err(_) => {
            // By checking 'null' first, we get an error message that mentions 'number'.
            let reason = reason_or_null
                .downcast_or_throw::<JsNumber, _>(&mut cx)?
                .value(&mut cx);
            Some(
                group_call::RingCancelReason::try_from(reason as i32)
                    .or_else(|err| cx.throw_error(err.to_string()))?,
            )
        }
    };

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .cancel_group_ring(group_id, ring_id.into(), reason)?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn proceed(mut cx: FunctionContext) -> JsResult<JsValue> {
    let call_id = CallId::new(get_id_arg(&mut cx, 0));
    let ice_server_username = cx.argument::<JsString>(1)?.value(&mut cx);
    let ice_server_password = cx.argument::<JsString>(2)?.value(&mut cx);
    let js_ice_server_urls = cx.argument::<JsArray>(3)?;
    let hide_ip = cx.argument::<JsBoolean>(4)?.value(&mut cx);
    let bandwidth_mode = cx.argument::<JsNumber>(5)?.value(&mut cx) as i32;
    let audio_levels_interval_millis = cx.argument::<JsNumber>(6)?.value(&mut cx) as u64;

    let mut ice_server_urls = Vec::with_capacity(js_ice_server_urls.len(&mut cx) as usize);
    for i in 0..js_ice_server_urls.len(&mut cx) {
        let url: String = js_ice_server_urls
            .get::<JsString, _, _>(&mut cx, i)?
            .value(&mut cx);
        ice_server_urls.push(url);
    }

    info!("proceed(): callId: {}, hideIp: {}", call_id, hide_ip);
    for ice_server_url in &ice_server_urls {
        info!("  server: {}", ice_server_url);
    }

    let ice_server = IceServer::new(ice_server_username, ice_server_password, ice_server_urls);

    let audio_levels_interval = if audio_levels_interval_millis == 0 {
        None
    } else {
        Some(Duration::from_millis(audio_levels_interval_millis))
    };

    with_call_endpoint(&mut cx, |endpoint| {
        let call_context = NativeCallContext::new(
            hide_ip,
            ice_server,
            endpoint.outgoing_audio_track.clone(),
            endpoint.outgoing_video_track.clone(),
            endpoint.incoming_video_sink.clone(),
        );
        endpoint.outgoing_video_track.set_content_hint(false);
        // This should be cleared at with "call concluded", but just in case
        // we'll clear here as well.
        endpoint.incoming_video_sink.clear();
        endpoint.call_manager.proceed(
            call_id,
            call_context,
            BandwidthMode::from_i32(bandwidth_mode),
            audio_levels_interval,
        )?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn hangup(mut cx: FunctionContext) -> JsResult<JsValue> {
    debug!("JsCallManager.hangup()");

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.hangup()?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    let opaque = cx.argument::<JsBuffer>(6)?;
    let sender_identity_key = cx.argument::<JsBuffer>(7)?;
    let receiver_identity_key = cx.argument::<JsBuffer>(8)?;

    let opaque = opaque.as_slice(&cx).to_vec();
    let sender_identity_key = sender_identity_key.as_slice(&cx).to_vec();
    let receiver_identity_key = receiver_identity_key.as_slice(&cx).to_vec();

    let call_media_type = match offer_type {
        1 => CallMediaType::Video,
        _ => CallMediaType::Audio, // TODO: Do something better.  Default matches are evil.
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
                receiver_device_id,
                // An electron client cannot be the primary device.
                receiver_device_is_primary: false,
                sender_identity_key,
                receiver_identity_key,
            },
        )?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedAnswer(mut cx: FunctionContext) -> JsResult<JsValue> {
    let _peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let sender_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let call_id = CallId::new(get_id_arg(&mut cx, 2));
    let opaque = cx.argument::<JsBuffer>(3)?;
    let sender_identity_key = cx.argument::<JsBuffer>(4)?;
    let receiver_identity_key = cx.argument::<JsBuffer>(5)?;

    let opaque = opaque.as_slice(&cx).to_vec();
    let sender_identity_key = sender_identity_key.as_slice(&cx).to_vec();
    let receiver_identity_key = receiver_identity_key.as_slice(&cx).to_vec();

    with_call_endpoint(&mut cx, |endpoint| {
        let answer = signaling::Answer::new(opaque)?;
        endpoint.call_manager.received_answer(
            call_id,
            signaling::ReceivedAnswer {
                answer,
                sender_device_id,
                sender_identity_key,
                receiver_identity_key,
            },
        )?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedIceCandidates(mut cx: FunctionContext) -> JsResult<JsValue> {
    let peer_id = cx.argument::<JsString>(0)?.value(&mut cx) as PeerId;
    let sender_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let call_id = CallId::new(get_id_arg(&mut cx, 2));
    let js_candidates = cx.argument::<JsArray>(3)?;

    let mut candidates = Vec::with_capacity(js_candidates.len(&mut cx) as usize);
    for i in 0..js_candidates.len(&mut cx) {
        let js_candidate = js_candidates.get::<JsBuffer, _, _>(&mut cx, i)?;
        let opaque = js_candidate.as_slice(&cx).to_vec();
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
                ice: signaling::Ice { candidates },
                sender_device_id,
            },
        )?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedCallMessage(mut cx: FunctionContext) -> JsResult<JsValue> {
    let remote_user_id = cx.argument::<JsBuffer>(0)?;
    let remote_user_id = remote_user_id.as_slice(&cx).to_vec();
    let remote_device_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DeviceId;
    let local_device_id = cx.argument::<JsNumber>(2)?.value(&mut cx) as DeviceId;
    let data = cx.argument::<JsBuffer>(3)?;
    let data = data.as_slice(&cx).to_vec();
    let message_age_sec = cx.argument::<JsNumber>(4)?.value(&mut cx) as u64;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.received_call_message(
            remote_user_id,
            remote_device_id,
            local_device_id,
            data,
            Duration::from_secs(message_age_sec),
        )?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receivedHttpResponse(mut cx: FunctionContext) -> JsResult<JsValue> {
    let request_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as u32;
    let status_code = cx.argument::<JsNumber>(1)?.value(&mut cx) as u16;
    let body = cx.argument::<JsBuffer>(2)?;
    let body = body.as_slice(&cx).to_vec();
    let response = http::Response {
        status: status_code.into(),
        body,
    };

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .received_http_response(request_id, Some(response));
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
            .received_http_response(request_id, None);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingAudioEnabled(mut cx: FunctionContext) -> JsResult<JsValue> {
    let enabled = cx.argument::<JsBoolean>(0)?.value(&mut cx);
    info!("#outgoing_audio_enabled: {}", enabled);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_audio_track.set_enabled(enabled);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingVideoEnabled(mut cx: FunctionContext) -> JsResult<JsValue> {
    let enabled = cx.argument::<JsBoolean>(0)?.value(&mut cx);
    debug!("JsCallManager.setOutgoingVideoEnabled({})", enabled);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_video_track.set_enabled(enabled);
        let mut active_connection = endpoint.call_manager.active_connection()?;
        active_connection.update_sender_status(signaling::SenderStatus {
            video_enabled: Some(enabled),
            ..Default::default()
        })?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingVideoIsScreenShare(mut cx: FunctionContext) -> JsResult<JsValue> {
    let is_screenshare = cx.argument::<JsBoolean>(0)?.value(&mut cx);
    debug!(
        "JsCallManager.setOutgoingVideoIsScreenShare({})",
        is_screenshare
    );

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .outgoing_video_track
            .set_content_hint(is_screenshare);
        let mut active_connection = endpoint.call_manager.active_connection()?;
        active_connection.update_sender_status(signaling::SenderStatus {
            sharing_screen: Some(is_screenshare),
            ..Default::default()
        })?;
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn sendVideoFrame(mut cx: FunctionContext) -> JsResult<JsValue> {
    let width = cx.argument::<JsNumber>(0)?.value(&mut cx) as u32;
    let height = cx.argument::<JsNumber>(1)?.value(&mut cx) as u32;
    let pixel_format = cx.argument::<JsNumber>(2)?.value(&mut cx) as i32;
    let buffer = cx.argument::<JsBuffer>(3)?;

    let pixel_format = VideoPixelFormat::from_i32(pixel_format);
    if pixel_format.is_none() {
        return cx.throw_error("Invalid pixel format");
    }
    let pixel_format = pixel_format.unwrap();

    let frame = VideoFrame::copy_from_slice(width, height, pixel_format, buffer.as_slice(&cx));
    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_video_source.push_frame(frame);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn receiveVideoFrame(mut cx: FunctionContext) -> JsResult<JsValue> {
    let mut rgba_buffer = cx.argument::<JsBuffer>(0)?;
    let frame = with_call_endpoint(&mut cx, |endpoint| endpoint.incoming_video_sink.pop(0));
    if let Some(frame) = frame {
        let frame = frame.apply_rotation();
        frame.to_rgba(rgba_buffer.as_mut_slice(&mut cx));
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
    let _client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let remote_demux_id = cx.argument::<JsNumber>(1)?.value(&mut cx) as DemuxId;
    let mut rgba_buffer = cx.argument::<JsBuffer>(2)?;

    let frame = with_call_endpoint(&mut cx, |endpoint| {
        endpoint.incoming_video_sink.pop(remote_demux_id)
    });

    if let Some(frame) = frame {
        let frame = frame.apply_rotation();
        frame.to_rgba(rgba_buffer.as_mut_slice(&mut cx));
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
    let hkdf_extra_info = cx.argument::<JsValue>(2)?.as_value(&mut cx);
    let audio_levels_interval_millis = cx.argument::<JsNumber>(3)?.value(&mut cx) as u64;

    let mut client_id = group_call::INVALID_CLIENT_ID;

    let group_id: std::vec::Vec<u8> = match group_id.downcast::<JsBuffer, _>(&mut cx) {
        Ok(handle) => handle.as_slice(&cx).to_vec(),
        Err(_) => {
            return Ok(cx.number(client_id).upcast());
        }
    };
    let hkdf_extra_info: std::vec::Vec<u8> = match hkdf_extra_info.downcast::<JsBuffer, _>(&mut cx)
    {
        Ok(handle) => handle.as_slice(&cx).to_vec(),
        Err(_) => {
            return Ok(cx.number(client_id).upcast());
        }
    };

    let audio_levels_interval = if audio_levels_interval_millis == 0 {
        None
    } else {
        Some(Duration::from_millis(audio_levels_interval_millis))
    };

    with_call_endpoint(&mut cx, |endpoint| {
        let peer_connection_factory = endpoint.peer_connection_factory.clone();
        let outgoing_audio_track = endpoint.outgoing_audio_track.clone();
        let outgoing_video_track = endpoint.outgoing_video_track.clone();
        let incoming_video_sink = endpoint.incoming_video_sink.clone();
        let result = endpoint.call_manager.create_group_call_client(
            group_id,
            sfu_url,
            hkdf_extra_info,
            audio_levels_interval,
            Some(peer_connection_factory),
            outgoing_audio_track,
            outgoing_video_track,
            Some(incoming_video_sink),
        );
        if let Ok(v) = result {
            client_id = v;
        }

        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.number(client_id).upcast())
}

#[allow(non_snake_case)]
fn deleteGroupCallClient(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.delete_group_call_client(client_id);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn connect(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.connect(client_id);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn join(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.outgoing_video_track.set_content_hint(false);
        endpoint.call_manager.join(client_id);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn leave(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        // When leaving, make sure outgoing media is stopped as soon as possible.
        endpoint.outgoing_audio_track.set_enabled(false);
        endpoint.outgoing_video_track.set_enabled(false);
        endpoint.outgoing_video_track.set_content_hint(false);
        endpoint.call_manager.leave(client_id);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn disconnect(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        // When disconnecting, make sure outgoing media is stopped as soon as possible.
        endpoint.outgoing_audio_track.set_enabled(false);
        endpoint.outgoing_video_track.set_enabled(false);
        endpoint.outgoing_video_track.set_content_hint(false);
        endpoint.call_manager.disconnect(client_id);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setPresenting(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let presenting = cx.argument::<JsBoolean>(1)?.value(&mut cx);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.set_presenting(client_id, presenting);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setOutgoingGroupCallVideoIsScreenShare(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let is_screenshare = cx.argument::<JsBoolean>(1)?.value(&mut cx);

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .outgoing_video_track
            .set_content_hint(is_screenshare);
        endpoint
            .call_manager
            .set_sharing_screen(client_id, is_screenshare);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn groupRing(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let recipient_or_undef = cx.argument::<JsValue>(1)?;
    let recipient = match recipient_or_undef.downcast::<JsUndefined, _>(&mut cx) {
        Ok(_) => None,
        Err(_) => {
            // By checking 'undefined' first, we get an error message that mentions Buffer.
            let recipient_buffer = recipient_or_undef.downcast_or_throw::<JsBuffer, _>(&mut cx)?;
            Some(recipient_buffer.as_slice(&cx).to_vec())
        }
    };

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.group_ring(client_id, recipient);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn resendMediaKeys(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.resend_media_keys(client_id);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn requestVideo(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let js_resolutions = cx.argument::<JsArray>(1)?;
    let active_speaker_height = cx.argument::<JsNumber>(2)?.value(&mut cx) as u16;

    let mut resolutions = Vec::with_capacity(js_resolutions.len(&mut cx) as usize);
    for i in 0..js_resolutions.len(&mut cx) {
        let js_resolution = js_resolutions.get::<JsObject, _, _>(&mut cx, i)?;

        let demux_id = js_resolution
            .get_opt::<JsNumber, _, _>(&mut cx, "demuxId")?
            .map(|handle| handle.value(&mut cx) as DemuxId);
        let width = js_resolution
            .get_opt::<JsNumber, _, _>(&mut cx, "width")?
            .map(|handle| handle.value(&mut cx) as u16);
        let height = js_resolution
            .get_opt::<JsNumber, _, _>(&mut cx, "height")?
            .map(|handle| handle.value(&mut cx) as u16);
        let framerate = js_resolution
            .get_opt::<JsNumber, _, _>(&mut cx, "framerate")?
            .map(|handle| handle.value(&mut cx) as u16);

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
        endpoint
            .call_manager
            .request_video(client_id, resolutions, active_speaker_height);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setGroupMembers(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let js_members = cx.argument::<JsArray>(1)?;

    let mut members = Vec::with_capacity(js_members.len(&mut cx) as usize);
    for i in 0..js_members.len(&mut cx) {
        let js_member = js_members.get::<JsObject, _, _>(&mut cx, i)?;
        let user_id = js_member
            .get_opt::<JsBuffer, _, _>(&mut cx, "userId")?
            .map(|handle| handle.as_slice(&cx).to_vec());
        let member_id = js_member
            .get_opt::<JsBuffer, _, _>(&mut cx, "userIdCipherText")?
            .map(|handle| handle.as_slice(&cx).to_vec());

        match (user_id, member_id) {
            (Some(user_id), Some(member_id)) => {
                members.push(GroupMember { user_id, member_id });
            }
            _ => {
                warn!("Ignoring invalid GroupMember");
            }
        };
    }

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.set_group_members(client_id, members);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn setMembershipProof(mut cx: FunctionContext) -> JsResult<JsValue> {
    let client_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as group_call::ClientId;
    let proof = cx.argument::<JsValue>(1)?.as_value(&mut cx);

    let proof: std::vec::Vec<u8> = match proof.downcast::<JsBuffer, _>(&mut cx) {
        Ok(handle) => handle.as_slice(&cx).to_vec(),
        Err(_) => {
            return Ok(cx.undefined().upcast());
        }
    };

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint.call_manager.set_membership_proof(client_id, proof);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
    Ok(cx.undefined().upcast())
}

#[allow(non_snake_case)]
fn peekGroupCall(mut cx: FunctionContext) -> JsResult<JsValue> {
    let request_id = cx.argument::<JsNumber>(0)?.value(&mut cx) as u32;

    let sfu_url = cx.argument::<JsString>(1)?.value(&mut cx) as PeerId;

    let membership_proof = cx.argument::<JsBuffer>(2)?;
    let membership_proof = membership_proof.as_slice(&cx).to_vec();

    let js_members = cx.argument::<JsArray>(3)?;
    let mut members = Vec::with_capacity(js_members.len(&mut cx) as usize);
    for i in 0..js_members.len(&mut cx) {
        let js_member = js_members.get::<JsObject, _, _>(&mut cx, i)?;
        let user_id = js_member
            .get_opt::<JsBuffer, _, _>(&mut cx, "userId")?
            .map(|handle| handle.as_slice(&cx).to_vec());

        let member_id = js_member
            .get_opt::<JsBuffer, _, _>(&mut cx, "userIdCipherText")?
            .map(|handle| handle.as_slice(&cx).to_vec());

        match (user_id, member_id) {
            (Some(user_id), Some(member_id)) => {
                members.push(GroupMember { user_id, member_id });
            }
            _ => {
                warn!("Ignoring invalid GroupMember");
            }
        };
    }

    with_call_endpoint(&mut cx, |endpoint| {
        endpoint
            .call_manager
            .peek_group_call(request_id, sfu_url, membership_proof, members);
        Ok(())
    })
    .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
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
fn processEvents(mut cx: FunctionContext) -> JsResult<JsValue> {
    let this = cx.this();
    let observer = this.get::<JsObject, _, _>(&mut cx, "observer")?;

    let log_entries = std::mem::take(&mut *LOG_MESSAGES.lock().expect("lock log messages"));
    for log_entry in log_entries {
        let method_name = "onLogMessage";
        let args: Vec<Handle<JsValue>> = vec![
            cx.number(log_entry.level).upcast(),
            cx.string(log_entry.file).upcast(),
            cx.number(log_entry.line).upcast(),
            cx.string(log_entry.message).upcast(),
        ];
        let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
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
                        let mut opaque = cx.buffer(offer.opaque.len())?;
                        opaque.as_mut_slice(&mut cx).copy_from_slice(&offer.opaque);

                        (
                            "onSendOffer",
                            cx.number(offer.call_media_type as i32).upcast(),
                            opaque.upcast(),
                            cx.undefined().upcast(),
                        )
                    }
                    signaling::Message::Answer(answer) => {
                        let mut opaque = cx.buffer(answer.opaque.len())?;
                        opaque.as_mut_slice(&mut cx).copy_from_slice(&answer.opaque);

                        (
                            "onSendAnswer",
                            opaque.upcast(),
                            cx.undefined().upcast(),
                            cx.undefined().upcast(),
                        )
                    }
                    signaling::Message::Ice(ice) => {
                        let js_candidates = JsArray::new(&mut cx, ice.candidates.len() as u32);
                        for (i, candidate) in ice.candidates.iter().enumerate() {
                            let opaque: neon::handle::Handle<JsValue> = {
                                let mut js_opaque = cx.buffer(candidate.opaque.len())?;
                                js_opaque
                                    .as_mut_slice(&mut cx)
                                    .copy_from_slice(candidate.opaque.as_ref());
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
                    signaling::Message::Busy => (
                        "onSendBusy",
                        cx.undefined().upcast(),
                        cx.undefined().upcast(),
                        cx.undefined().upcast(),
                    ),
                };
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
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

            Event::CallState(peer_id, call_id, CallState::Incoming(call_media_type)) => {
                let method_name = "onStartIncomingCall";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.string(peer_id).upcast(),
                    create_id_arg(&mut cx, call_id.as_u64()),
                    cx.boolean(call_media_type == CallMediaType::Video).upcast(),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::CallState(peer_id, call_id, CallState::Outgoing(_call_media_type)) => {
                let method_name = "onStartOutgoingCall";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.string(peer_id).upcast(),
                    create_id_arg(&mut cx, call_id.as_u64()),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::CallState(peer_id, call_id, CallState::Ended(reason)) => {
                let method_name = "onCallEnded";
                let reason_string = match reason {
                    EndReason::LocalHangup => "LocalHangup",
                    EndReason::RemoteHangup => "RemoteHangup",
                    EndReason::RemoteHangupNeedPermission => "RemoteHangupNeedPermission",
                    EndReason::Declined => "Declined",
                    EndReason::Busy => "Busy",
                    EndReason::Glare => "Glare",
                    EndReason::ReCall => "ReCall",
                    EndReason::ReceivedOfferExpired { .. } => "ReceivedOfferExpired",
                    EndReason::ReceivedOfferWhileActive => "ReceivedOfferWhileActive",
                    EndReason::ReceivedOfferWithGlare => "ReceivedOfferWithGlare",
                    EndReason::SignalingFailure => "SignalingFailure",
                    EndReason::GlareFailure => "GlareFailure",
                    EndReason::ConnectionFailure => "ConnectionFailure",
                    EndReason::InternalFailure => "InternalFailure",
                    EndReason::Timeout => "Timeout",
                    EndReason::AcceptedOnAnotherDevice => "AcceptedOnAnotherDevice",
                    EndReason::DeclinedOnAnotherDevice => "DeclinedOnAnotherDevice",
                    EndReason::BusyOnAnotherDevice => "BusyOnAnotherDevice",
                };
                let age = match reason {
                    EndReason::ReceivedOfferExpired { age } => age,
                    _ => Duration::ZERO,
                };
                let args = vec![
                    cx.string(peer_id).upcast(),
                    create_id_arg(&mut cx, call_id.as_u64()),
                    cx.string(reason_string).upcast(),
                    cx.number(age.as_secs_f64()).upcast(),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::CallState(peer_id, _call_id, state) => {
                let method_name = "onCallState";
                let state_string = match state {
                    CallState::Ringing => "ringing",
                    CallState::Connected => "connected",
                    CallState::Connecting => "connecting",
                    CallState::Concluded => {
                        // "Call Concluded" means that the core won't issue anymore
                        // notifications or events for the call. The Desktop client
                        // doesn't currently need this information for its state.

                        // However, it's a great time to clear things.
                        with_call_endpoint(&mut cx, |endpoint| {
                            endpoint.incoming_video_sink.clear();
                        });

                        // Make sure to keep handling subsequent events in this batch.
                        continue;
                    }
                    // All covered above.
                    CallState::Incoming(_) => "incoming",
                    CallState::Outgoing(_) => "outgoing",
                    CallState::Ended(_) => "ended",
                };
                let args = vec![
                    cx.string(peer_id).upcast(),
                    cx.string(state_string).upcast(),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::NetworkRouteChange(peer_id, network_route) => {
                let method_name = "onNetworkRouteChanged";
                let args = [
                    cx.string(peer_id).upcast::<JsValue>(),
                    cx.number(network_route.local_adapter_type as i32).upcast(),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::RemoteVideoStateChange(peer_id, enabled) => {
                if enabled {
                    // Clear out data from the last time video was enabled.
                    with_call_endpoint(&mut cx, |endpoint| {
                        endpoint.incoming_video_sink.clear();
                    });
                }

                let method_name = "onRemoteVideoEnabled";
                let args: Vec<Handle<JsValue>> =
                    vec![cx.string(peer_id).upcast(), cx.boolean(enabled).upcast()];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::RemoteSharingScreenChange(peer_id, enabled) => {
                let method_name = "onRemoteSharingScreen";
                let args: Vec<Handle<JsValue>> =
                    vec![cx.string(peer_id).upcast(), cx.boolean(enabled).upcast()];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::AudioLevels {
                peer_id,
                captured_level,
                received_level,
            } => {
                let method_name = "onAudioLevels";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.string(peer_id).upcast(),
                    cx.number(captured_level).upcast(),
                    cx.number(received_level).upcast(),
                ];

                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::SendHttpRequest {
                request_id,
                request:
                    http::Request {
                        method,
                        url,
                        headers,
                        body,
                    },
            } => {
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
                        let mut js_body = cx.buffer(body.len())?;
                        js_body.as_mut_slice(&mut cx).copy_from_slice(&body);
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
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::SendCallMessage {
                recipient_uuid,
                message,
                urgency,
            } => {
                let method_name = "sendCallMessage";
                let recipient_uuid = to_js_buffer(&mut cx, &recipient_uuid);
                let message = to_js_buffer(&mut cx, &message);
                let urgency = cx.number(urgency as i32).upcast();
                let args: Vec<Handle<JsValue>> = vec![recipient_uuid, message, urgency];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::SendCallMessageToGroup {
                group_id,
                message,
                urgency,
            } => {
                let method_name = "sendCallMessageToGroup";
                let group_id = to_js_buffer(&mut cx, &group_id);
                let message = to_js_buffer(&mut cx, &message);
                let urgency = cx.number(urgency as i32).upcast();
                let args: Vec<Handle<JsValue>> = vec![group_id, message, urgency];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            // Group Calls
            Event::GroupUpdate(GroupUpdate::RequestMembershipProof(client_id)) => {
                let method_name = "requestMembershipProof";

                let args: Vec<Handle<JsValue>> = vec![cx.number(client_id).upcast()];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::RequestGroupMembers(client_id)) => {
                let method_name = "requestGroupMembers";

                let args: Vec<Handle<JsValue>> = vec![cx.number(client_id).upcast()];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
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
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::NetworkRouteChanged(client_id, network_route)) => {
                let method_name = "handleNetworkRouteChanged";

                let args = [
                    cx.number(client_id).upcast::<JsValue>(),
                    cx.number(network_route.local_adapter_type as i32).upcast(),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::JoinStateChanged(client_id, join_state)) => {
                let method_name = "handleJoinStateChanged";

                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    cx.number(match join_state {
                        group_call::JoinState::NotJoined(_) => 0,
                        group_call::JoinState::Joining => 1,
                        group_call::JoinState::Joined(_) => 2,
                    })
                    .upcast(),
                    match join_state {
                        group_call::JoinState::Joined(demux_id) => cx.number(demux_id).upcast(),
                        _ => cx.null().upcast(),
                    },
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
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
                    let user_id = to_js_buffer(&mut cx, &remote_device_state.user_id);
                    let media_keys_received = cx.boolean(remote_device_state.media_keys_received);
                    let audio_muted: neon::handle::Handle<JsValue> =
                        match remote_device_state.heartbeat_state.audio_muted {
                            None => cx.undefined().upcast(),
                            Some(muted) => cx.boolean(muted).upcast(),
                        };
                    let video_muted: neon::handle::Handle<JsValue> =
                        match remote_device_state.heartbeat_state.video_muted {
                            None => cx.undefined().upcast(),
                            Some(muted) => cx.boolean(muted).upcast(),
                        };
                    let presenting: neon::handle::Handle<JsValue> =
                        match remote_device_state.heartbeat_state.presenting {
                            None => cx.undefined().upcast(),
                            Some(muted) => cx.boolean(muted).upcast(),
                        };
                    let sharing_screen: neon::handle::Handle<JsValue> =
                        match remote_device_state.heartbeat_state.sharing_screen {
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
                    let forwarding_video: neon::handle::Handle<JsValue> =
                        match remote_device_state.forwarding_video {
                            None => cx.undefined().upcast(),
                            Some(forwarding_video) => cx.boolean(forwarding_video).upcast(),
                        };
                    let is_higher_resolution_pending =
                        cx.boolean(remote_device_state.is_higher_resolution_pending);

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
                    js_remote_device_state.set(&mut cx, "presenting", presenting)?;
                    js_remote_device_state.set(&mut cx, "sharingScreen", sharing_screen)?;
                    js_remote_device_state.set(&mut cx, "addedTime", added_time)?;
                    js_remote_device_state.set(&mut cx, "speakerTime", speaker_time)?;
                    js_remote_device_state.set(&mut cx, "forwardingVideo", forwarding_video)?;
                    js_remote_device_state.set(
                        &mut cx,
                        "isHigherResolutionPending",
                        is_higher_resolution_pending,
                    )?;

                    js_remote_device_states.set(&mut cx, i as u32, js_remote_device_state)?;
                }

                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    js_remote_device_states.upcast(),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::PeekChanged {
                client_id,
                peek_info,
            }) => {
                let PeekInfo {
                    devices,
                    creator,
                    era_id,
                    max_devices,
                    device_count,
                } = peek_info;

                let method_name = "handlePeekChanged";

                let js_devices = JsArray::new(&mut cx, devices.len() as u32);
                for (i, device) in devices.into_iter().enumerate() {
                    let js_device = cx.empty_object();
                    let js_demux_id = cx.number(device.demux_id);
                    js_device.set(&mut cx, "demuxId", js_demux_id)?;
                    if let Some(user_id) = device.user_id {
                        let js_user_id = to_js_buffer(&mut cx, &user_id);
                        js_device.set(&mut cx, "userId", js_user_id)?;
                    }
                    js_devices.set(&mut cx, i as u32, js_device)?;
                }
                let js_creator: neon::handle::Handle<JsValue> = match creator {
                    Some(creator) => to_js_buffer(&mut cx, &creator).upcast(),
                    None => cx.undefined().upcast(),
                };
                let era_id: neon::handle::Handle<JsValue> = match era_id {
                    None => cx.undefined().upcast(),
                    Some(id) => cx.string(id).upcast(),
                };
                let max_devices: neon::handle::Handle<JsValue> = match max_devices {
                    None => cx.undefined().upcast(),
                    Some(max_devices) => cx.number(max_devices).upcast(),
                };
                let device_count: neon::handle::Handle<JsValue> = cx.number(device_count).upcast();

                let js_info = cx.empty_object();
                js_info.set(&mut cx, "devices", js_devices)?;
                js_info.set(&mut cx, "creator", js_creator)?;
                js_info.set(&mut cx, "eraId", era_id)?;
                js_info.set(&mut cx, "maxDevices", max_devices)?;
                js_info.set(&mut cx, "deviceCount", device_count)?;

                let args: Vec<Handle<JsValue>> =
                    vec![cx.number(client_id).upcast(), js_info.upcast()];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::PeekResult {
                request_id,
                peek_result,
            }) => {
                // TODO: Pass failure error codes to app.
                let PeekInfo {
                    devices,
                    creator,
                    era_id,
                    max_devices,
                    device_count,
                } = peek_result.unwrap_or_default();

                let method_name = "handlePeekResponse";
                let js_info = cx.empty_object();
                let js_devices = JsArray::new(&mut cx, devices.len() as u32);
                for (i, device) in devices.into_iter().enumerate() {
                    let js_device = cx.empty_object();
                    let js_demux_id = cx.number(device.demux_id);
                    js_device.set(&mut cx, "demuxId", js_demux_id)?;
                    if let Some(user_id) = device.user_id {
                        let js_user_id = to_js_buffer(&mut cx, &user_id);
                        js_device.set(&mut cx, "userId", js_user_id)?;
                    }
                    js_devices.set(&mut cx, i as u32, js_device)?;
                }
                let js_creator: neon::handle::Handle<JsValue> = match creator {
                    Some(creator) => to_js_buffer(&mut cx, &creator).upcast(),
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

                js_info.set(&mut cx, "devices", js_devices)?;
                js_info.set(&mut cx, "creator", js_creator)?;
                js_info.set(&mut cx, "eraId", era_id)?;
                js_info.set(&mut cx, "maxDevices", max_devices)?;
                js_info.set(&mut cx, "deviceCount", device_count)?;

                let args: Vec<Handle<JsValue>> =
                    vec![cx.number(request_id).upcast(), js_info.upcast()];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::Ended(client_id, reason)) => {
                let method_name = "handleEnded";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    cx.number(reason as i32).upcast(),
                ];
                with_call_endpoint(&mut cx, |endpoint| {
                    endpoint.incoming_video_sink.clear();
                    Ok(())
                })
                .or_else(|err: anyhow::Error| cx.throw_error(format!("{}", err)))?;
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::Ring {
                group_id,
                ring_id,
                sender,
                update,
            }) => {
                let method_name = "groupCallRingUpdate";

                let args = [
                    to_js_buffer(&mut cx, &group_id).upcast::<JsValue>(),
                    cx.string(ring_id.to_string()).upcast(),
                    to_js_buffer(&mut cx, &sender).upcast(),
                    cx.number(update as i32).upcast(),
                ];
                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }

            Event::GroupUpdate(GroupUpdate::AudioLevels(
                client_id,
                captured_level,
                received_levels,
            )) => {
                let js_received_levels = JsArray::new(&mut cx, received_levels.len() as u32);
                for (i, received_level) in received_levels.iter().enumerate() {
                    let js_received_level = JsObject::new(&mut cx);
                    let js_demux_id = cx.number(received_level.demux_id);
                    js_received_level.set(&mut cx, "demuxId", js_demux_id)?;
                    let js_level = cx.number(received_level.level);
                    js_received_level.set(&mut cx, "level", js_level)?;
                    js_received_levels.set(&mut cx, i as u32, js_received_level)?;
                }

                let method_name = "handleAudioLevels";
                let args: Vec<Handle<JsValue>> = vec![
                    cx.number(client_id).upcast(),
                    cx.number(captured_level).upcast(),
                    js_received_levels.upcast(),
                ];

                let method = observer.get::<JsFunction, _, _>(&mut cx, method_name)?;
                method.call(&mut cx, observer, args)?;
            }
        }
    }
    Ok(cx.undefined().upcast())
}

#[neon::main]
fn register(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("createCallEndpoint", createCallEndpoint)?;
    let js_property_key = cx.string(CALL_ENDPOINT_PROPERTY_KEY);
    cx.export_value("callEndpointPropertyKey", js_property_key)?;

    cx.export_function("cm_setSelfUuid", setSelfUuid)?;
    cx.export_function("cm_createOutgoingCall", createOutgoingCall)?;
    cx.export_function("cm_cancelGroupRing", cancelGroupRing)?;
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
    cx.export_function(
        "cm_setOutgoingVideoIsScreenShare",
        setOutgoingVideoIsScreenShare,
    )?;
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
    cx.export_function("cm_setPresenting", setPresenting)?;
    cx.export_function(
        "cm_setOutgoingGroupCallVideoIsScreenShare",
        setOutgoingGroupCallVideoIsScreenShare,
    )?;
    cx.export_function("cm_groupRing", groupRing)?;
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
    cx.export_function("cm_processEvents", processEvents)?;
    Ok(())
}
