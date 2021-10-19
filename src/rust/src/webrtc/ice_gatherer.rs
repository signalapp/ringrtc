//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC IceGatherer Interface.

use crate::webrtc;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ice_gatherer::RffiIceGatherer;
#[cfg(feature = "sim")]
use crate::webrtc::sim::ice_gatherer::RffiIceGatherer;

/// Rust wrapper around WebRTC C++ IceGatherer object.
#[derive(Debug)]
pub struct IceGatherer {
    rffi: webrtc::Arc<RffiIceGatherer>,
}

impl IceGatherer {
    /// Create a new Rust IceGatherer object from a WebRTC C++ IceGatherer object.
    pub fn new(rffi: webrtc::Arc<RffiIceGatherer>) -> Self {
        Self { rffi }
    }

    pub fn rffi(&self) -> &webrtc::Arc<RffiIceGatherer> {
        &self.rffi
    }
}
