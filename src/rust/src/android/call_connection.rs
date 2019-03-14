//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android CallConnection Interface.

use std::collections::HashMap;
use std::fmt;
use std::thread;

use jni::{
    JNIEnv,
    JavaVM,
};
use jni::objects::{
    JObject,
    JClass,
    JString,
    JList,
    GlobalRef,
};
use jni::sys::{
    jlong,
    jboolean,
    jint,
    JNI_FALSE,
    JNI_TRUE,
};

use crate::android::call_connection_observer::AndroidCallConnectionObserver;
use crate::android::error::{
    AndroidError,
    ServiceError,
};
use crate::android::jni_util::*;
use crate::android::webrtc_java_media_stream::JavaMediaStream;
use crate::common::{
    Result,
    CallId,
    CallState,
    CallDirection,
};
use crate::core::call_connection::{
    CallConnectionInterface,
    CallConnectionHandle,
    ClientStreamTrait,
    ClientRecipientTrait,
};
use crate::core::call_connection_observer::{
    CallConnectionObserver,
    ClientEvent,
};
use crate::error::RingRtcError;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::media_stream::{
    MediaStream,
    RffiMediaStreamInterface,
};
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::sdp_observer::SessionDescriptionInterface;

/// Concrete type for Android ClientStream objects.
pub type AndroidClientStream = GlobalRef;
impl ClientStreamTrait for AndroidClientStream {}

/// Concrete type for Android ClientRecipient objects.
pub type AndroidClientRecipient = GlobalRef;
impl ClientRecipientTrait for AndroidClientRecipient {}

type AndroidCallConnectionHandle = CallConnectionHandle<AndroidCallConnection>;

/// Android implementation of a core::CallConnectionInterface object.
pub struct AndroidCallConnection {
    /// Java JVM object.
    jvm:                      Option<JavaVM>,
    /// Raw pointer to C++ webrtc::jni::OwnedPeerConnection object.
    jni_owned_pc:             Option<jlong>,
    /// Raw pointer to C++ webrtc::PeerConnectionInterface object.
    pc_interface:             Option<PeerConnection>,
    /// Rust DataChannel object.
    data_channel:             Option<DataChannel>,
    /// Rust DataChannelObserver object.
    data_channel_observer:    Option<DataChannelObserver<Self>>,
    /// Java org.signal.ringrtc.CallConnection object.
    jcall_connection:         Option<GlobalRef>,
    /// Java org.signal.ringrtc.SignalMessageRecipient object.
    jrecipient:               GlobalRef,
    /// Cached org.whispersystems.signalservice.api.messages.calls.IceUpdateMessage class.
    ice_update_message_class: Option<GlobalRef>,
    /// Call state variable.
    state:                    CallState,
    /// Unique call identifier.
    call_id:                  CallId,
    /// Call direction.
    direction:                CallDirection,
    /// CallConnectionObserver object.
    cc_observer:              Option<Box<AndroidCallConnectionObserver>>,

    /// For outgoing calls, buffer local ICE candidates until an SDP
    /// answer is received in response to the outbound SDP offer.
    pending_outbound_ice_candidates: Vec<IceCandidate>,

    /// For incoming calls, buffer remote ICE candidates until the SDP
    /// answer is sent in response to the remote SDP offer.
    pending_inbound_ice_candidates: Vec<IceCandidate>,
    stream_map:                     HashMap<*const RffiMediaStreamInterface,
                                            JavaMediaStream>,
}

// needed to share raw *const pointer types
unsafe impl Sync for AndroidCallConnection {}
unsafe impl Send for AndroidCallConnection {}

impl fmt::Display for AndroidCallConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(thread: {:?}, direction: {:?}, jni_owned_pc: {:?}, pc_interface: ({:?}), call_id: 0x{:x}, state: {:?})",
               thread::current().id(), self.direction, self.jni_owned_pc, self.pc_interface, self.call_id, self.state)
    }
}

impl fmt::Debug for AndroidCallConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for AndroidCallConnection {
    fn drop(&mut self) {
        info!("Dropping AndroidCallConnection");
        // ensure this thread is attached to the JVM as our GlobalRefs
        // go out of scope
        let _ = self.get_java_env();
    }
}

impl CallConnectionInterface for AndroidCallConnection {

    type ClientStream = AndroidClientStream;
    type ClientRecipient = AndroidClientRecipient;

    fn get_call_id(&self) -> CallId {
        self.call_id
    }

    fn get_state(&self) -> CallState {
        self.state
    }

    fn set_state(&mut self, state: CallState) {
        self.state = state;
    }

    fn get_direction(&self) -> CallDirection {
        self.direction
    }

    fn set_direction(&mut self, direction: CallDirection) {
        self.direction = direction;
    }

    fn get_pc_interface(&self) -> Result<&PeerConnection> {
        if let Some(pc_interface) = self.pc_interface.as_ref() {
            Ok(pc_interface)
        } else {
            Err(RingRtcError::OptionValueNotSet("get_pc_interface".to_string(),
                                                "pc_interface".to_string()).into())
        }
    }

    fn get_data_channel(&self) -> Result<&DataChannel> {
        if let Some(data_channel) = self.data_channel.as_ref() {
            Ok(data_channel)
        } else {
            Err(RingRtcError::OptionValueNotSet("get_data_channel".to_string(),
                                                "data_channel".to_string()).into())
        }
    }

    fn send_offer(&self) -> Result<()> {

        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        let offer = self.create_offer()?;
        self.set_local_description(&offer)?;

        // send offer via signal
        let result = jni_send_offer(&env, jcall_connection, self.jrecipient.as_obj(), self.call_id, offer)?;
        self.handle_client_result(&env, result, "SendOffer")

    }

    fn accept_answer(&mut self, answer: String) -> Result<()> {

        self.send_pending_ice_updates()?;

        let desc = SessionDescriptionInterface::create_sdp_answer(answer)?;
        self.set_remote_description(&desc)?;
        Ok(())

    }

    fn accept_offer(&self, offer: String) -> Result<()> {

        let desc = SessionDescriptionInterface::create_sdp_offer(offer)?;
        self.set_remote_description(&desc)?;

        let answer = self.create_answer()?;
        self.set_local_description(&answer)?;

        // send answer via signal
        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        let result = jni_send_answer(&env, jcall_connection, self.jrecipient.as_obj(), self.call_id, answer)?;
        self.handle_client_result(&env, result, "AcceptOffer")

    }

    fn add_local_candidate(&mut self, candidate: IceCandidate) {
        self.pending_outbound_ice_candidates.push(candidate);
        debug!("add_local_candidate(): outbound_ice_candidates: {}", self.pending_outbound_ice_candidates.len());
    }

    fn add_remote_candidate(&mut self, candidate: IceCandidate) {
        self.pending_inbound_ice_candidates.push(candidate);
        debug!("add_remote_candidate(): inbound_ice_candidates: {}", self.pending_inbound_ice_candidates.len());
    }

    fn send_pending_ice_updates(&mut self) -> Result<()> {

        if self.pending_outbound_ice_candidates.is_empty() {
            return Ok(());
        }

        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        // convert pending_outbound_ice_candidates vector into Java
        // list of
        // org/whispersystems/signalservice/api/messages/calls/IceUpdateMessage
        // objects.
        let ice_update_list = jni_new_linked_list(&env)?;

        for candidate in &self.pending_outbound_ice_candidates {
            const ICE_UPDATE_MESSAGE_SIG: &str = "(JLjava/lang/String;ILjava/lang/String;)V";

            let sdp_mid = env.new_string(&candidate.sdp_mid)?;
            let sdp = env.new_string(&candidate.sdp)?;
            let args = [ self.call_id.into(),
                         JObject::from(sdp_mid).into(),
                         candidate.sdp_mline_index.into(),
                         JObject::from(sdp).into(),
            ];
            let ice_update_message_obj = env.new_object(self.get_ice_update_message_class()?,
                                                        ICE_UPDATE_MESSAGE_SIG,
                                                        &args)?;
                ice_update_list.add(ice_update_message_obj)?;
        }

        // send ice updates via signal
        let client_result = jni_send_ice_updates(&env, jcall_connection, self.jrecipient.as_obj(), ice_update_list)?;
        let result = self.handle_client_result(&env, client_result, "IceUpdates");

        self.pending_outbound_ice_candidates.clear();
        result
    }

    fn process_remote_ice_updates(&mut self) -> Result<()> {

        if self.pending_inbound_ice_candidates.is_empty() {
            return Ok(());
        }

        debug!("process_remote_ice_updates(): Remote ICE candidates length: {}", self.pending_inbound_ice_candidates.len());
        for candidate in &self.pending_inbound_ice_candidates {
            self.get_pc_interface()?.add_ice_candidate(candidate)?;
        }
        self.pending_inbound_ice_candidates.clear();

        Ok(())
    }

    fn send_signal_message_hang_up(&self) -> Result<()> {
        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        // send hangup via signal
        jni_send_hangup(&env, jcall_connection, self.jrecipient.as_obj(), self.call_id)
    }

    fn send_busy(&self, recipient: Self::ClientRecipient, call_id: CallId) -> Result<()> {
        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;
        let jrecipient = recipient as GlobalRef;

        // send busy via signal
        jni_send_busy(&env, jcall_connection, jrecipient.as_obj(), call_id)
    }

    fn notify_client(&self, event: ClientEvent) -> Result<()> {
        if let Some(observer) = &self.cc_observer {
            info!("android:notify_client(): event: {}", event);
            observer.notify_event(event);
        }
        Ok(())
    }

    fn notify_error(&self, error: failure::Error) -> Result<()> {
        if let Some(observer) = &self.cc_observer {
            observer.notify_error(error);
        }
        Ok(())
    }

    #[allow(clippy::map_entry)]
    fn notify_on_add_stream(&mut self, stream: MediaStream) -> Result<()> {
        let media_stream_interface = stream.get_rffi_interface();
        if !self.stream_map.contains_key(&media_stream_interface) {
            let java_media_stream = JavaMediaStream::new(stream)?;
            self.stream_map.insert(media_stream_interface, java_media_stream);
        }
        let java_media_stream = self.stream_map.get(&media_stream_interface).unwrap();
        let java_media_stream_ref = java_media_stream.get_global_ref(&self.get_java_env()?)?;
        if let Some(observer) = &self.cc_observer {
            observer.notify_on_add_stream(java_media_stream_ref);
        }
        Ok(())
    }

    fn on_data_channel(&mut self,
                       data_channel: DataChannel,
                       cc_handle:    CallConnectionHandle<Self>) -> Result<()>
    {
        debug!("on_data_channel()");
        let dc_observer = DataChannelObserver::new(cc_handle)?;
        data_channel.register_observer(dc_observer.get_rffi_interface())?;
        self.set_data_channel(data_channel);
        self.set_data_channel_observer(dc_observer);
        Ok(())
    }

}

impl AndroidCallConnection {

    /// Create a new AndroidCallConnection object.
    pub fn new(call_id: CallId, direction: CallDirection, jrecipient: GlobalRef) -> Self {

        Self {
            jvm:                   None,
            jni_owned_pc:          None,
            pc_interface:          None,
            data_channel:          None,
            data_channel_observer: None,
            jcall_connection:      None,
            jrecipient,
            state:                 CallState::Idle,
            call_id,
            direction,
            cc_observer:           None,
            stream_map:            Default::default(),
            ice_update_message_class: None,
            pending_outbound_ice_candidates: Vec::new(),
            pending_inbound_ice_candidates:  Vec::new(),
        }
    }

    /// Update a number of AndroidCallConnection fields.
    ///
    /// Initializing an AndroidCallConnection object is a multi-step
    /// process.  This step initializes the object using the input
    /// parameters.
    pub fn update_pc(&mut self, env: &JNIEnv, jni_owned_pc: jlong,
                     pc_interface: PeerConnection,
                     cc_observer:  Box<AndroidCallConnectionObserver>) -> Result<()> {

        let jvm = env.get_java_vm()?;

        self.jvm = Some(jvm);
        self.jni_owned_pc = Some(jni_owned_pc);
        self.pc_interface = Some(pc_interface);
        self.cc_observer = Some(cc_observer);
        self.state = CallState::Idle;

        const ICE_UPDATE_MESSAGE_CLASS: &str = "org/whispersystems/signalservice/api/messages/calls/IceUpdateMessage";
        let ice_update_message_class = match env.find_class(ICE_UPDATE_MESSAGE_CLASS) {
            Ok(v) => v,
            Err(_) => return Err(AndroidError::JniClassLookup(String::from(ICE_UPDATE_MESSAGE_CLASS)).into()),
        };
        self.ice_update_message_class = Some(env.new_global_ref(JObject::from(ice_update_message_class))?);

        Ok(())
    }

    /// Store the DataChannel
    pub fn set_data_channel(&mut self, data_channel: DataChannel) {
        self.data_channel = Some(data_channel);
    }

    /// Store the DataChannelObserver
    pub fn set_data_channel_observer(&mut self, data_channel_observer: DataChannelObserver<Self>) {
        self.data_channel_observer = Some(data_channel_observer);
    }

    /// Return the Java JNIEnv
    fn get_java_env(&self) -> Result <JNIEnv> {

        if let Some(jvm) = self.jvm.as_ref() {
            match jvm.get_env() {
                Ok(v) => Ok(v),
                Err(_e) => Ok(jvm.attach_current_thread_as_daemon()?),
            }
        } else {
            Err(RingRtcError::OptionValueNotSet("get_java_env()".to_string(),
                                                "jvm".to_string()).into())
        }
    }

    /// Set the org.signal.ringrtc.CallConnection object this object belongs to
    fn set_jcall_connection(&mut self, jcall_connection: GlobalRef) {
        self.jcall_connection = Some(jcall_connection);
    }

    /// Return the org.signal.ringrtc.CallConnection object
    fn get_jcall_connection(&self) -> Result<JObject> {
        match self.jcall_connection.as_ref() {
            Some(v) => Ok(v.as_obj()),
            None => Err(RingRtcError::OptionValueNotSet("get_jcall_connection()".to_string(),
                                                        "jcall_connection".to_string()).into()),
        }
    }

    /// Returned the cached
    /// org.whispersystems.signalservice.api.messages.calls.IceUpdateMessage
    /// class.
    fn get_ice_update_message_class(&self) -> Result<JClass> {

        match self.ice_update_message_class.as_ref() {
            Some(v) => Ok(JClass::from(v.as_obj())),
            None => Err(RingRtcError::OptionValueNotSet("get_ice_update_message_class()".to_string(),
                                                        "ice_update_message_class".to_string()).into()),
        }
    }

    /// If the result represents an exception, convert the exception
    /// to a GlobalRef.
    fn handle_client_result(&self, env: &JNIEnv, result: JObject, msg: &str) -> Result<()> {

        let original = result;
        let inner = result.into_inner();
        if !inner.is_null() {
            let global_ref = env.new_global_ref(original)?;
            let service_error = ServiceError::new(global_ref, String::from(msg));
            return Err(AndroidError::JniServiceFailure(service_error).into());
        }
        Ok(())
    }

    /// Shutdown the CallConnection object.
    fn close(&mut self) {
        // dispose of all the media stream objects
        self.stream_map.clear();

        if let Some(data_channel) = self.data_channel.take().as_mut() {
            if let Some(dc_observer) = self.data_channel_observer.take().as_mut() {
                data_channel.unregister_observer(dc_observer.get_rffi_interface());
            }
            data_channel.dispose();
        }
    }

}

/// Return the raw webrtc::PeerConnectionInterface pointer.
pub fn native_get_native_peer_connection(call_connection: jlong) -> Result<jlong> {

    let cc_handle: &AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    let cc = cc_handle.lock()?;
    if let Some(jni_owned_pc) = cc.jni_owned_pc.as_ref() {
        Ok(*jni_owned_pc)
    } else {
        Err(RingRtcError::OptionValueNotSet("native_get_native_peer_connection".to_string(),
                                            "jni_owned_pc".to_string()).into())
    }
}

/// Close the CallConnection.
pub fn native_close_call_connection(call_connection: jlong) -> Result<()> {

    // We want to drop the handle when it goes out of scope here, as this
    // is the destructor.
    let mut cc_handle: Box<AndroidCallConnectionHandle> = get_object_from_jlong(call_connection)?;
    cc_handle.terminate()?;
    if let Ok(mut cc) = cc_handle.lock() {
        cc.close();
    }
    Ok(())
}

/// Send the SDP offer via JNI.
fn jni_send_offer<'a>(env: &'a  JNIEnv,
              jcall_connection: JObject,
              recipient:        JObject,
              call_id:          CallId,
              offer:            SessionDescriptionInterface) -> Result<JObject<'a>> {

    let description = offer.get_description()?;
    info!("jni_send_offer(): {}", description);

    const SEND_OFFER_MESSAGE_METHOD: &str = "sendSignalServiceOffer";
    const SEND_OFFER_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;JLjava/lang/String;)Ljava/lang/Exception;";
    let args = [ recipient.into(), call_id.into(), JObject::from(env.new_string(description)?).into() ];
    let result = jni_call_method(env, jcall_connection,
                                 SEND_OFFER_MESSAGE_METHOD,
                                 SEND_OFFER_MESSAGE_SIG,
                                 &args)?.l()?;

    info!("jni_send_offer(): complete");

    Ok(result)
}

/// Inject a SendOffer event to the FSM.
pub fn native_send_offer(env:              &JNIEnv,
                         jcall_connection: JObject,
                         call_connection:  jlong) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    let jcall_connection_global_ref = env.new_global_ref(jcall_connection)?;

    if let Ok(mut cc) = cc_handle.lock() {
        cc.set_jcall_connection(jcall_connection_global_ref);
    }

    cc_handle.inject_send_offer()
}

/// Create a Rust CallConnectionObserver.
pub fn native_create_call_connection_observer(env:       &JNIEnv,
                                              observer:  JObject,
                                              call_id:   jlong,
                                              recipient: JObject) -> Result<jlong> {
    let jcc_observer = env.new_global_ref(observer)?;
    let jrecipient = env.new_global_ref(recipient)?;
    let cc_observer = AndroidCallConnectionObserver::new(env, jcc_observer, call_id, jrecipient)?;
    let cc_observer_box = Box::new(cc_observer);
    Ok(Box::into_raw(cc_observer_box) as jlong)
}

/// Verify the incoming SDP answer occurs while in the proper state.
pub fn native_validate_response_state(call_connection:   jlong) -> Result<jboolean> {

    let cc_handle: &AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    let cc = cc_handle.lock()?;

    if let CallState::SendingOffer = cc.state {
        Ok(JNI_TRUE)
    } else {
        Ok(JNI_FALSE)
    }

}

/// Inject an AcceptAnswer event into the FSM.
pub fn native_handle_offer_answer(env:             &JNIEnv,
                                  call_connection: jlong,
                                  session_desc:    JString) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    cc_handle.inject_accept_answer(env.get_string(session_desc)?.into())

}

/// Send an SDP answer via JNI.
fn jni_send_answer<'a>(env:              &'a JNIEnv,
                       jcall_connection: JObject,
                       recipient:        JObject,
                       call_id:          CallId,
                       offer:            SessionDescriptionInterface) -> Result<JObject<'a>> {

    let description = offer.get_description()?;
    info!("jni_send_answer(): {}", description);

    const SEND_ANSWER_MESSAGE_METHOD: &str = "sendSignalServiceAnswer";
    const SEND_ANSWER_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;JLjava/lang/String;)Ljava/lang/Exception;";
    let args = [ recipient.into(), call_id.into(), JObject::from(env.new_string(description)?).into() ];
    let result = jni_call_method(env, jcall_connection,
                                 SEND_ANSWER_MESSAGE_METHOD,
                                 SEND_ANSWER_MESSAGE_SIG,
                                 &args)?.l()?;

    info!("jni_send_answer(): complete");

    Ok(result)
}

/// Inject an AcceptOffer event into the FSM.
pub fn native_accept_offer(env:              &JNIEnv,
                           jcall_connection: JObject,
                           call_connection:  jlong,
                           offer:            JString) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    let jcall_connection_global_ref = env.new_global_ref(jcall_connection)?;

    if let Ok(mut cc) = cc_handle.lock() {
        cc.set_jcall_connection(jcall_connection_global_ref);
    }

    cc_handle.inject_accept_offer(env.get_string(offer)?.into())
}

/// Send a ICE update to the remote peer via JNI.
fn jni_send_ice_updates<'a>(env:              &'a JNIEnv,
                            jcall_connection: JObject,
                            recipient:        JObject,
                            ice_update_list:  JList) -> Result<JObject<'a>> {

    info!("jni_send_ice_updates():");

    const SEND_ICE_UPDATE_MESSAGE_METHOD: &str = "sendSignalServiceIceUpdates";
    const SEND_ICE_UPDATE_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;Ljava/util/List;)Ljava/lang/Exception;";
    let args = [
        recipient.into(),
        JObject::from(ice_update_list).into(),
    ];
    let result = jni_call_method(env, jcall_connection,
                                 SEND_ICE_UPDATE_MESSAGE_METHOD,
                                 SEND_ICE_UPDATE_MESSAGE_SIG,
                                 &args)?.l()?;

    info!("jni_send_ice_updates(): complete");

    Ok(result)
}

/// Send a HangUp message to the remote peer via JNI.
fn jni_send_hangup(env:         &JNIEnv,
              jcall_connection: JObject,
              recipient:        JObject,
              call_id:          CallId) -> Result<()> {

    info!("jni_send_hangup():");

    const SEND_HANGUP_MESSAGE_METHOD: &str = "sendSignalServiceHangup";
    const SEND_HANGUP_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;J)V";
    let args = [ recipient.into(), call_id.into() ];
    let _ = jni_call_method(env, jcall_connection,
                            SEND_HANGUP_MESSAGE_METHOD,
                            SEND_HANGUP_MESSAGE_SIG,
                            &args)?;

    info!("jni_send_hangup(): complete");

    Ok(())
}

/// Inject a HangUp event into the FSM.
pub fn native_hang_up(call_connection: jlong) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    cc_handle.inject_hang_up()

}

/// Inject a AnswerCall event into the FSM.
pub fn native_answer_call(call_connection: jlong) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    cc_handle.inject_answer_call()

}

/// Inject a LocalVideoStatus event into the FSM.
pub fn native_send_video_status(call_connection: jlong, enabled: bool) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    cc_handle.inject_local_video_status(enabled)

}

/// Inject a RemoteIceCandidate event into the FSM.
pub fn native_add_ice_candidate(env:              &JNIEnv,
                                call_connection:  jlong,
                                sdp_mid:          JString,
                                sdp_mline_index:  jint,
                                sdp:              JString) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    let ice_candidate = IceCandidate::new(
        env.get_string(sdp_mid)?.into(),
        sdp_mline_index as i32,
        env.get_string(sdp)?.into(),
    );

    cc_handle.inject_remote_ice_candidate(ice_candidate)
}

/// Send a Busy message to the remote peer via JNI.
fn jni_send_busy(env:              &JNIEnv,
                 jcall_connection: JObject,
                 recipient:        JObject,
                 call_id:          CallId) -> Result<()> {

    info!("jni_send_busy():");

    const SEND_BUSY_MESSAGE_METHOD: &str = "sendSignalServiceBusy";
    const SEND_BUSY_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;J)V";
    let args = [ recipient.into(), call_id.into() ];
    let _ = jni_call_method(env, jcall_connection,
                            SEND_BUSY_MESSAGE_METHOD,
                            SEND_BUSY_MESSAGE_SIG,
                            &args)?;

    info!("jni_send_busy(): complete");

    Ok(())
}

/// Inject a SendBusy event into the FSM.
pub fn native_send_busy(env:             &JNIEnv,
                        call_connection: jlong,
                        recipient:       JObject,
                        call_id:         CallId) -> Result<()> {

    let cc_handle: &mut AndroidCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    cc_handle.inject_send_busy(env.new_global_ref(recipient)?, call_id)

}
