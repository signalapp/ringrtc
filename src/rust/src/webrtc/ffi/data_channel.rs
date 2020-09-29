//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
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

    pub fn Rust_dataChannelIsReliable(data_channel: *const RffiDataChannel) -> bool;
}
