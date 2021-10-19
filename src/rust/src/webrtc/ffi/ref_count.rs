//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Wrapper around rtc::RefCountInterface

use crate::webrtc;

#[repr(C)]
pub struct RffiRefCounted {
    _private: [u8; 0],
}

impl webrtc::RefCounted for RffiRefCounted {}

/// Decrements the ref count.
/// If the ref count goes to zero, the object is deleted.
pub fn dec<T: webrtc::ptr::RefCounted>(rc: webrtc::ptr::OwnedRc<T>) {
    unsafe {
        Rust_decRc(webrtc::ptr::OwnedRc::from_ptr(
            rc.as_ptr() as *const RffiRefCounted
        ));
    }
}

/// Increments the ref count.
/// The borrowed RC becomes an owned RC.
/// # Safety
/// The pointee must still be alive
pub unsafe fn inc<T: webrtc::ptr::RefCounted>(
    rc: webrtc::ptr::BorrowedRc<T>,
) -> webrtc::ptr::OwnedRc<T> {
    Rust_incRc(webrtc::ptr::BorrowedRc::from_ptr(
        rc.as_ptr() as *const RffiRefCounted
    ));
    webrtc::ptr::OwnedRc::from_ptr(rc.as_ptr())
}

extern "C" {
    fn Rust_decRc(rc: webrtc::ptr::OwnedRc<RffiRefCounted>);
    fn Rust_incRc(rc: webrtc::ptr::BorrowedRc<RffiRefCounted>);
}
