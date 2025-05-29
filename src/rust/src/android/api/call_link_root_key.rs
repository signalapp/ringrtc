//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::borrow::Cow;

use jni::{
    objects::{JByteArray, JClass, JObject, JString},
    sys::jint,
    JNIEnv,
};

use crate::{
    android::{error, jni_util::*},
    core::util::try_scoped,
    lite::call_links::{CallLinkEpoch, CallLinkRootKey},
};

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeParseKeyString<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass,
    string: JString,
) -> JByteArray<'local> {
    try_scoped(|| {
        let string = env.get_string(&string)?;
        let key = CallLinkRootKey::try_from(Cow::from(&string).as_ref())?;
        Ok(env.byte_array_from_slice(&key.bytes())?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&mut env, e);
        JByteArray::default()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeValidateKeyBytes(
    mut env: JNIEnv,
    _class: JClass,
    bytes: JByteArray,
) {
    try_scoped(|| {
        let bytes = env.convert_byte_array(bytes)?;
        let _ = CallLinkRootKey::try_from(bytes.as_slice())?;
        Ok(())
    })
    .unwrap_or_else(|e| error::throw_error(&mut env, e))
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_generate<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass,
) -> JObject<'local> {
    try_scoped(|| {
        let key = CallLinkRootKey::generate(rand::rngs::OsRng);
        let bytes = env.byte_array_from_slice(&key.bytes())?;
        let object = jni_new_object(
            &mut env,
            jni_class_name!(org.signal.ringrtc.CallLinkRootKey),
            jni_args!((bytes => [byte]) -> void),
        )?;
        Ok(object)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&mut env, e);
        JObject::default()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_generateAdminPasskey<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass,
) -> JByteArray<'local> {
    try_scoped(|| {
        let passkey = CallLinkRootKey::generate_admin_passkey(rand::rngs::OsRng);
        Ok(env.byte_array_from_slice(&passkey)?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&mut env, e);
        JByteArray::default()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeDeriveRoomId<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass,
    key_bytes: JByteArray,
) -> JByteArray<'local> {
    try_scoped(|| {
        let key_bytes = env.convert_byte_array(key_bytes)?;
        let key = CallLinkRootKey::try_from(key_bytes.as_slice())?;
        Ok(env.byte_array_from_slice(&key.derive_room_id())?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&mut env, e);
        JByteArray::default()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeToFormattedString<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass,
    key_bytes: JByteArray,
) -> JString<'local> {
    try_scoped(|| {
        let key_bytes = env.convert_byte_array(key_bytes)?;
        let key = CallLinkRootKey::try_from(key_bytes.as_slice())?;
        Ok(env.new_string(key.to_formatted_string())?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&mut env, e);
        JString::default()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkEpoch_nativeParse(
    mut env: JNIEnv,
    _class: JClass,
    string: JString,
) -> jint {
    try_scoped(|| {
        let string = env.get_string(&string)?;
        let epoch = CallLinkEpoch::try_from(Cow::from(&string).as_ref())?;
        let value: u32 = epoch.into();
        Ok(value as jint)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&mut env, e);
        0
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkEpoch_nativeToFormattedString<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass,
    value: jint,
) -> JString<'local> {
    try_scoped(|| {
        let epoch = CallLinkEpoch::from(value as u32);
        Ok(env.new_string(epoch.to_formatted_string())?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&mut env, e);
        JString::default()
    })
}
