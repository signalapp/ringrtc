//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Data Channel

use std::fmt;
use std::fmt::Debug;
use std::ptr;

use bytes::BytesMut;

use crate::common::Result;
use crate::core::util::CppObject;
use crate::error::RingRtcError;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::{data_channel as dc, ref_count};

#[cfg(feature = "sim")]
use crate::webrtc::sim::{data_channel as dc, ref_count};

use crate::webrtc::peer_connection::RffiDataChannel;

/// Rust wrapper around WebRTC C++ DataChannel object.
pub struct DataChannel {
    rffi: *const RffiDataChannel,
}

impl Debug for DataChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.rffi.fmt(f)
    }
}

// Implementing Sync and Sync required to share raw *const pointer
// across threads
unsafe impl Sync for DataChannel {}
unsafe impl Send for DataChannel {}

impl Drop for DataChannel {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl DataChannel {
    /// # Safety
    ///
    /// Create a new Rust DataChannel object from a WebRTC C++ DataChannel object.
    pub unsafe fn new(rffi: *const RffiDataChannel) -> Self {
        Self { rffi }
    }

    /// Free resources related to the DataChannel object.
    pub fn dispose(&mut self) {
        if !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as CppObject);
            self.rffi = ptr::null();
        }
    }

    /// Send data via the DataChannel.
    pub fn send_data(&self, bytes: &BytesMut) -> Result<()> {
        let buffer: *const u8 = bytes.as_ptr();

        // Setting Binary to true relies on a custom change in WebRTC.
        let result = unsafe { dc::Rust_dataChannelSend(self.rffi, buffer, bytes.len(), true) };

        if result {
            Ok(())
        } else {
            Err(RingRtcError::DataChannelSend.into())
        }
    }
}
