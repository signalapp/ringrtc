//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::borrow::Cow;

use jni::{
    objects::{JClass, JString},
    sys::{jbyteArray, jobject, jstring},
    JNIEnv,
};

use crate::{
    android::{error, jni_util::*},
    core::util::try_scoped,
    lite::call_links::CallLinkRootKey,
};

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeParseKeyString(
    env: JNIEnv,
    _class: JClass,
    string: JString,
) -> jbyteArray {
    try_scoped(|| {
        let string = env.get_string(string)?;
        let key = CallLinkRootKey::try_from(Cow::from(&string).as_ref())?;
        Ok(env.byte_array_from_slice(&key.bytes())?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&env, e);
        std::ptr::null_mut()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeValidateKeyBytes(
    env: JNIEnv,
    _class: JClass,
    bytes: jbyteArray,
) {
    try_scoped(|| {
        let bytes = env.convert_byte_array(bytes)?;
        let _ = CallLinkRootKey::try_from(bytes.as_slice())?;
        Ok(())
    })
    .unwrap_or_else(|e| error::throw_error(&env, e))
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_generate(
    env: JNIEnv,
    _class: JClass,
) -> jobject {
    try_scoped(|| {
        let key = CallLinkRootKey::generate(rand::rngs::OsRng);
        let bytes = env.byte_array_from_slice(&key.bytes())?;
        Ok(jni_new_object(
            &env,
            jni_class_name!(org.signal.ringrtc.CallLinkRootKey),
            jni_args!((bytes => [byte]) -> void),
        )?
        .into_inner())
    })
    .unwrap_or_else(|e| {
        error::throw_error(&env, e);
        std::ptr::null_mut()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_generateAdminPasskey(
    env: JNIEnv,
    _class: JClass,
) -> jbyteArray {
    try_scoped(|| {
        let passkey = CallLinkRootKey::generate_admin_passkey(rand::rngs::OsRng);
        Ok(env.byte_array_from_slice(&passkey)?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&env, e);
        std::ptr::null_mut()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeDeriveRoomId(
    env: JNIEnv,
    _class: JClass,
    key_bytes: jbyteArray,
) -> jbyteArray {
    try_scoped(|| {
        let key_bytes = env.convert_byte_array(key_bytes)?;
        let key = CallLinkRootKey::try_from(key_bytes.as_slice())?;
        Ok(env.byte_array_from_slice(&key.derive_room_id())?)
    })
    .unwrap_or_else(|e| {
        error::throw_error(&env, e);
        std::ptr::null_mut()
    })
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeToFormattedString(
    env: JNIEnv,
    _class: JClass,
    key_bytes: jbyteArray,
) -> jstring {
    try_scoped(|| {
        let key_bytes = env.convert_byte_array(key_bytes)?;
        let key = CallLinkRootKey::try_from(key_bytes.as_slice())?;
        Ok(env.new_string(&key.to_formatted_string())?.into_inner())
    })
    .unwrap_or_else(|e| {
        error::throw_error(&env, e);
        std::ptr::null_mut()
    })
}
