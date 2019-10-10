//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation Peer Connection Interface

use std::os::raw::c_char;

use crate::webrtc::data_channel::{
    RffiDataChannelInit,
};

use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver,
    RffiSessionDescriptionInterface,
    RffiSetSessionDescriptionObserver,
};

/// Simulation type for PeerConnectionInterface.
pub type RffiPeerConnectionInterface = u32;

/// Simulation type for DataChannelInterface.
pub type RffiDataChannelInterface = u32;

static FAKE_DC_INTERFACE: u32 = 9;

#[allow(non_snake_case)]
pub unsafe fn Rust_createOffer(_pc_interface: *const RffiPeerConnectionInterface,
                               _csd_observer: *const RffiCreateSessionDescriptionObserver) {
    info!("Rust_createOffer():");
}

#[allow(non_snake_case)]
pub unsafe fn Rust_setLocalDescription(_pc_interface: *const RffiPeerConnectionInterface,
                                       _ssd_observer: *const RffiSetSessionDescriptionObserver,
                                       _desc: *const RffiSessionDescriptionInterface) {
    info!("Rust_setLocalDescription():");
}

#[allow(non_snake_case)]
pub unsafe fn Rust_createAnswer(_pc_interface: *const RffiPeerConnectionInterface,
                                _csd_observer: *const RffiCreateSessionDescriptionObserver) {
    info!("Rust_createAnswer():");
}

#[allow(non_snake_case)]
pub unsafe fn Rust_setRemoteDescription(_pc_interface: *const RffiPeerConnectionInterface,
                                        _ssd_observer: *const RffiSetSessionDescriptionObserver,
                                        _desc:         *const RffiSessionDescriptionInterface) {
    info!("Rust_setRemoteDescription():");
}

#[allow(non_snake_case)]
pub unsafe fn Rust_createDataChannel(_pc_interface: *const RffiPeerConnectionInterface,
                                     _label:        *const c_char,
                                     _config:       *const RffiDataChannelInit)
                                     -> *const RffiDataChannelInterface {
    info!("Rust_createDataChannel():");
    &FAKE_DC_INTERFACE
}

#[allow(non_snake_case)]
pub unsafe fn Rust_addIceCandidate(_pc_interface:    *const RffiPeerConnectionInterface,
                                   _sdp_mid:         *const c_char,
                                   _sdp_mline_index: i32,
                                   _sdp:             *const c_char) -> bool {
    info!("Rust_addIceCandidate():");
    true
}
