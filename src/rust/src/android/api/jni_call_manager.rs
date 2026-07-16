//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! JNI Call Manager interface functions.
//!
//! Native JNI interfaces, called by
//! org.signal.ringrtc.CallManager objects.

use std::time::Duration;

use anyhow::Result;
use jni::{
    EnvUnowned,
    objects::{JByteArray, JClass, JObject, JString},
    sys::{jboolean, jbyte, jint, jlong, jobject},
};

use crate::{
    android::{
        android_platform::AndroidPlatform, call_manager, call_manager::AndroidCallManager,
        error::ThrowCallException,
    },
    common::{CallConfig, CallMediaType, DataMode, DeviceId},
    core::{connection::Connection, group_call, signaling},
    webrtc,
};

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcGetBuildInfo<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass,
) -> JObject<'local> {
    unowned_env
        .with_env(|env| -> Result<_> { call_manager::get_build_info(env) })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcInitialize(
    mut unowned_env: EnvUnowned,
    _class: JClass,
) {
    unowned_env
        .with_env(|env| -> Result<()> { call_manager::initialize(env) })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCreateCallManager(
    mut unowned_env: EnvUnowned,
    _class: JClass,
    jni_call_manager: JObject,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<_> { call_manager::create_call_manager(env, jni_call_manager) })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCreatePeerConnection(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    peer_connection_factory: jlong,
    native_connection_borrowed: jlong,
    jni_rtc_config: JObject,
    jni_media_constraints: JObject,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<_> {
            call_manager::create_peer_connection(
                env,
                peer_connection_factory,
                webrtc::ptr::Borrowed::from_ptr(
                    native_connection_borrowed as *mut Connection<AndroidPlatform>,
                ),
                jni_rtc_config,
                jni_media_constraints,
            )
        })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcSetSelfUuid(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    uuid: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::set_self_uuid(env, call_manager as *mut AndroidCallManager, uuid)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcAddAsset(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    asset_group: JString,
    file_path: JString,
    content: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::add_asset(
                env,
                call_manager as *mut AndroidCallManager,
                asset_group,
                file_path,
                content,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCall(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    jni_remote: JObject,
    call_media_type: jint,
    local_device: jint,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::call(
                env,
                call_manager as *mut AndroidCallManager,
                jni_remote,
                CallMediaType::from_i32(call_media_type),
                local_device as DeviceId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcProceed(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_call_context: JObject,
    data_mode: jint,
    audio_levels_interval_millis: jint,
    dred_duration: jbyte,
    enable_vp9_encode: jboolean,
    enable_vp9_decode: jboolean,
) {
    let audio_levels_interval = if audio_levels_interval_millis <= 0 {
        None
    } else {
        Some(Duration::from_millis(audio_levels_interval_millis as u64))
    };

    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::proceed(
                env,
                call_manager as *mut AndroidCallManager,
                call_id,
                jni_call_context,
                CallConfig::default()
                    .with_data_mode(DataMode::from_i32(data_mode))
                    .with_dred_duration(dred_duration as u8)
                    .with_enable_vp9_encode(enable_vp9_encode)
                    .with_enable_vp9_decode(enable_vp9_decode),
                audio_levels_interval,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcMessageSent(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::message_sent(call_manager as *mut AndroidCallManager, call_id)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcMessageSendFailure(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::message_send_failure(call_manager as *mut AndroidCallManager, call_id)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcHangup(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::hangup(call_manager as *mut AndroidCallManager)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCancelGroupRing(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    group_id: JByteArray,
    ring_id: jlong,
    reason: jint,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::cancel_group_ring(
                env,
                call_manager as *mut AndroidCallManager,
                group_id,
                ring_id,
                reason,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedAnswer(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_remote: JObject,
    remote_device: jint,
    opaque: JByteArray,
    sender_identity_key: JByteArray,
    receiver_identity_key: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::received_answer(
                env,
                call_manager as *mut AndroidCallManager,
                call_id,
                jni_remote,
                remote_device as DeviceId,
                opaque,
                sender_identity_key,
                receiver_identity_key,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedOffer(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_remote: JObject,
    remote_device: jint,
    opaque: JByteArray,
    message_age_sec: jlong,
    call_media_type: jint,
    local_device: jint,
    sender_identity_key: JByteArray,
    receiver_identity_key: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::received_offer(
                env,
                call_manager as *mut AndroidCallManager,
                call_id,
                jni_remote,
                remote_device as DeviceId,
                opaque,
                message_age_sec as u64,
                CallMediaType::from_i32(call_media_type),
                local_device as DeviceId,
                sender_identity_key,
                receiver_identity_key,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedIceCandidates(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_remote: JObject,
    remote_device: jint,
    jni_ice_candidates: JObject,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::received_ice(
                env,
                call_manager as *mut AndroidCallManager,
                call_id,
                jni_remote,
                remote_device as DeviceId,
                jni_ice_candidates,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedHangup(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_remote: JObject,
    remote_device: jint,
    hangup_type: jint,
    device_id: jint,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::received_hangup(
                env,
                call_manager as *mut AndroidCallManager,
                call_id,
                jni_remote,
                remote_device as DeviceId,
                signaling::HangupType::from_i32(hangup_type)
                    .unwrap_or(signaling::HangupType::Normal),
                device_id as DeviceId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedBusy(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
    jni_remote: JObject,
    remote_device: jint,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::received_busy(
                env,
                call_manager as *mut AndroidCallManager,
                call_id,
                jni_remote,
                remote_device as DeviceId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedCallMessage(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    sender_uuid: JByteArray,
    sender_device_id: jint,
    local_device_id: jint,
    message: JByteArray,
    message_age_sec: jlong,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::received_call_message(
                env,
                call_manager as *mut AndroidCallManager,
                sender_uuid,
                sender_device_id as DeviceId,
                local_device_id as DeviceId,
                message,
                message_age_sec as u64,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReceivedHttpResponse(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    request_id: jlong,
    status_code: jint,
    body: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::received_http_response(
                env,
                call_manager as *mut AndroidCallManager,
                request_id,
                status_code,
                body,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcHttpRequestFailed(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    request_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::http_request_failed(call_manager as *mut AndroidCallManager, request_id)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcAcceptCall(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::accept_call(call_manager as *mut AndroidCallManager, call_id)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcGetActiveConnection(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
) -> jobject {
    unowned_env
        .with_env(|_env| -> Result<_> {
            call_manager::get_active_connection(call_manager as *mut AndroidCallManager)
        })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcGetActiveCallContext(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
) -> jobject {
    unowned_env
        .with_env(|_env| -> Result<_> {
            call_manager::get_active_call_context(call_manager as *mut AndroidCallManager)
        })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcSetAudioEnable(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    enable: jboolean,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_audio_enable(call_manager as *mut AndroidCallManager, enable)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcSetVideoEnable(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    enable: jboolean,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_video_enable(call_manager as *mut AndroidCallManager, enable)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcSetOutgoingVideoIsScreenShare(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    is_screenshare: jboolean,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_outgoing_video_is_screenshare(
                call_manager as *mut AndroidCallManager,
                is_screenshare,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcUpdateDataMode(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    data_mode: jint,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::update_data_mode(
                call_manager as *mut AndroidCallManager,
                DataMode::from_i32(data_mode),
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcDrop(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    call_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::drop_call(call_manager as *mut AndroidCallManager, call_id)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReset(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::reset(call_manager as *mut AndroidCallManager)
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcClose(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::close(call_manager as *mut AndroidCallManager)
        })
        .resolve::<ThrowCallException>();
}

// Call Links

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcReadCallLink(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    sfu_url: JString,
    auth_credential_presentation: JByteArray,
    root_key: JByteArray,
    request_id: jlong,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::read_call_link(
                env,
                call_manager as *mut AndroidCallManager,
                sfu_url,
                auth_credential_presentation,
                root_key,
                request_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcCreateCallLink(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    sfu_url: JString,
    create_credential_presentation: JByteArray,
    root_key: JByteArray,
    admin_passkey: JByteArray,
    call_link_public_params: JByteArray,
    restrictions: jint,
    request_id: jlong,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::create_call_link(
                env,
                call_manager as *mut AndroidCallManager,
                sfu_url,
                create_credential_presentation,
                root_key,
                admin_passkey,
                call_link_public_params,
                restrictions,
                request_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcUpdateCallLink(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    sfu_url: JString,
    auth_credential_presentation: JByteArray,
    root_key: JByteArray,
    admin_passkey: JByteArray,
    new_name: JString,
    new_restrictions: jint,
    new_revoked: jint,
    request_id: jlong,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::update_call_link(
                env,
                call_manager as *mut AndroidCallManager,
                sfu_url,
                auth_credential_presentation,
                root_key,
                admin_passkey,
                new_name,
                new_restrictions,
                new_revoked,
                request_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcDeleteCallLink(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    sfu_url: JString,
    auth_credential_presentation: JByteArray,
    root_key: JByteArray,
    admin_passkey: JByteArray,
    request_id: jlong,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::delete_call_link(
                env,
                call_manager as *mut AndroidCallManager,
                sfu_url,
                auth_credential_presentation,
                root_key,
                admin_passkey,
                request_id,
            )
        })
        .resolve::<ThrowCallException>();
}

// Group Calls

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcPeekGroupCall(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    request_id: jlong,
    sfu_url: JString,
    membership_proof: JByteArray,
    jni_serialized_group_members: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::peek_group_call(
                env,
                call_manager as *mut AndroidCallManager,
                request_id,
                sfu_url,
                membership_proof,
                jni_serialized_group_members,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallManager_ringrtcPeekCallLinkCall(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    request_id: jlong,
    sfu_url: JString,
    auth_credential_presentation: JByteArray,
    root_key: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::peek_call_link_call(
                env,
                call_manager as *mut AndroidCallManager,
                request_id,
                sfu_url,
                auth_credential_presentation,
                root_key,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcCreateGroupCallClient(
    mut unowned_env: EnvUnowned,
    _cls: JClass,
    call_manager: jlong,
    group_id: JByteArray,
    sfu_url: JString,
    hkdf_extra_info: JByteArray,
    audio_levels_interval_millis: jint,
    dred_duration: jbyte,
    native_peer_connection_factory_borrowed_rc: jlong,
    native_audio_track_borrowed_rc: jlong,
    native_video_track_borrowed_rc: jlong,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<_> {
            Ok(call_manager::create_group_call_client(
                env,
                call_manager as *mut AndroidCallManager,
                group_id,
                sfu_url,
                hkdf_extra_info,
                audio_levels_interval_millis,
                dred_duration,
                native_peer_connection_factory_borrowed_rc,
                native_audio_track_borrowed_rc,
                native_video_track_borrowed_rc,
            )? as i64)
        })
        // Note: The ErrorPolicy returns T::default() which for i64 is 0. This implicitly
        // matches group_call::INVALID_CLIENT_ID.
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcCreateCallLinkCallClient(
    mut unowned_env: EnvUnowned,
    _cls: JClass,
    call_manager: jlong,
    sfu_url: JString,
    endorsement_public_key: JByteArray,
    auth_presentation: JByteArray,
    call_link_bytes: JByteArray,
    admin_passkey: JByteArray,
    hkdf_extra_info: JByteArray,
    audio_levels_interval_millis: jint,
    dred_duration: jbyte,
    native_peer_connection_factory_borrowed_rc: jlong,
    native_audio_track_borrowed_rc: jlong,
    native_video_track_borrowed_rc: jlong,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<_> {
            Ok(call_manager::create_call_link_call_client(
                env,
                call_manager as *mut AndroidCallManager,
                sfu_url,
                endorsement_public_key,
                auth_presentation,
                call_link_bytes,
                admin_passkey,
                hkdf_extra_info,
                audio_levels_interval_millis,
                dred_duration,
                native_peer_connection_factory_borrowed_rc,
                native_audio_track_borrowed_rc,
                native_video_track_borrowed_rc,
            )? as i64)
        })
        // Note: The ErrorPolicy returns T::default() which for i64 is 0. This implicitly
        // matches group_call::INVALID_CLIENT_ID.
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcDeleteGroupCallClient(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::delete_group_call_client(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcConnect(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::connect(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcJoin(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::join(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcLeave(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::leave(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcDisconnect(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::disconnect(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetOutgoingAudioMuted(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    muted: bool,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_outgoing_audio_muted(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                muted,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetOutgoingAudioMutedRemotely(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    source_demux_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_outgoing_audio_muted_remotely(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                source_demux_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSendRemoteMuteRequest(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    target_demux_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::send_remote_mute_request(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                target_demux_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetOutgoingVideoMuted(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    muted: bool,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_outgoing_video_muted(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                muted,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetPresenting(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    presenting: jboolean,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_presenting(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                presenting,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetOutgoingVideoIsScreenShare(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    is_screenshare: jboolean,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_outgoing_group_call_video_is_screenshare(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                is_screenshare,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcRing(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    recipient: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::group_ring(
                env,
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                recipient,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcResendMediaKeys(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::resend_media_keys(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetDataMode(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    data_mode: jint,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::set_data_mode(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                DataMode::from_i32(data_mode),
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcRequestVideo(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    jni_rendered_resolutions: JObject,
    active_speaker_height: jint,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::request_video(
                env,
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                jni_rendered_resolutions,
                active_speaker_height,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcApproveUser(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    other_user_id: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::approve_user(
                env,
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                other_user_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcDenyUser(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    other_user_id: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::deny_user(
                env,
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                other_user_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcRemoveClient(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    other_client_demux_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::remove_client(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                other_client_demux_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcBlockClient(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    other_client_demux_id: jlong,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::block_client(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                other_client_demux_id,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetGroupMembers(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    jni_serialized_group_members: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::set_group_members(
                env,
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                jni_serialized_group_members,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcSetMembershipProof(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    proof: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::set_membership_proof(
                env,
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                proof,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcReact(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    value: JString,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            call_manager::react(
                env,
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                value,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_GroupCall_ringrtcRaiseHand(
    mut unowned_env: EnvUnowned,
    _object: JObject,
    call_manager: jlong,
    client_id: jlong,
    raise: bool,
) {
    unowned_env
        .with_env(|_env| -> Result<()> {
            call_manager::raise_hand(
                call_manager as *mut AndroidCallManager,
                client_id as group_call::ClientId,
                raise,
            )
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallId_ringrtcFromEraId(
    mut unowned_env: EnvUnowned,
    _class: JClass,
    era: JString,
) -> jlong {
    unowned_env
        .with_env(|env| -> Result<_> {
            let era_string = era.try_to_string(env)?;
            Ok(group_call::RingId::from_era_id(&era_string).into())
        })
        .resolve::<ThrowCallException>()
}
