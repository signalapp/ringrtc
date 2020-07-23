//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation Data Channel Interface.

use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

use libc::{size_t, strdup};

use crate::webrtc::data_channel_observer::RffiDataChannelObserverInterface;
use crate::webrtc::peer_connection::RffiDataChannelInterface;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_dataChannelSend(
    _dc_interface: *const RffiDataChannelInterface,
    _buffer: *const u8,
    _len: size_t,
    _binary: bool,
) -> bool {
    info!("Rust_dataChannelSend(): ");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_registerDataChannelObserver(
    _dc_interface: *const RffiDataChannelInterface,
    _dc_observer: *const RffiDataChannelObserverInterface,
) {
    info!("Rust_registerDataChannelObserver(): ");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_unregisterDataChannelObserver(
    _dc_interface: *const RffiDataChannelInterface,
    _dc_observer: *const RffiDataChannelObserverInterface,
) {
    info!("Rust_unregisterDataChannelObserver(): ");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_dataChannelGetLabel(
    _dc_interface: *const RffiDataChannelInterface,
) -> *const c_char {
    info!("Rust_dataChannelGetLabel(): ");
    match CString::new("test-data-channel-proto") {
        Ok(cstr) => strdup(cstr.as_ptr()),
        Err(_) => ptr::null(),
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_dataChannelIsReliable(_dc_interface: *const RffiDataChannelInterface) -> bool {
    info!("Rust_dataChannelIsReliable(): ");
    false
}
