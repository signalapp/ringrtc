//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use anyhow::Result;
use jni::{
    EnvUnowned, jni_str,
    objects::{JByteArray, JClass, JObject, JString},
};

use crate::{android::error::ThrowCallException, lite::call_links::CallLinkRootKey};

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeParseKeyString<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass,
    string: JString,
) -> JByteArray<'local> {
    unowned_env
        .with_env(|env| -> Result<_> {
            let string = string.try_to_string(env)?;
            let key = CallLinkRootKey::try_from(string.as_str())?;
            Ok(env.byte_array_from_slice(key.as_slice())?)
        })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeValidateKeyBytes(
    mut unowned_env: EnvUnowned,
    _class: JClass,
    bytes: JByteArray,
) {
    unowned_env
        .with_env(|env| -> Result<()> {
            let bytes = env.convert_byte_array(bytes)?;
            let _ = CallLinkRootKey::try_from(bytes.as_slice())?;
            Ok(())
        })
        .resolve::<ThrowCallException>();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_generate<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass,
) -> JObject<'local> {
    unowned_env
        .with_env(|env| -> Result<_> {
            let key = CallLinkRootKey::generate(rand::rngs::OsRng);
            let bytes = env.byte_array_from_slice(key.as_slice())?;
            let object = jni_new_object!(env, jni_str!("org/signal/ringrtc/CallLinkRootKey"), (
                bytes => [byte],
            ))?;
            Ok(object)
        })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_generateAdminPasskey<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass,
) -> JByteArray<'local> {
    unowned_env
        .with_env(|env| -> Result<_> {
            let passkey = CallLinkRootKey::generate_admin_passkey(rand::rngs::OsRng);
            Ok(env.byte_array_from_slice(&passkey)?)
        })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeDeriveRoomId<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass,
    key_bytes: JByteArray,
) -> JByteArray<'local> {
    unowned_env
        .with_env(|env| -> Result<_> {
            let key_bytes = env.convert_byte_array(key_bytes)?;
            let key = CallLinkRootKey::try_from(key_bytes.as_slice())?;
            Ok(env.byte_array_from_slice(&key.derive_room_id())?)
        })
        .resolve::<ThrowCallException>()
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "C" fn Java_org_signal_ringrtc_CallLinkRootKey_nativeToFormattedString<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass,
    key_bytes: JByteArray,
) -> JString<'local> {
    unowned_env
        .with_env(|env| -> Result<_> {
            let key_bytes = env.convert_byte_array(key_bytes)?;
            let key = CallLinkRootKey::try_from(key_bytes.as_slice())?;
            Ok(env.new_string(key.to_formatted_string())?)
        })
        .resolve::<ThrowCallException>()
}
