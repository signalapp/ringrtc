//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Android Platform Interface.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use jni::objects::{AutoLocal, GlobalRef, JObject, JValue};
use jni::sys::{jint, jlong, jshort};
use jni::{JNIEnv, JavaVM};

use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::android::webrtc_java_media_stream::JavaMediaStream;
use crate::common::{
    ApplicationEvent, CallConfig, CallDirection, CallId, CallMediaType, DeviceId, Result,
};
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
use crate::core::{group_call, signaling};
use crate::lite::call_links::{CallLinkRestrictions, CallLinkState, Empty};
use crate::lite::{
    http, sfu,
    sfu::{DemuxId, PeekInfo, PeekResult, UserId},
};
use crate::webrtc::media::{MediaStream, VideoTrack};
use crate::webrtc::peer_connection::{AudioLevel, ReceivedAudioLevel};
use crate::webrtc::peer_connection_observer::NetworkRoute;

const RINGRTC_PACKAGE: &str = jni_class_name!(org.signal.ringrtc);
const CALL_LINK_STATE_CLASS: &str = jni_class_name!(org.signal.ringrtc.CallLinkState);
const CALL_MANAGER_CLASS: &str = "CallManager";
const GROUP_CALL_CLASS: &str = "GroupCall";
const HTTP_HEADER_CLASS: &str = jni_class_name!(org.signal.ringrtc.HttpHeader);
const HTTP_RESULT_CLASS: &str = jni_class_name!(org.signal.ringrtc.CallManager::HttpResult);
const PEEK_INFO_CLASS: &str = jni_class_name!(org.signal.ringrtc.PeekInfo);
const REACTION_CLASS: &str = jni_class_name!(org.signal.ringrtc.GroupCall::Reaction);
const REMOTE_DEVICE_STATE_CLASS: &str =
    jni_class_name!(org.signal.ringrtc.GroupCall::RemoteDeviceState);
const RECEIVED_AUDIO_LEVEL_CLASS: &str =
    jni_class_name!(org.signal.ringrtc.GroupCall::ReceivedAudioLevel);

/// Android implementation for platform::Platform::AppIncomingMedia
pub type AndroidMediaStream = JavaMediaStream;
impl PlatformItem for AndroidMediaStream {}

/// Android implementation for platform::Platform::AppRemotePeer
pub type AndroidGlobalRef = GlobalRef;
impl PlatformItem for AndroidGlobalRef {}

/// Android implementation for platform::Platform::AppCallContext
struct JavaCallContext {
    /// Java JVM object.
    platform: AndroidPlatform,
    /// Java CallContext object.
    jni_call_context: GlobalRef,
}

impl Drop for JavaCallContext {
    fn drop(&mut self) {
        info!("JavaCallContext::drop()");

        // call into CMI to close CallContext object
        if let Ok(env) = &mut self.platform.java_env() {
            let jni_call_manager = self.platform.jni_call_manager.as_obj();
            let jni_call_context = self.jni_call_context.as_obj();

            let _ = jni_call_method(
                env,
                jni_call_manager,
                "closeCall",
                jni_args!((
                    jni_call_context => org.signal.ringrtc.CallManager::CallContext,
                ) -> void),
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
    platform: AndroidPlatform,
    /// Java Connection object.
    jni_connection: GlobalRef,
}

impl Drop for JavaConnection {
    fn drop(&mut self) {
        info!("JavaConnection::drop()");

        // call into CMI to close Connection object
        if let Ok(env) = &mut self.platform.java_env() {
            let jni_call_manager = self.platform.jni_call_manager.as_obj();
            let jni_connection = self.jni_connection.as_obj();

            let _ = jni_call_method(
                env,
                jni_call_manager,
                "closeConnection",
                jni_args!((
                    jni_connection => org.signal.ringrtc.Connection,
                ) -> void),
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
    jvm: JavaVM,
    /// Java org.signal.ringrtc.CallManager object.
    jni_call_manager: GlobalRef,
    /// Cache of Java classes needed at runtime
    class_cache: ClassCache,
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
        let mut env = match $s.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = $s.jni_call_manager.as_obj();
        let jni_client_id = $i as jlong;

        const METHOD: &str = $f;
        let result = jni_call_method(
            &mut env,
            jni_call_manager,
            METHOD,
            jni_args!((jni_client_id => long) -> void)
        );
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
        call_config: CallConfig,
        audio_levels_interval: Option<Duration>,
    ) -> Result<Connection<Self>> {
        info!(
            "create_connection(): call_id: {} remote_device_id: {} signaling_version: {:?}, call_config: {:?}, audio_levels_interval: {:?}",
            call.call_id(),
            remote_device_id,
            signaling_version,
            call_config,
            audio_levels_interval,
        );

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let audio_jitter_buffer_max_packets = call_config.audio_jitter_buffer_config.max_packets;
        let audio_jitter_buffer_max_target_delay_ms =
            call_config.audio_jitter_buffer_config.max_target_delay_ms;

        let connection = Connection::new(
            call.clone(),
            remote_device_id,
            connection_type,
            call_config,
            audio_levels_interval,
            None, // The app adds sinks to VideoTracks.
        )?;

        let connection_ptr = connection.get_connection_ptr()?;
        let call_id_jlong = u64::from(call.call_id()) as jlong;
        let jni_remote_device_id = remote_device_id as jint;

        // call into CMI to create webrtc PeerConnection
        let android_call_context = call.call_context()?;
        let jni_call_context = android_call_context.to_jni();

        let jni_connection = jni_call_method(
            env,
            jni_call_manager,
            "createConnection",
            jni_args!((
                (connection_ptr.as_ptr() as jlong) => long,
                call_id_jlong => long,
                jni_remote_device_id => int,
                jni_call_context.as_obj() => org.signal.ringrtc.CallManager::CallContext,
                audio_jitter_buffer_max_packets => int,
                audio_jitter_buffer_max_target_delay_ms => int,
            ) -> org.signal.ringrtc.Connection),
        )?;

        if jni_connection.is_null() {
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

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote = remote_peer.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let is_outgoing = match direction {
            CallDirection::OutGoing => true,
            CallDirection::InComing => false,
        };
        let jni_call_media_type = match self.java_enum(
            env,
            CALL_MANAGER_CLASS,
            "CallMediaType",
            call_media_type as i32,
        ) {
            Ok(v) => AutoLocal::new(v, env),
            Err(error) => {
                return Err(error);
            }
        };

        jni_call_method(
            env,
            jni_call_manager,
            "onStartCall",
            jni_args!((
                jni_remote => org.signal.ringrtc.Remote,
                call_id_jlong => long,
                is_outgoing => boolean,
                jni_call_media_type => org.signal.ringrtc.CallManager::CallMediaType,
            ) -> void),
        )?;

        Ok(())
    }

    fn on_event(
        &self,
        remote_peer: &Self::AppRemotePeer,
        _call_id: CallId,
        event: ApplicationEvent,
    ) -> Result<()> {
        info!("on_event(): {}", event);

        let env = &mut self.java_env()?;

        let jni_remote = remote_peer.as_obj();
        let jni_event = match self.java_enum(env, CALL_MANAGER_CLASS, "CallEvent", event as i32) {
            Ok(v) => AutoLocal::new(v, env),
            Err(error) => {
                return Err(error);
            }
        };

        jni_call_method(
            env,
            self.jni_call_manager.as_obj(),
            "onEvent",
            jni_args!((
                jni_remote => org.signal.ringrtc.Remote,
                jni_event => org.signal.ringrtc.CallManager::CallEvent,
            ) -> void),
        )?;

        Ok(())
    }

    // Network route changes for 1:1 calls
    fn on_network_route_changed(
        &self,
        remote_peer: &Self::AppRemotePeer,
        network_route: NetworkRoute,
    ) -> Result<()> {
        trace!(
            "on_network_route_changed(): network_route: {:?}",
            network_route
        );

        let env = &mut self.java_env()?;

        jni_call_method(
            env,
            self.jni_call_manager.as_obj(),
            "onNetworkRouteChanged",
            jni_args!((
                remote_peer.as_obj() => org.signal.ringrtc.Remote,
                network_route.local_adapter_type as i32 => int,
            ) -> void),
        )?;
        Ok(())
    }

    fn on_audio_levels(
        &self,
        remote_peer: &Self::AppRemotePeer,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        trace!(
            "on_audio_levels(): captured_level: {}; received_level: {}",
            captured_level,
            received_level
        );

        let env = &mut self.java_env()?;

        jni_call_method(
            env,
            self.jni_call_manager.as_obj(),
            "onAudioLevels",
            jni_args!((
                remote_peer.as_obj() => org.signal.ringrtc.Remote,
                captured_level as i32 => int,
                received_level as i32 => int,
            ) -> void),
        )?;
        Ok(())
    }

    fn on_low_bandwidth_for_video(
        &self,
        remote_peer: &Self::AppRemotePeer,
        recovered: bool,
    ) -> Result<()> {
        info!("on_low_bandwidth_for_video(): recovered: {}", recovered);

        let env = &mut self.java_env()?;

        jni_call_method(
            env,
            self.jni_call_manager.as_obj(),
            "onLowBandwidthForVideo",
            jni_args!((
                    remote_peer.as_obj() => org.signal.ringrtc.Remote,
                    recovered => boolean,
            ) -> void),
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

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (3).
        let capacity = 8;
        env.with_local_frame(capacity, |env| -> Result<()> {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;
            let jni_opaque = JObject::from(env.byte_array_from_slice(&offer.opaque)?);
            let jni_call_media_type = match self.java_enum(
                env,
                CALL_MANAGER_CLASS,
                "CallMediaType",
                offer.call_media_type as i32,
            ) {
                Ok(v) => v,
                Err(error) => {
                    error!("jni_call_media_type: {:?}", error);
                    return Ok(());
                }
            };

            let result = jni_call_method(
                env,
                jni_call_manager,
                "onSendOffer",
                jni_args!((
                    call_id_jlong => long,
                    jni_remote => org.signal.ringrtc.Remote,
                    receiver_device_id => int,
                    broadcast => boolean,
                    jni_opaque => [byte],
                    jni_call_media_type => org.signal.ringrtc.CallManager::CallMediaType,
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(())
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

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (2).
        let capacity = 7;
        env.with_local_frame(capacity, |env| -> Result<()> {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;
            let jni_opaque = JObject::from(env.byte_array_from_slice(&send.answer.opaque)?);

            let result = jni_call_method(
                env,
                jni_call_manager,
                "onSendAnswer",
                jni_args!((
                    call_id_jlong => long,
                    jni_remote => org.signal.ringrtc.Remote,
                    receiver_device_id => int,
                    broadcast => boolean,
                    jni_opaque => [byte],
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(())
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

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (3) + elements (N * 3 object per element).
        let capacity = (8 + send.ice.candidates.len() * 3) as i32;
        env.with_local_frame(capacity, |env| -> Result<()> {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;

            let list = jni_new_arraylist(env, send.ice.candidates.len())?;
            let ice_candidate_list = match env.get_list(&list) {
                Ok(v) => v,
                Err(error) => {
                    error!("ice_candidate_list: {:?}", error);
                    return Ok(());
                }
            };

            for candidate in send.ice.candidates {
                let jni_opaque = JObject::from(env.byte_array_from_slice(&candidate.opaque)?);
                let result = ice_candidate_list.add(env, &jni_opaque);
                if result.is_err() {
                    error!("ice_candidate_list.add: {:?}", result.err());
                    continue;
                }
            }

            let result = jni_call_method(
                env,
                jni_call_manager,
                "onSendIceCandidates",
                jni_args!((
                    call_id_jlong => long,
                    jni_remote => org.signal.ringrtc.Remote,
                    receiver_device_id => int,
                    broadcast => boolean,
                    ice_candidate_list => java.util.List,
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(())
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

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote = remote_peer.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;

        let (hangup_type, hangup_device_id) = send.hangup.to_type_and_device_id();
        // We set the device_id to 0 in case it is not defined. It will
        // only be used for hangup types other than Normal.
        let hangup_device_id = hangup_device_id.unwrap_or(0) as jint;
        let jni_hangup_type =
            match self.java_enum(env, CALL_MANAGER_CLASS, "HangupType", hangup_type as i32) {
                Ok(v) => AutoLocal::new(v, env),
                Err(error) => {
                    return Err(error);
                }
            };

        jni_call_method(
            env,
            jni_call_manager,
            "onSendHangup",
            jni_args!((
                call_id_jlong => long,
                jni_remote => org.signal.ringrtc.Remote,
                receiver_device_id => int,
                broadcast => boolean,
                jni_hangup_type => org.signal.ringrtc.CallManager::HangupType,
                hangup_device_id => int,
            ) -> void),
        )?;

        Ok(())
    }

    fn on_send_busy(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()> {
        // Busy messages are always broadcast
        let broadcast = true;
        let receiver_device_id = 0;

        info!("on_send_busy(): call_id: {}", call_id);

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote = remote_peer.as_obj();
        let call_id_jlong = u64::from(call_id) as jlong;
        let receiver_device_id = receiver_device_id as jint;

        jni_call_method(
            env,
            jni_call_manager,
            "onSendBusy",
            jni_args!((
                call_id_jlong => long,
                jni_remote => org.signal.ringrtc.Remote,
                receiver_device_id => int,
                broadcast => boolean,
            ) -> void),
        )?;

        Ok(())
    }

    fn send_call_message(
        &self,
        recipient_uuid: UserId,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (2).
        let capacity = 7;
        env.with_local_frame(capacity, |env| -> Result<()> {
            let jni_recipient_uuid = JObject::from(env.byte_array_from_slice(&recipient_uuid)?);
            let jni_message = JObject::from(env.byte_array_from_slice(&message)?);

            let result = jni_call_method(
                env,
                jni_call_manager,
                "sendCallMessage",
                jni_args!((
                    jni_recipient_uuid => [byte],
                    jni_message => [byte],
                    urgency as i32 => int,
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(())
        })?;

        Ok(())
    }

    fn send_call_message_to_group(
        &self,
        group_id: group_call::GroupId,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
        recipients_override: HashSet<UserId>,
    ) -> Result<()> {
        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (3) + elements (N * 1 object per element).
        let capacity = (8 + recipients_override.len()) as i32;
        env.with_local_frame(capacity, |env| -> Result<()> {
            let jni_group_id = JObject::from(env.byte_array_from_slice(&group_id)?);

            let list = jni_new_arraylist(env, recipients_override.len())?;
            let recipients_override_list = env.get_list(&list)?;
            for recipient in recipients_override {
                let jni_opaque_user_id = match env.byte_array_from_slice(&recipient) {
                    Ok(v) => JObject::from(v),
                    Err(error) => {
                        error!("{:?}", error);
                        continue;
                    }
                };

                let result = recipients_override_list.add(env, &jni_opaque_user_id);
                if result.is_err() {
                    error!("{:?}", result.err());
                    continue;
                }
            }

            let jni_message = JObject::from(env.byte_array_from_slice(&message)?);

            let result = jni_call_method(
                env,
                jni_call_manager,
                "sendCallMessageToGroup",
                jni_args!((
                    jni_group_id => [byte],
                    jni_message => [byte],
                    urgency as i32 => int,
                    recipients_override_list => java.util.List,
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(())
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

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_call_context = app_call_context.to_jni();
        let jni_media_stream = incoming_media.global_ref(env)?;

        jni_call_method(
            env,
            jni_call_manager,
            "onConnectMedia",
            jni_args!((
                jni_call_context.as_obj() => org.signal.ringrtc.CallManager::CallContext,
                jni_media_stream.as_obj() => org.webrtc.MediaStream,
            ) -> void),
        )?;

        Ok(())
    }

    fn disconnect_incoming_media(&self, app_call_context: &Self::AppCallContext) -> Result<()> {
        info!("disconnect_incoming_media():");

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_call_context = app_call_context.to_jni();

        jni_call_method(
            env,
            jni_call_manager,
            "onCloseMedia",
            jni_args!((
                jni_call_context.as_obj() => org.signal.ringrtc.CallManager::CallContext,
            ) -> void),
        )?;

        Ok(())
    }

    fn compare_remotes(
        &self,
        remote_peer1: &Self::AppRemotePeer,
        remote_peer2: &Self::AppRemotePeer,
    ) -> Result<bool> {
        info!("remotes_equal():");

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote1 = remote_peer1.as_obj();
        let jni_remote2 = remote_peer2.as_obj();

        let result = jni_call_method(
            env,
            jni_call_manager,
            "compareRemotes",
            jni_args!((
                jni_remote1 => org.signal.ringrtc.Remote,
                jni_remote2 => org.signal.ringrtc.Remote,
            ) -> boolean),
        )?;

        Ok(result != 0)
    }

    fn on_offer_expired(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        _age: Duration,
    ) -> Result<()> {
        // Android already keeps track of the offer timestamp, so no need to pass the age through.
        self.on_event(remote_peer, call_id, ApplicationEvent::ReceivedOfferExpired)
    }

    fn on_call_concluded(&self, remote_peer: &Self::AppRemotePeer, _call_id: CallId) -> Result<()> {
        info!("on_call_concluded():");

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        let jni_remote_peer = remote_peer.as_obj();

        jni_call_method(
            env,
            jni_call_manager,
            "onCallConcluded",
            jni_args!((
                jni_remote_peer => org.signal.ringrtc.Remote,
            ) -> void),
        )?;

        Ok(())
    }

    // Group Calls

    fn group_call_ring_update(
        &self,
        group_id: group_call::GroupId,
        ring_id: group_call::RingId,
        sender: UserId,
        update: group_call::RingUpdate,
    ) {
        info!("group_call_ring_update():");

        if let Ok(env) = &mut self.java_env() {
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

            let result = jni_call_method(
                env,
                self.jni_call_manager.as_obj(),
                "groupCallRingUpdate",
                jni_args!((
                    group_id => [byte],
                    ring_id => long,
                    sender => [byte],
                    update => int,
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
        }
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

        if let Ok(env) = &mut self.java_env() {
            let jni_client_id = client_id as jlong;
            let jni_connection_state = match self.java_enum(
                env,
                GROUP_CALL_CLASS,
                "ConnectionState",
                connection_state.ordinal(),
            ) {
                Ok(v) => AutoLocal::new(v, env),
                Err(error) => {
                    error!("{:?}", error);
                    return;
                }
            };

            let result = jni_call_method(
                env,
                self.jni_call_manager.as_obj(),
                "handleConnectionStateChanged",
                jni_args!((
                    jni_client_id => long,
                    jni_connection_state => org.signal.ringrtc.GroupCall::ConnectionState
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
        }
    }

    fn handle_network_route_changed(
        &self,
        client_id: group_call::ClientId,
        network_route: NetworkRoute,
    ) {
        info!(
            "handle_network_route_changed(): client_id: {}, network_route: {:?}",
            client_id, network_route
        );

        if let Ok(env) = &mut self.java_env() {
            let _ = jni_call_method(
                env,
                self.jni_call_manager.as_obj(),
                "handleNetworkRouteChanged",
                jni_args!((
                    client_id as jlong => long,
                    network_route.local_adapter_type as i32 => int,
                ) -> void),
            );
        }
    }

    fn handle_audio_levels(
        &self,
        client_id: group_call::ClientId,
        captured_level: AudioLevel,
        received_levels: Vec<ReceivedAudioLevel>,
    ) {
        trace!(
            "handle_audio_levels(): client_id: {}, captured_level: {:?}, received_levels: {:?}",
            client_id,
            captured_level,
            received_levels,
        );

        if let Ok(mut env) = self.java_env() {
            // Set a frame capacity of min (5) + objects (2) + elements (N * 2 per level).
            let capacity = (5 + 2 + received_levels.len() * 2) as i32;
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                // create Java List<GroupCall.ReceivedAudioLevel>
                let received_level_class =
                    self.class_cache.get_class(RECEIVED_AUDIO_LEVEL_CLASS)?;

                let list = jni_new_arraylist(env, received_levels.len())?;
                let received_levels_list = env.get_list(&list)?;

                for received in received_levels {
                    let args = jni_args!((
                        received.demux_id as jlong => long,
                        received.level as jint => int,
                    ) -> void);

                    let received_level_obj =
                        match env.new_object(received_level_class, args.sig, &args.args) {
                            Ok(v) => v,
                            Err(error) => {
                                error!("jni_received_level: {:?}", error);
                                continue;
                            }
                        };

                    let result = received_levels_list.add(env, &received_level_obj);
                    if result.is_err() {
                        error!("jni_received_levels_list.add: {:?}", result.err());
                        continue;
                    }
                }

                let _ = jni_call_method(
                    env,
                    self.jni_call_manager.as_obj(),
                    "handleAudioLevels",
                    jni_args!((
                        client_id as jlong => long,
                        captured_level as jint => int,
                        received_levels_list => java.util.List,
                    ) -> void),
                );

                Ok(())
            }) {
                error!("handle_audio_levels: {:?}", e);
            }
        }
    }

    fn handle_low_bandwidth_for_video(&self, client_id: group_call::ClientId, recovered: bool) {
        info!(
            "handle_low_bandwidth_for_video(): client_id: {}, recovered: {}",
            client_id, recovered
        );

        if let Ok(env) = &mut self.java_env() {
            // Set a frame capacity of min (5).
            let capacity = 5;
            let _ = env.with_local_frame(capacity, |env| -> Result<()> {
                let _ = jni_call_method(
                    env,
                    self.jni_call_manager.as_obj(),
                    "handleLowBandwidthForVideo",
                    jni_args!((
                        client_id as jlong => long,
                        recovered => boolean,
                    ) -> void),
                );

                Ok(())
            });
        }
    }

    fn handle_reactions(
        &self,
        client_id: group_call::ClientId,
        reactions: Vec<group_call::Reaction>,
    ) {
        trace!(
            "handle_reactions(): client_id: {}, reactions: {:?}",
            client_id,
            reactions,
        );

        if let Ok(mut env) = self.java_env() {
            // Set a frame capacity of min (5) + objects (1) + elements (N * 2 per reaction).
            let capacity = (5 + 1 + reactions.len() * 2) as i32;
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                // create Java List<GroupCall.Reaction>
                let reaction_class = self.class_cache.get_class(REACTION_CLASS)?;

                let list = jni_new_arraylist(env, reactions.len())?;
                let reactions_list = env.get_list(&list)?;

                for reaction in reactions {
                    let jni_value = JObject::from(env.new_string(reaction.value)?);
                    let args = jni_args!((
                        reaction.demux_id as jlong => long,
                        jni_value => java.lang.String,
                    ) -> void);

                    let reaction_obj = match env.new_object(reaction_class, args.sig, &args.args) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_reaction: {:?}", error);
                            continue;
                        }
                    };

                    let result = reactions_list.add(env, &reaction_obj);
                    if result.is_err() {
                        error!("jni_reaction.add: {:?}", result.err());
                        continue;
                    }
                }

                let _ = jni_call_method(
                    env,
                    self.jni_call_manager.as_obj(),
                    "handleReactions",
                    jni_args!((
                        client_id as jlong => long,
                        reactions_list => java.util.List,
                    ) -> void),
                );

                Ok(())
            }) {
                error!("handle_reactions: {:?}", e);
            }
        }
    }

    fn handle_raised_hands(&self, client_id: group_call::ClientId, raised_hands: Vec<DemuxId>) {
        info!(
            "handle_raised_hands(): client_id: {}, raised_hands: {:?}",
            client_id, raised_hands,
        );

        if let Ok(mut env) = self.java_env() {
            // Set a frame capacity of min (5) + objects (1) + N elements.
            let capacity = (5 + 1 + raised_hands.len()) as i32;
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                // create Java List<Long>
                let long_class = self
                    .class_cache
                    .get_class(jni_class_name!(java.lang.Long))?;

                let list = jni_new_arraylist(env, raised_hands.len())?;
                let raised_hands_list = env.get_list(&list)?;

                for raised_hand in raised_hands {
                    let args = jni_args!((
                        raised_hand as jlong => long,
                    ) -> void);

                    let raised_hand_obj = match env.new_object(long_class, args.sig, &args.args) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_raised_hands: {:?}", error);
                            continue;
                        }
                    };

                    let result = raised_hands_list.add(env, &raised_hand_obj);
                    if result.is_err() {
                        error!("jni_raised_hands.add: {:?}", result.err());
                        continue;
                    }
                }

                let _ = jni_call_method(
                    env,
                    self.jni_call_manager.as_obj(),
                    "handleRaisedHands",
                    jni_args!((
                        client_id as jlong => long,
                        raised_hands_list => java.util.List,
                    ) -> void),
                );

                Ok(())
            }) {
                error!("handle_raised_hands: {:?}", e);
            }
        }
    }

    fn handle_join_state_changed(
        &self,
        client_id: group_call::ClientId,
        join_state: group_call::JoinState,
    ) {
        info!("handle_join_state_changed():");

        if let Ok(env) = &mut self.java_env() {
            let jni_client_id = client_id as jlong;
            let jni_join_state =
                match self.java_enum(env, GROUP_CALL_CLASS, "JoinState", join_state.ordinal()) {
                    Ok(v) => AutoLocal::new(v, env),
                    Err(error) => {
                        error!("{:?}", error);
                        return;
                    }
                };
            let jni_demux_id = match join_state {
                group_call::JoinState::Pending(demux_id)
                | group_call::JoinState::Joined(demux_id) => {
                    match self.get_optional_u32_long_object(env, Some(demux_id)) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("{:?}", error);
                            return;
                        }
                    }
                }
                _ => JObject::null(),
            };

            let result = jni_call_method(
                env,
                self.jni_call_manager.as_obj(),
                "handleJoinStateChanged",
                jni_args!((
                    jni_client_id => long,
                    jni_join_state => org.signal.ringrtc.GroupCall::JoinState,
                    jni_demux_id => java.lang.Long
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
        }
    }

    fn handle_remote_devices_changed(
        &self,
        client_id: group_call::ClientId,
        remote_device_states: &[group_call::RemoteDeviceState],
        _reason: group_call::RemoteDevicesChangedReason,
    ) {
        info!("handle_remote_devices_changed():");

        if let Ok(env) = &mut self.java_env() {
            // Set a frame capacity of min (5) + objects (2) + elements (N * 2 object per element).
            let capacity = (7 + remote_device_states.len() * 2) as i32;
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_client_id = client_id as jlong;

                // create Java List<GroupCall.RemoteDeviceState>
                let remote_device_state_class =
                    self.class_cache.get_class(REMOTE_DEVICE_STATE_CLASS)?;

                let list = jni_new_arraylist(env, remote_device_states.len())?;
                let remote_device_state_list = env.get_list(&list)?;

                for remote_device_state in remote_device_states {
                    let jni_demux_id = remote_device_state.demux_id as jlong;
                    let jni_user_id_byte_array =
                        match env.byte_array_from_slice(&remote_device_state.user_id) {
                            Ok(v) => JObject::from(v),
                            Err(error) => {
                                error!("jni_user_id_byte_array: {:?}", error);
                                continue;
                            }
                        };
                    let jni_audio_muted = match self.get_optional_boolean_object(
                        env,
                        remote_device_state.heartbeat_state.audio_muted,
                    ) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_audio_muted: {:?}", error);
                            continue;
                        }
                    };
                    let jni_video_muted = match self.get_optional_boolean_object(
                        env,
                        remote_device_state.heartbeat_state.video_muted,
                    ) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_video_muted: {:?}", error);
                            continue;
                        }
                    };
                    let jni_presenting = match self.get_optional_boolean_object(
                        env,
                        remote_device_state.heartbeat_state.presenting,
                    ) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_presenting: {:?}", error);
                            continue;
                        }
                    };
                    let jni_sharing_screen = match self.get_optional_boolean_object(
                        env,
                        remote_device_state.heartbeat_state.sharing_screen,
                    ) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_sharing_screen: {:?}", error);
                            continue;
                        }
                    };
                    let jni_added_time = remote_device_state.added_time_as_unix_millis() as jlong;
                    let jni_speaker_time =
                        remote_device_state.speaker_time_as_unix_millis() as jlong;
                    let jni_forwarding_video = match self
                        .get_optional_boolean_object(env, remote_device_state.forwarding_video)
                    {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_forwarding_video: {:?}", error);
                            continue;
                        }
                    };

                    let args = jni_args!((
                        jni_demux_id => long,
                        jni_user_id_byte_array => [byte],
                        remote_device_state.media_keys_received => boolean,
                        jni_audio_muted => java.lang.Boolean,
                        jni_video_muted => java.lang.Boolean,
                        jni_presenting => java.lang.Boolean,
                        jni_sharing_screen => java.lang.Boolean,
                        jni_added_time => long,
                        jni_speaker_time => long,
                        jni_forwarding_video => java.lang.Boolean,
                        remote_device_state.is_higher_resolution_pending => boolean,
                    ) -> void);

                    let remote_device_state_obj =
                        match env.new_object(remote_device_state_class, args.sig, &args.args) {
                            Ok(v) => v,
                            Err(error) => {
                                error!("remote_device_state_obj: {:?}", error);
                                continue;
                            }
                        };

                    let result = remote_device_state_list.add(env, &remote_device_state_obj);
                    if result.is_err() {
                        error!("remote_device_state_list.add: {:?}", result.err());
                        continue;
                    }
                }

                let result = jni_call_method(
                    env,
                    self.jni_call_manager.as_obj(),
                    "handleRemoteDevicesChanged",
                    jni_args!((
                        jni_client_id => long,
                        remote_device_state_list => java.util.List,
                    ) -> void),
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            }) {
                error!("handle_remote_devices_changed {:?}", e);
            }
        }
    }

    fn handle_incoming_video_track(
        &self,
        client_id: group_call::ClientId,
        remote_demux_id: DemuxId,
        incoming_video_track: VideoTrack,
    ) {
        info!("handle_incoming_video_track():");

        if let Ok(env) = &mut self.java_env() {
            let jni_client_id = client_id as jlong;
            let jni_remote_demux_id = remote_demux_id as jlong;
            let jni_native_video_track_owned_rc =
                incoming_video_track.rffi().clone().into_owned().as_ptr() as jlong;

            let result = jni_call_method(
                env,
                self.jni_call_manager.as_obj(),
                "handleIncomingVideoTrack",
                jni_args!((
                    jni_client_id => long,
                    jni_remote_demux_id => long,
                    jni_native_video_track_owned_rc => long
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
        }
    }

    fn handle_peek_changed(
        &self,
        client_id: group_call::ClientId,
        peek_info: &PeekInfo,
        joined_members: &HashSet<UserId>,
    ) {
        info!("handle_peek_changed():");

        if let Ok(env) = &mut self.java_env() {
            // Set a frame capacity of min (5) + objects (5) + elements (N * 1 object per element).
            let capacity = (10 + joined_members.len()) as i32;
            let _ = env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_client_id = client_id as jlong;

                let jni_peek_info =
                    match self.make_peek_info_object(env, peek_info, &mut joined_members.iter()) {
                        Ok(value) => value,
                        Err(e) => {
                            error!("make_peek_info_object: {:?}", e);
                            return Ok(());
                        }
                    };

                let result = jni_call_method(
                    env,
                    self.jni_call_manager.as_obj(),
                    "handlePeekChanged",
                    jni_args!((
                        jni_client_id => long,
                        jni_peek_info => org.signal.ringrtc.PeekInfo,
                    ) -> void),
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            });
        }
    }

    fn handle_ended(&self, client_id: group_call::ClientId, reason: group_call::EndReason) {
        info!("handle_ended():");

        if let Ok(env) = &mut self.java_env() {
            let jni_client_id = client_id as jlong;
            let jni_end_reason =
                match self.java_enum(env, GROUP_CALL_CLASS, "GroupCallEndReason", reason as i32) {
                    Ok(v) => AutoLocal::new(v, env),
                    Err(error) => {
                        error!("{:?}", error);
                        return;
                    }
                };

            let result = jni_call_method(
                env,
                self.jni_call_manager.as_obj(),
                "handleEnded",
                jni_args!((
                    jni_client_id => long,
                    jni_end_reason => org.signal.ringrtc.GroupCall::GroupCallEndReason
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
        }
    }
}

impl AndroidPlatform {
    /// Create a new AndroidPlatform object.
    pub fn new(env: &mut JNIEnv, jni_call_manager: GlobalRef) -> Result<Self> {
        let mut class_cache = ClassCache::new();
        for class in &[
            jni_class_name!(org.signal.ringrtc.CallManager::CallEvent),
            jni_class_name!(org.signal.ringrtc.CallManager::CallMediaType),
            jni_class_name!(org.signal.ringrtc.CallManager::HangupType),
            jni_class_name!(org.signal.ringrtc.CallManager::HttpMethod),
            jni_class_name!(org.signal.ringrtc.GroupCall::ConnectionState),
            jni_class_name!(org.signal.ringrtc.GroupCall::JoinState),
            jni_class_name!(org.signal.ringrtc.GroupCall::GroupCallEndReason),
            CALL_LINK_STATE_CLASS,
            HTTP_HEADER_CLASS,
            HTTP_RESULT_CLASS,
            PEEK_INFO_CLASS,
            REACTION_CLASS,
            REMOTE_DEVICE_STATE_CLASS,
            RECEIVED_AUDIO_LEVEL_CLASS,
            jni_class_name!(java.lang.Boolean),
            jni_class_name!(java.lang.Float),
            jni_class_name!(java.lang.Integer),
            jni_class_name!(java.lang.Long),
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
    fn java_env(&self) -> Result<ExceptionCheckingJNIEnv> {
        Ok(self.jvm.attach_current_thread_as_daemon()?.into())
    }

    pub fn try_clone(&self) -> Result<Self> {
        let env = self.java_env()?;
        Ok(Self {
            jvm: env.get_java_vm()?,
            jni_call_manager: self.jni_call_manager.clone(),
            class_cache: self.class_cache.clone(),
        })
    }

    fn java_enum<'a>(
        &self,
        env: &mut JNIEnv<'a>,
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
        &self,
        env: &mut JNIEnv<'a>,
        value: Option<bool>,
    ) -> Result<JObject<'a>> {
        match value {
            None => Ok(JObject::null()),
            Some(value) => {
                let class_name = jni_class_name!(java.lang.Boolean);
                let class = match self.class_cache.get_class(class_name) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(
                            AndroidError::JniGetLangClassNotFound(class_name.to_string()).into(),
                        );
                    }
                };

                let args = jni_args!((value => boolean) -> void);
                let jni_object = match env.new_object(class, args.sig, &args.args) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(
                            AndroidError::JniNewLangObjectFailed(class_name.to_string()).into()
                        );
                    }
                };

                Ok(jni_object)
            }
        }
    }

    // Converts Option<u32> to a Java Long.
    fn get_optional_u32_long_object<'local>(
        &self,
        env: &mut JNIEnv<'local>,
        value: Option<u32>,
    ) -> Result<JObject<'local>> {
        match value {
            None => Ok(JObject::null()),
            Some(value) => {
                let class_name = jni_class_name!(java.lang.Long);
                let class = match self.class_cache.get_class(class_name) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(
                            AndroidError::JniGetLangClassNotFound(class_name.to_string()).into(),
                        );
                    }
                };

                let args = jni_args!((value as jni::sys::jlong => long) -> void);
                let jni_object = match env.new_object(class, args.sig, &args.args) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(
                            AndroidError::JniNewLangObjectFailed(class_name.to_string()).into()
                        );
                    }
                };

                Ok(jni_object)
            }
        }
    }

    fn send_http_request(&self, request_id: u32, request: http::Request) -> Result<()> {
        info!("send_request(): request_id: {}", request_id);

        let http::Request {
            method,
            url,
            headers,
            body,
        } = request;

        let env = &mut self.java_env()?;
        let jni_call_manager = self.jni_call_manager.as_obj();

        // Set a frame capacity of min (5) + objects (4) + elements (N * 3 objects per element).
        let capacity = (9 + headers.len() * 3) as i32;
        env.with_local_frame(capacity, |env| -> Result<()> {
            let jni_request_id = request_id as jlong;
            let jni_url = JObject::from(env.new_string(url)?);
            let jni_method =
                match self.java_enum(env, CALL_MANAGER_CLASS, "HttpMethod", method as i32) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("jni_method: {:?}", error);
                        return Ok(());
                    }
                };

            // create Java List<HttpHeader>
            let http_header_class = match self.class_cache.get_class(HTTP_HEADER_CLASS) {
                Ok(v) => v,
                Err(error) => {
                    error!("http_header_class: {:?}", error);
                    return Ok(());
                }
            };
            let list = jni_new_arraylist(env, headers.len())?;
            let jni_headers = match env.get_list(&list) {
                Ok(v) => v,
                Err(error) => {
                    error!("jni_headers: {:?}", error);
                    return Ok(());
                }
            };
            for (name, value) in headers.iter() {
                let jni_name = JObject::from(env.new_string(name)?);
                let jni_value = JObject::from(env.new_string(value)?);
                let args = jni_args!((
                    jni_name => java.lang.String,
                    jni_value => java.lang.String,
                ) -> void);
                let http_header_obj = env.new_object(http_header_class, args.sig, &args.args)?;
                jni_headers.add(env, &http_header_obj)?;
            }

            let jni_body = match body {
                None => JObject::null(),
                Some(body) => JObject::from(env.byte_array_from_slice(&body)?),
            };

            let result = jni_call_method(
                env,
                jni_call_manager,
                "sendHttpRequest",
                jni_args!((
                    jni_request_id => long,
                    jni_url => java.lang.String,
                    jni_method => org.signal.ringrtc.CallManager::HttpMethod,
                    jni_headers => java.util.List,
                    jni_body => [byte],
                ) -> void),
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }

            Ok(())
        })
    }

    pub fn handle_call_link_result(
        &self,
        request_id: u32,
        response: std::result::Result<CallLinkState, http::ResponseStatus>,
    ) {
        let mut env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        let http_result_class = match self.class_cache.get_class(HTTP_RESULT_CLASS) {
            Ok(v) => v,
            Err(error) => {
                error!("http_result_class: {:?}", error);
                return;
            }
        };

        let result_object = match response {
            Ok(result) => {
                let call_link_state = Some(result);
                match self.make_call_link_state_object(&mut env, &call_link_state) {
                    Ok(state) => {
                        // Unconstrained generics get erased to java.lang.Object.
                        let args = jni_args!((
                            state => java.lang.Object,
                        ) -> void);
                        match env.new_object(http_result_class, args.sig, &args.args) {
                            Ok(v) => v,
                            Err(error) => {
                                error!("new HttpResult(CallLinkState): {:?}", error);
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        error!("new CallLinkState: {:?}", error);
                        return;
                    }
                }
            }
            Err(status) => {
                let args = jni_args!((
                    status.code as jshort => short,
                ) -> void);
                match env.new_object(http_result_class, args.sig, &args.args) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("new HttpResult(short): {:?}", error);
                        return;
                    }
                }
            }
        };

        match jni_call_method(
            &mut env,
            jni_call_manager,
            "handleCallLinkResponse",
            jni_args!((
                request_id as jlong => long,
                result_object => org.signal.ringrtc.CallManager::HttpResult,
            ) -> void),
        ) {
            Ok(()) => {}
            Err(error) => {
                error!("handleCallLinkResponse: {:?}", error);
            }
        }
    }

    pub fn handle_empty_result(
        &self,
        request_id: u32,
        response: std::result::Result<Empty, http::ResponseStatus>,
    ) {
        let mut env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        let http_result_class = match self.class_cache.get_class(HTTP_RESULT_CLASS) {
            Ok(v) => v,
            Err(error) => {
                error!("http_result_class: {:?}", error);
                return;
            }
        };

        let result_object = match response {
            Ok(_) => {
                // need to provide a non-null Object, so we use java.lang.Boolean
                let success_filler = match self.get_optional_boolean_object(&mut env, Some(true)) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("empty result success filler java.lang.Boolean: {:?}", error);
                        return;
                    }
                };

                // Unconstrained generics get erased to java.lang.Object.
                let args = jni_args!((
                    success_filler => java.lang.Object,
                ) -> void);
                match env.new_object(http_result_class, args.sig, &args.args) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("new HttpResult(Boolean): {:?}", error);
                        return;
                    }
                }
            }
            Err(status) => {
                let args = jni_args!((
                    status.code as jshort => short,
                ) -> void);
                match env.new_object(http_result_class, args.sig, &args.args) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("new HttpResult(short): {:?}", error);
                        return;
                    }
                }
            }
        };

        match jni_call_method(
            &mut env,
            jni_call_manager,
            "handleEmptyResponse",
            jni_args!((
                request_id as jlong => long,
                result_object => org.signal.ringrtc.CallManager::HttpResult,
            ) -> void),
        ) {
            Ok(()) => {}
            Err(error) => {
                error!("handleEmptyResponse: {:?}", error);
            }
        }
    }

    fn make_call_link_state_object<'a>(
        &self,
        env: &mut JNIEnv<'a>,
        call_link_state: &Option<CallLinkState>,
    ) -> jni::errors::Result<JObject<'a>> {
        match call_link_state {
            None => Ok(JObject::null()),
            Some(state) => {
                let call_link_state_class = match self.class_cache.get_class(CALL_LINK_STATE_CLASS)
                {
                    Ok(v) => v,
                    Err(error) => {
                        error!("call_link_state_class: {:?}", error);
                        return Ok(JObject::null());
                    }
                };

                let name_object = JObject::from(env.new_string(state.name.clone())?);

                let raw_restrictions: jint = match state.restrictions {
                    CallLinkRestrictions::None => 0,
                    CallLinkRestrictions::AdminApproval => 1,
                    CallLinkRestrictions::Unknown => -1,
                };
                let expiration_in_epoch_seconds = state
                    .expiration
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let args = jni_args!((
                    name_object => java.lang.String,
                    raw_restrictions => int,
                    state.revoked => boolean,
                    expiration_in_epoch_seconds as jlong => long,
                ) -> void);

                let object = env.new_object(call_link_state_class, args.sig, &args.args);
                object
            }
        }
    }

    fn make_peek_info_object<'a>(
        &self,
        env: &mut JNIEnv<'a>,
        peek_info: &PeekInfo,
        joined_members: &mut dyn ExactSizeIterator<Item = &UserId>,
    ) -> Result<JObject<'a>> {
        let list = jni_new_arraylist(env, joined_members.len())?;
        let joined_member_list = env.get_list(&list)?;
        for joined_member in joined_members {
            let jni_opaque_user_id = match env.byte_array_from_slice(joined_member) {
                Ok(v) => JObject::from(v),
                Err(error) => {
                    error!("{:?}", error);
                    continue;
                }
            };

            let result = joined_member_list.add(env, &jni_opaque_user_id);
            if result.is_err() {
                error!("{:?}", result.err());
                continue;
            }
        }
        let jni_creator = match peek_info.creator.as_ref() {
            None => JObject::null(),
            Some(creator) => match env.byte_array_from_slice(creator) {
                Ok(v) => JObject::from(v),
                Err(error) => {
                    error!("{:?}", error);
                    JObject::null()
                }
            },
        };
        let jni_era_id = match peek_info.era_id.as_ref() {
            None => JObject::null(),
            Some(era_id) => env.new_string(era_id)?.into(),
        };
        let jni_max_devices = self.get_optional_u32_long_object(env, peek_info.max_devices)?;
        let jni_device_count_including_pending =
            peek_info.device_count_including_pending_devices() as jlong;
        let jni_device_count_excluding_pending = peek_info.devices.len() as jlong;

        let pending_users = peek_info.unique_pending_users();
        let pending_user_list = jni_new_arraylist(env, pending_users.len())?;
        let pending_user_list = env.get_list(&pending_user_list)?;
        for pending_user in pending_users {
            let jni_opaque_user_id = match env.byte_array_from_slice(pending_user) {
                Ok(v) => JObject::from(v),
                Err(error) => {
                    error!("{:?}", error);
                    continue;
                }
            };

            let result = pending_user_list.add(env, &jni_opaque_user_id);
            if result.is_err() {
                error!("{:?}", result.err());
                continue;
            }
        }

        let jni_call_link_state =
            match self.make_call_link_state_object(env, &peek_info.call_link_state) {
                Ok(value) => value,
                Err(e) => {
                    error!("make_call_link_state_object: {:?}", e);
                    return Ok(JObject::null());
                }
            };

        let args = jni_args!((
            joined_member_list => java.util.List,
            jni_creator => [byte],
            jni_era_id => java.lang.String,
            jni_max_devices => java.lang.Long,
            jni_device_count_including_pending => long,
            jni_device_count_excluding_pending => long,
            pending_user_list => java.util.List,
            jni_call_link_state => org.signal.ringrtc.CallLinkState,
        ) -> org.signal.ringrtc.PeekInfo);
        let result = env.call_static_method(
            self.class_cache.get_class(PEEK_INFO_CLASS)?,
            "fromNative",
            args.sig,
            &args.args,
        )?;
        Ok(result.l()?)
    }
}

impl http::Delegate for AndroidPlatform {
    fn send_request(&self, request_id: u32, request: http::Request) {
        if let Err(err) = self.send_http_request(request_id, request) {
            error!("AndroidPlatform.send_http_request failed: {:?}", err);
        }
    }
}

impl sfu::Delegate for AndroidPlatform {
    fn handle_peek_result(&self, request_id: u32, peek_result: PeekResult) {
        info!("handle_peek_response():");

        let mut env = match self.java_env() {
            Ok(v) => v,
            Err(error) => {
                error!("{:?}", error);
                return;
            }
        };
        let jni_call_manager = self.jni_call_manager.as_obj();

        let http_result_class = match self.class_cache.get_class(HTTP_RESULT_CLASS) {
            Ok(v) => v,
            Err(error) => {
                error!("http_result_class: {:?}", error);
                return;
            }
        };

        let result_object = match peek_result {
            Ok(peek_info) => {
                let joined_members = peek_info.unique_users();

                // Set a frame capacity of min (5) + objects (5) + elements (N * 1 object per element).
                let capacity = (10 + joined_members.len()) as i32;
                let result = env.with_local_frame_returning_local(capacity, |env| -> Result<_> {
                    let jni_peek_info = match self.make_peek_info_object(
                        env,
                        &peek_info,
                        &mut joined_members.into_iter(),
                    ) {
                        Ok(value) => value,
                        Err(e) => {
                            error!("make_peek_info_object: {:?}", e);
                            return Ok(JObject::null());
                        }
                    };

                    let args = jni_args!((
                        jni_peek_info => java.lang.Object,
                    ) -> void);
                    Ok(env.new_object(http_result_class, args.sig, &args.args)?)
                });
                match result {
                    Ok(v) if !v.is_null() => v,
                    Ok(_) => {
                        // Already logged, so just bail out early.
                        return;
                    }
                    Err(error) => {
                        error!("new HttpResult(PeekInfo): {:?}", error);
                        return;
                    }
                }
            }
            Err(status) => {
                let args = jni_args!((
                    status.code as jshort => short,
                ) -> void);
                match env.new_object(http_result_class, args.sig, &args.args) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("new HttpResult(short): {:?}", error);
                        return;
                    }
                }
            }
        };

        let result = jni_call_method(
            &mut env,
            jni_call_manager,
            "handlePeekResponse",
            jni_args!((
                request_id as jlong => long,
                result_object => org.signal.ringrtc.CallManager::HttpResult,
            ) -> void),
        );
        if result.is_err() {
            error!("jni_call_method: {:?}", result.err());
        }
    }
}
