//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation Peer Connection Interface

use std::net::SocketAddr;
use std::os::raw::c_char;
use std::sync::{Arc, Mutex};

use crate::core::platform::PlatformItem;
use crate::webrtc;
use crate::webrtc::media::RffiAudioEncoderConfig;
use crate::webrtc::rtp;
use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver,
    RffiSessionDescription,
    RffiSetSessionDescriptionObserver,
};
use crate::webrtc::network::RffiIpPort;
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

impl webrtc::RefCounted for RffiPeerConnection {}

impl RffiPeerConnection {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RffiPeerConnectionState {
                local_description_set:  false,
                remote_description_set: false,
                outgoing_audio_enabled: true,
                rtp_packet_sink:        None,
                removed_ice_candidates: vec![],
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

    fn set_incoming_media_enabled(&self, _enabled: bool) {
        let _state = self.state.lock().unwrap();
        // Do nothing; the sim implementation doesn't use this.
    }

    pub fn set_rtp_packet_sink(&self, rtp_packet_sink: BoxedRtpPacketSink) {
        let mut state = self.state.lock().unwrap();
        state.rtp_packet_sink = Some(rtp_packet_sink);
    }

    fn remove_ice_candidates(&self, removed_addresses: impl Iterator<Item=SocketAddr>) {
        self.state.lock().unwrap().removed_ice_candidates.extend(removed_addresses);
    }

    pub fn removed_ice_candidates(&self) -> Vec<SocketAddr> {
        let state = self.state.lock().unwrap();
        state.removed_ice_candidates.clone()
    }
}

pub type BoxedRtpPacketSink = Box<dyn Fn(rtp::Header, &[u8]) + Send + 'static>;

struct RffiPeerConnectionState {
    local_description_set:  bool,
    remote_description_set: bool,
    outgoing_audio_enabled: bool,
    rtp_packet_sink:        Option<BoxedRtpPacketSink>,
    removed_ice_candidates: Vec<SocketAddr>,
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
pub unsafe fn Rust_setAudioPlayoutEnabled(
    _peer_connection: *const RffiPeerConnection,
    _enabled: bool,
) {
    info!("Rust_setAudioPlayoutEnabled:");
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
pub unsafe fn Rust_addIceCandidateFromServer(
    _peer_connection: *const RffiPeerConnection,
    _ip: RffiIp,
    _port: u16,
    _tcp: bool,
) -> bool {
    info!("Rust_addIceCandidateFromServer():");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_removeIceCandidates(
    peer_connection: *const RffiPeerConnection,
    removed_addresses_data: *const RffiIpPort,
    removed_addresses_len: usize,
) -> bool {
    info!("Rust_removeIceCandidates():");
    let removed_addresses = std::slice::from_raw_parts(removed_addresses_data, removed_addresses_len).iter().map(|ip_port| ip_port.into());
    (*peer_connection).remove_ice_candidates(removed_addresses);
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
pub unsafe fn Rust_setSendBitrates(
    _peer_connection: *const RffiPeerConnection,
    _min_bitrate_bps: i32,
    _start_bitrate_bps: i32,
    _max_bitrate_bps: i32,
) {
    info!("Rust_setSendBitrates:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_sendRtp(
    peer_connection: *const RffiPeerConnection,
    pt: rtp::PayloadType,
    seqnum: rtp::SequenceNumber,
    timestamp: rtp::Timestamp,
    ssrc: rtp::Ssrc,
    payload_data: *const u8,
    payload_size: usize,
) -> bool {
    info!("Rust_sendRtp:");
    let state = (*peer_connection).state.lock().unwrap();
    if let Some(rtp_packet_sink) = &state.rtp_packet_sink {
        let header = rtp::Header {
            pt,
            seqnum,
            timestamp,
            ssrc,
        };
        let payload = std::slice::from_raw_parts(payload_data, payload_size as usize);
        rtp_packet_sink(header, payload);
    }
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_receiveRtp(
    _peer_connection: *const RffiPeerConnection,
    _pt: rtp::PayloadType,
) -> bool {
    info!("Rust_receiveRtp:");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_configureAudioEncoders(
    _peer_connection: *const RffiPeerConnection,
    _config: *const RffiAudioEncoderConfig,
) {
    info!("Rust_configureAudioEncoders:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_closePeerConnection(_peer_connection: *const RffiPeerConnection) {
    info!("Rust_closePeerConnection:");
}
