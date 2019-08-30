//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Call Connection Global interface functions.
//!
//! RingRTC interfaces, called by CallConnection Swift objects.

use std::ptr;

use std::ffi::c_void;

use crate::ios::logging::{
    IOSLogger,
    init_logging,
};

#[no_mangle]
#[allow(non_snake_case)]
/// Library initialization routine.
///
/// Sets up the logging infrastructure.
pub extern fn ringRtcInitialize(logObject: IOSLogger) -> *mut c_void {
    // Directly initialize the logging singleton.
    match init_logging(logObject) {
        Ok(_v) => {
            // Return non-null pointer to indicate success.
            1 as *mut c_void
        },
        Err(_e) => {
            ptr::null_mut()
        },
    }
}
