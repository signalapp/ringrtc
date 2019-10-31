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

use crate::ios::call_connection_observer::IOSObserver;
use crate::ios::ios_platform;
use crate::ios::ios_platform::IOSCallConnection;
use crate::ios::ios_util::AppCallConnection;

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcClose(callConnection: *mut c_void) -> *mut c_void {
    match ios_platform::native_close_call_connection(callConnection as *mut IOSCallConnection) {
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
pub extern fn ringRtcDispose(callConnection: *mut c_void) -> *mut c_void {
    match ios_platform::native_dispose_call_connection(callConnection as *mut IOSCallConnection) {
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
                                                    callId: u64) -> *mut c_void {
    match ios_platform::native_create_call_connection_observer(observer, callId) {
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
    match ios_platform::native_send_offer(callConnection as *mut IOSCallConnection,
                                          appCallConnection as *mut AppCallConnection) {
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
            match ios_platform::native_handle_answer(callConnection as *mut IOSCallConnection,
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
            match ios_platform::native_handle_offer(callConnection as *mut IOSCallConnection,
                                                    appCallConnection as *mut AppCallConnection,
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
    match ios_platform::native_hang_up(callConnection as *mut IOSCallConnection) {
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
    match ios_platform::native_accept_call(callConnection as *mut IOSCallConnection) {
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
    match ios_platform::native_send_video_status(callConnection as *mut IOSCallConnection,
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

    match ios_platform::native_add_ice_candidate(callConnection as *mut IOSCallConnection,
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
                                     _callId: u64) -> *mut c_void {
    // TODO -- remove this call all together

    // Return the object reference back as indication of success.
    callConnection
}
