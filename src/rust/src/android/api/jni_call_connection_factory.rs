//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! JNI Call Connection Factory interface functions.
//!
//! Native JNI interfaces, called by
//! org.signal.ringrtc.CallConnectionFactory objects.

use jni::JNIEnv;
use jni::objects::{
    JObject,
    JClass,
};
use jni::sys::jlong;

use crate::android::error;
use crate::android::call_connection_factory;

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnectionFactory_nativeInitialize(env:    JNIEnv,
                                                                                    _class: JClass) {
    if let Err(e) = call_connection_factory::native_initialize() {
        error::throw_error(&env, e);
    }

}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn
    Java_org_signal_ringrtc_CallConnectionFactory_nativeCreateCallConnectionFactory(env:    JNIEnv,
                                                                                    _class: JClass,
                                                                                    peer_connection_factory: jlong) -> jlong {
    match call_connection_factory::native_create_call_connection_factory(peer_connection_factory) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn
    Java_org_signal_ringrtc_CallConnectionFactory_nativeFreeFactory(env:     JNIEnv,
                                                                    _class:  JClass,
                                                                    factory: jlong)
{
    match call_connection_factory::native_free_factory(factory) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern
fn Java_org_signal_ringrtc_CallConnectionFactory_nativeCreateCallConnection(env:               JNIEnv,
                                                                            class:             JClass,
                                                                            native_factory:    jlong,
                                                                            call_config:       JObject,
                                                                            native_observer:   jlong,
                                                                            rtc_config:        JObject,
                                                                            media_constraints: JObject,
                                                                            ssl_cert_verifier: JObject) -> jlong {
    match call_connection_factory::native_create_call_connection(&env, class, native_factory,
                                                                 call_config, native_observer,
                                                                 rtc_config, media_constraints,
                                                                 ssl_cert_verifier) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0
        },
    }
}
