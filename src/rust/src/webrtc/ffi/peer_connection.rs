//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC FFI Peer Connection Interface

use std::os::raw::c_char;

use crate::webrtc::ffi::ice_gatherer::RffiIceGatherer;
use crate::webrtc::ffi::peer_connection_observer::RffiPeerConnectionObserver;
use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver,
    RffiSessionDescription,
    RffiSetSessionDescriptionObserver,
};
use crate::webrtc::stats_observer::RffiStatsObserver;

/// Incomplete type for C++ PeerConnection.
#[repr(C)]
pub struct RffiPeerConnection {
    _private: [u8; 0],
}

/// Incomplete type for C++ DataChannelInterface.
#[repr(C)]
pub struct RffiDataChannel {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_createOffer(
        peer_connection: *const RffiPeerConnection,
        csd_observer: *const RffiCreateSessionDescriptionObserver,
    );

    pub fn Rust_setLocalDescription(
        peer_connection: *const RffiPeerConnection,
        ssd_observer: *const RffiSetSessionDescriptionObserver,
        local_description: *const RffiSessionDescription,
    );

    pub fn Rust_createAnswer(
        peer_connection: *const RffiPeerConnection,
        csd_observer: *const RffiCreateSessionDescriptionObserver,
    );

    pub fn Rust_setRemoteDescription(
        peer_connection: *const RffiPeerConnection,
        ssd_observer: *const RffiSetSessionDescriptionObserver,
        remote_description: *const RffiSessionDescription,
    );

    pub fn Rust_setOutgoingMediaEnabled(peer_connection: *const RffiPeerConnection, enabled: bool);

    pub fn Rust_setIncomingMediaEnabled(
        peer_connection: *const RffiPeerConnection,
        enabled: bool,
    ) -> bool;

    pub fn Rust_createSignalingDataChannel(
        peer_connection: *const RffiPeerConnection,
        pc_observer: *const RffiPeerConnectionObserver,
    ) -> *const RffiDataChannel;

    pub fn Rust_addIceCandidateFromSdp(
        peer_connection: *const RffiPeerConnection,
        sdp: *const c_char,
    ) -> bool;

    pub fn Rust_createSharedIceGatherer(
        peer_connection: *const RffiPeerConnection,
    ) -> *const RffiIceGatherer;

    pub fn Rust_useSharedIceGatherer(
        peer_connection: *const RffiPeerConnection,
        ice_gatherer: *const RffiIceGatherer,
    ) -> bool;

    pub fn Rust_getStats(
        peer_connection: *const RffiPeerConnection,
        stats_observer: *const RffiStatsObserver,
    );

    pub fn Rust_setMaxSendBitrate(peer_connection: *const RffiPeerConnection, max_bitrate_bps: i32);

    pub fn Rust_closePeerConnection(peer_connection: *const RffiPeerConnection);
}
