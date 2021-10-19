//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Data Channel

use bytes::BytesMut;

use crate::common::Result;
use crate::error::RingRtcError;

use crate::webrtc;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::data_channel as dc;
#[cfg(feature = "sim")]
use crate::webrtc::sim::data_channel as dc;

use crate::webrtc::peer_connection::RffiDataChannel;

/// Rust wrapper around WebRTC C++ DataChannel object.
#[derive(Debug)]
pub struct DataChannel {
    rffi: webrtc::Arc<RffiDataChannel>,
}

impl DataChannel {
    pub fn new(rffi: webrtc::Arc<RffiDataChannel>) -> Self {
        Self { rffi }
    }

    /// Send data via the DataChannel.
    pub fn send_data(&self, bytes: &BytesMut) -> Result<()> {
        let buffer = webrtc::ptr::Borrowed::from_ptr(bytes.as_ptr());

        // Setting Binary to true relies on a custom change in WebRTC.
        let result =
            unsafe { dc::Rust_dataChannelSend(self.rffi.as_borrowed(), buffer, bytes.len(), true) };

        if result {
            Ok(())
        } else {
            Err(RingRtcError::DataChannelSend.into())
        }
    }
}
