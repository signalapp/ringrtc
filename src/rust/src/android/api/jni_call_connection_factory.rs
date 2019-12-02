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
use jni::sys::{
    jlong,
    jobject,
};

use crate::android::error;
use crate::android::call_connection_factory;
use crate::android::call_connection_factory::{
    AndroidCallConnectionFactory,
    AppPeerConnectionFactory,
};
use crate::android::call_connection_observer::AndroidCallConnectionObserver;

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnectionFactory_ringrtcGetBuildInfo(env:    JNIEnv,
                                                                                       _class: JClass) -> jobject {
    match call_connection_factory::get_build_info(&env) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0 as jobject
        },
    }

}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnectionFactory_ringrtcInitialize(env:    JNIEnv,
                                                                                     _class: JClass) {
    if let Err(e) = call_connection_factory::initialize(&env) {
        error::throw_error(&env, e);
    }

}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn
    Java_org_signal_ringrtc_CallConnectionFactory_ringrtcCreateCallConnectionFactory(env:    JNIEnv,
                                                                                     _class: JClass,
                                                                                     native_pc_factory: jlong) -> jlong {
    match call_connection_factory::create_call_connection_factory(native_pc_factory as *mut AppPeerConnectionFactory) {
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
    Java_org_signal_ringrtc_CallConnectionFactory_ringrtcFreeFactory(env:     JNIEnv,
                                                                     _class:  JClass,
                                                                     factory: jlong)
{
    match call_connection_factory::free_factory(factory as *mut AndroidCallConnectionFactory) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern
fn Java_org_signal_ringrtc_CallConnectionFactory_ringrtcCreateCallConnection(env:               JNIEnv,
                                                                             class:             JClass,
                                                                             native_factory:    jlong,
                                                                             call_config:       JObject,
                                                                             native_observer:   jlong,
                                                                             rtc_config:        JObject,
                                                                             media_constraints: JObject,
                                                                             ssl_cert_verifier: JObject) -> jlong {
    match call_connection_factory::create_call_connection(&env,
                                                          class,
                                                          native_factory as *mut AndroidCallConnectionFactory,
                                                          call_config,
                                                          native_observer as *mut AndroidCallConnectionObserver,
                                                          rtc_config,
                                                          media_constraints,
                                                          ssl_cert_verifier) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0
        },
    }
}
