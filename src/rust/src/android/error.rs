//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Android Error Codes and Utilities.

use std::panic::{AssertUnwindSafe, catch_unwind};

use jni::{
    Env,
    errors::ErrorPolicy,
    jni_str,
    objects::JThrowable,
    strings::{JNIStr, JNIString},
};
use thiserror::Error;

use crate::core::util::try_scoped;

const CALL_EXCEPTION_CLASS: &JNIStr = jni_str!("org/signal/ringrtc/CallException");

/// `ErrorPolicy` that converts Rust errors and panics into a Java
/// `org.signal.ringrtc.CallException` and throws it to the client.
#[derive(Debug, Default)]
pub struct ThrowCallException;

impl<T: Default> ErrorPolicy<T, anyhow::Error> for ThrowCallException {
    type Captures<'unowned_env_local: 'native_method, 'native_method> = ();

    fn on_error<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _cap: &mut Self::Captures<'unowned_env_local, 'native_method>,
        err: anyhow::Error,
    ) -> jni::errors::Result<T> {
        // If the closure returned an error while a Java exception was already
        // pending, preserve that exception as the cause.
        if let Some(exception) = env.exception_occurred() {
            env.exception_clear();
            let _ = try_scoped(|| {
                let message = env.new_string(err.to_string())?;
                let call_exception = jni_new_object!(env, CALL_EXCEPTION_CLASS, (
                    message => java.lang.String,
                    exception => java.lang.Throwable,
                ))?;
                // SAFETY: We just constructed CallException, which extends Throwable.
                let throwable = unsafe { JThrowable::from_raw(env, call_exception.as_raw()) };
                Ok(env.throw(throwable)?)
            });
        } else {
            let jni_msg = JNIString::from(format!("{}", err));
            let _ = env.throw_new(CALL_EXCEPTION_CLASS, &jni_msg);
        }
        Ok(T::default())
    }

    fn on_panic<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _cap: &mut Self::Captures<'unowned_env_local, 'native_method>,
        payload: Box<dyn std::any::Any + Send + 'static>,
    ) -> jni::errors::Result<T> {
        let panic_string = match payload.downcast::<&'static str>() {
            Ok(s) => (*s).to_string(),
            Err(payload) => match payload.downcast::<String>() {
                Ok(s) => *s,
                Err(payload) => {
                    if let Err(drop_panic) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                        log::error!("Panic while dropping panic payload: {:?}", drop_panic);
                        std::mem::forget(drop_panic);
                    }
                    "".to_string()
                }
            },
        };
        let jni_msg = JNIString::from(format!("Rust panic: {panic_string}"));
        let _ = env.throw_new(CALL_EXCEPTION_CLASS, &jni_msg);
        Ok(T::default())
    }
}

/// Android specific error codes.
#[derive(Error, Debug)]
pub enum AndroidError {
    // Android JNI error codes
    #[error("JNI: static method lookup failed.  Class: {0}, Method: {1}, Sig: {2}")]
    JniStaticMethodLookup(String, String, String),
    #[error("JNI: invalid serialized buffer.")]
    JniInvalidSerializedBuffer,

    // Android Misc error codes
    #[error("Creating JNI PeerConnection failed")]
    CreateJniPeerConnection,
    #[error("Extracting native PeerConnection failed")]
    ExtractNativePeerConnection,
    #[error("Creating JNI Connection failed")]
    CreateJniConnection,

    // WebRTC / JNI C++ error codes
    #[error("Unable to create C++ JavaMediaStream")]
    CreateJavaMediaStream,
}
