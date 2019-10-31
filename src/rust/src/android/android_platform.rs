//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android CallPlatform Interface.

use std::fmt;

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
};
use crate::core::call_connection::{
    CallConnection,
    CallPlatform,
    AppMediaStreamTrait,
};
use crate::core::call_connection_observer::{
    CallConnectionObserver,
    ClientEvent,
};
use crate::core::util::{
    ptr_as_box,
    ptr_as_mut,
    ptr_as_ref,
};
use crate::error::RingRtcError;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media_stream::MediaStream;
use crate::webrtc::sdp_observer::SessionDescriptionInterface;

/// Concrete type for Android AppMediaStream objects.
pub type AndroidMediaStream = JavaMediaStream;
impl AppMediaStreamTrait for AndroidMediaStream {}

/// Public type for Android CallConnection object.
pub type AndroidCallConnection = CallConnection<AndroidPlatform>;

/// Android implementation of core::CallPlatform.
pub struct AndroidPlatform {
    /// Java JVM object.
    jvm:                      JavaVM,
    /// Java org.signal.ringrtc.SignalMessageRecipient object.
    recipient:                GlobalRef,
    /// Raw pointer to C++ webrtc::jni::OwnedPeerConnection object.
    jni_owned_pc:             Option<jlong>,
    /// Java org.signal.ringrtc.CallConnection object.
    jcall_connection:         Option<GlobalRef>,
    /// Cached org.whispersystems.signalservice.api.messages.calls.IceUpdateMessage class.
    ice_update_message_class: Option<GlobalRef>,
    /// CallConnectionObserver object.
    cc_observer:              Option<Box<AndroidCallConnectionObserver>>,
}

unsafe impl Sync for AndroidPlatform {}
unsafe impl Send for AndroidPlatform {}

impl fmt::Display for AndroidPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let jni_owned_pc = match self.jni_owned_pc {
            Some(v) => format!("0x{:x}", v),
            None    => "None".to_string(),
        };
        write!(f, "jni_owned_pc: {}", jni_owned_pc)
    }
}

impl fmt::Debug for AndroidPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for AndroidPlatform {
    fn drop(&mut self) {
        info!("Dropping AndroidPlatform");
        // ensure this thread is attached to the JVM as our GlobalRefs
        // go out of scope
        let _ = self.get_java_env();
    }
}

impl CallPlatform for AndroidPlatform {

    type AppMediaStream = AndroidMediaStream;

    fn app_send_offer(&self,
                      call_id:   CallId,
                      offer:     SessionDescriptionInterface) -> Result<()> {

        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        // send offer via signal
        let result = jni_send_offer(&env, jcall_connection, self.recipient.as_obj(), call_id, offer)?;
        self.handle_client_result(&env, result, "SendOffer")
    }

    fn app_send_answer(&self,
                       call_id:   CallId,
                       answer:    SessionDescriptionInterface) -> Result<()> {

        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        // send answer via signal
        let result = jni_send_answer(&env, jcall_connection, self.recipient.as_obj(), call_id, answer)?;
        self.handle_client_result(&env, result, "AcceptOffer")

    }

    fn app_send_ice_updates(&self,
                            call_id:    CallId,
                            candidates: &[IceCandidate]) -> Result<()> {

        if candidates.is_empty() {
            return Ok(());
        }

        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        // convert ice_candidates slice into Java list of
        // org/whispersystems/signalservice/api/messages/calls/IceUpdateMessage
        // objects.
        let ice_update_list = jni_new_linked_list(&env)?;

        let call_id_jlong = call_id as jlong;

        for candidate in candidates {
            const ICE_UPDATE_MESSAGE_SIG: &str = "(JLjava/lang/String;ILjava/lang/String;)V";

            let sdp_mid = env.new_string(&candidate.sdp_mid)?;
            let sdp = env.new_string(&candidate.sdp)?;
            let args = [ call_id_jlong.into(),
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
        let client_result = jni_send_ice_updates(&env, jcall_connection, self.recipient.as_obj(), ice_update_list)?;
        self.handle_client_result(&env, client_result, "IceUpdates")
    }

    fn app_send_hangup(&self, call_id: CallId) -> Result<()> {

        let env = self.get_java_env()?;
        let jcall_connection = self.get_jcall_connection()?;

        // send hangup via signal
        jni_send_hangup(&env, jcall_connection, self.recipient.as_obj(), call_id)
    }

    fn create_media_stream(&self, stream: MediaStream) -> Result<Self::AppMediaStream> {
        JavaMediaStream::new(stream)
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

    /// Notify the client application about an avilable MediaStream.
    fn notify_on_add_stream(&self, stream: &Self::AppMediaStream) -> Result<()> {
        let java_media_stream = stream as &JavaMediaStream;
        let java_media_stream_ref = java_media_stream.get_global_ref(&self.get_java_env()?)?;
        if let Some(observer) = &self.cc_observer {
            observer.notify_on_add_stream(java_media_stream_ref);
        }
        Ok(())
    }
}

impl AndroidPlatform {

    /// Create a new AndroidPlatform object.
    pub fn new(jvm: JavaVM, recipient: GlobalRef) -> Self {

        Self {
            jvm,
            recipient,
            jni_owned_pc:             None,
            jcall_connection:         None,
            ice_update_message_class: None,
            cc_observer:              None,
        }
    }

    /// Update a number of AndroidPlatform fields.
    ///
    /// Initializing an AndroidPlatform object is a multi-step
    /// process.  This step initializes the object using the input
    /// parameters.
    pub fn update(&mut self,
                  jni_owned_pc: jlong,
                  cc_observer:  Box<AndroidCallConnectionObserver>) -> Result<()> {

        self.jni_owned_pc = Some(jni_owned_pc);
        self.cc_observer = Some(cc_observer);

        let env = self.get_java_env()?;

        const ICE_UPDATE_MESSAGE_CLASS: &str = "org/whispersystems/signalservice/api/messages/calls/IceUpdateMessage";
        let ice_update_message_class = match env.find_class(ICE_UPDATE_MESSAGE_CLASS) {
            Ok(v) => v,
            Err(_) => return Err(AndroidError::JniClassLookup(String::from(ICE_UPDATE_MESSAGE_CLASS)).into()),
        };
        self.ice_update_message_class = Some(env.new_global_ref(JObject::from(ice_update_message_class))?);

        Ok(())
    }

    /// Return the Java JNIEnv.
    fn get_java_env(&self) -> Result <JNIEnv> {

        match self.jvm.get_env() {
            Ok(v) => Ok(v),
            Err(_e) => Ok(self.jvm.attach_current_thread_as_daemon()?),
        }
    }

    /// Set the org.signal.ringrtc.CallConnection object this Rust
    /// object belongs to.
    fn set_jcall_connection(&mut self, jcall_connection: GlobalRef) {
        self.jcall_connection = Some(jcall_connection);
    }

    /// Return the org.signal.ringrtc.CallConnection object.
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

}

/// Return the raw webrtc::PeerConnectionInterface pointer.
pub fn native_get_native_peer_connection(call_connection: *mut AndroidCallConnection) -> Result<jlong> {

    let cc = unsafe { ptr_as_ref(call_connection)? };
    let platform = cc.platform()?;
    if let Some(jni_owned_pc) = platform.jni_owned_pc.as_ref() {
        Ok(*jni_owned_pc)
    } else {
        Err(RingRtcError::OptionValueNotSet("native_get_native_peer_connection".to_string(),
                                            "jni_owned_pc".to_string()).into())
    }
}

/// Close the CallConnection and quiesce related threads.
pub fn native_close_call_connection(call_connection: *mut AndroidCallConnection) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };
    cc.close()
}

/// Dispose of the CallConnection allocated on the heap.
pub fn native_dispose_call_connection(call_connection: *mut AndroidCallConnection) -> Result<()> {

    // Convert the pointer back into a box, allowing it to go out of
    // scope.
    let cc_box = unsafe { ptr_as_box(call_connection)? };

    debug_assert_eq!(CallState::Closed, cc_box.state()?,
                     "Must call close() before calling dispose()!");

    Ok(())
}

/// Send the SDP offer via JNI.
fn jni_send_offer<'a>(env: &'a  JNIEnv,
              jcall_connection: JObject,
              recipient:        JObject,
              call_id:          CallId,
              offer:            SessionDescriptionInterface) -> Result<JObject<'a>> {

    let description = offer.get_description()?;
    info!("jni_send_offer():");

    let call_id_jlong = call_id as jlong;

    const SEND_OFFER_MESSAGE_METHOD: &str = "sendSignalServiceOffer";
    const SEND_OFFER_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;JLjava/lang/String;)Ljava/lang/Exception;";
    let args = [ recipient.into(), call_id_jlong.into(), JObject::from(env.new_string(description)?).into() ];
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
                         call_connection:  *mut AndroidCallConnection) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };

    let jcall_connection_global_ref = env.new_global_ref(jcall_connection)?;

    if let Ok(mut platform) = cc.platform() {
        platform.set_jcall_connection(jcall_connection_global_ref);
    }

    cc.inject_send_offer()
}

/// Create a Rust CallConnectionObserver.
pub fn native_create_call_connection_observer(env:       &JNIEnv,
                                              observer:  JObject,
                                              call_id:   CallId,
                                              recipient: JObject) -> Result<jlong> {
    let jcc_observer = env.new_global_ref(observer)?;
    let jrecipient = env.new_global_ref(recipient)?;
    let cc_observer = AndroidCallConnectionObserver::new(env, jcc_observer, call_id, jrecipient)?;
    let cc_observer_box = Box::new(cc_observer);
    Ok(Box::into_raw(cc_observer_box) as jlong)
}

/// Verify the incoming SDP answer occurs while in the proper state.
pub fn native_validate_response_state(call_connection: *mut AndroidCallConnection) -> Result<jboolean> {

    let cc = unsafe { ptr_as_ref(call_connection)? };

    if let CallState::SendingOffer = cc.state()? {
        Ok(JNI_TRUE)
    } else {
        Ok(JNI_FALSE)
    }

}

/// Inject a HandleAnswer event into the FSM.
pub fn native_handle_answer(env:             &JNIEnv,
                            call_connection: *mut AndroidCallConnection,
                            session_desc:    JString) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };

    cc.inject_handle_answer(env.get_string(session_desc)?.into())

}

/// Send an SDP answer via JNI.
fn jni_send_answer<'a>(env:              &'a JNIEnv,
                       jcall_connection: JObject,
                       recipient:        JObject,
                       call_id:          CallId,
                       offer:            SessionDescriptionInterface) -> Result<JObject<'a>> {

    let description = offer.get_description()?;
    info!("jni_send_answer():");

    let call_id_jlong = call_id as jlong;

    const SEND_ANSWER_MESSAGE_METHOD: &str = "sendSignalServiceAnswer";
    const SEND_ANSWER_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;JLjava/lang/String;)Ljava/lang/Exception;";
    let args = [ recipient.into(), call_id_jlong.into(), JObject::from(env.new_string(description)?).into() ];
    let result = jni_call_method(env, jcall_connection,
                                 SEND_ANSWER_MESSAGE_METHOD,
                                 SEND_ANSWER_MESSAGE_SIG,
                                 &args)?.l()?;

    info!("jni_send_answer(): complete");

    Ok(result)
}

/// Inject a HandleOffer event into the FSM.
pub fn native_handle_offer(env:              &JNIEnv,
                           jcall_connection: JObject,
                           call_connection:  *mut AndroidCallConnection,
                           offer:            JString) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };

    let jcall_connection_global_ref = env.new_global_ref(jcall_connection)?;

    if let Ok(mut platform) = cc.platform() {
        platform.set_jcall_connection(jcall_connection_global_ref);
    }

    cc.inject_handle_offer(env.get_string(offer)?.into())
}

/// Send an ICE update to the remote peer via JNI.
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

    let call_id_jlong = call_id as jlong;

    const SEND_HANGUP_MESSAGE_METHOD: &str = "sendSignalServiceHangup";
    const SEND_HANGUP_MESSAGE_SIG: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;J)V";
    let args = [ recipient.into(), call_id_jlong.into() ];
    let _ = jni_call_method(env, jcall_connection,
                            SEND_HANGUP_MESSAGE_METHOD,
                            SEND_HANGUP_MESSAGE_SIG,
                            &args)?;

    info!("jni_send_hangup(): complete");

    Ok(())
}

/// Inject a HangUp event into the FSM.
pub fn native_hang_up(call_connection: *mut AndroidCallConnection) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };
    cc.inject_hang_up()

}

/// Inject an AcceptCall event into the FSM.
pub fn native_accept_call(call_connection: *mut AndroidCallConnection) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };
    cc.inject_accept_call()

}

/// Inject a LocalVideoStatus event into the FSM.
pub fn native_send_video_status(call_connection: *mut AndroidCallConnection, enabled: bool) -> Result<()> {

    let cc: &mut AndroidCallConnection = unsafe { ptr_as_mut(call_connection)? };
    cc.inject_local_video_status(enabled)

}

/// Inject a RemoteIceCandidate event into the FSM.
pub fn native_add_ice_candidate(env:             &JNIEnv,
                                call_connection: *mut AndroidCallConnection,
                                sdp_mid:         JString,
                                sdp_mline_index: jint,
                                sdp:             JString) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };

    let ice_candidate = IceCandidate::new(
        env.get_string(sdp_mid)?.into(),
        sdp_mline_index as i32,
        env.get_string(sdp)?.into(),
    );

    cc.inject_remote_ice_candidate(ice_candidate)
}
