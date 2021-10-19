//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Simulation of Wrapper around rtc::RefCountInterface

use crate::webrtc;

/// # Safety
pub fn dec<T: webrtc::ptr::RefCounted>(_rc: webrtc::ptr::OwnedRc<T>) {
    info!("ref_count::dec()");
}

/// # Safety
pub unsafe fn inc<T: webrtc::ptr::RefCounted>(
    rc: webrtc::ptr::BorrowedRc<T>,
) -> webrtc::ptr::OwnedRc<T> {
    info!("ref_count::inc()");
    webrtc::ptr::OwnedRc::from_ptr(rc.as_ptr())
}
