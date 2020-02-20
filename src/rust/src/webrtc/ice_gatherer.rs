//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC IceGatherer Interface.

use crate::core::util::CppObject;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ice_gatherer::RffiIceGathererInterface;
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count;

#[cfg(feature = "sim")]
use crate::webrtc::sim::ice_gatherer::RffiIceGathererInterface;
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count;

/// Rust wrapper around WebRTC C++ IceGatherer object.
#[derive(Debug)]
pub struct IceGatherer {
    ice_gatherer: *const RffiIceGathererInterface,
}

// Implementing Sync and Sync required to share raw *const pointer
// across threads
unsafe impl Sync for IceGatherer {}
unsafe impl Send for IceGatherer {}

impl Drop for IceGatherer {
    fn drop(&mut self) {
        if !self.ice_gatherer.is_null() {
            ref_count::release_ref(self.ice_gatherer as CppObject);
            self.ice_gatherer = std::ptr::null();
        }
    }
}

impl IceGatherer {
    /// Create a new Rust IceGatherer object from a WebRTC C++ IceGatherer object.
    pub fn new(ice_gatherer: *const RffiIceGathererInterface) -> Self {
        Self { ice_gatherer }
    }

    pub fn rffi(&self) -> *const RffiIceGathererInterface {
        self.ice_gatherer
    }
}
