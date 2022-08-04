//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Android CallManager Interface.

use std::convert::TryFrom;
use std::panic;
use std::time::Duration;

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jbyteArray, jint, jlong, jobject};
use jni::JNIEnv;
use log::Level;

use crate::android::android_platform::{AndroidCallContext, AndroidPlatform};
use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::android::logging::init_logging;
use crate::android::webrtc_peer_connection_factory::*;

use crate::common::{CallId, CallMediaType, DeviceId, Result};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call_manager::CallManager;
use crate::core::connection::Connection;
use crate::core::util::{ptr_as_box, ptr_as_mut};
use crate::core::{group_call, signaling};
use crate::error::RingRtcError;
use crate::lite::{http, sfu::GroupMember};
use crate::webrtc;
use crate::webrtc::media;
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::peer_connection_factory::{self as pcf, PeerConnectionFactory};
use crate::webrtc::peer_connection_observer::PeerConnectionObserver;

/// Public type for Android CallManager
pub type AndroidCallManager = CallManager<AndroidPlatform>;

/// CMI request for build time information
pub fn get_build_info(env: &JNIEnv) -> Result<jobject> {
    #[cfg(all(debug_assertions, not(test)))]
    let debug = true;
    #[cfg(any(not(debug_assertions), test))]
    let debug = false;

    let result = jni_new_object(
        env,
        jni_class_name!(org.signal.ringrtc.BuildInfo),
        jni_args!((debug => boolean) -> void),
    )?
    .into_inner();

    Ok(result)
}

/// Library initialization routine.
///
/// Sets up the logging infrastructure.
pub fn initialize(env: &JNIEnv) -> Result<()> {
    init_logging(env, Level::Debug)?;

    // Set a custom panic handler that uses the logger instead of
    // stderr, which is of no use on Android.
    panic::set_hook(Box::new(|panic_info| {
        error!("Critical error: {}", panic_info);
    }));

    Ok(())
}

/// Creates a new AndroidCallManager object.
pub fn create_call_manager(env: &JNIEnv, jni_call_manager: JObject) -> Result<jlong> {
    info!("create_call_manager():");
    let platform = AndroidPlatform::new(env, env.new_global_ref(jni_call_manager)?)?;

    let http_client = http::DelegatingClient::new(platform.try_clone()?);

    let call_manager = AndroidCallManager::new(platform, http_client)?;

    let call_manager_box = Box::new(call_manager);
    Ok(Box::into_raw(call_manager_box) as jlong)
}

/// Create a org.webrtc.PeerConnection object
pub fn create_peer_connection(
    env: JNIEnv,
    peer_connection_factory: jlong,
    native_connection: webrtc::ptr::Borrowed<Connection<AndroidPlatform>>,
    jni_rtc_config: JObject,
    jni_media_constraints: JObject,
) -> Result<jlong> {
    let connection = unsafe { native_connection.as_mut() }.ok_or_else(|| {
        RingRtcError::NullPointer(
            "create_peer_connection".to_owned(),
            "native_connection".to_owned(),
        )
    })?;

    // native_connection is an un-boxed Connection<AndroidPlatform> on the heap.
    // pass ownership of it to the PeerConnectionObserver.
    let pc_observer = PeerConnectionObserver::new(
        native_connection,
        false, /* enable_frame_encryption */
        false, /* enable_video_frame_event */
        false, /* enable_video_frame_content */
    )?;

    // construct JNI OwnedPeerConnection object
    let jni_owned_pc = unsafe {
        Java_org_webrtc_PeerConnectionFactory_nativeCreatePeerConnection(
            env,
            JClass::from(JObject::null()),
            peer_connection_factory,
            jni_rtc_config,
            jni_media_constraints,
            pc_observer.into_rffi().into_owned().as_ptr() as jlong,
            JObject::null(),
        )
    };
    info!("jni_owned_pc: {}", jni_owned_pc);

    if jni_owned_pc == 0 {
        return Err(AndroidError::CreateJniPeerConnection.into());
    }

    let rffi_pc = unsafe {
        webrtc::Arc::from_borrowed(Rust_borrowPeerConnectionFromJniOwnedPeerConnection(
            jni_owned_pc,
        ))
    };
    if rffi_pc.is_null() {
        return Err(AndroidError::ExtractNativePeerConnection.into());
    }

    // Note: We have to make sure the PeerConnectionFactory outlives this PC because we're not getting
    // any help from the type system when passing in a None for the PeerConnectionFactory here.
    // We can't "webrtc::Arc::from_borrowed(peer_connection_factory)" here because
    // peer_connection_factory is actually an OwnedFactoryAndThreads, not a PeerConnectionFactory.
    // We'd need to unwrap it with something like Rust_borrowPeerConnectionFromJniOwnedPeerConnection.
    let peer_connection = PeerConnection::new(rffi_pc, None, None);

    connection.set_peer_connection(peer_connection)?;

    info!("connection: {:?}", connection);

    Ok(jni_owned_pc)
}

/// Application notification updating the current user's UUID
pub fn set_self_uuid(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    uuid: jbyteArray,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("set_self_uuid():");
    call_manager.set_self_uuid(env.convert_byte_array(uuid)?)
}

/// Application notification to start a new call
pub fn call(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    jni_remote: JObject,
    call_media_type: CallMediaType,
    local_device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("call():");

    let app_remote_peer = env.new_global_ref(jni_remote)?;

    call_manager.call(app_remote_peer, call_media_type, local_device_id)
}

/// Application notification to proceed with a new call
pub fn proceed(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    jni_call_context: JObject,
    bandwidth_mode: BandwidthMode,
    audio_levels_interval: Option<Duration>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("proceed(): {}", call_id);

    let platform = call_manager.platform()?.try_clone()?;
    let android_call_context =
        AndroidCallContext::new(platform, env.new_global_ref(jni_call_context)?);

    call_manager.proceed(
        call_id,
        android_call_context,
        bandwidth_mode,
        audio_levels_interval,
    )
}

/// Application notification that signal message was sent successfully
pub fn message_sent(call_manager: *mut AndroidCallManager, call_id: jlong) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("message_sent(): call_id: {}", call_id);
    call_manager.message_sent(call_id)
}

/// Application notification that signal message was not sent successfully
pub fn message_send_failure(call_manager: *mut AndroidCallManager, call_id: jlong) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("message_send_failure(): call_id: {}", call_id);
    call_manager.message_send_failure(call_id)
}

/// Application notification of local hangup
pub fn hangup(call_manager: *mut AndroidCallManager) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("hangup():");
    call_manager.hangup()
}

/// Application notification cancelling a group call ring
pub fn cancel_group_ring(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    group_id: jbyteArray,
    ring_id: jlong,
    reason: jint,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("cancel_group_ring():");
    let reason = if reason == -1 {
        None
    } else {
        Some(group_call::RingCancelReason::try_from(reason)?)
    };
    call_manager.cancel_group_ring(
        env.convert_byte_array(group_id)?,
        group_call::RingId::from(ring_id),
        reason,
    )
}

/// Application notification of received answer message
#[allow(clippy::too_many_arguments)]
pub fn received_answer(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    sender_device_id: DeviceId,
    opaque: jbyteArray,
    sender_identity_key: jbyteArray,
    receiver_identity_key: jbyteArray,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!(
        "received_answer(): call_id: {} sender_device_id: {}",
        call_id, sender_device_id
    );

    let opaque = if opaque.is_null() {
        return Err(RingRtcError::OptionValueNotSet(
            "received_answer()".to_owned(),
            "opaque".to_owned(),
        )
        .into());
    } else {
        env.convert_byte_array(opaque)?
    };

    let sender_identity_key = env.convert_byte_array(sender_identity_key)?;
    let receiver_identity_key = env.convert_byte_array(receiver_identity_key)?;
    call_manager.received_answer(
        call_id,
        signaling::ReceivedAnswer {
            answer: signaling::Answer::new(opaque)?,
            sender_device_id,
            sender_identity_key,
            receiver_identity_key,
        },
    )
}

/// Application notification of received offer message
#[allow(clippy::too_many_arguments)]
pub fn received_offer(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    remote_peer: JObject,
    sender_device_id: DeviceId,
    opaque: jbyteArray,
    age_sec: u64,
    call_media_type: CallMediaType,
    receiver_device_id: DeviceId,
    receiver_device_is_primary: bool,
    sender_identity_key: jbyteArray,
    receiver_identity_key: jbyteArray,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    let remote_peer = env.new_global_ref(remote_peer)?;

    info!(
        "received_offer(): call_id: {} sender_device_id: {}",
        call_id, sender_device_id
    );

    let opaque = if opaque.is_null() {
        return Err(RingRtcError::OptionValueNotSet(
            "received_offer()".to_owned(),
            "opaque".to_owned(),
        )
        .into());
    } else {
        env.convert_byte_array(opaque)?
    };

    let sender_identity_key = env.convert_byte_array(sender_identity_key)?;
    let receiver_identity_key = env.convert_byte_array(receiver_identity_key)?;
    call_manager.received_offer(
        remote_peer,
        call_id,
        signaling::ReceivedOffer {
            offer: signaling::Offer::new(call_media_type, opaque)?,
            age: Duration::from_secs(age_sec),
            sender_device_id,
            receiver_device_id,
            receiver_device_is_primary,
            sender_identity_key,
            receiver_identity_key,
        },
    )
}

/// Application notification to add ICE candidates to a Connection
pub fn received_ice(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    sender_device_id: DeviceId,
    candidates: JObject,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    // Convert Java list of byte[] into Rust Vector of IceCandidate
    let jni_ice_candidates = env.get_list(candidates)?;
    let mut ice_candidates = Vec::new();
    for jni_ice_candidate in jni_ice_candidates.iter()? {
        let opaque = env.convert_byte_array(*jni_ice_candidate)?;
        ice_candidates.push(signaling::IceCandidate::new(opaque));
    }

    info!(
        "received_ice(): call_id: {} sender_device_id: {} candidates: {}",
        call_id,
        sender_device_id,
        ice_candidates.len()
    );

    call_manager.received_ice(
        call_id,
        signaling::ReceivedIce {
            ice: signaling::Ice {
                candidates: ice_candidates,
            },
            sender_device_id,
        },
    )
}

/// Application notification of received Hangup message
pub fn received_hangup(
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    sender_device_id: DeviceId,
    hangup_type: signaling::HangupType,
    hangup_device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!(
        "received_hangup(): call_id: {} sender_device_id: {}",
        call_id, sender_device_id
    );

    call_manager.received_hangup(
        call_id,
        signaling::ReceivedHangup {
            sender_device_id,
            hangup: signaling::Hangup::from_type_and_device_id(hangup_type, hangup_device_id),
        },
    )
}

/// Application notification of received Busy message
pub fn received_busy(
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    sender_device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!(
        "received_busy(): call_id: {} sender_device_id: {}",
        call_id, sender_device_id
    );

    call_manager.received_busy(call_id, signaling::ReceivedBusy { sender_device_id })
}

/// Application notification of received call message.
pub fn received_call_message(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    sender_uuid: jbyteArray,
    sender_device_id: DeviceId,
    local_device_id: DeviceId,
    message: jbyteArray,
    message_age_sec: u64,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("received_call_message():");

    let sender_uuid = if sender_uuid.is_null() {
        error!("Invalid sender_uuid");
        return Ok(());
    } else {
        env.convert_byte_array(sender_uuid)?
    };

    let message = if message.is_null() {
        error!("Invalid message");
        return Ok(());
    } else {
        env.convert_byte_array(message)?
    };

    call_manager.received_call_message(
        sender_uuid,
        sender_device_id,
        local_device_id,
        message,
        Duration::from_secs(message_age_sec),
    )
}

/// Application notification of received HTTP response.
pub fn received_http_response(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    request_id: jlong,
    status_code: jint,
    body: jbyteArray,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("received_http_response(): request_id: {}", request_id);

    let body = if body.is_null() {
        error!("Invalid body");
        return Ok(());
    } else {
        env.convert_byte_array(body)?
    };

    let response = http::Response {
        status: (status_code as u16).into(),
        body,
    };

    call_manager.received_http_response(request_id as u32, Some(response));
    Ok(())
}

/// Application notification of failed HTTP request.
pub fn http_request_failed(call_manager: *mut AndroidCallManager, request_id: jlong) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("http_request_failed(): request_id: {}", request_id);

    call_manager.received_http_response(request_id as u32, None);
    Ok(())
}

/// Application notification to accept the incoming call
pub fn accept_call(call_manager: *mut AndroidCallManager, call_id: jlong) -> Result<()> {
    let call_id = CallId::from(call_id);

    info!("accept_call(): {}", call_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.accept_call(call_id)
}

/// CMI request for the active Connection object
pub fn get_active_connection(call_manager: *mut AndroidCallManager) -> Result<jobject> {
    info!("get_active_connection():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection = call_manager.active_connection()?;
    let android_connection = connection.app_connection()?;

    Ok(android_connection.to_jni().as_obj().into_inner())
}

/// CMI request for the active CallContext object
pub fn get_active_call_context(call_manager: *mut AndroidCallManager) -> Result<jobject> {
    info!("get_active_call_context():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call = call_manager.active_call()?;
    let android_call_context = call.call_context()?;

    Ok(android_call_context.to_jni().as_obj().into_inner())
}

/// CMI request to set the video status
pub fn set_video_enable(call_manager: *mut AndroidCallManager, enable: bool) -> Result<()> {
    info!("set_video_enable():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    if let Ok(mut active_connection) = call_manager.active_connection() {
        active_connection.update_sender_status(signaling::SenderStatus {
            video_enabled: Some(enable),
            ..Default::default()
        })
    } else {
        Ok(())
    }
}

/// Request to update the bandwidth mode on the direct connection
pub fn update_bandwidth_mode(
    call_manager: *mut AndroidCallManager,
    bandwidth_mode: BandwidthMode,
) -> Result<()> {
    info!("update_bandwidth_mode():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let mut active_connection = call_manager.active_connection()?;
    active_connection.inject_update_bandwidth_mode(bandwidth_mode)
}

/// CMI request to drop the active call
pub fn drop_call(call_manager: *mut AndroidCallManager, call_id: jlong) -> Result<()> {
    let call_id = CallId::from(call_id);

    info!("drop_call(): {}", call_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.drop_call(call_id)
}

/// CMI request to reset the Call Manager
pub fn reset(call_manager: *mut AndroidCallManager) -> Result<()> {
    info!("reset():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.reset()
}

/// CMI request to close down the Call Manager.
///
/// This is a blocking call.
pub fn close(call_manager: *mut AndroidCallManager) -> Result<()> {
    info!("close():");

    // Convert the raw pointer back into a Box and let it go out of
    // scope when this function exits.
    let mut call_manager = unsafe { ptr_as_box(call_manager)? };
    call_manager.close()
}

// Group Calls

/// Convert a byte[] with 32-byte chunks in to a GroupMember struct vector.
fn deserialize_to_group_member_info(
    mut serialized_group_members: Vec<u8>,
) -> Result<Vec<GroupMember>> {
    if serialized_group_members.len() % 81 != 0 {
        error!(
            "Serialized buffer is not a multiple of 81: {}",
            serialized_group_members.len()
        );
        return Err(AndroidError::JniInvalidSerializedBuffer.into());
    }

    let mut group_members = Vec::new();
    for chunk in serialized_group_members.chunks_exact_mut(81) {
        group_members.push(GroupMember {
            user_id: chunk[..16].into(),
            member_id: chunk[16..].into(),
        })
    }

    Ok(group_members)
}

pub fn peek_group_call(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    request_id: jlong,
    sfu_url: JString,
    membership_proof: jbyteArray,
    jni_serialized_group_members: jbyteArray,
) -> Result<()> {
    info!("peek_group_call():");

    let request_id = request_id as u32;

    let sfu_url = env.get_string(sfu_url)?.into();

    let membership_proof = env.convert_byte_array(membership_proof)?;

    let group_members =
        deserialize_to_group_member_info(env.convert_byte_array(jni_serialized_group_members)?)?;

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.peek_group_call(request_id, sfu_url, membership_proof, group_members);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn create_group_call_client(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    group_id: jbyteArray,
    sfu_url: JString,
    hkdf_extra_info: jbyteArray,
    audio_levels_interval_millis: jint,
    native_pcf_borrowed_rc: jlong,
    native_audio_track_borrowed_rc: jlong,
    native_video_track_borrowed_rc: jlong,
) -> Result<group_call::ClientId> {
    info!("create_group_call_client():");

    let group_id = env.convert_byte_array(group_id)?;
    let sfu_url = env.get_string(sfu_url)?.into();
    let hkdf_extra_info = env.convert_byte_array(hkdf_extra_info)?;

    let peer_connection_factory = unsafe {
        PeerConnectionFactory::from_native_factory(webrtc::Arc::from_borrowed(
            webrtc::ptr::BorrowedRc::from_ptr(
                native_pcf_borrowed_rc as *const pcf::RffiPeerConnectionFactoryInterface,
            ),
        ))
    };

    // This is safe because the track given to us should still be alive.
    let outgoing_audio_track = media::AudioTrack::new(
        unsafe {
            webrtc::Arc::from_borrowed(webrtc::ptr::BorrowedRc::from_ptr(
                native_audio_track_borrowed_rc as *const media::RffiAudioTrack,
            ))
        },
        Some(peer_connection_factory.rffi().clone()),
    );

    // This is safe because the track given to us should still be alive.
    let outgoing_video_track = media::VideoTrack::new(
        unsafe {
            webrtc::Arc::from_borrowed(webrtc::ptr::BorrowedRc::from_ptr(
                native_video_track_borrowed_rc as *const media::RffiVideoTrack,
            ))
        },
        Some(peer_connection_factory.rffi().clone()),
    );

    let audio_levels_interval = if audio_levels_interval_millis <= 0 {
        None
    } else {
        Some(Duration::from_millis(audio_levels_interval_millis as u64))
    };

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.create_group_call_client(
        group_id,
        sfu_url,
        hkdf_extra_info,
        audio_levels_interval,
        Some(peer_connection_factory),
        outgoing_audio_track,
        outgoing_video_track,
        None,
    )
}

pub fn delete_group_call_client(
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
) -> Result<()> {
    info!("delete_group_call_client(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.delete_group_call_client(client_id);
    Ok(())
}

pub fn connect(
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
) -> Result<()> {
    info!("connect(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.connect(client_id);
    Ok(())
}

pub fn join(call_manager: *mut AndroidCallManager, client_id: group_call::ClientId) -> Result<()> {
    info!("join(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.join(client_id);
    Ok(())
}

pub fn leave(call_manager: *mut AndroidCallManager, client_id: group_call::ClientId) -> Result<()> {
    info!("leave(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.leave(client_id);
    Ok(())
}

pub fn disconnect(
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
) -> Result<()> {
    info!("disconnect(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.disconnect(client_id);
    Ok(())
}

pub fn set_outgoing_audio_muted(
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    muted: bool,
) -> Result<()> {
    info!("set_outgoing_audio_muted(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_outgoing_audio_muted(client_id, muted);
    Ok(())
}

pub fn set_outgoing_video_muted(
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    muted: bool,
) -> Result<()> {
    info!("set_outgoing_video_muted(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_outgoing_video_muted(client_id, muted);
    Ok(())
}

pub fn group_ring(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    recipient: jbyteArray,
) -> Result<()> {
    info!("group_ring(): id: {}", client_id);

    let recipient = if recipient.is_null() {
        None
    } else {
        Some(env.convert_byte_array(recipient)?)
    };

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.group_ring(client_id, recipient);
    Ok(())
}

pub fn resend_media_keys(
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
) -> Result<()> {
    info!("resend_media_keys(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.resend_media_keys(client_id);
    Ok(())
}

pub fn set_bandwidth_mode(
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    bandwidth_mode: BandwidthMode,
) -> Result<()> {
    info!("set_bandwidth_mode(): id: {}", client_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_bandwidth_mode(client_id, bandwidth_mode);
    Ok(())
}

pub fn request_video(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    jni_rendered_resolutions: JObject,
    active_speaker_height: jint,
) -> Result<()> {
    info!("request_video(): id: {}", client_id);

    // Convert Java list of VideoRequest into Rust Vec<group_call::VideoRequest>.
    let jni_rendered_resolution_list = env.get_list(jni_rendered_resolutions)?;
    let mut rendered_resolutions: Vec<group_call::VideoRequest> = Vec::new();
    for jni_rendered_resolution in jni_rendered_resolution_list.iter()? {
        const LONG_TYPE: &str = jni_signature!(long);
        const INT_TYPE: &str = jni_signature!(int);
        const NULLABLE_INT_TYPE: &str = jni_signature!(java.lang.Integer);

        const DEMUX_ID_FIELD: &str = "demuxId";
        let demux_id =
            jni_get_field(env, jni_rendered_resolution, DEMUX_ID_FIELD, LONG_TYPE)?.j()?;
        let demux_id = demux_id as u32;

        const WIDTH_FIELD: &str = "width";
        let width = jni_get_field(env, jni_rendered_resolution, WIDTH_FIELD, INT_TYPE)?.i()?;
        let width = width as u16;

        const HEIGHT_FIELD: &str = "height";
        let height = jni_get_field(env, jni_rendered_resolution, HEIGHT_FIELD, INT_TYPE)?.i()?;
        let height = height as u16;

        const FRAMERATE_FIELD: &str = "framerate";
        let framerate = jni_get_field(
            env,
            jni_rendered_resolution,
            FRAMERATE_FIELD,
            NULLABLE_INT_TYPE,
        )?
        .l()?;
        let framerate = if framerate.is_null() {
            None
        } else {
            // We have java.lang.Integer, so we need to invoke the function to get the actual
            // int value that is attached to it.
            match env.call_method(framerate, "intValue", jni_signature!(() -> int), &[]) {
                Ok(jvalue) => {
                    match jvalue.i() {
                        Ok(int) => Some(int.to_owned() as u16),
                        Err(_) => {
                            // The framerate can be ignored.
                            None
                        }
                    }
                }
                Err(_) => {
                    // The framerate can be ignored.
                    None
                }
            }
        };

        let rendered_resolution = group_call::VideoRequest {
            demux_id,
            width,
            height,
            framerate,
        };

        rendered_resolutions.push(rendered_resolution);
    }

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.request_video(
        client_id,
        rendered_resolutions,
        active_speaker_height as u16,
    );
    Ok(())
}

pub fn set_group_members(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    jni_serialized_group_members: jbyteArray,
) -> Result<()> {
    info!("set_group_members(): id: {}", client_id);

    let group_members =
        deserialize_to_group_member_info(env.convert_byte_array(jni_serialized_group_members)?)?;

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_group_members(client_id, group_members);
    Ok(())
}

pub fn set_membership_proof(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    proof: jbyteArray,
) -> Result<()> {
    info!("set_group_membership_proof(): id: {}", client_id);

    let proof = env.convert_byte_array(proof)?;

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_membership_proof(client_id, proof);
    Ok(())
}
