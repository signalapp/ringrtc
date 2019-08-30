//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Utility helpers for iOS Application (Swift) access.

use std::sync::Arc;
use std::ffi::c_void;

use libc::size_t;

use crate::common::Result;
use crate::core::util::{
    get_arc_from_ptr,
    get_arc_ptr_from_ptr,
    get_object_ref_from_ptr,
    get_object_from_ptr,
    ArcPtr,
};

/// For convenience, define jlong as a native i64 type for easier
/// compatibility with JNI implementation.
#[allow(non_camel_case_types)]
pub type jlong = i64;

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

// Structure for passing multiple Ice Candidates to Swift.
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
    pub onSendOffer: extern fn(object: *mut c_void, callId: i64, offer: IOSByteSlice),
    pub onSendAnswer: extern fn(object: *mut c_void, callId: i64, answer: IOSByteSlice),
    pub onSendIceCandidates: extern fn(object: *mut c_void, callId: i64, iceCandidate: *const IOSIceCandidateArray),
    pub onSendHangup: extern fn(object: *mut c_void, callId: i64),
    pub onSendBusy: extern fn(object: *mut c_void, callId: i64),
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
    pub callId: i64,
    pub outBound: bool,
    pub recipient: IOSRecipient,
}

/// Returns a Arc<T> from a jlong.
pub fn get_arc_from_jlong<T>(object: jlong) -> Result<Arc<T>> {
    get_arc_from_ptr(object as *mut T)
}

/// Returns a ArcPtr<T> from a jlong.
pub fn get_arc_ptr_from_jlong<T>(object: jlong) -> Result<ArcPtr<T>> {
    get_arc_ptr_from_ptr(object as *mut T)
}

/// Returns a &T from a jlong.
pub fn get_object_ref_from_jlong<T>(object: jlong) -> Result<&'static mut T> {
    get_object_ref_from_ptr(object as *mut T)
}

/// Returns a Box<T> from a jlong.
pub fn get_object_from_jlong<T>(object: jlong) -> Result<Box<T>> {
    get_object_from_ptr(object as *mut T)
}
