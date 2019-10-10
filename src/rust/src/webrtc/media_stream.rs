//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Media Stream Interface.

use std::fmt;
use std::marker::Send;
use std::ptr;

use crate::core::util::CppObject;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count;

#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count;

/// Incomplete type for WebRTC C++ MediaStreamInterface.
#[repr(C)]
pub struct RffiMediaStreamInterface { _private: [u8; 0] }

/// Rust wrapper around WebRTC C++ MediaStreamInterface object.
pub struct MediaStream
{
    /// Pointer to C++ webrtc::MediaStreamInterface object.
    rffi_ms_interface: *const RffiMediaStreamInterface,
}

// Send and Sync needed to share *const pointer types across threads.
unsafe impl Send for MediaStream {}
unsafe impl Sync for MediaStream {}

impl fmt::Display for MediaStream
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ms_interface: {:p}", self.rffi_ms_interface)
    }
}

impl fmt::Debug for MediaStream
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Default for MediaStream
{
    fn default() -> Self {
        Self {
            rffi_ms_interface: ptr::null(),
        }
    }
}

impl Drop for MediaStream {
    fn drop(&mut self) {
        if !self.rffi_ms_interface.is_null() {
            ref_count::release_ref(self.rffi_ms_interface as CppObject);
        }
    }
}

impl MediaStream
{
    /// Create new MediaStream object from C++ MediaStreamInterface.
    pub fn new(rffi_ms_interface: *const RffiMediaStreamInterface) -> Self {
        Self {
            rffi_ms_interface,
        }
    }

    /// Return inner C++ MediaStreamInterface pointer.
    pub fn rffi_interface(&self) -> *const RffiMediaStreamInterface {
        self.rffi_ms_interface
    }

    /// Take ownership of the MediaStreamInterface pointer.
    pub fn own_rffi_interface(&mut self) -> *const RffiMediaStreamInterface {
        let rffi_ms_interface = self.rffi_ms_interface;
        self.rffi_ms_interface = ptr::null();
        rffi_ms_interface
    }

}
