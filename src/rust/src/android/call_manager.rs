//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android CallManager Interface.

use std::panic;

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jlong, jobject};
use jni::JNIEnv;
use log::Level;

use crate::android::android_platform::{AndroidCallContext, AndroidPlatform};
use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::android::logging::init_logging;
use crate::android::webrtc_peer_connection_factory::*;
use crate::common::{
    AnswerParameters,
    CallDirection,
    CallId,
    CallMediaType,
    ConnectionId,
    DeviceId,
    FeatureLevel,
    HangupParameters,
    HangupType,
    OfferParameters,
    Result,
    DATA_CHANNEL_NAME,
};
use crate::core::connection::Connection;
use crate::core::util::{ptr_as_box, ptr_as_mut};

use crate::core::call_manager::CallManager;

use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::ice_candidate::IceCandidate;
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
    let pc_interface = PeerConnection::new(rffi_pc_interface);

    if let CallDirection::OutGoing = connection.direction() {
        // Create data channel observer and data channel
        let dc_observer = DataChannelObserver::new(connection.clone())?;
        let data_channel = pc_interface.create_data_channel(DATA_CHANNEL_NAME.to_string())?;
        unsafe { data_channel.register_observer(dc_observer.rffi_interface())? };
        connection.set_data_channel(data_channel)?;
        connection.set_data_channel_observer(dc_observer)?;
    }

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
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("call():");

    let app_remote_peer = env.new_global_ref(jni_remote)?;

    call_manager.call(app_remote_peer, call_media_type)
}

/// Application notification to proceed with a new call
pub fn proceed(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    jni_call_context: JObject,
    local_device_id: DeviceId,
    jni_remote_devices: JObject,
    enable_forking: bool,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("proceed(): {}", call_id);

    // Convert Java List<Integer> into a Rust Vec<DeviceId>.
    let mut remote_devices = Vec::<DeviceId>::new();
    let device_list = env.get_list(jni_remote_devices)?;
    for device in device_list.iter()? {
        let device_id = jni_call_method(env, device, "intValue", "()I", &[])?.i()? as DeviceId;
        remote_devices.push(device_id);
    }

    info!("proceed(): remote_devices size: {}", remote_devices.len());
    for device in &remote_devices {
        info!("proceed(): device id: {}", device);
    }

    let platform = call_manager.platform()?.try_clone()?;
    let android_call_context =
        AndroidCallContext::new(platform, env.new_global_ref(jni_call_context)?);

    call_manager.proceed(
        call_id,
        android_call_context,
        local_device_id,
        remote_devices,
        enable_forking,
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

/// Application notification of received SDP answer message
pub fn received_answer(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    remote_device: DeviceId,
    jni_answer: JString,
    remote_feature_level: FeatureLevel,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_answer(): id: {}", connection_id);
    call_manager.received_answer(
        connection_id,
        AnswerParameters::new(env.get_string(jni_answer)?.into(), remote_feature_level),
    )
}

/// Application notification of received SDP offer message
pub fn received_offer(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    jni_remote: JObject,
    remote_device: DeviceId,
    jni_offer: JString,
    timestamp: u64,
    call_media_type: CallMediaType,
    remote_feature_level: FeatureLevel,
    is_local_device_primary: bool,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_offer(): id: {}", connection_id);

    let app_remote_peer = env.new_global_ref(jni_remote)?;

    call_manager.received_offer(
        app_remote_peer,
        connection_id,
        OfferParameters::new(
            env.get_string(jni_offer)?.into(),
            timestamp,
            call_media_type,
            remote_feature_level,
            is_local_device_primary,
        ),
    )
}

/// Application notification to add ICE candidates to a Connection
pub fn received_ice_candidates(
    env: &JNIEnv,
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    remote_device: DeviceId,
    jni_ice_candidates: JObject,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    // Convert Java list of org.webrtc.IceCandidate into Rust Vector of IceCandidate
    let candidate_list = env.get_list(jni_ice_candidates)?;
    let mut ice_candidates = Vec::new();
    for jni_candidate in candidate_list.iter()? {
        const SDP_MID_FIELD: &str = "sdpMid";
        const STRING_TYPE: &str = "Ljava/lang/String;";
        let sdp_mid = jni_get_field(&env, jni_candidate, SDP_MID_FIELD, STRING_TYPE)?.l()?;
        let sdp_mid = env.get_string(JString::from(sdp_mid))?.into();

        const SDP_M_LINE_INDEX_FIELD: &str = "sdpMLineIndex";
        const SDP_M_LINE_INDEX_TYPE: &str = "I";
        let sdp_m_line = jni_get_field(
            &env,
            jni_candidate,
            SDP_M_LINE_INDEX_FIELD,
            SDP_M_LINE_INDEX_TYPE,
        )?
        .i()? as i32;

        const SDP_FIELD: &str = "sdp";
        let sdp = jni_get_field(&env, jni_candidate, SDP_FIELD, STRING_TYPE)?.l()?;
        let sdp = env.get_string(JString::from(sdp))?.into();

        let ice_candidate = IceCandidate::new(sdp_mid, sdp_m_line, sdp);
        ice_candidates.push(ice_candidate);
    }

    info!(
        "received_ice_candidates(): id: {} len: {}",
        connection_id,
        ice_candidates.len()
    );

    call_manager.received_ice_candidates(connection_id, &ice_candidates)
}

/// Application notification of received Hangup message
pub fn received_hangup(
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    remote_device: DeviceId,
    hangup_type: HangupType,
    device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_hangup(): id: {}", connection_id);

    call_manager.received_hangup(
        connection_id,
        HangupParameters::new(hangup_type, Some(device_id)),
    )
}

/// Application notification of received Busy message
pub fn received_busy(
    call_manager: *mut AndroidCallManager,
    call_id: jlong,
    remote_device: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_busy(): id: {}", connection_id);

    call_manager.received_busy(connection_id)
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
    active_connection.inject_local_video_status(enable)
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
