//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! JNI Call Connection interface functions.
//!
//! Native JNI interfaces, called by org.signal.ringtrc.CallConnection
//! objects.


use jni::JNIEnv;
use jni::objects::{
    JObject,
    JClass,
    JString,
};
use jni::sys::{
    jlong,
    jboolean,
    jint,
    JNI_FALSE,
    JNI_TRUE,
};

use crate::android::error;
use crate::android::call_connection;

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern
fn Java_org_signal_ringrtc_CallConnection_nativeGetNativePeerConnection(env:             JNIEnv,
                                                                        _class:          JClass,
                                                                        call_connection: jlong) -> jlong {
    match call_connection::native_get_native_peer_connection(call_connection) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeClose(env:             JNIEnv,
                                                                        _object:         JObject,
                                                                        call_connection: jlong) {
    match call_connection::native_close_call_connection(call_connection) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeSendOffer(env:             JNIEnv,
                                                                            jcall_connection:JObject,
                                                                            call_connection: jlong) {

    match call_connection::native_send_offer(&env, jcall_connection, call_connection) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeCreateCallConnectionObserver(env:       JNIEnv,
                                                                                               _class:    JClass,
                                                                                               observer:  JObject,
                                                                                               call_id:   jlong,
                                                                                               recipient: JObject) -> jlong {
    match call_connection::native_create_call_connection_observer(&env, observer, call_id, recipient) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            0
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeValidateResponseState(env:             JNIEnv,
                                                                                        _object:         JObject,
                                                                                        call_connection: jlong) -> jboolean {
    match call_connection::native_validate_response_state(call_connection) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
            JNI_FALSE
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeHandleOfferAnswer(env:             JNIEnv,
                                                                                    _object:         JObject,
                                                                                    call_connection: jlong,
                                                                                    session_desc:    JString) {
    match call_connection::native_handle_offer_answer(&env, call_connection, session_desc) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e)
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeAcceptOffer(env:             JNIEnv,
                                                                              jcall_connection:JObject,
                                                                              call_connection: jlong,
                                                                              offer:           JString) {
    match call_connection::native_accept_offer(&env, jcall_connection, call_connection, offer) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeHangUp(env:             JNIEnv,
                                                                         _object:         JObject,
                                                                         call_connection: jlong) {
    match call_connection::native_hang_up(call_connection) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeAnswerCall(env:             JNIEnv,
                                                                             _object:         JObject,
                                                                             call_connection: jlong) {
    match call_connection::native_answer_call(call_connection) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeSendVideoStatus(env:             JNIEnv,
                                                                                  _object:         JObject,
                                                                                  call_connection: jlong,
                                                                                  enabled:         jboolean) {
    match call_connection::native_send_video_status(call_connection, enabled == JNI_TRUE) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeAddIceCandidate(env:             JNIEnv,
                                                                                  _object:         JObject,
                                                                                  call_connection: jlong,
                                                                                  sdp_mid:         JString,
                                                                                  sdp_mline_index: jint,
                                                                                  sdp:             JString) {

    match call_connection::native_add_ice_candidate(&env, call_connection,
                                                    sdp_mid, sdp_mline_index, sdp) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }

}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern fn Java_org_signal_ringrtc_CallConnection_nativeSendBusy(env:             JNIEnv,
                                                                           _object:         JObject,
                                                                           call_connection: jlong,
                                                                           recipient:       JObject,
                                                                           call_id:         jlong) {
    match call_connection::native_send_busy(&env, call_connection,
                                            recipient, call_id) {
        Ok(v) => v,
        Err(e) => {
            error::throw_error(&env, e);
        },
    }
}

