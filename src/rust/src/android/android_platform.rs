//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Android Platform Interface.

use std::fmt;
use std::sync::Arc;

use jni::objects::{AutoLocal, GlobalRef, JObject, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{JNIEnv, JavaVM};

use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::android::webrtc_java_media_stream::JavaMediaStream;
use crate::common::{
    ApplicationEvent,
    CallDirection,
    CallId,
    CallMediaType,
    DeviceId,
    HttpMethod,
    Result,
};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
use crate::core::{group_call, signaling};
use crate::webrtc::media::{MediaStream, VideoTrack};
use std::collections::HashMap;

const RINGRTC_PACKAGE: &str = "org/signal/ringrtc";
const CALL_MANAGER_CLASS: &str = "CallManager";
const HTTP_HEADER_CLASS: &str = "org/signal/ringrtc/HttpHeader";
const REMOTE_DEVICE_STATE_CLASS: &str = "org/signal/ringrtc/GroupCall$RemoteDeviceState";

/// Android implementation for platform::Platform::AppIncomingMedia
pub type AndroidMediaStream = JavaMediaStream;
impl PlatformItem for AndroidMediaStream {}

/// Android implementation for platform::Platform::AppRemotePeer
pub type AndroidGlobalRef = GlobalRef;
impl PlatformItem for AndroidGlobalRef {}

/// Android implementation for platform::Platform::AppCallContext
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

/// Android implementation for platform::Platform::AppConnection
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

macro_rules! request_update_via_jni {
    (
        $s:ident,
        $f:literal,
        $i:ident
    ) => {{
        let env = match $s.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = $s.jni_call_manager.as_obj();
        let jni_client_id = $i as jlong;

        const METHOD: &str = $f;
        const SIG: &str = "(J)V";

        let args = [jni_client_id.into()];
        let result = jni_call_method(&env, jni_call_manager, METHOD, SIG, &args);
        if result.is_err() {
            error!("jni_call_method: {:?}", result.err());
        }
    }};
}

macro_rules! handle_state_change_via_jni {
    (
        $self:ident,     /// self
        $method:literal, /// The callback method name
        $sig:literal,    /// The signature for the state (has to be const?)
        $parent:literal, /// The parent class
        $class:literal,  /// The class name
        $id:ident,       /// The client_id
        $state:ident     /// The state value to be casted to jint
    ) => {{
        let env = match $self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = $self.jni_call_manager.as_obj();
        let jni_client_id = $id as jlong;
        let jni_state = match $self.java_enum(&env, $parent, $class, $state as jint) {
            Ok(v) => AutoLocal::new(&env, v),
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };

        const METHOD: &str = $method;
        const SIG: &str = $sig;

        let args = [jni_client_id.into(), jni_state.as_obj().into()];
        let result = jni_call_method(&env, jni_call_manager, METHOD, SIG, &args);
        if result.is_err() {
            error!("jni_call_method: {:?}", result.err());
        }
    }};
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
        bandwidth_mode: BandwidthMode,
    ) -> Result<Connection<Self>> {
        info!(
            "create_connection(): call_id: {} remote_device_id: {} signaling_version: {:?}, bandwidth_mode: {}",
            call.call_id(),
            remote_device_id,
            signaling_version,
            bandwidth_mode
        );

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let connection = Connection::new(
            call.clone(),
            remote_device_id,
            connection_type,
            bandwidth_mode,
        )?;

        let connection_ptr = connection.get_connection_ptr()?;
        let call_id_jlong = u64::from(call.call_id()) as jlong;
        let jni_remote_device_id = remote_device_id as jint;

        // call into CMI to create webrtc PeerConnection
        let android_call_context = call.call_context()?;
        let jni_call_context = android_call_context.to_jni();

        const CREATE_CONNECTION_METHOD: &str = "createConnection";
        const CREATE_CONNECTION_SIG: &str =
            "(JJILorg/signal/ringrtc/CallManager$CallContext;ZZ)Lorg/signal/ringrtc/Connection;";
        let args = [
            (connection_ptr as jlong).into(),
            call_id_jlong.into(),
            jni_remote_device_id.into(),
            jni_call_context.as_obj().into(),
            false.into(), /* always disable DTLS */
            true.into(), /* always enable the RTP data channel */
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
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote = remote_peer.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let is_outgoing = match direction {
            CallDirection::OutGoing => true,
            CallDirection::InComing => false,
        };
        let jni_call_media_type = match self.java_enum(
            &env,
            CALL_MANAGER_CLASS,
            "CallMediaType",
            call_media_type as i32,
        ) {
            Ok(v) => AutoLocal::new(&env, v),
            Err(error) => {
                return Err(error);
            }
        };

        const START_CALL_METHOD: &str = "onStartCall";
        const START_CALL_SIG: &str =
            "(Lorg/signal/ringrtc/Remote;JZLorg/signal/ringrtc/CallManager$CallMediaType;)V";

        let args = [
            jni_remote.into(),
            call_id_jlong.into(),
            is_outgoing.into(),
            jni_call_media_type.as_obj().into(),
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
        let jni_event = match self.java_enum(&env, CALL_MANAGER_CLASS, "CallEvent", event as i32) {
            Ok(v) => AutoLocal::new(&env, v),
            Err(error) => {
                return Err(error);
            }
        };

        const ON_EVENT_METHOD: &str = "onEvent";
        const ON_EVENT_SIG: &str =
            "(Lorg/signal/ringrtc/Remote;Lorg/signal/ringrtc/CallManager$CallEvent;)V";

        let args = [jni_remote.into(), jni_event.as_obj().into()];

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
        let broadcast = true;
        let receiver_device_id = 0;

        info!("on_send_offer(): call_id: {}", call_id);

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (3).
        let capacity = 8;
        let _ = env.with_local_frame(capacity, || {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;
            let jni_opaque = JObject::from(env.byte_array_from_slice(&offer.opaque)?);
            let jni_call_media_type = match self.java_enum(
                &env,
                CALL_MANAGER_CLASS,
                "CallMediaType",
                offer.call_media_type as i32,
            ) {
                Ok(v) => v,
                Err(error) => {
                    error!("jni_call_media_type: {:?}", error);
                    return Ok(JObject::null());
                }
            };

            const SEND_OFFER_MESSAGE_METHOD: &str = "onSendOffer";
            const SEND_OFFER_MESSAGE_SIG: &str =
                "(JLorg/signal/ringrtc/Remote;IZ[BLorg/signal/ringrtc/CallManager$CallMediaType;)V";

            let args = [
                call_id_jlong.into(),
                jni_remote.into(),
                receiver_device_id.into(),
                broadcast.into(),
                jni_opaque.into(),
                jni_call_media_type.into(),
            ];
            let result = jni_call_method(
                &env,
                jni_call_manager,
                SEND_OFFER_MESSAGE_METHOD,
                SEND_OFFER_MESSAGE_SIG,
                &args,
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        })?;

        Ok(())
    }

    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendAnswer,
    ) -> Result<()> {
        // Answers are never broadcast
        let broadcast = false;
        let receiver_device_id = send.receiver_device_id;

        info!(
            "on_send_answer(): call_id: {}, receiver_device_id: {}",
            call_id, receiver_device_id
        );

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (2).
        let capacity = 7;
        let _ = env.with_local_frame(capacity, || {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;
            let jni_opaque = JObject::from(env.byte_array_from_slice(&send.answer.opaque)?);

            const SEND_ANSWER_MESSAGE_METHOD: &str = "onSendAnswer";
            const SEND_ANSWER_MESSAGE_SIG: &str = "(JLorg/signal/ringrtc/Remote;IZ[B)V";

            let args = [
                call_id_jlong.into(),
                jni_remote.into(),
                receiver_device_id.into(),
                broadcast.into(),
                jni_opaque.into(),
            ];
            let result = jni_call_method(
                &env,
                jni_call_manager,
                SEND_ANSWER_MESSAGE_METHOD,
                SEND_ANSWER_MESSAGE_SIG,
                &args,
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        })?;

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
            None => (true, 0),
            Some(receiver_device_id) => (false, receiver_device_id),
        };

        info!(
            "on_send_ice(): call_id: {}, receiver_device_id: {}, broadcast: {}",
            call_id, receiver_device_id, broadcast
        );

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (3) + elements (N * 3 object per element).
        let capacity = (8 + send.ice.candidates_added.len() * 3) as i32;
        let _ = env.with_local_frame(capacity, || {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;

            let ice_candidate_list = match jni_new_linked_list(&env) {
                Ok(v) => v,
                Err(error) => {
                    error!("ice_candidate_list: {:?}", error);
                    return Ok(JObject::null());
                }
            };

            for candidate in send.ice.candidates_added {
                let jni_opaque = JObject::from(env.byte_array_from_slice(&candidate.opaque)?);
                let result = ice_candidate_list.add(jni_opaque);
                if result.is_err() {
                    error!("ice_candidate_list.add: {:?}", result.err());
                    continue;
                }
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
            let result = jni_call_method(
                &env,
                jni_call_manager,
                ON_SEND_ICE_CANDIDATES_METHOD,
                ON_SEND_ICE_CANDIDATES_SIG,
                &args,
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        })?;

        Ok(())
    }

    fn on_send_hangup(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendHangup,
    ) -> Result<()> {
        // Hangups are always broadcast
        let broadcast = true;
        let receiver_device_id = 0;

        info!("on_send_hangup(): call_id: {}", call_id);

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote = remote_peer.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;

        let (hangup_type, hangup_device_id) = send.hangup.to_type_and_device_id();
        // We set the device_id to 0 in case it is not defined. It will
        // only be used for hangup types other than Normal.
        let hangup_device_id = hangup_device_id.unwrap_or(0) as jint;
        let jni_hangup_type =
            match self.java_enum(&env, CALL_MANAGER_CLASS, "HangupType", hangup_type as i32) {
                Ok(v) => AutoLocal::new(&env, v),
                Err(error) => {
                    return Err(error);
                }
            };

        const SEND_HANGUP_MESSAGE_METHOD: &str = "onSendHangup";
        const SEND_HANGUP_MESSAGE_SIG: &str =
            "(JLorg/signal/ringrtc/Remote;IZLorg/signal/ringrtc/CallManager$HangupType;IZ)V";

        let args = [
            call_id_jlong.into(),
            jni_remote.into(),
            receiver_device_id.into(),
            broadcast.into(),
            jni_hangup_type.as_obj().into(),
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
        let broadcast = true;
        let receiver_device_id = 0;

        info!("on_send_busy(): call_id: {}", call_id);

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote = remote_peer.as_obj();
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

    fn send_call_message(
        &self,
        recipient_uuid: Vec<u8>,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        info!("send_call_message():");

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (2).
        let capacity = 7;
        env.with_local_frame(capacity, || {
            let jni_recipient_uuid = JObject::from(env.byte_array_from_slice(&recipient_uuid)?);
            let jni_message = JObject::from(env.byte_array_from_slice(&message)?);

            const SEND_CALL_MESSAGE_METHOD: &str = "sendCallMessage";
            const SEND_CALL_MESSAGE_SIG: &str = "([B[BI)V";

            let args = [
                jni_recipient_uuid.into(),
                jni_message.into(),
                (urgency as i32).into(),
            ];
            let result = jni_call_method(
                &env,
                jni_call_manager,
                SEND_CALL_MESSAGE_METHOD,
                SEND_CALL_MESSAGE_SIG,
                &args,
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        })?;

        Ok(())
    }

    fn send_call_message_to_group(
        &self,
        group_id: Vec<u8>,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        info!("send_call_message_to_group():");

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (2).
        let capacity = 7;
        env.with_local_frame(capacity, || {
            let jni_group_id = JObject::from(env.byte_array_from_slice(&group_id)?);
            let jni_message = JObject::from(env.byte_array_from_slice(&message)?);

            const SEND_CALL_MESSAGE_TO_GROUP_METHOD: &str = "sendCallMessageToGroup";
            const SEND_CALL_MESSAGE_TO_GROUP_SIG: &str = "([B[BI)V";

            let args = [
                jni_group_id.into(),
                jni_message.into(),
                (urgency as i32).into(),
            ];
            let result = jni_call_method(
                &env,
                jni_call_manager,
                SEND_CALL_MESSAGE_TO_GROUP_METHOD,
                SEND_CALL_MESSAGE_TO_GROUP_SIG,
                &args,
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        })?;

        Ok(())
    }

    fn send_http_request(
        &self,
        request_id: u32,
        url: String,
        method: HttpMethod,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<()> {
        info!("send_http_request(): request_id: {}", request_id);

        let env = self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (4) + elements (N * 3 objects per element).
        let capacity = (9 + headers.len() * 3) as i32;
        env.with_local_frame(capacity, || {
            let jni_request_id = request_id as jlong;
            let jni_url = JObject::from(env.new_string(url)?);
            let jni_method = match self.java_enum(&env, CALL_MANAGER_CLASS, "HttpMethod", method as i32) {
                Ok(v) => v,
                Err(error) => {
                    error!("jni_method: {:?}", error);
                    return Ok(JObject::null());
                },
            };

            // create Java List<HttpHeader>
            let http_header_class = match self.class_cache.get_class(HTTP_HEADER_CLASS) {
                Ok(v) => v,
                Err(error) => {
                    error!("http_header_class: {:?}", error);
                    return Ok(JObject::null());
                },
            };
            let jni_headers = match jni_new_linked_list(&env) {
                Ok(v) => v,
                Err(error) => {
                    error!("jni_headers: {:?}", error);
                    return Ok(JObject::null());
                },
            };
            for (name, value) in headers.iter() {
                const HTTP_HEADER_CTOR_SIG: &str = "(Ljava/lang/String;Ljava/lang/String;)V";
                let jni_name = JObject::from(env.new_string(name)?);
                let jni_value = JObject::from(env.new_string(value)?);
                let args = [jni_name.into(), jni_value.into()];
                let http_header_obj = env.new_object(http_header_class, HTTP_HEADER_CTOR_SIG, &args)?;
                jni_headers.add(http_header_obj)?;
            }

            let jni_body = match body {
                None => JObject::null(),
                Some(body) => JObject::from(env.byte_array_from_slice(&body)?),
            };

            const SEND_HTTP_REQUEST_METHOD: &str = "sendHttpRequest";
            const SEND_HTTP_REQUEST_SIG: &str =
                "(JLjava/lang/String;Lorg/signal/ringrtc/CallManager$HttpMethod;Ljava/util/List;[B)V";

            let args = [
                jni_request_id.into(),
                jni_url.into(),
                jni_method.into(),
                JObject::from(jni_headers).into(),
                jni_body.into(),
            ];
            let result = jni_call_method(
                &env,
                jni_call_manager,
                SEND_HTTP_REQUEST_METHOD,
                SEND_HTTP_REQUEST_SIG,
                &args,
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        })?;

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

    // Group Calls

    fn group_call_ring_update(
        &self,
        group_id: group_call::GroupId,
        ring_id: group_call::RingId,
        sender: group_call::UserId,
        update: group_call::RingUpdate,
    ) {
        info!("group_call_ring_update():");

        let env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        let group_id = match env.byte_array_from_slice(&group_id) {
            Ok(slice) => JObject::from(slice),
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let ring_id = jlong::from(ring_id);
        let sender = match env.byte_array_from_slice(&sender) {
            Ok(slice) => JObject::from(slice),
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let update = update as jint;

        const GROUP_CALL_RING_UPDATE_METHOD: &str = "groupCallRingUpdate";
        const GROUP_CALL_RING_UPDATE_SIG: &str = "([BJ[BI)V";

        let args = [
            group_id.into(),
            ring_id.into(),
            sender.into(),
            update.into(),
        ];
        let result = jni_call_method(
            &env,
            jni_call_manager,
            GROUP_CALL_RING_UPDATE_METHOD,
            GROUP_CALL_RING_UPDATE_SIG,
            &args,
        );
        if result.is_err() {
            error!("jni_call_method: {:?}", result.err());
        }
    }

    fn handle_peek_response(
        &self,
        request_id: u32,
        joined_members: &[group_call::UserId],
        creator: Option<group_call::UserId>,
        era_id: Option<&str>,
        max_devices: Option<u32>,
        device_count: u32,
    ) {
        info!("handle_peek_response():");

        let env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (5) + elements (N * 1 object per element).
        let capacity = (10 + joined_members.len()) as i32;
        let _ = env.with_local_frame(capacity, || {
            let jni_request_id = request_id as jlong;

            let joined_member_list = match jni_new_linked_list(&env) {
                Ok(v) => v,
                Err(error) => {
                    error!("{:?}", error);
                    return Ok(JObject::null());
                }
            };

            for joined_member in joined_members {
                let jni_opaque_user_id = match env.byte_array_from_slice(joined_member) {
                    Ok(v) => JObject::from(v),
                    Err(error) => {
                        error!("{:?}", error);
                        continue;
                    }
                };

                let result = joined_member_list.add(jni_opaque_user_id);
                if result.is_err() {
                    error!("{:?}", result.err());
                    continue;
                }
            }

            let jni_creator = match creator {
                None => JObject::null(),
                Some(creator) => match env.byte_array_from_slice(&creator) {
                    Ok(v) => JObject::from(v),
                    Err(error) => {
                        error!("{:?}", error);
                        return Ok(JObject::null());
                    }
                },
            };

            let jni_era_id = match era_id {
                None => JObject::null(),
                Some(era_id) => match env.new_string(era_id) {
                    Ok(v) => JObject::from(v),
                    Err(error) => {
                        error!("{:?}", error);
                        return Ok(JObject::null());
                    }
                },
            };

            let jni_max_devices = match self.get_optional_u32_long_object(&env, max_devices) {
                Ok(v) => v,
                Err(error) => {
                    error!("{:?}", error);
                    return Ok(JObject::null());
                }
            };

            let jni_device_count = device_count as jlong;

            const METHOD: &str = "handlePeekResponse";
            const SIG: &str = "(JLjava/util/List;[BLjava/lang/String;Ljava/lang/Long;J)V";
            let args = [
                jni_request_id.into(),
                JObject::from(joined_member_list).into(),
                jni_creator.into(),
                jni_era_id.into(),
                jni_max_devices.into(),
                jni_device_count.into(),
            ];
            let result = jni_call_method(&env, jni_call_manager, METHOD, SIG, &args);
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        });
    }

    fn request_membership_proof(&self, client_id: group_call::ClientId) {
        info!("request_membership_proof():");
        request_update_via_jni!(self, "requestMembershipProof", client_id);
    }

    fn request_group_members(&self, client_id: group_call::ClientId) {
        info!("request_group_members():");
        request_update_via_jni!(self, "requestGroupMembers", client_id);
    }

    fn handle_connection_state_changed(
        &self,
        client_id: group_call::ClientId,
        connection_state: group_call::ConnectionState,
    ) {
        info!("handle_connection_state_changed():");
        handle_state_change_via_jni!(
            self,
            "handleConnectionStateChanged",
            "(JLorg/signal/ringrtc/GroupCall$ConnectionState;)V",
            "GroupCall",
            "ConnectionState",
            client_id,
            connection_state
        );
    }

    fn handle_join_state_changed(
        &self,
        client_id: group_call::ClientId,
        join_state: group_call::JoinState,
    ) {
        info!("handle_join_state_changed():");

        let join_state = match join_state {
            group_call::JoinState::NotJoined(_) => 0,
            group_call::JoinState::Joining => 1,
            group_call::JoinState::Joined(_, _) => 2,
        };

        handle_state_change_via_jni!(
            self,
            "handleJoinStateChanged",
            "(JLorg/signal/ringrtc/GroupCall$JoinState;)V",
            "GroupCall",
            "JoinState",
            client_id,
            join_state
        );
    }

    fn handle_remote_devices_changed(
        &self,
        client_id: group_call::ClientId,
        remote_device_states: &[group_call::RemoteDeviceState],
        _reason: group_call::RemoteDevicesChangedReason,
    ) {
        info!("handle_remote_devices_changed():");

        let env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (2) + elements (N * 2 object per element).
        let capacity = (7 + remote_device_states.len() * 2) as i32;
        let _ = env.with_local_frame(capacity, || {
            let jni_client_id = client_id as jlong;

            // create Java List<GroupCall.RemoteDeviceState>
            let remote_device_state_class =
                match self.class_cache.get_class(REMOTE_DEVICE_STATE_CLASS) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("{:?}", error);
                        return Ok(JObject::null());
                    }
                };

            let remote_device_state_list = match jni_new_linked_list(&env) {
                Ok(v) => v,
                Err(error) => {
                    error!("{:?}", error);
                    return Ok(JObject::null());
                }
            };

            for remote_device_state in remote_device_states {
                const REMOTE_DEVICE_STATE_CTOR_SIG: &str =
                    "(J[BZLjava/lang/Boolean;Ljava/lang/Boolean;Ljava/lang/Boolean;Ljava/lang/Boolean;JJ)V";

                let jni_demux_id = remote_device_state.demux_id as jlong;
                let jni_user_id_byte_array =
                    match env.byte_array_from_slice(&remote_device_state.user_id) {
                        Ok(v) => JObject::from(v),
                        Err(error) => {
                            error!("jni_user_id_byte_array: {:?}", error);
                            continue;
                        }
                    };
                let jni_media_keys_received = remote_device_state.media_keys_received as jboolean;
                let jni_audio_muted = match self.get_optional_boolean_object(
                    &env,
                    remote_device_state.heartbeat_state.audio_muted,
                ) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("jni_audio_muted: {:?}", error);
                        continue;
                    }
                };
                let jni_video_muted = match self.get_optional_boolean_object(
                    &env,
                    remote_device_state.heartbeat_state.video_muted,
                ) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("jni_video_muted: {:?}", error);
                        continue;
                    }
                };
                let jni_presenting = match self.get_optional_boolean_object(
                    &env,
                    remote_device_state.heartbeat_state.presenting,
                ) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("jni_presenting: {:?}", error);
                        continue;
                    }
                };
                let jni_sharing_screen = match self.get_optional_boolean_object(
                    &env,
                    remote_device_state.heartbeat_state.sharing_screen,
                ) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("jni_sharing_screen: {:?}", error);
                        continue;
                    }
                };
                let jni_added_time = remote_device_state.added_time_as_unix_millis() as jlong;
                let jni_speaker_time = remote_device_state.speaker_time_as_unix_millis() as jlong;

                let args = [
                    jni_demux_id.into(),
                    jni_user_id_byte_array.into(),
                    jni_media_keys_received.into(),
                    jni_audio_muted.into(),
                    jni_video_muted.into(),
                    jni_presenting.into(),
                    jni_sharing_screen.into(),
                    jni_added_time.into(),
                    jni_speaker_time.into(),
                ];

                let remote_device_state_obj = match env.new_object(
                    remote_device_state_class,
                    REMOTE_DEVICE_STATE_CTOR_SIG,
                    &args,
                ) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("remote_device_state_obj: {:?}", error);
                        continue;
                    }
                };

                let result = remote_device_state_list.add(remote_device_state_obj);
                if result.is_err() {
                    error!("remote_device_state_list.add: {:?}", result.err());
                    continue;
                }
            }

            const METHOD: &str = "handleRemoteDevicesChanged";
            const SIG: &str = "(JLjava/util/List;)V";

            let args = [
                jni_client_id.into(),
                JObject::from(remote_device_state_list).into(),
            ];
            let result = jni_call_method(&env, jni_call_manager, METHOD, SIG, &args);
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        });
    }

    fn handle_incoming_video_track(
        &self,
        client_id: group_call::ClientId,
        remote_demux_id: group_call::DemuxId,
        incoming_video_track: VideoTrack,
    ) {
        info!("handle_incoming_video_track():");

        let env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_client_id = client_id as jlong;
        let jni_remote_demux_id = remote_demux_id as jlong;
        let jni_native_video_track = incoming_video_track.rffi() as jlong;

        const METHOD: &str = "handleIncomingVideoTrack";
        const SIG: &str = "(JJJ)V";

        let args = [
            jni_client_id.into(),
            jni_remote_demux_id.into(),
            jni_native_video_track.into(),
        ];
        let result = jni_call_method(&env, jni_call_manager, METHOD, SIG, &args);
        if result.is_err() {
            error!("jni_call_method: {:?}", result.err());
        }
    }

    fn handle_peek_changed(
        &self,
        client_id: group_call::ClientId,
        joined_members: &[group_call::UserId],
        creator: Option<group_call::UserId>,
        era_id: Option<&str>,
        max_devices: Option<u32>,
        device_count: u32,
    ) {
        info!("handle_peek_changed():");

        let env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (5) + elements (N * 1 object per element).
        let capacity = (10 + joined_members.len()) as i32;
        let _ = env.with_local_frame(capacity, || {
            let jni_client_id = client_id as jlong;

            let joined_member_list = match jni_new_linked_list(&env) {
                Ok(v) => v,
                Err(error) => {
                    error!("{:?}", error);
                    return Ok(JObject::null());
                }
            };

            for joined_member in joined_members {
                let jni_opaque_user_id = match env.byte_array_from_slice(joined_member) {
                    Ok(v) => JObject::from(v),
                    Err(error) => {
                        error!("{:?}", error);
                        continue;
                    }
                };

                let result = joined_member_list.add(jni_opaque_user_id);
                if result.is_err() {
                    error!("{:?}", result.err());
                    continue;
                }
            }

            let jni_creator = match creator {
                None => JObject::null(),
                Some(creator) => match env.byte_array_from_slice(&creator) {
                    Ok(v) => JObject::from(v),
                    Err(error) => {
                        error!("{:?}", error);
                        return Ok(JObject::null());
                    }
                },
            };

            let jni_era_id = match era_id {
                None => JObject::null(),
                Some(era_id) => match env.new_string(era_id) {
                    Ok(v) => JObject::from(v),
                    Err(error) => {
                        error!("{:?}", error);
                        return Ok(JObject::null());
                    }
                },
            };

            let jni_max_devices = match self.get_optional_u32_long_object(&env, max_devices) {
                Ok(v) => v,
                Err(error) => {
                    error!("{:?}", error);
                    return Ok(JObject::null());
                }
            };

            let jni_device_count = device_count as jlong;

            const METHOD: &str = "handlePeekChanged";
            const SIG: &str = "(JLjava/util/List;[BLjava/lang/String;Ljava/lang/Long;J)V";
            let args = [
                jni_client_id.into(),
                JObject::from(joined_member_list).into(),
                jni_creator.into(),
                jni_era_id.into(),
                jni_max_devices.into(),
                jni_device_count.into(),
            ];
            let result = jni_call_method(&env, jni_call_manager, METHOD, SIG, &args);
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(JObject::null())
        });
    }

    fn handle_ended(&self, client_id: group_call::ClientId, reason: group_call::EndReason) {
        info!("handle_ended():");

        // We can treat ended as a state, since it is just an i32 value.
        handle_state_change_via_jni!(
            self,
            "handleEnded",
            "(JLorg/signal/ringrtc/GroupCall$GroupCallEndReason;)V",
            "GroupCall",
            "GroupCallEndReason",
            client_id,
            reason
        );
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
            "org/signal/ringrtc/CallManager$HttpMethod",
            "org/signal/ringrtc/GroupCall$ConnectionState",
            "org/signal/ringrtc/GroupCall$JoinState",
            "org/signal/ringrtc/GroupCall$GroupCallEndReason",
            HTTP_HEADER_CLASS,
            REMOTE_DEVICE_STATE_CLASS,
            "java/lang/Boolean",
            "java/lang/Float",
            "java/lang/Integer",
            "java/lang/Long",
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

    fn java_enum<'a>(
        &'a self,
        env: &'a JNIEnv,
        parent: &str,
        class: &str,
        value: i32,
    ) -> Result<JObject<'a>> {
        let class_path = format!("{}/{}${}", RINGRTC_PACKAGE, parent, class);
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

    fn get_optional_boolean_object<'a>(
        &'a self,
        env: &'a JNIEnv,
        value: Option<bool>,
    ) -> Result<JObject<'a>> {
        match value {
            None => Ok(JObject::null()),
            Some(value) => {
                let class = match self.class_cache.get_class("java/lang/Boolean") {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(AndroidError::JniGetLangClassNotFound(
                            "java/lang/Boolean".to_string(),
                        )
                        .into());
                    }
                };

                let jni_object = match env.new_object(
                    class,
                    "(Z)V",
                    &[JValue::Bool(value as jni::sys::jboolean)],
                ) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(AndroidError::JniNewLangObjectFailed(
                            "java/lang/Boolean".to_string(),
                        )
                        .into());
                    }
                };

                Ok(jni_object)
            }
        }
    }

    // Converts Option<u32> to a Java Long.
    fn get_optional_u32_long_object<'a>(
        &'a self,
        env: &'a JNIEnv,
        value: Option<u32>,
    ) -> Result<JObject<'a>> {
        match value {
            None => Ok(JObject::null()),
            Some(value) => {
                let class = match self.class_cache.get_class("java/lang/Long") {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(AndroidError::JniGetLangClassNotFound(
                            "java/lang/Long".to_string(),
                        )
                        .into());
                    }
                };

                let jni_object = match env.new_object(
                    class,
                    "(J)V",
                    &[JValue::Long(value as jni::sys::jlong)],
                ) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(AndroidError::JniNewLangObjectFailed(
                            "java/lang/Long".to_string(),
                        )
                        .into());
                    }
                };

                Ok(jni_object)
            }
        }
    }
}
