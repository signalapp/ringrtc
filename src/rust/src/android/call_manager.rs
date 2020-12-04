//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android CallManager Interface.

use std::panic;
use std::time::Duration;

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jobject};
use jni::JNIEnv;
use log::Level;

use crate::android::android_platform::{AndroidCallContext, AndroidPlatform};
use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::android::logging::init_logging;
use crate::android::webrtc_peer_connection_factory::*;
use crate::common::{
    BandwidthMode,
    CallId,
    CallMediaType,
    DeviceId,
    FeatureLevel,
    HttpResponse,
    Result,
};
use crate::core::call_manager::CallManager;
use crate::core::connection::Connection;
use crate::core::util::{ptr_as_box, ptr_as_mut};
use crate::core::{group_call, signaling};

use crate::webrtc::media;
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::peer_connection_observer::PeerConnectionObserver;

/// Public type for Android CallManager
pub type AndroidCallManager = CallManager<AndroidPlatform>;

/// CMI request for build time information
pub fn get_build_info(env: &JNIEnv) -> Result<jobject> {
    #[cfg(all(debug_assertions, not(test)))]
    let debug = true;
    #[cfg(any(not(debug_assertions), test))]
    let debug = false;

    const BUILD_INFO_CLASS: &str = "org/signal/ringrtc/BuildInfo";
    const BUILD_INFO_SIG: &str = "(Z)V";
    let args = [debug.into()];

    let result = jni_new_object(&env, BUILD_INFO_CLASS, BUILD_INFO_SIG, &args)?.into_inner();

    Ok(result)
}

/// Library initialization routine.
///
/// Sets up the logging infrastructure.
pub fn initialize(env: &JNIEnv) -> Result<()> {
    init_logging(&env, Level::Debug)?;

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
    let platform = AndroidPlatform::new(&env, env.new_global_ref(jni_call_manager)?)?;

    let call_manager = AndroidCallManager::new(platform)?;

    let call_manager_box = Box::new(call_manager);
    Ok(Box::into_raw(call_manager_box) as jlong)
}

/// Create a org.webrtc.PeerConnection object
pub fn create_peer_connection(
    env: &JNIEnv,
    peer_connection_factory: jlong,
    native_connection: *mut Connection<AndroidPlatform>,
    jni_rtc_config: JObject,
    jni_media_constraints: JObject,
    enable_dtls: bool,
    enable_rtp_data_channel: bool,
) -> Result<jlong> {
    // native_connection is an un-boxed Connection<AndroidPlatform> on the heap.
    // pass ownership of it to the PeerConnectionObserver.
    let pc_observer =
        PeerConnectionObserver::new(native_connection, false /* enable_frame_encryption */)?;
    let connection = unsafe { ptr_as_mut(native_connection)? };

    // construct JNI OwnedPeerConnection object
    let jni_owned_pc = unsafe {
        Java_org_webrtc_PeerConnectionFactory_nativeCreatePeerConnection(
            env.clone(),
            JClass::from(JObject::null()),
            peer_connection_factory,
            jni_rtc_config,
            jni_media_constraints,
            pc_observer.rffi() as jlong,
            JObject::null(),
            enable_dtls as jboolean,
            enable_rtp_data_channel as jboolean,
        )
    };
    info!("jni_owned_pc: {}", jni_owned_pc);

    if jni_owned_pc == 0 {
        return Err(AndroidError::CreateJniPeerConnection.into());
    }

    // Retrieve the underlying PeerConnection object from the
    // JNI OwnedPeerConnection object.
    let rffi_pc = unsafe { Rust_getPeerConnectionFromJniOwnedPeerConnection(jni_owned_pc) };
    if rffi_pc.is_null() {
        return Err(AndroidError::ExtractNativePeerConnection.into());
    }

    let peer_connection = PeerConnection::unowned(rffi_pc, pc_observer.rffi());

    connection.set_peer_connection(peer_connection)?;

    info!("connection: {:?}", connection);

    Ok(jni_owned_pc)
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
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("proceed(): {}", call_id);

    let platform = call_manager.platform()?.try_clone()?;
    let android_call_context =
        AndroidCallContext::new(platform, env.new_global_ref(jni_call_context)?);

    call_manager.proceed(call_id, android_call_context)
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

/// Application notification of received answer message
#[allow(clippy::too_many_arguments)]
pub fn received_answer(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    sender_device_id: DeviceId,
    opaque: jbyteArray,
    sdp: JString,
    sender_device_feature_level: FeatureLevel,
    sender_identity_key: jbyteArray,
    receiver_identity_key: jbyteArray,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    let opaque = if opaque.is_null() {
        None
    } else {
        Some(env.convert_byte_array(opaque)?)
    };
    let sdp = if sdp.is_null() {
        None
    } else {
        Some(env.get_string(sdp)?.into())
    };

    info!(
        "received_answer(): call_id: {} sender_device_id: {}",
        call_id, sender_device_id
    );

    let sender_identity_key = env.convert_byte_array(sender_identity_key)?;
    let receiver_identity_key = env.convert_byte_array(receiver_identity_key)?;
    call_manager.received_answer(
        call_id,
        signaling::ReceivedAnswer {
            answer: signaling::Answer::from_opaque_or_sdp(opaque, sdp)?,
            sender_device_id,
            sender_device_feature_level,
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
    sdp: JString,
    age_sec: u64,
    call_media_type: CallMediaType,
    receiver_device_id: DeviceId,
    sender_device_feature_level: FeatureLevel,
    receiver_device_is_primary: bool,
    sender_identity_key: jbyteArray,
    receiver_identity_key: jbyteArray,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    let remote_peer = env.new_global_ref(remote_peer)?;
    let opaque = if opaque.is_null() {
        None
    } else {
        Some(env.convert_byte_array(opaque)?)
    };
    let sdp = if sdp.is_null() {
        None
    } else {
        Some(env.get_string(sdp)?.into())
    };

    info!(
        "received_offer(): call_id: {} sender_device_id: {}",
        call_id, sender_device_id
    );

    let sender_identity_key = env.convert_byte_array(sender_identity_key)?;
    let receiver_identity_key = env.convert_byte_array(receiver_identity_key)?;
    call_manager.received_offer(
        remote_peer,
        call_id,
        signaling::ReceivedOffer {
            offer: signaling::Offer::from_opaque_or_sdp(call_media_type, opaque, sdp)?,
            age: Duration::from_secs(age_sec),
            sender_device_id,
            sender_device_feature_level,
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
    jni_candidates: JObject,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    // Convert Java list of IceCandidate into Rust Vector of IceCandidate
    let jni_candidate_list = env.get_list(jni_candidates)?;
    let mut signaling_candidates = Vec::new();
    for jni_candidate in jni_candidate_list.iter()? {
        const OPAQUE_FIELD: &str = "opaque";
        const BYTES_TYPE: &str = "[B";
        let opaque = jni_get_field(&env, jni_candidate, OPAQUE_FIELD, BYTES_TYPE)?.l()?;
        let opaque = if opaque.is_null() {
            None
        } else {
            Some(env.convert_byte_array(opaque.into_inner())?)
        };

        const SDP_FIELD: &str = "sdp";
        const STRING_TYPE: &str = "Ljava/lang/String;";
        let sdp = jni_get_field(&env, jni_candidate, SDP_FIELD, STRING_TYPE)?.l()?;
        let sdp = if sdp.is_null() {
            None
        } else {
            Some(env.get_string(JString::from(sdp))?.into())
        };

        signaling_candidates.push(signaling::IceCandidate::from_opaque_or_sdp(opaque, sdp));
    }

    info!(
        "received_ice(): call_id: {} sender_device_id: {} candidates: {}",
        call_id,
        sender_device_id,
        signaling_candidates.len()
    );

    call_manager.received_ice(
        call_id,
        signaling::ReceivedIce {
            ice: signaling::Ice {
                candidates_added: signaling_candidates,
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
        message_age_sec,
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

    let response = HttpResponse {
        status_code: status_code as u16,
        body,
    };

    call_manager.received_http_response(request_id as u32, Some(response))
}

/// Application notification of failed HTTP request.
pub fn http_request_failed(call_manager: *mut AndroidCallManager, request_id: jlong) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("http_request_failed(): request_id: {}", request_id);

    call_manager.received_http_response(request_id as u32, None)
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
    let mut active_connection = call_manager.active_connection()?;
    active_connection.inject_send_sender_status_via_data_channel(enable)
}

/// CMI request to set the low bandwidth mode
pub fn set_direct_bandwidth_mode(
    call_manager: *mut AndroidCallManager,
    mode: BandwidthMode,
) -> Result<()> {
    info!("set_direct_bandwidth_mode():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let mut active_connection = call_manager.active_connection()?;
    active_connection.set_bandwidth_mode(mode)
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

pub fn peek_group_call(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    request_id: jlong,
    sfu_url: JString,
    membership_proof: jbyteArray,
    jni_members: JObject,
) -> Result<()> {
    info!("peek_group_call():");

    let request_id = request_id as u32;

    let sfu_url = env.get_string(sfu_url)?.into();

    let membership_proof = env.convert_byte_array(membership_proof)?;

    // Convert Java list of GroupMemberInfo into Rust Vec<group_call::GroupMemberInfo>.
    let jni_member_list = env.get_list(jni_members)?;
    let mut group_members = Vec::new();
    for jni_member in jni_member_list.iter()? {
        const BYTES_TYPE: &str = "[B";

        const USER_ID_FIELD: &str = "userIdByteArray";
        let user_id = jni_get_field(&env, jni_member, USER_ID_FIELD, BYTES_TYPE)?.l()?;
        let user_id = if user_id.is_null() {
            warn!("Invalid userId/ByteArray");
            continue;
        } else {
            Some(env.convert_byte_array(user_id.into_inner())?)
        };

        const USER_ID_CIPHER_TEXT_FIELD: &str = "userIdCipherText";
        let user_id_ciphertext =
            jni_get_field(&env, jni_member, USER_ID_CIPHER_TEXT_FIELD, BYTES_TYPE)?.l()?;
        let user_id_ciphertext = if user_id_ciphertext.is_null() {
            warn!("Invalid userId/CipherText");
            continue;
        } else {
            Some(env.convert_byte_array(user_id_ciphertext.into_inner())?)
        };

        group_members.push(group_call::GroupMemberInfo {
            user_id:            user_id.unwrap(),
            user_id_ciphertext: user_id_ciphertext.unwrap(),
        })
    }

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.peek_group_call(request_id, sfu_url, membership_proof, group_members);
    Ok(())
}

pub fn create_group_call_client(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    group_id: jbyteArray,
    sfu_url: JString,
    native_audio_track: jlong,
    native_video_track: jlong,
) -> Result<group_call::ClientId> {
    info!("create_group_call_client():");

    let group_id = env.convert_byte_array(group_id)?;
    let sfu_url = env.get_string(sfu_url)?.into();

    let outgoing_audio_track =
        media::AudioTrack::unowned(native_audio_track as *const media::RffiAudioTrack);

    let outgoing_video_track =
        media::VideoTrack::unowned(native_video_track as *const media::RffiVideoTrack);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.create_group_call_client(
        group_id,
        sfu_url,
        None,
        outgoing_audio_track,
        outgoing_video_track,
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
) -> Result<()> {
    info!("request_video(): id: {}", client_id);

    // Convert Java list of VideoRequest into Rust Vec<group_call::VideoRequest>.
    let jni_rendered_resolution_list = env.get_list(jni_rendered_resolutions)?;
    let mut rendered_resolutions: Vec<group_call::VideoRequest> = Vec::new();
    for jni_rendered_resolution in jni_rendered_resolution_list.iter()? {
        const LONG_TYPE: &str = "J";
        const INT_TYPE: &str = "I";
        const NULLABLE_INT_TYPE: &str = "Ljava/lang/Integer;";

        const DEMUX_ID_FIELD: &str = "demuxId";
        let demux_id =
            jni_get_field(&env, jni_rendered_resolution, DEMUX_ID_FIELD, LONG_TYPE)?.j()?;
        let demux_id = demux_id as u32;

        const WIDTH_FIELD: &str = "width";
        let width = jni_get_field(&env, jni_rendered_resolution, WIDTH_FIELD, INT_TYPE)?.i()?;
        let width = width as u16;

        const HEIGHT_FIELD: &str = "height";
        let height = jni_get_field(&env, jni_rendered_resolution, HEIGHT_FIELD, INT_TYPE)?.i()?;
        let height = height as u16;

        const FRAMERATE_FIELD: &str = "framerate";
        let framerate = jni_get_field(
            &env,
            jni_rendered_resolution,
            FRAMERATE_FIELD,
            NULLABLE_INT_TYPE,
        )?
        .l()?;
        let framerate = if framerate.is_null() {
            None
        } else {
            // We have java/lang/Integer, so we need to invoke the function to get the actual
            // int value that is attached to it.
            match env.call_method(framerate, "intValue", "()I", &[]) {
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
    call_manager.request_video(client_id, rendered_resolutions);
    Ok(())
}

pub fn set_group_members(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    client_id: group_call::ClientId,
    jni_members: JObject,
) -> Result<()> {
    info!("set_group_members(): id: {}", client_id);

    // Convert Java list of GroupMemberInfo into Rust Vec<group_call::GroupMemberInfo>.
    let jni_member_list = env.get_list(jni_members)?;
    let mut group_members = Vec::new();
    for jni_member in jni_member_list.iter()? {
        const BYTES_TYPE: &str = "[B";

        const USER_ID_FIELD: &str = "userIdByteArray";
        let user_id = jni_get_field(&env, jni_member, USER_ID_FIELD, BYTES_TYPE)?.l()?;
        let user_id = if user_id.is_null() {
            warn!("Invalid userId/ByteArray");
            continue;
        } else {
            Some(env.convert_byte_array(user_id.into_inner())?)
        };

        const USER_ID_CIPHER_TEXT_FIELD: &str = "userIdCipherText";
        let user_id_ciphertext =
            jni_get_field(&env, jni_member, USER_ID_CIPHER_TEXT_FIELD, BYTES_TYPE)?.l()?;
        let user_id_ciphertext = if user_id_ciphertext.is_null() {
            warn!("Invalid userId/CipherText");
            continue;
        } else {
            Some(env.convert_byte_array(user_id_ciphertext.into_inner())?)
        };

        group_members.push(group_call::GroupMemberInfo {
            user_id:            user_id.unwrap(),
            user_id_ciphertext: user_id_ciphertext.unwrap(),
        })
    }

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
