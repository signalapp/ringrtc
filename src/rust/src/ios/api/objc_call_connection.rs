//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Call Connection interface functions.
//!
//! RingRTC interfaces, called by CallConnection Swift objects.

use std::slice;
use std::str;
use std::ptr;

use libc::size_t;

use std::ffi::c_void;

use crate::ios::call_connection;
use crate::ios::ios_util::*;
use crate::ios::call_connection_observer::IOSObserver;

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcClose(callConnection: *mut c_void) -> *mut c_void {
    match call_connection::native_close_call_connection(callConnection as jlong) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callConnection
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcCreateCallConnectionObserver(observer: IOSObserver,
                                                    callId: i64) -> *mut c_void {
    match call_connection::native_create_call_connection_observer(observer, callId) {
        Ok(v) => {
            v as *mut c_void
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcSendOffer(callConnection: *mut c_void,
                            appCallConnection: *mut c_void) -> *mut c_void {
    match call_connection::native_send_offer(callConnection as jlong,
                                             appCallConnection as jlong) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callConnection
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcReceivedAnswer(callConnection: *mut c_void,
                                             bytes: *const u8,
                                               len: size_t) -> *mut c_void {
    // Build the Rust string.
    let answer_bytes = unsafe {
        slice::from_raw_parts(bytes, len as usize)
    };
    
    match str::from_utf8(answer_bytes) {
        Ok(session_desc) => {
            match call_connection::native_handle_offer_answer(callConnection as jlong,
                                                              session_desc) {
                Ok(_v) => {
                    // Return the object reference back as indication of success.
                    callConnection
                },
                Err(_e) => {
                    ptr::null_mut()
                },
            }
        },
        Err(_e) => {
            ptr::null_mut()
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcReceivedOffer(callConnection: *mut c_void,
                                appCallConnection: *mut c_void,
                                            bytes: *const u8,
                                              len: size_t) -> *mut c_void {
    // Build the Rust string.
    let offer_bytes = unsafe {
        slice::from_raw_parts(bytes, len as usize)
    };
    
    match str::from_utf8(offer_bytes) {
        Ok(session_desc) => {
            match call_connection::native_accept_offer(callConnection as jlong,
                                                       appCallConnection as jlong,
                                                       session_desc) {
                Ok(_v) => {
                    // Return the object reference back as indication of success.
                    callConnection
                },
                Err(_e) => {
                    ptr::null_mut()
                },
            }
        },
        Err(_e) => {
            ptr::null_mut()
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcHangup(callConnection: *mut c_void) -> *mut c_void {
    match call_connection::native_hang_up(callConnection as jlong) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callConnection
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcAccept(callConnection: *mut c_void) -> *mut c_void {
    match call_connection::native_answer_call(callConnection as jlong) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callConnection
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcSendVideoStatus(callConnection: *mut c_void,
                                            enabled: bool) -> *mut c_void {
    match call_connection::native_send_video_status(callConnection as jlong,
                                                    enabled) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callConnection
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcReceivedIceCandidate(callConnection: *mut c_void,
                                                sdpBytes: *const u8,
                                                  sdpLen: size_t,
                                               lineIndex: i32,
                                             sdpMidBytes: *const u8,
                                               sdpMidLen: size_t) -> *mut c_void {
    // Build the Rust strings.
    let sdp_bytes = unsafe {
        slice::from_raw_parts(sdpBytes, sdpLen as usize)
    };

    let sdp_string = match str::from_utf8(sdp_bytes) {
        Ok(sdp_desc) => {
            sdp_desc
        },
        Err(_e) => {
            ""
        }
    };

    let sdp_mid_bytes = unsafe {
        slice::from_raw_parts(sdpMidBytes, sdpMidLen as usize)
    };

    let sdp_mid_string = match str::from_utf8(sdp_mid_bytes) {
        Ok(sdp_mid_desc) => {
            sdp_mid_desc
        },
        Err(_e) => {
            ""
        }
    };

    debug!("ringRtcReceivedIceCandidate:");
    debug!("  sdp:        {}", sdp_string);
    debug!("  sdp_mid:    {}", sdp_mid_string);
    debug!("  line_index: {}", lineIndex);

    if sdp_string.is_empty() || sdp_mid_string.is_empty() {
        return ptr::null_mut();
    }

    match call_connection::native_add_ice_candidate(callConnection as jlong,
                                                    sdp_mid_string,
                                                    lineIndex,
                                                    sdp_string) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callConnection
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcSendBusy(callConnection: *mut c_void,
                                      callId: i64) -> *mut c_void {
    match call_connection::native_send_busy(callConnection as jlong, callId) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callConnection
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}
