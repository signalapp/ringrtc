//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Data Channel

use libc::size_t;

use crate::webrtc::{self, peer_connection::RffiDataChannel};

extern "C" {
    pub fn Rust_dataChannelSend(
        dc: webrtc::ptr::BorrowedRc<RffiDataChannel>,
        buffer: webrtc::ptr::Borrowed<u8>,
        len: size_t,
        binary: bool,
    ) -> bool;
}
