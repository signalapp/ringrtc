//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC FFI Peer Connection Interface

use std::os::raw::c_char;

use crate::webrtc::data_channel::{
    RffiDataChannelInit,
};

use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver,
    RffiSessionDescriptionInterface,
    RffiSetSessionDescriptionObserver,
};

/// Incomplete type for C++ PeerConnectionInterface.
#[repr(C)]
pub struct RffiPeerConnectionInterface { _private: [u8; 0] }

/// Incomplete type for C++ DataChannelInterface.
#[repr(C)]
pub struct RffiDataChannelInterface { _private: [u8; 0] }


extern {
    pub fn Rust_createOffer(pc_interface: *const RffiPeerConnectionInterface,
                            csd_observer: *const RffiCreateSessionDescriptionObserver);

    pub fn Rust_setLocalDescription(pc_interface: *const RffiPeerConnectionInterface,
                                    ssd_observer: *const RffiSetSessionDescriptionObserver,
                                    desc: *const RffiSessionDescriptionInterface);

    pub fn Rust_createAnswer(pc_interface: *const RffiPeerConnectionInterface,
                             csd_observer: *const RffiCreateSessionDescriptionObserver);

    pub fn Rust_setRemoteDescription(pc_interface: *const RffiPeerConnectionInterface,
                                     ssd_observer: *const RffiSetSessionDescriptionObserver,
                                     desc:         *const RffiSessionDescriptionInterface);

    pub fn Rust_createDataChannel(pc_interface: *const RffiPeerConnectionInterface,
                                  label:        *const c_char,
                                  config:       *const RffiDataChannelInit)
                                  -> *const RffiDataChannelInterface;

    pub fn Rust_addIceCandidate(pc_interface:    *const RffiPeerConnectionInterface,
                                sdp_mid:         *const c_char,
                                sdp_mline_index: i32,
                                sdp:             *const c_char) -> bool;
}
