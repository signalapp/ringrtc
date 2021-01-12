//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Data Channel

use libc::size_t;

use crate::webrtc::peer_connection::RffiDataChannel;

extern "C" {
    pub fn Rust_dataChannelSend(
        data_channel: *const RffiDataChannel,
        buffer: *const u8,
        len: size_t,
        binary: bool,
    ) -> bool;
}
