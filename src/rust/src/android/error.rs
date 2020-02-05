//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android Error Codes and Utilities.

use failure::Error;
use jni::errors;
use jni::objects::JObject;
use jni::JNIEnv;

const CALL_EXCEPTION_CLASS: &str = "org/signal/ringrtc/CallException";

/// Convert a `Error` into a Java `org.signal.ringrtc.CallException`
/// and throw it.
///
/// This is used to communicate synchronous errors to the client
/// application.
pub fn throw_error(env: &JNIEnv, error: Error) {
    if env.exception_check().is_ok() {
        if let Ok(exception) = env.exception_occurred() {
            if env.exception_clear().is_ok() {
                let args = [];
                let java_exception: String;
                match env.call_method(
                    JObject::from(exception),
                    "toString",
                    "()Ljava/lang/String;",
                    &args,
                ) {
                    Ok(v) => {
                        java_exception = {
                            if let Ok(jstring) = v.l() {
                                if let Ok(rstring) = env.get_string(jstring.into()) {
                                    rstring.into()
                                } else {
                                    String::from("unknown -- unable to decode exception")
                                }
                            } else {
                                String::from("unknown -- unable to decode exception")
                            }
                        }
                    }
                    Err(_) => {
                        java_exception = String::from("unknown -- unable to decode exception")
                    }
                }

                let _ = env.throw_new(
                    CALL_EXCEPTION_CLASS,
                    format!("{} caused by java exception:\n{}", error, java_exception),
                );
            }
        }
    } else {
        let _ = env.throw_new(CALL_EXCEPTION_CLASS, format!("{}", error));
    }
}

/// Android specific error codes.
#[derive(Fail, Debug)]
pub enum AndroidError {
    // Android JNI error codes
    #[fail(
        display = "JNI: static method lookup failed.  Class: {}, Method: {}, Sig: {}",
        _0, _1, _2
    )]
    JniStaticMethodLookup(String, String, String),
    #[fail(
        display = "JNI: calling method failed.  Method: {}, Sig: {}, Error: {}",
        _0, _1, _2
    )]
    JniCallMethod(String, String, errors::Error),
    #[fail(
        display = "JNI: calling static method failed.  Class: {}, Method: {}, Sig: {}",
        _0, _1, _2
    )]
    JniCallStaticMethod(String, String, String),
    #[fail(
        display = "JNI: calling constructor failed.  Constructor: {}, Sig: {}",
        _0, _1
    )]
    JniCallConstructor(String, String),
    #[fail(display = "JNI: getting field failed.  Field: {}, Type: {}", _0, _1)]
    JniGetField(String, String),

    // Android Class Cache error codes
    #[fail(display = "ClassCache: Class is already in cache: {}", _0)]
    ClassCacheDuplicate(String),
    #[fail(display = "ClassCache: class not found in jvm: {}", _0)]
    ClassCacheNotFound(String),
    #[fail(display = "ClassCache: class not found in cache: {}", _0)]
    ClassCacheLookup(String),

    // Android Misc error codes
    #[fail(display = "Creating JNI PeerConnection failed")]
    CreateJniPeerConnection,
    #[fail(display = "Extracting native PeerConnectionInterface failed")]
    ExtractNativePeerConnectionInterface,
    #[fail(display = "Creating JNI Connection failed")]
    CreateJniConnection,

    // WebRTC / JNI C++ error codes
    #[fail(display = "Unable to create C++ JavaMediaStream")]
    CreateJavaMediaStream,
}
