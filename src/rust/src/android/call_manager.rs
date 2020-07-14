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
use jni::sys::{jboolean, jbyteArray, jlong, jobject};
use jni::JNIEnv;
use log::Level;

use crate::android::android_platform::{AndroidCallContext, AndroidPlatform};
use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::android::logging::init_logging;
use crate::android::webrtc_peer_connection_factory::*;
use crate::common::{BandwidthMode, CallId, CallMediaType, DeviceId, FeatureLevel, Result};
use crate::core::connection::Connection;
use crate::core::signaling;
use crate::core::util::{ptr_as_box, ptr_as_mut};

use crate::core::call_manager::CallManager;

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
    let pc_observer = PeerConnectionObserver::new(native_connection)?;
    let connection = unsafe { ptr_as_mut(native_connection)? };

    // construct JNI OwnedPeerConnection object
    let jni_owned_pc = unsafe {
        Java_org_webrtc_PeerConnectionFactory_nativeCreatePeerConnection(
            env.clone(),
            JClass::from(JObject::null()),
            peer_connection_factory,
            jni_rtc_config,
            jni_media_constraints,
            pc_observer.rffi_interface() as jlong,
            JObject::null(),
            enable_dtls as jboolean,
            enable_rtp_data_channel as jboolean,
        )
    };
    info!("jni_owned_pc: {}", jni_owned_pc);

    if jni_owned_pc == 0 {
        return Err(AndroidError::CreateJniPeerConnection.into());
    }

    // Retrieve the underlying PeerConnectionInterface object from the
    // JNI OwnedPeerConnection object.
    let rffi_pc_interface = unsafe { Rust_getPeerConnectionInterface(jni_owned_pc) };
    if rffi_pc_interface.is_null() {
        return Err(AndroidError::ExtractNativePeerConnectionInterface.into());
    }
    let pc_interface = PeerConnection::unowned(rffi_pc_interface);

    connection.set_pc_interface(pc_interface)?;

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
pub fn received_answer(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    sender_device_id: DeviceId,
    opaque: jbyteArray,
    sdp: JString,
    sender_device_feature_level: FeatureLevel,
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
    call_manager.received_answer(
        call_id,
        signaling::ReceivedAnswer {
            answer: signaling::Answer::from_opaque_or_sdp(opaque, sdp),
            sender_device_id,
            sender_device_feature_level,
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

    call_manager.received_offer(
        remote_peer,
        call_id,
        signaling::ReceivedOffer {
            offer: signaling::Offer::from_opaque_or_sdp(call_media_type, opaque, sdp),
            age: Duration::from_secs(age_sec),
            sender_device_id,
            sender_device_feature_level,
            receiver_device_id,
            receiver_device_is_primary,
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
pub fn set_bandwidth_mode(
    call_manager: *mut AndroidCallManager,
    mode: BandwidthMode,
) -> Result<()> {
    info!("set_low_bandwidth_mode():");

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
