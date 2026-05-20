//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Android Platform Interface.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
    time::{Duration, SystemTime},
};

use jni::{
    Env, JavaVM, jni_str,
    objects::{Global, JObject},
    refs::{IntoAuto, LoaderContext, Reference as _},
    sys::{jint, jlong, jshort},
};

use crate::{
    android::{
        error::AndroidError,
        types::{
            self, CallEndReason as JCallEndReason, CallEvent, CallLinkRootKey as JCallLinkRootKey,
            CallLinkState as JCallLinkState, CallMediaType as JCallMediaType,
            CallSummary as JCallSummary, ConnectionState, HangupType, HttpHeader, HttpMethod,
            HttpResult, JArrayList, JBoolean, JFloat, JHashMap, JLong, JoinState,
            MediaQualityStats as JMediaQualityStats, PeekInfo as JPeekInfo,
            QualityStats as JQualityStats, Reaction, ReceivedAudioLevel as JReceivedAudioLevel,
            RemoteDeviceState, SpeechEvent,
        },
        webrtc_java_media_stream::JavaMediaStream,
    },
    common::{
        ApplicationEvent, CallConfig, CallDirection, CallEndReason, CallId, CallMediaType,
        DeviceId, Result,
    },
    core::{
        call::Call,
        call_summary::{CallSummary, MediaQualityStats, QualityStats},
        connection::{Connection, ConnectionType},
        group_call,
        platform::{Platform, PlatformItem},
        signaling,
        util::try_scoped,
    },
    lite::{
        call_links::{CallLinkRestrictions, CallLinkRootKey, CallLinkState, Empty},
        http,
        sfu::{self, DemuxId, PeekInfo, PeekResult, UserId},
    },
    webrtc::{
        media::{MediaStream, VideoTrack},
        peer_connection::{AudioLevel, ReceivedAudioLevel},
        peer_connection_observer::NetworkRoute,
    },
};

/// Android implementation for platform::Platform::AppIncomingMedia
pub type AndroidMediaStream = JavaMediaStream;
impl PlatformItem for AndroidMediaStream {}

/// Android implementation for platform::Platform::AppRemotePeer.
pub type AndroidGlobalRef = Arc<Global<JObject<'static>>>;
impl PlatformItem for AndroidGlobalRef {}

/// Android implementation for platform::Platform::AppCallContext
struct JavaCallContext {
    /// Java JVM object.
    platform: AndroidPlatform,
    /// Java CallContext object.
    jni_call_context: Global<JObject<'static>>,
}

impl Drop for JavaCallContext {
    fn drop(&mut self) {
        info!("JavaCallContext::drop()");

        // call into CMI to close CallContext object
        let _ = self.platform.with_java_env(|env| -> Result<()> {
            let jni_call_manager = self.platform.jni_call_manager.as_obj();
            let jni_call_context = self.jni_call_context.as_obj();

            jni_call_method!(
                env,
                jni_call_manager,
                jni_str!("closeCall"),
                (
                    jni_call_context => org.signal.ringrtc.CallManager::CallContext,
                ) -> void
            )?;
            Ok(())
        });
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
    pub fn new(platform: AndroidPlatform, jni_call_context: Global<JObject<'static>>) -> Self {
        Self {
            inner: Arc::new(JavaCallContext {
                platform,
                jni_call_context,
            }),
        }
    }

    pub fn to_jni(&self) -> &Global<JObject<'static>> {
        &self.inner.jni_call_context
    }
}

/// Android implementation for platform::Platform::AppConnection
struct JavaConnection {
    /// Java JVM object.
    platform: AndroidPlatform,
    /// Java Connection object.
    jni_connection: Global<JObject<'static>>,
}

impl Drop for JavaConnection {
    fn drop(&mut self) {
        info!("JavaConnection::drop()");

        // call into CMI to close Connection object
        let jni_call_manager = self.platform.jni_call_manager.as_obj();
        let jni_connection = self.jni_connection.as_obj();
        let _ = self.platform.with_java_env(|env| -> Result<()> {
            jni_call_method!(
                env,
                jni_call_manager,
                jni_str!("closeConnection"),
                (
                    jni_connection => org.signal.ringrtc.Connection,
                ) -> void
            )?;
            Ok(())
        });
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
    fn new(platform: AndroidPlatform, jni_connection: Global<JObject<'static>>) -> Self {
        Self {
            inner: Arc::new(JavaConnection {
                platform,
                jni_connection,
            }),
        }
    }

    pub fn to_jni(&self) -> &Global<JObject<'static>> {
        &self.inner.jni_connection
    }
}

/// Android implementation of platform::Platform.
pub struct AndroidPlatform {
    /// Java JVM object.
    jvm: JavaVM,
    /// Java org.signal.ringrtc.CallManager object.
    jni_call_manager: Global<JObject<'static>>,
}

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
    }
}

macro_rules! request_update_via_jni {
    (
        $s:ident,
        $f:literal,
        $i:ident
    ) => {{
        let jni_call_manager = $s.jni_call_manager.as_obj();
        let jni_client_id = $i as jlong;

        const METHOD: &jni::strings::JNIStr = jni_str!($f);
        if let Err(error) = $s.with_java_env(|env| -> Result<()> {
            jni_call_method!(
                env,
                jni_call_manager,
                METHOD,
                (
                    jni_client_id => long,
                ) -> void
            )?;
            Ok(())
        }) {
            error!("jni_call_method: {:?}", error);
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

        self.with_java_env(|env| {
            let jni_connection = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("createConnection"),
                (
                    connection_ptr.as_ptr() as jlong => long,
                    call_id_jlong => long,
                    jni_remote_device_id => int,
                    jni_call_context.as_obj() => org.signal.ringrtc.CallManager::CallContext,
                    audio_jitter_buffer_max_packets => int,
                    audio_jitter_buffer_max_target_delay_ms => int,
                ) -> org.signal.ringrtc.Connection
            )?
            .into_object()?;

            if jni_connection.is_null() {
                return Err(AndroidError::CreateJniConnection.into());
            }
            let jni_connection = env.new_global_ref(jni_connection)?;
            let platform = self.try_clone()?;
            let android_connection = AndroidConnection::new(platform, jni_connection);
            connection.set_app_connection(android_connection)?;

            Ok(connection)
        })
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

        self.with_java_env(|env| {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let is_outgoing = match direction {
                CallDirection::Outgoing => true,
                CallDirection::Incoming => false,
            };
            let jni_call_media_type =
                JCallMediaType::from_native_index(env, call_media_type as i32)?.auto();

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onStartCall"),
                (
                    jni_remote => org.signal.ringrtc.Remote,
                    call_id_jlong => long,
                    is_outgoing => boolean,
                    jni_call_media_type => org.signal.ringrtc.CallManager::CallMediaType,
                ) -> void
            )?;

            Ok(())
        })
    }

    fn on_call_ended(
        &self,
        remote_peer: &Self::AppRemotePeer,
        _call_id: CallId,
        reason: CallEndReason,
        summary: CallSummary,
    ) -> Result<()> {
        info!("on_call_ended(): {}", reason);

        self.with_java_env(|env| {
            let jni_remote = remote_peer.as_obj();
            let jni_summary = self.make_call_summary_object(env, &summary)?;
            let jni_reason = JCallEndReason::from_native_index(env, reason as i32)?.auto();

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onCallEnded"),
                (
                    jni_remote => org.signal.ringrtc.Remote,
                    jni_reason => org.signal.ringrtc.CallManager::CallEndReason,
                    jni_summary => org.signal.ringrtc.CallSummary,
                ) -> void
            )?;

            Ok(())
        })
    }

    fn on_event(
        &self,
        remote_peer: &Self::AppRemotePeer,
        _call_id: CallId,
        event: ApplicationEvent,
    ) -> Result<()> {
        info!("on_event(): {}", event);

        self.with_java_env(|env| {
            let jni_remote = remote_peer.as_obj();
            let jni_event = CallEvent::from_native_index(env, event as i32)?.auto();

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onEvent"),
                (
                    jni_remote => org.signal.ringrtc.Remote,
                    jni_event => org.signal.ringrtc.CallManager::CallEvent,
                ) -> void
            )?;

            Ok(())
        })
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

        self.with_java_env(|env| {
            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onNetworkRouteChanged"),
                (
                    remote_peer.as_obj() => org.signal.ringrtc.Remote,
                    network_route.local_adapter_type as i32 => int,
                ) -> void
            )?;
            Ok(())
        })
    }

    fn on_audio_levels(
        &self,
        remote_peer: &Self::AppRemotePeer,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        trace!(
            "on_audio_levels(): captured_level: {}; received_level: {}",
            captured_level, received_level
        );

        self.with_java_env(|env| {
            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onAudioLevels"),
                (
                    remote_peer.as_obj() => org.signal.ringrtc.Remote,
                    captured_level as i32 => int,
                    received_level as i32 => int,
                ) -> void
            )?;
            Ok(())
        })
    }

    fn on_low_bandwidth_for_video(
        &self,
        remote_peer: &Self::AppRemotePeer,
        recovered: bool,
    ) -> Result<()> {
        info!("on_low_bandwidth_for_video(): recovered: {}", recovered);

        self.with_java_env(|env| {
            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onLowBandwidthForVideo"),
                (
                    remote_peer.as_obj() => org.signal.ringrtc.Remote,
                    recovered => boolean,
                ) -> void
            )?;
            Ok(())
        })
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

        self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (3).
            let capacity = 8;
            env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_remote = remote_peer.as_obj();
                let call_id_jlong = u64::from(call_id) as jlong;
                let receiver_device_id = receiver_device_id as jint;
                let jni_opaque = JObject::from(env.byte_array_from_slice(&offer.opaque)?);
                let jni_call_media_type =
                    match JCallMediaType::from_native_index(env, offer.call_media_type as i32) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("jni_call_media_type: {:?}", error);
                            return Ok(());
                        }
                    };

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("onSendOffer"),
                    (
                        call_id_jlong => long,
                        jni_remote => org.signal.ringrtc.Remote,
                        receiver_device_id => int,
                        broadcast => boolean,
                        jni_opaque => [byte],
                        jni_call_media_type => org.signal.ringrtc.CallManager::CallMediaType,
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        })
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

        self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (2).
            let capacity = 7;
            env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_remote = remote_peer.as_obj();
                let call_id_jlong = u64::from(call_id) as jlong;
                let receiver_device_id = receiver_device_id as jint;
                let jni_opaque = JObject::from(env.byte_array_from_slice(&send.answer.opaque)?);

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("onSendAnswer"),
                    (
                        call_id_jlong => long,
                        jni_remote => org.signal.ringrtc.Remote,
                        receiver_device_id => int,
                        broadcast => boolean,
                        jni_opaque => [byte],
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        })
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

        self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (3) + elements (N * 3 object per element).
            let capacity = 8 + send.ice.candidates.len() * 3;
            env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_remote = remote_peer.as_obj();
                let call_id_jlong = u64::from(call_id) as jlong;
                let receiver_device_id = receiver_device_id as jint;

                let list = JArrayList::with_capacity(env, send.ice.candidates.len() as jint)?;
                let ice_candidate_list = match jni::objects::JList::cast_local(env, list) {
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

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("onSendIceCandidates"),
                    (
                        call_id_jlong => long,
                        jni_remote => org.signal.ringrtc.Remote,
                        receiver_device_id => int,
                        broadcast => boolean,
                        ice_candidate_list => java.util.List,
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        })
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

        self.with_java_env(|env| {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;

            let (hangup_type, hangup_device_id) = send.hangup.to_type_and_device_id();
            // We set the device_id to 0 in case it is not defined. It will
            // only be used for hangup types other than Normal.
            let hangup_device_id = hangup_device_id.unwrap_or(0) as jint;
            let jni_hangup_type = HangupType::from_native_index(env, hangup_type as i32)?.auto();

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onSendHangup"),
                (
                    call_id_jlong => long,
                    jni_remote => org.signal.ringrtc.Remote,
                    receiver_device_id => int,
                    broadcast => boolean,
                    jni_hangup_type => org.signal.ringrtc.CallManager::HangupType,
                    hangup_device_id => int,
                ) -> void
            )?;

            Ok(())
        })
    }

    fn on_send_busy(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()> {
        // Busy messages are always broadcast
        let broadcast = true;
        let receiver_device_id = 0;

        info!("on_send_busy(): call_id: {}", call_id);

        self.with_java_env(|env| {
            let jni_remote = remote_peer.as_obj();
            let call_id_jlong = u64::from(call_id) as jlong;
            let receiver_device_id = receiver_device_id as jint;

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onSendBusy"),
                (
                    call_id_jlong => long,
                    jni_remote => org.signal.ringrtc.Remote,
                    receiver_device_id => int,
                    broadcast => boolean,
                ) -> void
            )?;

            Ok(())
        })
    }

    fn send_call_message(
        &self,
        recipient_uuid: UserId,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (2).
            let capacity = 7;
            env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_recipient_uuid = JObject::from(env.byte_array_from_slice(&recipient_uuid)?);
                let jni_message = JObject::from(env.byte_array_from_slice(&message)?);

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("sendCallMessage"),
                    (
                        jni_recipient_uuid => [byte],
                        jni_message => [byte],
                        urgency as i32 => int,
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        })
    }

    fn send_call_message_to_group(
        &self,
        group_id: group_call::GroupId,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
        recipients_override: HashSet<UserId>,
    ) -> Result<()> {
        self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (3) + elements (N * 1 object per element).
            let capacity = 8 + recipients_override.len();
            env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_group_id = JObject::from(env.byte_array_from_slice(&group_id)?);

                let list = JArrayList::with_capacity(env, recipients_override.len() as jint)?;
                let recipients_override_list = jni::objects::JList::cast_local(env, list)?;
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

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("sendCallMessageToGroup"),
                    (
                        jni_group_id => [byte],
                        jni_message => [byte],
                        urgency as i32 => int,
                        recipients_override_list => java.util.List,
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        })
    }

    fn send_call_message_to_adhoc_group(
        &self,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
        expiration: u64,
        recipients_to_endorsements: HashMap<UserId, Vec<u8>>,
    ) -> Result<()> {
        self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (3) + key-values (2 objects per pair).
            let capacity = 8 + 2 * recipients_to_endorsements.len();
            env.with_local_frame(capacity, |env| -> Result<()> {
                let map = JHashMap::with_capacity(env, recipients_to_endorsements.len() as jint)?;
                let jni_recipients_map = jni::objects::JMap::cast_local(env, map)?;
                for (recipient, endorsement) in recipients_to_endorsements {
                    let jni_opaque_user_id = match env.byte_array_from_slice(&recipient) {
                        Ok(v) => JObject::from(v),
                        Err(error) => {
                            error!("{:?}", error);
                            continue;
                        }
                    };
                    let jni_endorsement = match env.byte_array_from_slice(&endorsement) {
                        Ok(v) => JObject::from(v),
                        Err(error) => {
                            error!("{:?}", error);
                            continue;
                        }
                    };

                    let result = jni_recipients_map.put(env, &jni_opaque_user_id, &jni_endorsement);
                    if result.is_err() {
                        error!("{:?}", result.err());
                        continue;
                    }
                }

                let jni_message = JObject::from(env.byte_array_from_slice(&message)?);
                let jni_expiration = expiration as jlong;

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("sendCallMessageToAdhocGroup"),
                    (
                        jni_message => [byte],
                        urgency as i32 => int,
                        jni_expiration => long,
                        jni_recipients_map => java.util.Map,
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        })
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

        let jni_call_context = app_call_context.to_jni();
        self.with_java_env(|env| {
            let jni_media_stream = incoming_media.global_ref(env)?;

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onConnectMedia"),
                (
                    jni_call_context.as_obj() => org.signal.ringrtc.CallManager::CallContext,
                    jni_media_stream.as_obj() => org.webrtc.MediaStream,
                ) -> void
            )?;

            Ok(())
        })
    }

    fn disconnect_incoming_media(&self, app_call_context: &Self::AppCallContext) -> Result<()> {
        info!("disconnect_incoming_media():");

        let jni_call_context = app_call_context.to_jni();
        self.with_java_env(|env| {
            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onCloseMedia"),
                (
                    jni_call_context.as_obj() => org.signal.ringrtc.CallManager::CallContext,
                ) -> void
            )?;

            Ok(())
        })
    }

    fn compare_remotes(
        &self,
        remote_peer1: &Self::AppRemotePeer,
        remote_peer2: &Self::AppRemotePeer,
    ) -> Result<bool> {
        info!("remotes_equal():");

        self.with_java_env(|env| {
            let jni_remote1 = remote_peer1.as_obj();
            let jni_remote2 = remote_peer2.as_obj();

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("compareRemotes"),
                (
                    jni_remote1 => org.signal.ringrtc.Remote,
                    jni_remote2 => org.signal.ringrtc.Remote,
                ) -> boolean
            )?
            .into_bool()?;

            Ok(result)
        })
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

        self.with_java_env(|env| {
            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("onCallConcluded"),
                (
                    remote_peer.as_obj() => org.signal.ringrtc.Remote,
                ) -> void
            )?;

            Ok(())
        })
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

        let _ = self.with_java_env(|env| {
            let group_id = JObject::from(env.byte_array_from_slice(&group_id)?);
            let ring_id = jlong::from(ring_id);
            let sender = JObject::from(env.byte_array_from_slice(&sender)?);
            let update = update as jint;

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("groupCallRingUpdate"),
                (
                    group_id => [byte],
                    ring_id => long,
                    sender => [byte],
                    update => int,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
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

        let _ = self.with_java_env(|env| {
            let jni_client_id = client_id as jlong;
            let jni_connection_state =
                ConnectionState::from_native_index(env, connection_state.ordinal())?.auto();

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleConnectionStateChanged"),
                (
                    jni_client_id => long,
                    jni_connection_state => org.signal.ringrtc.GroupCall::ConnectionState,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
        });
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

        let _ = self.with_java_env(|env| {
            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleNetworkRouteChanged"),
                (
                    client_id as jlong => long,
                    network_route.local_adapter_type as i32 => int,
                ) -> void
            )?;
            Ok(())
        });
    }

    fn handle_speaking_notification(
        &self,
        client_id: group_call::ClientId,
        event: group_call::SpeechEvent,
    ) {
        info!(
            "handle_speaking_notification(): client_id {}, event: {:?}",
            client_id, event
        );

        let _ = self.with_java_env(|env| {
            let jni_speech_event = SpeechEvent::from_native_index(env, event.ordinal())?.auto();

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleSpeakingNotification"),
                (
                    client_id as jlong => long,
                    jni_speech_event => org.signal.ringrtc.GroupCall::SpeechEvent,
                ) -> void
            )?;
            Ok(())
        });
    }

    fn handle_audio_levels(
        &self,
        client_id: group_call::ClientId,
        captured_level: AudioLevel,
        received_levels: Vec<ReceivedAudioLevel>,
    ) {
        trace!(
            "handle_audio_levels(): client_id: {}, captured_level: {:?}, received_levels: {:?}",
            client_id, captured_level, received_levels,
        );

        let _ = self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (2) + elements (N * 2 per level).
            let capacity = 5 + 2 + received_levels.len() * 2;
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                // create Java List<GroupCall.ReceivedAudioLevel>
                let received_level_class =
                    JReceivedAudioLevel::lookup_class(env, &LoaderContext::default())?;

                let list = JArrayList::with_capacity(env, received_levels.len() as jint)?;
                let received_levels_list = jni::objects::JList::cast_local(env, list)?;

                for received in received_levels {
                    let received_level_obj = match jni_new_object!(env, &*received_level_class, (
                        received.demux_id as jlong => long,
                        received.level as jint => int,
                    )) {
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

                let _ = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("handleAudioLevels"),
                    (
                        client_id as jlong => long,
                        captured_level as jint => int,
                        received_levels_list => java.util.List,
                    ) -> void
                );

                Ok(())
            }) {
                error!("handle_audio_levels: {:?}", e);
            }
            Ok(())
        });
    }

    fn handle_low_bandwidth_for_video(&self, client_id: group_call::ClientId, recovered: bool) {
        info!(
            "handle_low_bandwidth_for_video(): client_id: {}, recovered: {}",
            client_id, recovered
        );

        let _ = self.with_java_env(|env| {
            // Set a frame capacity of min (5).
            let capacity = 5;
            env.with_local_frame(capacity, |env| -> Result<()> {
                let _ = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("handleLowBandwidthForVideo"),
                    (
                        client_id as jlong => long,
                        recovered => boolean,
                    ) -> void
                );

                Ok(())
            })
        });
    }

    fn handle_reactions(
        &self,
        client_id: group_call::ClientId,
        reactions: Vec<group_call::Reaction>,
    ) {
        trace!(
            "handle_reactions(): client_id: {}, reactions: {:?}",
            client_id, reactions,
        );

        let _ = self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (1) + elements (N * 2 per reaction).
            let capacity = 5 + 1 + reactions.len() * 2;
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                // create Java List<GroupCall.Reaction>
                let reaction_class = Reaction::lookup_class(env, &LoaderContext::default())?;

                let list = JArrayList::with_capacity(env, reactions.len() as jint)?;
                let reactions_list = jni::objects::JList::cast_local(env, list)?;

                for reaction in reactions {
                    let jni_value = JObject::from(env.new_string(reaction.value)?);
                    let reaction_obj = match jni_new_object!(env, &*reaction_class, (
                        reaction.demux_id as jlong => long,
                        jni_value => java.lang.String,
                    )) {
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

                let _ = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("handleReactions"),
                    (
                        client_id as jlong => long,
                        reactions_list => java.util.List,
                    ) -> void
                );

                Ok(())
            }) {
                error!("handle_reactions: {:?}", e);
            }
            Ok(())
        });
    }

    fn handle_raised_hands(&self, client_id: group_call::ClientId, raised_hands: Vec<DemuxId>) {
        info!(
            "handle_raised_hands(): client_id: {}, raised_hands: {:?}",
            client_id, raised_hands,
        );

        let _ = self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (1) + N elements.
            let capacity = 5 + 1 + raised_hands.len();
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                // create Java List<Long>
                let list = JArrayList::with_capacity(env, raised_hands.len() as jint)?;
                let raised_hands_list = jni::objects::JList::cast_local(env, list)?;

                for raised_hand in raised_hands {
                    let raised_hand_obj = match JLong::new(env, raised_hand as jlong) {
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

                let _ = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("handleRaisedHands"),
                    (
                        client_id as jlong => long,
                        raised_hands_list => java.util.List,
                    ) -> void
                );

                Ok(())
            }) {
                error!("handle_raised_hands: {:?}", e);
            }
            Ok(())
        });
    }

    fn handle_join_state_changed(
        &self,
        client_id: group_call::ClientId,
        join_state: group_call::JoinState,
    ) {
        info!("handle_join_state_changed():");

        let _ = self.with_java_env(|env| {
            let jni_client_id = client_id as jlong;
            let jni_join_state = JoinState::from_native_index(env, join_state.ordinal())?.auto();
            let jni_demux_id = match join_state {
                group_call::JoinState::Pending(demux_id)
                | group_call::JoinState::Joined(demux_id) => {
                    self.get_optional_u32_long_object(env, Some(demux_id))?
                }
                _ => JObject::null(),
            };

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleJoinStateChanged"),
                (
                    jni_client_id => long,
                    jni_join_state => org.signal.ringrtc.GroupCall::JoinState,
                    jni_demux_id => java.lang.Long,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
        });
    }

    fn handle_remote_devices_changed(
        &self,
        client_id: group_call::ClientId,
        remote_device_states: &[group_call::RemoteDeviceState],
        _reason: group_call::RemoteDevicesChangedReason,
    ) {
        info!("handle_remote_devices_changed():");

        let _ = self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (2) + elements (N * 2 object per element).
            let capacity = 7 + remote_device_states.len() * 2;
            if let Err(e) = env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_client_id = client_id as jlong;

                // create Java List<GroupCall.RemoteDeviceState>
                let remote_device_state_class =
                    RemoteDeviceState::lookup_class(env, &LoaderContext::default())?;

                let list = JArrayList::with_capacity(env, remote_device_states.len() as jint)?;
                let remote_device_state_list = jni::objects::JList::cast_local(env, list)?;

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

                    let remote_device_state_obj = match jni_new_object!(
                        env,
                        &*remote_device_state_class,
                        (
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
                        )
                    ) {
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

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("handleRemoteDevicesChanged"),
                    (
                        jni_client_id => long,
                        remote_device_state_list => java.util.List,
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            }) {
                error!("handle_remote_devices_changed {:?}", e);
            }
            Ok(())
        });
    }

    fn handle_incoming_video_track(
        &self,
        client_id: group_call::ClientId,
        remote_demux_id: DemuxId,
        incoming_video_track: VideoTrack,
    ) {
        info!("handle_incoming_video_track():");

        let _ = self.with_java_env(|env| {
            let jni_client_id = client_id as jlong;
            let jni_remote_demux_id = remote_demux_id as jlong;
            let jni_native_video_track_owned_rc =
                incoming_video_track.rffi().clone().into_owned().as_ptr() as jlong;

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleIncomingVideoTrack"),
                (
                    jni_client_id => long,
                    jni_remote_demux_id => long,
                    jni_native_video_track_owned_rc => long,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
        });
    }

    fn handle_peek_changed(
        &self,
        client_id: group_call::ClientId,
        peek_info: &PeekInfo,
        joined_members: &HashSet<UserId>,
    ) {
        info!("handle_peek_changed():");

        let _ = self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (5) + elements (N * 1 object per element).
            let capacity = 10 + joined_members.len();
            env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_client_id = client_id as jlong;

                let jni_peek_info =
                    match self.make_peek_info_object(env, peek_info, &mut joined_members.iter()) {
                        Ok(value) => value,
                        Err(e) => {
                            error!("make_peek_info_object: {:?}", e);
                            return Ok(());
                        }
                    };

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("handlePeekChanged"),
                    (
                        jni_client_id => long,
                        jni_peek_info => org.signal.ringrtc.PeekInfo,
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        });
    }

    fn handle_ended(
        &self,
        client_id: group_call::ClientId,
        reason: CallEndReason,
        summary: CallSummary,
    ) {
        info!("handle_ended():");

        let _ = self.with_java_env(|env| {
            let jni_client_id = client_id as jlong;
            let jni_reason = JCallEndReason::from_native_index(env, reason as i32)?.auto();
            let jni_summary = self.make_call_summary_object(env, &summary)?;

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleEnded"),
                (
                    jni_client_id => long,
                    jni_reason => org.signal.ringrtc.CallManager::CallEndReason,
                    jni_summary => org.signal.ringrtc.CallSummary,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
        });
    }

    fn handle_remote_mute_request(&self, client_id: group_call::ClientId, mute_source: DemuxId) {
        info!(
            "handle_remote_mute_request(): client_id: {}, mute_source: {}",
            client_id, mute_source,
        );

        let _ = self.with_java_env(|env| {
            let jni_client_id = client_id as jlong;
            let jni_mute_source = mute_source as jlong;

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleRemoteMuteRequest"),
                (
                    jni_client_id => long,
                    jni_mute_source => long,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
        });
    }

    fn handle_observed_remote_mute(
        &self,
        client_id: group_call::ClientId,
        mute_source: DemuxId,
        mute_target: DemuxId,
    ) {
        info!(
            "handle_observed_remote_mute(): client_id: {}, mute_source: {}, mute_target: {}",
            client_id, mute_source, mute_target
        );

        let _ = self.with_java_env(|env| {
            let jni_client_id = client_id as jlong;
            let jni_mute_source = mute_source as jlong;
            let jni_mute_target = mute_target as jlong;

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleObservedRemoteMute"),
                (
                    jni_client_id => long,
                    jni_mute_source => long,
                    jni_mute_target => long,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
        });
    }
}

impl AndroidPlatform {
    /// Create a new AndroidPlatform object.
    pub fn new(env: &mut Env, jni_call_manager: Global<JObject<'static>>) -> Result<Self> {
        types::init_class_cache(env)?;

        Ok(Self {
            jvm: env.get_java_vm()?,
            jni_call_manager,
        })
    }

    /// Run a closure with a JNI Env attached to the current thread.
    ///
    /// If the function `f` leaves a Java exception pending, for example, if
    /// the Java callback threw. Reports uncaught exceptions on destruction.
    fn with_java_env<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut jni::Env) -> Result<R>,
    {
        self.jvm.attach_current_thread(|env| {
            let result = f(env);
            if let Some(exception) = env.exception_occurred() {
                env.exception_clear();
                let _ = try_scoped(|| -> Result<()> {
                    let thread = jni_call_static_method!(
                        env,
                        jni_str!("java/lang/Thread"),
                        jni_str!("currentThread"),
                        () -> java.lang.Thread
                    )?
                    .into_object()?;
                    let handler = jni_call_method!(
                        env,
                        &thread,
                        jni_str!("getUncaughtExceptionHandler"),
                        () -> java.lang.Thread::UncaughtExceptionHandler
                    )?
                    .into_object()?;
                    jni_call_method!(
                        env,
                        &handler,
                        jni_str!("uncaughtException"),
                        (
                            thread => java.lang.Thread,
                            exception => java.lang.Throwable,
                        ) -> void
                    )?;
                    Ok(())
                });
            }
            result
        })
    }

    pub fn try_clone(&self) -> Result<Self> {
        self.with_java_env(|env| {
            Ok(Self {
                jvm: env.get_java_vm()?,
                jni_call_manager: env.new_global_ref(self.jni_call_manager.as_obj())?,
            })
        })
    }

    fn get_optional_boolean_object<'a>(
        &self,
        env: &mut Env<'a>,
        value: Option<bool>,
    ) -> Result<JObject<'a>> {
        match value {
            None => Ok(JObject::null()),
            Some(value) => Ok(JBoolean::new(env, value)?.into()),
        }
    }

    // Converts Option<u32> to a Java Long.
    fn get_optional_u32_long_object<'local>(
        &self,
        env: &mut Env<'local>,
        value: Option<u32>,
    ) -> Result<JObject<'local>> {
        match value {
            None => Ok(JObject::null()),
            Some(value) => Ok(JLong::new(env, value as jlong)?.into()),
        }
    }

    // Converts Option<f32> to Java Float.
    fn get_optional_f32_float_object<'local>(
        &self,
        env: &mut Env<'local>,
        value: Option<f32>,
    ) -> Result<JObject<'local>> {
        match value {
            None => Ok(JObject::null()),
            Some(value) => Ok(JFloat::new(env, value)?.into()),
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

        self.with_java_env(|env| {
            // Set a frame capacity of min (5) + objects (4) + elements (N * 3 objects per element).
            let capacity = 9 + headers.len() * 3;
            env.with_local_frame(capacity, |env| -> Result<()> {
                let jni_request_id = request_id as jlong;
                let jni_url = JObject::from(env.new_string(url)?);
                let jni_method = match HttpMethod::from_native_index(env, method as i32) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("jni_method: {:?}", error);
                        return Ok(());
                    }
                };

                // create Java List<HttpHeader>
                let http_header_class =
                    match HttpHeader::lookup_class(env, &LoaderContext::default()) {
                        Ok(v) => v,
                        Err(error) => {
                            error!("http_header_class: {:?}", error);
                            return Ok(());
                        }
                    };
                let list = JArrayList::with_capacity(env, headers.len() as jint)?;
                let jni_headers = match jni::objects::JList::cast_local(env, list) {
                    Ok(v) => v,
                    Err(error) => {
                        error!("jni_headers: {:?}", error);
                        return Ok(());
                    }
                };
                for (name, value) in headers.iter() {
                    let jni_name = JObject::from(env.new_string(name)?);
                    let jni_value = JObject::from(env.new_string(value)?);
                    let http_header_obj = jni_new_object!(env, &*http_header_class, (
                        jni_name => java.lang.String,
                        jni_value => java.lang.String,
                    ))?;
                    jni_headers.add(env, &http_header_obj)?;
                }

                let jni_body = match body {
                    None => JObject::null(),
                    Some(body) => JObject::from(env.byte_array_from_slice(&body)?),
                };

                let result = jni_call_method!(
                    env,
                    self.jni_call_manager.as_obj(),
                    jni_str!("sendHttpRequest"),
                    (
                        jni_request_id => long,
                        jni_url => java.lang.String,
                        jni_method => org.signal.ringrtc.CallManager::HttpMethod,
                        jni_headers => java.util.List,
                        jni_body => [byte],
                    ) -> void
                );
                if result.is_err() {
                    error!("jni_call_method: {:?}", result.err());
                }

                Ok(())
            })
        })
    }

    pub fn handle_call_link_result(
        &self,
        request_id: u32,
        response: std::result::Result<CallLinkState, http::ResponseStatus>,
    ) {
        let _ = self.with_java_env(|env| {
            let http_result_class = HttpResult::lookup_class(env, &LoaderContext::default())?;

            let result_object = match response {
                Ok(result) => {
                    let call_link_state = Some(result);
                    let state = self.make_call_link_state_object(env, &call_link_state)?;
                    // Unconstrained generics get erased to java.lang.Object.
                    jni_new_object!(env, &*http_result_class, (
                        state => java.lang.Object,
                    ))?
                }
                Err(status) => jni_new_object!(env, &*http_result_class, (
                    status.code as jshort => short,
                ))?,
            };

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleCallLinkResponse"),
                (
                    request_id as jlong => long,
                    result_object => org.signal.ringrtc.CallManager::HttpResult,
                ) -> void
            )?;
            Ok(())
        });
    }

    pub fn handle_empty_result(
        &self,
        request_id: u32,
        response: std::result::Result<Empty, http::ResponseStatus>,
    ) {
        let _ = self.with_java_env(|env| {
            let http_result_class = HttpResult::lookup_class(env, &LoaderContext::default())?;

            let result_object = match response {
                Ok(_) => {
                    // need to provide a non-null Object, so we use java.lang.Boolean
                    let success_filler = self.get_optional_boolean_object(env, Some(true))?;

                    // Unconstrained generics get erased to java.lang.Object.
                    jni_new_object!(env, &*http_result_class, (
                        success_filler => java.lang.Object,
                    ))?
                }
                Err(status) => jni_new_object!(env, &*http_result_class, (
                    status.code as jshort => short,
                ))?,
            };

            jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handleEmptyResponse"),
                (
                    request_id as jlong => long,
                    result_object => org.signal.ringrtc.CallManager::HttpResult,
                ) -> void
            )?;
            Ok(())
        });
    }

    fn make_media_quality_stats_object<'a>(
        &self,
        env: &mut Env<'a>,
        media_quality_stats: &MediaQualityStats,
    ) -> jni::errors::Result<JObject<'a>> {
        let media_quality_stats_class =
            match JMediaQualityStats::lookup_class(env, &LoaderContext::default()) {
                Ok(v) => v,
                Err(error) => {
                    error!("media_quality_stats_class: {:?}", error);
                    return Ok(JObject::null());
                }
            };
        let rtt_median =
            match self.get_optional_f32_float_object(env, media_quality_stats.rtt_median) {
                Ok(v) => v,
                Err(error) => {
                    error!("media_quality_rtt_median: {:?}", error);
                    return Ok(JObject::null());
                }
            };
        let jitter_median_send =
            match self.get_optional_f32_float_object(env, media_quality_stats.jitter_median_send) {
                Ok(v) => v,
                Err(error) => {
                    error!("media_quality_jitter_median_send: {:?}", error);
                    return Ok(JObject::null());
                }
            };
        let jitter_median_recv =
            match self.get_optional_f32_float_object(env, media_quality_stats.jitter_median_recv) {
                Ok(v) => v,
                Err(error) => {
                    error!("media_quality_jitter_median_recv: {:?}", error);
                    return Ok(JObject::null());
                }
            };
        let packet_loss_fraction_send = match self
            .get_optional_f32_float_object(env, media_quality_stats.packet_loss_fraction_send)
        {
            Ok(v) => v,
            Err(error) => {
                error!("media_quality_packet_loss_fraction_send: {:?}", error);
                return Ok(JObject::null());
            }
        };
        let packet_loss_fraction_recv = match self
            .get_optional_f32_float_object(env, media_quality_stats.packet_loss_fraction_recv)
        {
            Ok(v) => v,
            Err(error) => {
                error!("media_quality_packet_loss_fraction_recv: {:?}", error);
                return Ok(JObject::null());
            }
        };

        jni_new_object!(env, &*media_quality_stats_class, (
            rtt_median => java.lang.Float,
            jitter_median_send => java.lang.Float,
            jitter_median_recv => java.lang.Float,
            packet_loss_fraction_send => java.lang.Float,
            packet_loss_fraction_recv => java.lang.Float,
        ))
    }

    fn make_quality_stats_object<'a>(
        &self,
        env: &mut Env<'a>,
        quality_stats: &QualityStats,
    ) -> jni::errors::Result<JObject<'a>> {
        let quality_stats_class = match JQualityStats::lookup_class(env, &LoaderContext::default())
        {
            Ok(v) => v,
            Err(error) => {
                error!("quality_stats_class: {:?}", error);
                return Ok(JObject::null());
            }
        };

        let rtt_median_connection =
            match self.get_optional_f32_float_object(env, quality_stats.rtt_median_connection) {
                Ok(v) => v,
                Err(error) => {
                    error!("quality_stats_rtt_median_connection: {:?}", error);
                    return Ok(JObject::null());
                }
            };
        let audio_stats = self.make_media_quality_stats_object(env, &quality_stats.audio_stats)?;
        let video_stats = self.make_media_quality_stats_object(env, &quality_stats.video_stats)?;

        jni_new_object!(env, &*quality_stats_class, (
            rtt_median_connection => java.lang.Float,
            audio_stats => org.signal.ringrtc.CallSummary::MediaQualityStats,
            video_stats => org.signal.ringrtc.CallSummary::MediaQualityStats,
        ))
    }

    fn make_call_summary_object<'a>(
        &self,
        env: &mut Env<'a>,
        call_summary: &CallSummary,
    ) -> jni::errors::Result<JObject<'a>> {
        let call_summary_class = match JCallSummary::lookup_class(env, &LoaderContext::default()) {
            Ok(v) => v,
            Err(error) => {
                error!("call_summary_class: {:?}", error);
                return Ok(JObject::null());
            }
        };

        let call_id_hash = {
            match call_summary.call_id_hash.as_ref() {
                Some(id) => env.byte_array_from_slice(id)?.into(),
                None => JObject::null(),
            }
        };
        let quality_stats_object =
            self.make_quality_stats_object(env, &call_summary.quality_stats)?;
        let start_time = u64::from(call_summary.start_time) as jlong;
        let end_time = u64::from(call_summary.end_time) as jlong;
        let raw_stats_object = {
            match call_summary.raw_stats.as_ref() {
                Some(raw_stats) => env.byte_array_from_slice(raw_stats)?.into(),
                None => JObject::null(),
            }
        };
        let raw_stats_text = {
            match call_summary.raw_stats_text.as_ref() {
                Some(raw_stats_text) => env.new_string(raw_stats_text)?.into(),
                None => JObject::null(),
            }
        };
        let raw_call_end_reason_text = env.new_string(call_summary.call_end_reason_text.clone())?;
        let is_survey_candidate = call_summary.is_survey_candidate;

        jni_new_object!(env, &*call_summary_class, (
            call_id_hash => [byte],
            start_time => long,
            end_time => long,
            quality_stats_object => org.signal.ringrtc.CallSummary::QualityStats,
            raw_stats_object => [byte],
            raw_stats_text => java.lang.String,
            raw_call_end_reason_text => java.lang.String,
            is_survey_candidate => boolean,
        ))
    }

    fn make_call_link_root_key_object<'a>(
        &self,
        env: &mut Env<'a>,
        root_key: &CallLinkRootKey,
    ) -> jni::errors::Result<JObject<'a>> {
        let call_link_root_key_class =
            match JCallLinkRootKey::lookup_class(env, &LoaderContext::default()) {
                Ok(v) => v,
                Err(error) => {
                    error!("call_link_root_key_class: {:?}", error);
                    return Ok(JObject::null());
                }
            };

        let key_bytes = JObject::from(env.byte_array_from_slice(root_key.as_slice())?);
        jni_new_object!(env, &*call_link_root_key_class, (
            key_bytes => [byte],
            false => boolean,
        ))
    }

    fn make_call_link_state_object<'a>(
        &self,
        env: &mut Env<'a>,
        call_link_state: &Option<CallLinkState>,
    ) -> jni::errors::Result<JObject<'a>> {
        match call_link_state {
            None => Ok(JObject::null()),
            Some(state) => {
                let call_link_state_class =
                    match JCallLinkState::lookup_class(env, &LoaderContext::default()) {
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

                let root_key = self.make_call_link_root_key_object(env, &state.root_key)?;

                jni_new_object!(env, &*call_link_state_class, (
                    name_object => java.lang.String,
                    raw_restrictions => int,
                    state.revoked => boolean,
                    expiration_in_epoch_seconds as jlong => long,
                    root_key => org.signal.ringrtc.CallLinkRootKey,
                ))
            }
        }
    }

    fn make_peek_info_object<'a>(
        &self,
        env: &mut Env<'a>,
        peek_info: &PeekInfo,
        joined_members: &mut dyn ExactSizeIterator<Item = &UserId>,
    ) -> Result<JObject<'a>> {
        let list = JArrayList::with_capacity(env, joined_members.len() as jint)?;
        let joined_member_list = jni::objects::JList::cast_local(env, list)?;
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
        let pending_user_list = JArrayList::with_capacity(env, pending_users.len() as jint)?;
        let pending_user_list = jni::objects::JList::cast_local(env, pending_user_list)?;
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

        let peek_info_class = JPeekInfo::lookup_class(env, &LoaderContext::default())?;
        let result = jni_call_static_method!(
            env,
            &*peek_info_class,
            jni_str!("fromNative"),
            (
                joined_member_list => java.util.List,
                jni_creator => [byte],
                jni_era_id => java.lang.String,
                jni_max_devices => java.lang.Long,
                jni_device_count_including_pending => long,
                jni_device_count_excluding_pending => long,
                pending_user_list => java.util.List,
                jni_call_link_state => org.signal.ringrtc.CallLinkState,
            ) -> org.signal.ringrtc.PeekInfo
        )?;
        Ok(result.into_object()?)
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

        let _ = self.with_java_env(|env| {
            let result_object = match peek_result {
                Ok(peek_info) => {
                    let joined_members = peek_info.unique_users();

                    // Set a frame capacity of min (5) + objects (5) + elements (N * 1 object per element).
                    let capacity = 10 + joined_members.len();
                    let result = env
                        .with_local_frame_returning_local::<_, JObject<'static>, anyhow::Error>(
                            capacity,
                            |env| {
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

                                let http_result_class =
                                    HttpResult::lookup_class(env, &LoaderContext::default())?;
                                Ok(jni_new_object!(env, &*http_result_class, (
                                    jni_peek_info => java.lang.Object,
                                ))?)
                            },
                        );
                    match result {
                        Ok(v) if !v.is_null() => v,
                        Ok(_) => return Ok(()),
                        Err(error) => {
                            error!("new HttpResult(PeekInfo): {:?}", error);
                            return Ok(());
                        }
                    }
                }
                Err(status) => {
                    let http_result_class =
                        HttpResult::lookup_class(env, &LoaderContext::default())?;
                    jni_new_object!(env, &*http_result_class, (
                        status.code as jshort => short,
                    ))?
                }
            };

            let result = jni_call_method!(
                env,
                self.jni_call_manager.as_obj(),
                jni_str!("handlePeekResponse"),
                (
                    request_id as jlong => long,
                    result_object => org.signal.ringrtc.CallManager::HttpResult,
                ) -> void
            );
            if result.is_err() {
                error!("jni_call_method: {:?}", result.err());
            }
            Ok(())
        });
    }
}
