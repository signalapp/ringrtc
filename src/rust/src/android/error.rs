//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Android Error Codes and Utilities.

use anyhow::Error;
use jni::{errors, objects::JThrowable, JNIEnv};
use thiserror::Error;

use crate::{android::jni_util::*, core::util::try_scoped};

const CALL_EXCEPTION_CLASS: &str = jni_class_name!(org.signal.ringrtc.CallException);

/// Convert a `Error` into a Java `org.signal.ringrtc.CallException`
/// and throw it.
///
/// This is used to communicate synchronous errors to the client
/// application.
pub fn throw_error(env: &mut JNIEnv, error: Error) {
    if let Ok(exception) = env.exception_occurred() {
        if env.exception_clear().is_ok() {
            let _ = try_scoped(|| {
                let message = env.new_string(error.to_string())?;
                let call_exception: JThrowable = jni_new_object(
                    env,
                    CALL_EXCEPTION_CLASS,
                    jni_args!((
                        message => java.lang.String,
                        exception => java.lang.Throwable,
                    ) -> void),
                )?
                .into();
                Ok(env.throw(call_exception)?)
            });
        } else {
            // Don't try to throw our own exception on top of another exception.
        }
    } else {
        let _ = env.throw_new(CALL_EXCEPTION_CLASS, format!("{}", error));
    }
}

/// Android specific error codes.
#[derive(Error, Debug)]
pub enum AndroidError {
    // Android JNI error codes
    #[error("JNI: static method lookup failed.  Class: {0}, Method: {1}, Sig: {2}")]
    JniStaticMethodLookup(String, String, String),
    #[error("JNI: calling method failed.  Method: {0}, Sig: {1}, Error: {2}")]
    JniCallMethod(String, String, errors::Error),
    #[error("JNI: calling static method failed.  Class: {0}, Method: {1}, Sig: {2}")]
    JniCallStaticMethod(String, String, String),
    #[error("JNI: calling constructor failed.  Constructor: {0}, Sig: {1}")]
    JniCallConstructor(String, String),
    #[error("JNI: getting field failed.  Field: {0}, Type: {1}")]
    JniGetField(String, String),
    #[error("JNI: class not found.  Type: {0} Add to the cache?")]
    JniGetLangClassNotFound(String),
    #[error("JNI: new object failed.  Type: {0}")]
    JniNewLangObjectFailed(String),
    #[error("JNI: invalid serialized buffer.")]
    JniInvalidSerializedBuffer,

    // Android Class Cache error codes
    #[error("ClassCache: Class is already in cache: {0}")]
    ClassCacheDuplicate(String),
    #[error("ClassCache: class not found in jvm: {0}")]
    ClassCacheNotFound(String),
    #[error("ClassCache: class not found in cache: {0}")]
    ClassCacheLookup(String),

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
