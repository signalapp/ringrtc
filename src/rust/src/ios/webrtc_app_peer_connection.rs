//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Application direct access functions for the
//! RTCPeerConnection object.

use std::ffi::c_void;

extern "C" {
    /// Create a 'native' WebRTC via iOS Application Call Connection,
    /// passing in a custom observer implemented by RingRTC.
    #[allow(non_snake_case)]
    pub fn appCreatePeerConnection(appFactory: *mut c_void,
                            appCallConnection: *mut c_void,
                                    rtcConfig: *mut c_void,
                               rtcConstraints: *mut c_void,
                               customObserver: *mut c_void) -> *mut c_void;
}
