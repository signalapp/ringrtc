//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Call Connection Factory interface functions.
//!
//! RingRTC interfaces, called by CallConnection Swift objects.

use std::ptr;

use std::ffi::c_void;

use crate::ios::call_connection_factory;
use crate::ios::call_connection_factory::IOSCallConnectionFactory;
use crate::ios::call_connection_observer::IOSCallConnectionObserver;
use crate::ios::ios_util::*;

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcCreateCallConnectionFactory(appCallConnectionFactory: *mut c_void) -> *mut c_void {
    match call_connection_factory::native_create_call_connection_factory(appCallConnectionFactory as *mut AppPeerConnectionFactory) {
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
pub extern fn ringRtcFreeFactory(factory: *mut c_void) -> *mut c_void
{
    match call_connection_factory::native_free_factory(factory as *mut IOSCallConnectionFactory) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            factory
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern fn ringRtcCreateCallConnection(callConnectionFactory: *mut c_void,
                                              appCallConnection: *mut c_void,
                                                     callConfig: IOSCallConfig,
                                         callConnectionObserver: *mut c_void,
                                                      rtcConfig: *mut c_void,
                                                 rtcConstraints: *mut c_void) -> *mut c_void {
    match call_connection_factory::native_create_call_connection(callConnectionFactory as *mut IOSCallConnectionFactory,
                                                                 appCallConnection as *mut AppCallConnection,
                                                                 callConfig,
                                                                 callConnectionObserver as *mut IOSCallConnectionObserver,
                                                                 rtcConfig,
                                                                 rtcConstraints) {
        Ok(v) => {
            v as *mut c_void
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}
