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
use crate::webrtc::data_channel::RffiDataChannelInit;
use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver,
    RffiSessionDescriptionInterface,
    RffiSetSessionDescriptionObserver,
};
use crate::webrtc::sim::ice_gatherer::{RffiIceGathererInterface, FAKE_ICE_GATHERER};
use crate::webrtc::stats_observer::RffiStatsObserver;

/// Simulation type for PeerConnectionInterface.
#[derive(Clone)]
pub struct RffiPeerConnectionInterface {
    state: Arc<Mutex<RffiPeerConnectionState>>,
}

impl Default for RffiPeerConnectionInterface {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformItem for RffiPeerConnectionInterface {}

impl RffiPeerConnectionInterface {
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

    fn set_outgoing_audio_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if !(state.local_description_set && state.remote_description_set) {
            panic!("Can't Rust_setOutgoingAudioEnabled if you haven't received an answer yet.");
        }
        state.outgoing_audio_enabled = enabled;
    }

    pub fn outgoing_audio_enabled(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.outgoing_audio_enabled
    }

    fn set_incoming_rtp_enabled(&self, enabled: bool) {
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
pub type RffiDataChannelInterface = u32;

static FAKE_DC_INTERFACE: RffiDataChannelInterface = 9;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createOffer(
    _pc_interface: *const RffiPeerConnectionInterface,
    _csd_observer: *const RffiCreateSessionDescriptionObserver,
) {
    info!("Rust_createOffer():");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setLocalDescription(
    pc_interface: *const RffiPeerConnectionInterface,
    _ssd_observer: *const RffiSetSessionDescriptionObserver,
    _desc: *const RffiSessionDescriptionInterface,
) {
    info!("Rust_setLocalDescription():");
    (*pc_interface).set_local_description();
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createAnswer(
    _pc_interface: *const RffiPeerConnectionInterface,
    _csd_observer: *const RffiCreateSessionDescriptionObserver,
) {
    info!("Rust_createAnswer():");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setRemoteDescription(
    pc_interface: *const RffiPeerConnectionInterface,
    _ssd_observer: *const RffiSetSessionDescriptionObserver,
    _desc: *const RffiSessionDescriptionInterface,
) {
    info!("Rust_setRemoteDescription():");
    (*pc_interface).set_remote_description();
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setOutgoingAudioEnabled(
    pc_interface: *const RffiPeerConnectionInterface,
    enabled: bool,
) {
    info!("Rust_setOutgoingAudioEnabled({})", enabled);
    (*pc_interface).set_outgoing_audio_enabled(enabled);
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setIncomingRtpEnabled(
    pc_interface: *const RffiPeerConnectionInterface,
    enabled: bool,
) -> bool {
    info!("Rust_setIncomingRtpEnabled({})", enabled);
    (*pc_interface).set_incoming_rtp_enabled(enabled);
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createDataChannel(
    _pc_interface: *const RffiPeerConnectionInterface,
    _label: *const c_char,
    _config: *const RffiDataChannelInit,
) -> *const RffiDataChannelInterface {
    info!("Rust_createDataChannel():");
    &FAKE_DC_INTERFACE
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_addIceCandidate(
    _pc_interface: *const RffiPeerConnectionInterface,
    _sdp: *const c_char,
) -> bool {
    info!("Rust_addIceCandidate():");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createSharedIceGatherer(
    _pc_interface: *const RffiPeerConnectionInterface,
) -> *const RffiIceGathererInterface {
    info!("Rust_createSharedIceGatherer:");
    &FAKE_ICE_GATHERER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_useSharedIceGatherer(
    _pc_interface: *const RffiPeerConnectionInterface,
    _ice_gatherer: *const RffiIceGathererInterface,
) -> bool {
    info!("Rust_useSharedIceGatherer:");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getStats(
    _pc_interface: *const RffiPeerConnectionInterface,
    _stats_observer: *const RffiStatsObserver,
) {
    info!("Rust_getStats:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setMaxSendBitrate(
    _pc_interface: *const RffiPeerConnectionInterface,
    _max_bitrate_bps: i32,
) {
    info!("Rust_setMaxSendBitrate:");
}
