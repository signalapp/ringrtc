//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Utility helpers for iOS Application (Swift) access.

use std::ffi::c_void;

use libc::size_t;

/// Incomplete type for application's Swift/Objective-C peer connection factory object.
#[repr(C)]
pub struct AppPeerConnectionFactory { _private: [u8; 0] }

/// Incomplete type for application's Swift/Objective-C call connection object.
#[repr(C)]
pub struct AppCallConnection { _private: [u8; 0] }

/// Structure for passing buffers (such as strings) to Swift.
#[repr(C)]
pub struct IOSByteSlice {
    pub bytes: *const u8,
    pub len: size_t,
}

/// Structure for passing Ice Candidates to/from Swift.
#[repr(C)]
#[allow(non_snake_case)]
pub struct IOSIceCandidate {
    pub sdpMid: IOSByteSlice,
    pub sdpMLineIndex: i32,
    pub sdp: IOSByteSlice,
    // @note serverUrl is not supported.
}

/// Structure for passing multiple Ice Candidates to Swift.
#[repr(C)]
#[allow(non_snake_case)]
pub struct IOSIceCandidateArray {
    pub candidates: *const IOSIceCandidate,
    pub count: size_t,
}

/// Recipient object for interfacing with Swift.
#[repr(C)]
#[allow(non_snake_case)]
pub struct IOSRecipient {
    pub object: *mut c_void,
    pub destroy: extern fn(object: *mut c_void),
    pub onSendOffer: extern fn(object: *mut c_void, callId: u64, offer: IOSByteSlice),
    pub onSendAnswer: extern fn(object: *mut c_void, callId: u64, answer: IOSByteSlice),
    pub onSendIceCandidates: extern fn(object: *mut c_void, callId: u64, iceCandidate: *const IOSIceCandidateArray),
    pub onSendHangup: extern fn(object: *mut c_void, callId: u64),
    pub onSendBusy: extern fn(object: *mut c_void, callId: u64),
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for IOSRecipient {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for IOSRecipient {}

// Rust owns the recipient object from Swift. Drop it when it goes out of
// scope.
impl Drop for IOSRecipient {
    fn drop(&mut self) {
        (self.destroy)(self.object);
    }
}

/// Structure for passing common configuration options.
#[repr(C)]
#[allow(non_snake_case)]
pub struct IOSCallConfig {
    pub callId: u64,
    pub outBound: bool,
    pub recipient: IOSRecipient,
}
