//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation Data Channel Interface.

use libc::size_t;

use crate::webrtc::peer_connection::RffiDataChannel;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_dataChannelSend(
    _data_channel: *const RffiDataChannel,
    _buffer: *const u8,
    _len: size_t,
    _binary: bool,
) -> bool {
    info!("Rust_dataChannelSend(): ");
    true
}
