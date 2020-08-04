//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android Platform Interface.

use std::fmt;
use std::sync::Arc;

use jni::objects::{GlobalRef, JObject, JValue};
use jni::sys::{jint, jlong};
use jni::{JNIEnv, JavaVM};

// use crate::android::call_connection_observer::AndroidCallConnectionObserver;
use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::android::webrtc_java_media_stream::JavaMediaStream;
use crate::common::{ApplicationEvent, CallDirection, CallId, CallMediaType, DeviceId, Result};
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
use crate::core::signaling;
use crate::webrtc::media::MediaStream;

const RINGRTC_PACKAGE: &str = "org/signal/ringrtc";
const CALL_MANAGER_CLASS: &str = "CallManager";
const ICE_CANDIDATE_CLASS: &str = "org/signal/ringrtc/IceCandidate";

/// Android implmentation for platform::Platform::AppIncomingMedia
pub type AndroidMediaStream = JavaMediaStream;
impl PlatformItem for AndroidMediaStream {}

/// Android implmentation for platform::Platform::AppRemotePeer
pub type AndroidGlobalRef = GlobalRef;
impl PlatformItem for AndroidGlobalRef {}

/// Android implmentation for platform::Platform::AppCallContext
struct JavaCallContext {
    /// Java JVM object.
    platform:         AndroidPlatform,
    /// Java CallContext object.
    jni_call_context: GlobalRef,
}

impl Drop for JavaCallContext {
    fn drop(&mut self) {
        info!("JavaCallContext::drop()");

        // call into CMI to close CallContext object
        if let Ok(env) = self.platform.java_env() {
            let jni_call_manager = self.platform.jni_call_manager.as_obj();
            let jni_call_context = self.jni_call_context.as_obj();

            const CLOSE_CALL_METHOD: &str = "closeCall";
            const CLOSE_CALL_SIG: &str = "(Lorg/signal/ringrtc/CallManager$CallContext;)V";
            let args = [jni_call_context.into()];
            let _ = jni_call_method(
                &env,
                jni_call_manager,
                CLOSE_CALL_METHOD,
                CLOSE_CALL_SIG,
                &args,
            );
        }
    }
}

#[derive(Clone)]
pub struct AndroidCallContext {
    inner: Arc<JavaCallContext>,
}

unsafe impl Sync for AndroidCallContext {}
unsafe impl Send for AndroidCallContext {}
impl PlatformItem for AndroidCallContext {}

impl AndroidCallContext {
    pub fn new(platform: AndroidPlatform, jni_call_context: GlobalRef) -> Self {
        Self {
            inner: Arc::new(JavaCallContext {
                platform,
                jni_call_context,
            }),
        }
    }

    pub fn to_jni(&self) -> GlobalRef {
        self.inner.jni_call_context.clone()
    }
}

/// Android implmentation for platform::Platform::AppConnection
struct JavaConnection {
    /// Java JVM object.
    platform:       AndroidPlatform,
    /// Java Connection object.
    jni_connection: GlobalRef,
}

impl Drop for JavaConnection {
    fn drop(&mut self) {
        info!("JavaConnection::drop()");

        // call into CMI to close Connection object
        if let Ok(env) = self.platform.java_env() {
            let jni_call_manager = self.platform.jni_call_manager.as_obj();
            let jni_connection = self.jni_connection.as_obj();

            const CLOSE_CONNECTION_METHOD: &str = "closeConnection";
            const CLOSE_CONNECTION_SIG: &str = "(Lorg/signal/ringrtc/Connection;)V";
            let args = [jni_connection.into()];
            let _ = jni_call_method(
                &env,
                jni_call_manager,
                CLOSE_CONNECTION_METHOD,
                CLOSE_CONNECTION_SIG,
                &args,
            );
        }
    }
}

#[derive(Clone)]
pub struct AndroidConnection {
    inner: Arc<JavaConnection>,
}

unsafe impl Sync for AndroidConnection {}
unsafe impl Send for AndroidConnection {}
impl PlatformItem for AndroidConnection {}

impl AndroidConnection {
    fn new(platform: AndroidPlatform, jni_connection: GlobalRef) -> Self {
        Self {
            inner: Arc::new(JavaConnection {
                platform,
                jni_connection,
            }),
        }
    }

    pub fn to_jni(&self) -> GlobalRef {
        self.inner.jni_connection.clone()
    }
}

/// Android implementation of platform::Platform.
pub struct AndroidPlatform {
    /// Java JVM object.
    jvm:              JavaVM,
    /// Java org.signal.ringrtc.CallManager object.
    jni_call_manager: GlobalRef,
    /// Cache of Java classes needed at runtime
    class_cache:      ClassCache,
}

unsafe impl Sync for AndroidPlatform {}
unsafe impl Send for AndroidPlatform {}

impl fmt::Display for AndroidPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AndroidPlatform")
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
        let _ = self.java_env();
    }
}

impl Platform for AndroidPlatform {
    type AppIncomingMedia = AndroidMediaStream;
    type AppRemotePeer = AndroidGlobalRef;
    type AppConnection = AndroidConnection;
    type AppCallContext = AndroidCallContext;

    fn create_connection(
        &mut self,
        call: &Call<Self>,
        remote_device_id: DeviceId,
        connection_type: ConnectionType,
        signaling_version: signaling::Version,
    ) -> Result<Connection<Self>> {
        info!(
            "create_connection(): call_id: {} remote_device_id: {} signaling_version: {:?}",
            call.call_id(),
            remote_device_id,
            signaling_version
        );

        let connection = Connection::new(call.clone(), remote_device_id, connection_type)?;

        let connection_ptr = connection.get_connection_ptr()?;
        let call_id_jlong = u64::from(call.call_id()) as jlong;
        let jni_remote_device_id = remote_device_id as jint;

        // call into CMI to create webrtc PeerConnection
        let env = self.java_env()?;
        let android_call_context = call.call_context()?;
        let jni_call_context = android_call_context.to_jni();
        let jni_call_manager = self.jni_call_manager.as_obj();

        const CREATE_CONNECTION_METHOD: &str = "createConnection";
        const CREATE_CONNECTION_SIG: &str =
            "(JJILorg/signal/ringrtc/CallManager$CallContext;ZZ)Lorg/signal/ringrtc/Connection;";
        let args = [
            (connection_ptr as jlong).into(),
            call_id_jlong.into(),
            jni_remote_device_id.into(),
            jni_call_context.as_obj().into(),
            signaling_version.enable_dtls().into(),
            signaling_version.enable_rtp_data_channel().into(),
        ];
        let result = jni_call_method(
            &env,
            jni_call_manager,
            CREATE_CONNECTION_METHOD,
            CREATE_CONNECTION_SIG,
            &args,
        )?;

        let jni_connection = result.l()?;
        if (*jni_connection).is_null() {
            return Err(AndroidError::CreateJniConnection.into());
        }
        let jni_connection = env.new_global_ref(jni_connection)?;
        let platform = self.try_clone()?;
        let android_connection = AndroidConnection::new(platform, jni_connection);
        connection.set_app_connection(android_connection)?;

        Ok(connection)
    }

    fn on_start_call(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        direction: CallDirection,
        call_media_type: CallMediaType,
    ) -> Result<()> {
        info!(
            "on_start_call(): call_id: {}, direction: {}",
            call_id, direction
        );

        let env = self.java_env()?;
        let jni_remote = remote_peer.as_obj();
        let jni_call_manager = self.jni_call_manager.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let is_outgoing = match direction {
            CallDirection::OutGoing => true,
            CallDirection::InComing => false,
        };

        let jni_call_media_type = self.java_enum(&env, "CallMediaType", call_media_type as i32)?;

        const START_CALL_METHOD: &str = "onStartCall";
        const START_CALL_SIG: &str =
            "(Lorg/signal/ringrtc/Remote;JZLorg/signal/ringrtc/CallManager$CallMediaType;)V";

        let args = [
            jni_remote.into(),
            call_id_jlong.into(),
            is_outgoing.into(),
            jni_call_media_type.into(),
        ];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            START_CALL_METHOD,
            START_CALL_SIG,
            &args,
        )?;
        Ok(())
    }

    fn on_event(&self, remote_peer: &Self::AppRemotePeer, event: ApplicationEvent) -> Result<()> {
        info!("on_event(): {}", event);

        let env = self.java_env()?;
        let jni_remote = remote_peer.as_obj();

        let jni_event = self.java_enum(&env, "CallEvent", event as i32)?;

        const ON_EVENT_METHOD: &str = "onEvent";
        const ON_EVENT_SIG: &str =
            "(Lorg/signal/ringrtc/Remote;Lorg/signal/ringrtc/CallManager$CallEvent;)V";

        let args = [jni_remote.into(), jni_event.into()];

        let _ = jni_call_method(
            &env,
            self.jni_call_manager.as_obj(),
            ON_EVENT_METHOD,
            ON_EVENT_SIG,
            &args,
        )?;
        Ok(())
    }

    fn on_send_offer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        offer: signaling::Offer,
    ) -> Result<()> {
        // Offers are always broadcast
        // TODO: Simplify Java's onSendOffer method to assume broadcast
        let broadcast = true;
        let receiver_device_id = 0 as DeviceId;

        info!("on_send_offer(): call_id: {}", call_id);

        let env = self.java_env()?;
        let jni_remote = remote_peer.as_obj();
        let jni_call_manager = self.jni_call_manager.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;
        let jni_opaque = match offer.opaque {
            None => JObject::null(),
            Some(opaque) => JObject::from(env.byte_array_from_slice(&opaque)?),
        };
        let jni_sdp = match offer.sdp {
            None => JObject::null(),
            Some(sdp) => JObject::from(env.new_string(sdp)?),
        };
        let jni_call_media_type =
            self.java_enum(&env, "CallMediaType", offer.call_media_type as i32)?;
        const SEND_OFFER_MESSAGE_METHOD: &str = "onSendOffer";
        const SEND_OFFER_MESSAGE_SIG: &str = "(JLorg/signal/ringrtc/Remote;IZ[BLjava/lang/String;Lorg/signal/ringrtc/CallManager$CallMediaType;)V";

        let args = [
            call_id_jlong.into(),
            jni_remote.into(),
            receiver_device_id.into(),
            broadcast.into(),
            jni_opaque.into(),
            jni_sdp.into(),
            jni_call_media_type.into(),
        ];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            SEND_OFFER_MESSAGE_METHOD,
            SEND_OFFER_MESSAGE_SIG,
            &args,
        )?;
        Ok(())
    }

    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendAnswer,
    ) -> Result<()> {
        // Answers are never broadcast
        // TODO: Simplify Java's onSendAnswer method to assume no broadcast
        let broadcast = false;
        let receiver_device_id = send.receiver_device_id;

        info!(
            "on_send_answer(): call_id: {}, receiver_device_id: {}",
            call_id, receiver_device_id
        );

        let env = self.java_env()?;
        let jni_remote = remote_peer.as_obj();
        let jni_call_manager = self.jni_call_manager.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;
        let jni_opaque = match send.answer.opaque {
            None => JObject::null(),
            Some(opaque) => JObject::from(env.byte_array_from_slice(&opaque)?),
        };
        let jni_sdp = match send.answer.sdp {
            None => JObject::null(),
            Some(sdp) => JObject::from(env.new_string(sdp)?),
        };

        const SEND_ANSWER_MESSAGE_METHOD: &str = "onSendAnswer";
        const SEND_ANSWER_MESSAGE_SIG: &str =
            "(JLorg/signal/ringrtc/Remote;IZ[BLjava/lang/String;)V";

        let args = [
            call_id_jlong.into(),
            jni_remote.into(),
            receiver_device_id.into(),
            broadcast.into(),
            jni_opaque.into(),
            jni_sdp.into(),
        ];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            SEND_ANSWER_MESSAGE_METHOD,
            SEND_ANSWER_MESSAGE_SIG,
            &args,
        )?;
        Ok(())
    }

    fn on_send_ice(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendIce,
    ) -> Result<()> {
        let (broadcast, receiver_device_id) = match send.receiver_device_id {
            // The DeviceId doesn't matter if we're broadcasting
            None => (true, 0 as DeviceId),
            Some(receiver_device_id) => (false, receiver_device_id),
        };

        info!(
            "on_send_ice(): call_id: {}, receiver_device_id: {}, broadcast: {}",
            call_id, receiver_device_id, broadcast
        );

        let env = self.java_env()?;
        let jni_remote = remote_peer.as_obj();
        let jni_call_manager = self.jni_call_manager.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;

        // create Java List<org.webrtc.IceCandidate>
        let ice_candidate_class = self.class_cache.get_class(ICE_CANDIDATE_CLASS)?;
        let ice_candidate_list = jni_new_linked_list(&env)?;

        for candidate in send.ice.candidates_added {
            const ICE_CANDIDATE_CTOR_SIG: &str = "([BLjava/lang/String;)V";
            let jni_opaque = match candidate.opaque {
                None => JObject::null(),
                Some(opaque) => JObject::from(env.byte_array_from_slice(&opaque)?),
            };
            let jni_sdp = match candidate.sdp {
                None => JObject::null(),
                Some(sdp) => JObject::from(env.new_string(sdp)?),
            };

            let args = [jni_opaque.into(), jni_sdp.into()];
            let ice_message_obj =
                env.new_object(ice_candidate_class, ICE_CANDIDATE_CTOR_SIG, &args)?;
            ice_candidate_list.add(ice_message_obj)?;
        }

        const ON_SEND_ICE_CANDIDATES_METHOD: &str = "onSendIceCandidates";
        const ON_SEND_ICE_CANDIDATES_SIG: &str =
            "(JLorg/signal/ringrtc/Remote;IZLjava/util/List;)V";

        let args = [
            call_id_jlong.into(),
            jni_remote.into(),
            receiver_device_id.into(),
            broadcast.into(),
            JObject::from(ice_candidate_list).into(),
        ];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            ON_SEND_ICE_CANDIDATES_METHOD,
            ON_SEND_ICE_CANDIDATES_SIG,
            &args,
        )?;
        Ok(())
    }

    fn on_send_hangup(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendHangup,
    ) -> Result<()> {
        // Hangups are always broadcast
        // TODO: Simplify Java's onSendHangup method to assume broadcast
        let broadcast = true;
        let receiver_device_id = 0 as DeviceId;

        info!("on_send_hangup(): call_id: {}", call_id);

        let env = self.java_env()?;
        let jni_remote = remote_peer.as_obj();
        let jni_call_manager = self.jni_call_manager.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;

        let (hangup_type, hangup_device_id) = send.hangup.to_type_and_device_id();
        // We set the device_id to 0 in case it is not defined. It will
        // only be used for hangup types other than Normal.
        let hangup_device_id = hangup_device_id.unwrap_or(0 as DeviceId) as jint;
        let jni_hangup_type = self.java_enum(&env, "HangupType", hangup_type as i32)?;

        const SEND_HANGUP_MESSAGE_METHOD: &str = "onSendHangup";
        const SEND_HANGUP_MESSAGE_SIG: &str =
            "(JLorg/signal/ringrtc/Remote;IZLorg/signal/ringrtc/CallManager$HangupType;IZ)V";

        let args = [
            call_id_jlong.into(),
            jni_remote.into(),
            receiver_device_id.into(),
            broadcast.into(),
            jni_hangup_type.into(),
            hangup_device_id.into(),
            send.use_legacy.into(),
        ];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            SEND_HANGUP_MESSAGE_METHOD,
            SEND_HANGUP_MESSAGE_SIG,
            &args,
        )?;
        Ok(())
    }

    fn on_send_busy(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()> {
        // Busy messages are always broadcast
        // TODO: Simplify Java's onSendBusy method to assume broadcast
        let broadcast = true;
        let receiver_device_id = 0 as DeviceId;

        info!("on_send_busy(): call_id: {}", call_id);

        let env = self.java_env()?;
        let jni_remote = remote_peer.as_obj();
        let jni_call_manager = self.jni_call_manager.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;

        const SEND_BUSY_MESSAGE_METHOD: &str = "onSendBusy";
        const SEND_BUSY_MESSAGE_SIG: &str = "(JLorg/signal/ringrtc/Remote;IZ)V";

        let args = [
            call_id_jlong.into(),
            jni_remote.into(),
            receiver_device_id.into(),
            broadcast.into(),
        ];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            SEND_BUSY_MESSAGE_METHOD,
            SEND_BUSY_MESSAGE_SIG,
            &args,
        )?;
        Ok(())
    }

    fn create_incoming_media(
        &self,
        _connection: &Connection<Self>,
        incoming_media: MediaStream,
    ) -> Result<Self::AppIncomingMedia> {
        JavaMediaStream::new(incoming_media)
    }

    fn connect_incoming_media(
        &self,
        _remote_peer: &Self::AppRemotePeer,
        app_call_context: &Self::AppCallContext,
        incoming_media: &Self::AppIncomingMedia,
    ) -> Result<()> {
        info!("connect_incoming_media():");

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();
        let jni_call_context = app_call_context.to_jni();
        let jni_media_stream = incoming_media.global_ref(&env)?;

        const CONNECT_MEDIA_METHOD: &str = "onConnectMedia";
        const CONNECT_MEDIA_SIG: &str =
            "(Lorg/signal/ringrtc/CallManager$CallContext;Lorg/webrtc/MediaStream;)V";

        let args = [
            jni_call_context.as_obj().into(),
            jni_media_stream.as_obj().into(),
        ];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            CONNECT_MEDIA_METHOD,
            CONNECT_MEDIA_SIG,
            &args,
        )?;
        Ok(())
    }

    fn disconnect_incoming_media(&self, app_call_context: &Self::AppCallContext) -> Result<()> {
        info!("disconnect_incoming_media():");

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();
        let jni_call_context = app_call_context.to_jni();

        const CLOSE_MEDIA_METHOD: &str = "onCloseMedia";
        const CLOSE_MEDIA_SIG: &str = "(Lorg/signal/ringrtc/CallManager$CallContext;)V";

        let args = [jni_call_context.as_obj().into()];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            CLOSE_MEDIA_METHOD,
            CLOSE_MEDIA_SIG,
            &args,
        )?;
        Ok(())
    }

    fn compare_remotes(
        &self,
        remote_peer1: &Self::AppRemotePeer,
        remote_peer2: &Self::AppRemotePeer,
    ) -> Result<bool> {
        info!("remotes_equal():");

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();
        let jni_remote1 = remote_peer1.as_obj();
        let jni_remote2 = remote_peer2.as_obj();

        const COMPARE_REMOTES_METHOD: &str = "compareRemotes";
        const COMPARE_REMOTES_SIG: &str =
            "(Lorg/signal/ringrtc/Remote;Lorg/signal/ringrtc/Remote;)Z";

        let args = [jni_remote1.into(), jni_remote2.into()];
        let result = jni_call_method(
            &env,
            jni_call_manager,
            COMPARE_REMOTES_METHOD,
            COMPARE_REMOTES_SIG,
            &args,
        )?
        .z()?;
        Ok(result)
    }

    fn on_call_concluded(&self, remote_peer: &Self::AppRemotePeer) -> Result<()> {
        info!("on_call_concluded():");

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();
        let jni_remote_peer = remote_peer.as_obj();

        const CALL_CONCLUDED_METHOD: &str = "onCallConcluded";
        const CALL_CONCLUDED_SIG: &str = "(Lorg/signal/ringrtc/Remote;)V";

        let args = [jni_remote_peer.into()];
        let _ = jni_call_method(
            &env,
            jni_call_manager,
            CALL_CONCLUDED_METHOD,
            CALL_CONCLUDED_SIG,
            &args,
        )?;
        Ok(())
    }
}

impl AndroidPlatform {
    /// Create a new AndroidPlatform object.
    pub fn new(env: &JNIEnv, jni_call_manager: GlobalRef) -> Result<Self> {
        let mut class_cache = ClassCache::new();
        for class in &[
            "org/signal/ringrtc/CallManager$CallEvent",
            "org/signal/ringrtc/CallManager$CallMediaType",
            "org/signal/ringrtc/CallManager$HangupType",
            ICE_CANDIDATE_CLASS,
        ] {
            class_cache.add_class(env, class)?;
        }

        Ok(Self {
            jvm: env.get_java_vm()?,
            jni_call_manager,
            class_cache,
        })
    }

    /// Return the Java JNIEnv.
    fn java_env(&self) -> Result<JNIEnv> {
        match self.jvm.get_env() {
            Ok(v) => Ok(v),
            Err(_e) => Ok(self.jvm.attach_current_thread_as_daemon()?),
        }
    }

    pub fn try_clone(&self) -> Result<Self> {
        let env = self.java_env()?;
        Ok(Self {
            jvm:              env.get_java_vm()?,
            jni_call_manager: self.jni_call_manager.clone(),
            class_cache:      self.class_cache.clone(),
        })
    }

    fn java_enum<'a>(&'a self, env: &'a JNIEnv, class: &str, value: i32) -> Result<JObject<'a>> {
        let class_path = format!("{}/{}${}", RINGRTC_PACKAGE, CALL_MANAGER_CLASS, class);
        let class_object = self.class_cache.get_class(&class_path)?;
        const ENUM_FROM_NATIVE_INDEX_METHOD: &str = "fromNativeIndex";
        let method_signature = format!("(I)L{};", class_path);
        let args = [JValue::from(value)];
        match env.call_static_method(
            class_object,
            ENUM_FROM_NATIVE_INDEX_METHOD,
            &method_signature,
            &args,
        ) {
            Ok(v) => Ok(v.l()?),
            Err(_) => Err(AndroidError::JniCallStaticMethod(
                class_path,
                ENUM_FROM_NATIVE_INDEX_METHOD.to_string(),
                method_signature.to_string(),
            )
            .into()),
        }
    }
}
