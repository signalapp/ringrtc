//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation Peer Connection Interface

use std::os::raw::c_char;
use std::sync::{Arc, Mutex};

use crate::core::platform::PlatformItem;
use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver,
    RffiSessionDescription,
    RffiSetSessionDescriptionObserver,
};
use crate::webrtc::sim::ice_gatherer::{RffiIceGatherer, FAKE_ICE_GATHERER};
use crate::webrtc::sim::peer_connection_observer::RffiPeerConnectionObserver;
use crate::webrtc::stats_observer::RffiStatsObserver;

/// Simulation type for PeerConnection.
#[derive(Clone)]
pub struct RffiPeerConnection {
    state: Arc<Mutex<RffiPeerConnectionState>>,
}

impl Default for RffiPeerConnection {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RffiIp(u32);

impl From<std::net::IpAddr> for RffiIp {
    fn from(_ip: std::net::IpAddr) -> RffiIp {
        RffiIp(0)
    }
}

impl PlatformItem for RffiPeerConnection {}

impl RffiPeerConnection {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RffiPeerConnectionState {
                local_description_set:  false,
                remote_description_set: false,
                outgoing_audio_enabled: true,
                incoming_rtp_enabled:   true,
            })),
        }
    }

    fn set_local_description(&self) {
        let mut state = self.state.lock().unwrap();
        state.local_description_set = true;
    }

    fn set_remote_description(&self) {
        let mut state = self.state.lock().unwrap();
        state.remote_description_set = true;
    }

    fn set_outgoing_media_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if !(state.local_description_set && state.remote_description_set) {
            panic!("Can't Rust_setOutgoingMediaEnabled if you haven't received an answer yet.");
        }
        state.outgoing_audio_enabled = enabled;
    }

    pub fn outgoing_audio_enabled(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.outgoing_audio_enabled
    }

    fn set_incoming_media_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        state.incoming_rtp_enabled = enabled;
    }
}

struct RffiPeerConnectionState {
    local_description_set:  bool,
    remote_description_set: bool,
    outgoing_audio_enabled: bool,
    incoming_rtp_enabled:   bool,
}

/// Simulation type for DataChannelInterface.
pub type RffiDataChannel = u32;

static FAKE_DATA_CHANNEL: RffiDataChannel = 9;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createOffer(
    _peer_connection: *const RffiPeerConnection,
    _csd_observer: *const RffiCreateSessionDescriptionObserver,
) {
    info!("Rust_createOffer():");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setLocalDescription(
    peer_connection: *const RffiPeerConnection,
    _ssd_observer: *const RffiSetSessionDescriptionObserver,
    _local_desc: *const RffiSessionDescription,
) {
    info!("Rust_setLocalDescription():");
    (*peer_connection).set_local_description();
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createAnswer(
    _peer_connection: *const RffiPeerConnection,
    _csd_observer: *const RffiCreateSessionDescriptionObserver,
) {
    info!("Rust_createAnswer():");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setRemoteDescription(
    peer_connection: *const RffiPeerConnection,
    _ssd_observer: *const RffiSetSessionDescriptionObserver,
    _remote_desc: *const RffiSessionDescription,
) {
    info!("Rust_setRemoteDescription():");
    (*peer_connection).set_remote_description();
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setOutgoingMediaEnabled(
    peer_connection: *const RffiPeerConnection,
    enabled: bool,
) {
    info!("Rust_setOutgoingMediaEnabled({})", enabled);
    (*peer_connection).set_outgoing_media_enabled(enabled);
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setIncomingMediaEnabled(
    peer_connection: *const RffiPeerConnection,
    enabled: bool,
) -> bool {
    info!("Rust_setIncomingMediaEnabled({})", enabled);
    (*peer_connection).set_incoming_media_enabled(enabled);
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createSignalingDataChannel(
    _peer_connection: *const RffiPeerConnection,
    _pc_observer: *const RffiPeerConnectionObserver,
) -> *const RffiDataChannel {
    info!("Rust_createSignalingDataChannel():");
    &FAKE_DATA_CHANNEL
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_addIceCandidateFromSdp(
    _peer_connection: *const RffiPeerConnection,
    _sdp: *const c_char,
) -> bool {
    info!("Rust_addIceCandidateFromSdp():");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createSharedIceGatherer(
    _peer_connection: *const RffiPeerConnection,
) -> *const RffiIceGatherer {
    info!("Rust_createSharedIceGatherer:");
    &FAKE_ICE_GATHERER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_useSharedIceGatherer(
    _peer_connection: *const RffiPeerConnection,
    _ice_gatherer: *const RffiIceGatherer,
) -> bool {
    info!("Rust_useSharedIceGatherer:");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getStats(
    _peer_connection: *const RffiPeerConnection,
    _stats_observer: *const RffiStatsObserver,
) {
    info!("Rust_getStats:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setMaxSendBitrate(
    _peer_connection: *const RffiPeerConnection,
    _max_bitrate_bps: i32,
) {
    info!("Rust_setMaxSendBitrate:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_closePeerConnection(_peer_connection: *const RffiPeerConnection) {
    info!("Rust_closePeerConnection:");
}
