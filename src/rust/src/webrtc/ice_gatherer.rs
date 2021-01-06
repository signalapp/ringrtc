//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC IceGatherer Interface.

use crate::core::util::CppObject;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ice_gatherer::RffiIceGatherer;
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count;

#[cfg(feature = "sim")]
use crate::webrtc::sim::ice_gatherer::RffiIceGatherer;
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count;

/// Rust wrapper around WebRTC C++ IceGatherer object.
#[derive(Debug)]
pub struct IceGatherer {
    rffi: *const RffiIceGatherer,
}

// Implementing Sync and Sync required to share raw *const pointer
// across threads
unsafe impl Sync for IceGatherer {}
unsafe impl Send for IceGatherer {}

impl Drop for IceGatherer {
    fn drop(&mut self) {
        if !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as CppObject);
            self.rffi = std::ptr::null();
        }
    }
}

impl IceGatherer {
    /// Create a new Rust IceGatherer object from a WebRTC C++ IceGatherer object.
    pub fn new(rffi: *const RffiIceGatherer) -> Self {
        Self { rffi }
    }

    pub fn rffi(&self) -> *const RffiIceGatherer {
        self.rffi
    }
}
