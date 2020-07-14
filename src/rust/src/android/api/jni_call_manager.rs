//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! JNI Call Manager interface functions.
//!
//! Native JNI interfaces, called by
//! org.signal.ringrtc.CallManager objects.

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jobject};
use jni::JNIEnv;

use crate::android::android_platform::AndroidPlatform;
use crate::android::call_manager;
use crate::android::call_manager::AndroidCallManager;
use crate::android::error;
use crate::common::{BandwidthMode, CallMediaType, DeviceId, FeatureLevel};
use crate::core::connection::Connection;
use crate::core::signaling;

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcGetBuildInfo(
    env: JNIEnv,
    _class: JClass,
) -> jobject {
    match call_manager::get_build_info(&env) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0 as jobject
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcInitialize(
    env: JNIEnv,
    _class: JClass,
) {
    if let Err(e) = call_manager::initialize(&env) {
        error::throw_error(&env, e);
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCreateCallManager(
    env: JNIEnv,
    _class: JClass,
    jni_call_manager: JObject,
) -> jlong {
    match call_manager::create_call_manager(&env, jni_call_manager) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCreatePeerConnection(
    env: JNIEnv,
    _object: JObject,
    peer_connection_factory: jlong,
    native_connection: jlong,
    jni_rtc_config: JObject,
    jni_media_constraints: JObject,
    enable_dtls: bool,
    enable_rtp_data_channel: bool,
) -> jlong {
    match call_manager::create_peer_connection(
        &env,
        peer_connection_factory,
        native_connection as *mut Connection<AndroidPlatform>,
        jni_rtc_config,
        jni_media_constraints,
        enable_dtls,
        enable_rtp_data_channel,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCall(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
    jni_remote: JObject,
    call_media_type: jint,
    local_device: jint,
) {
    match call_manager::call(
        &env,
        call_manager as *mut AndroidCallManager,
        jni_remote,
        CallMediaType::from_i32(call_media_type),
        local_device as DeviceId,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcProceed(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_call_context: JObject,
) {
    match call_manager::proceed(
        &env,
        call_manager as *mut AndroidCallManager,
        call_id,
        jni_call_context,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcMessageSent(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    match call_manager::message_sent(call_manager as *mut AndroidCallManager, call_id) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcMessageSendFailure(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    match call_manager::message_send_failure(call_manager as *mut AndroidCallManager, call_id) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcHangup(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
) {
    match call_manager::hangup(call_manager as *mut AndroidCallManager) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedAnswer(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    remote_device: jint,
    opaque: jbyteArray,
    sdp: JString,
    remote_supports_multi_ring: jboolean,
) {
    let remote_feature_level = if remote_supports_multi_ring == jni::sys::JNI_TRUE {
        FeatureLevel::MultiRing
    } else {
        FeatureLevel::Unspecified
    };

    match call_manager::received_answer(
        &env,
        call_manager as *mut AndroidCallManager,
        call_id,
        remote_device as DeviceId,
        opaque,
        sdp,
        remote_feature_level,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedOffer(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_remote: JObject,
    remote_device: jint,
    opaque: jbyteArray,
    sdp: JString,
    message_age_sec: jlong,
    call_media_type: jint,
    local_device: jint,
    remote_supports_multi_ring: jboolean,
    jni_is_local_device_primary: jboolean,
) {
    let remote_feature_level = if remote_supports_multi_ring == jni::sys::JNI_TRUE {
        FeatureLevel::MultiRing
    } else {
        FeatureLevel::Unspecified
    };

    match call_manager::received_offer(
        &env,
        call_manager as *mut AndroidCallManager,
        call_id,
        jni_remote,
        remote_device as DeviceId,
        opaque,
        sdp,
        message_age_sec as u64,
        CallMediaType::from_i32(call_media_type),
        local_device as DeviceId,
        remote_feature_level,
        jni_is_local_device_primary == jni::sys::JNI_TRUE,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedIceCandidates(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    remote_device: jint,
    jni_ice_candidates: JObject,
) {
    match call_manager::received_ice(
        &env,
        call_manager as *mut AndroidCallManager,
        call_id,
        remote_device as DeviceId,
        jni_ice_candidates,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedHangup(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    remote_device: jint,
    hangup_type: jint,
    device_id: jint,
) {
    match call_manager::received_hangup(
        call_manager as *mut AndroidCallManager,
        call_id,
        remote_device as DeviceId,
        signaling::HangupType::from_i32(hangup_type).unwrap_or(signaling::HangupType::Normal),
        device_id as DeviceId,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedBusy(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    remote_device: jint,
) {
    match call_manager::received_busy(
        call_manager as *mut AndroidCallManager,
        call_id,
        remote_device as DeviceId,
    ) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcAcceptCall(
    env: JNIEnv,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    match call_manager::accept_call(call_manager as *mut AndroidCallManager, call_id) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcGetActiveConnection(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
) -> jobject {
    match call_manager::get_active_connection(call_manager as *mut AndroidCallManager) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0 as jobject
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcGetActiveCallContext(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
) -> jobject {
    match call_manager::get_active_call_context(call_manager as *mut AndroidCallManager) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0 as jobject
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcSetVideoEnable(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
    enable: jboolean,
) {
    match call_manager::set_video_enable(call_manager as *mut AndroidCallManager, enable != 0) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcSetLowBandwidthMode(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
    enabled: bool,
) {
    let mode = if enabled {
        BandwidthMode::Low
    } else {
        BandwidthMode::Normal
    };

    match call_manager::set_bandwidth_mode(call_manager as *mut AndroidCallManager, mode) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcDrop(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    match call_manager::drop_call(call_manager as *mut AndroidCallManager, call_id) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReset(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
) {
    match call_manager::reset(call_manager as *mut AndroidCallManager) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcClose(
    env: JNIEnv<'static>,
    _object: JObject,
    call_manager: jlong,
) {
    match call_manager::close(call_manager as *mut AndroidCallManager) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        }
    }
}
