//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation Peer Connection Interface

use std::net::SocketAddr;
use std::os::raw::c_char;
use std::sync::{Arc, Mutex};

use prost::Message;

use crate::core::platform::PlatformItem;
use crate::webrtc;
use crate::webrtc::media::RffiAudioEncoderConfig;
use crate::webrtc::network::RffiIpPort;
use crate::webrtc::peer_connection::{RffiAudioLevel, RffiReceivedAudioLevel};
use crate::webrtc::rtp;
use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver, RffiSessionDescription, RffiSetSessionDescriptionObserver,
};
use crate::webrtc::sim::ice_gatherer::{RffiIceGatherer, FAKE_ICE_GATHERER};
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
                local_description_set: false,
                remote_description_set: false,
                outgoing_audio_enabled: true,
                rtp_packet_sink: None,
                removed_ice_candidates: vec![],
                max_bitrate_bps: None,
                last_sent_rtp_data: None,
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

    fn remove_ice_candidates(&self, removed_addresses: impl Iterator<Item = SocketAddr>) {
        self.state
            .lock()
            .unwrap()
            .removed_ice_candidates
            .extend(removed_addresses);
    }

    pub fn removed_ice_candidates(&self) -> Vec<SocketAddr> {
        let state = self.state.lock().unwrap();
        state.removed_ice_candidates.clone()
    }

    pub fn max_bitrate_bps(&self) -> Option<i32> {
        let state = self.state.lock().unwrap();
        state.max_bitrate_bps
    }

    pub fn last_sent_rtp_message(&self) -> Option<crate::protobuf::rtp_data::Message> {
        let state = self.state.lock().unwrap();
        Some(
            crate::protobuf::rtp_data::Message::decode(&state.last_sent_rtp_data.as_deref()?[4..])
                .unwrap(),
        )
    }

    pub fn last_sent_max_bitrate_bps(&self) -> Option<u64> {
        self.last_sent_rtp_message()?
            .receiver_status?
            .max_bitrate_bps
    }
}

pub type BoxedRtpPacketSink = Box<dyn Fn(rtp::Header, &[u8]) + Send + 'static>;

struct RffiPeerConnectionState {
    local_description_set: bool,
    remote_description_set: bool,
    outgoing_audio_enabled: bool,
    rtp_packet_sink: Option<BoxedRtpPacketSink>,
    removed_ice_candidates: Vec<SocketAddr>,
    max_bitrate_bps: Option<i32>,
    last_sent_rtp_data: Option<Vec<u8>>,
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createOffer(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _csd_observer: webrtc::ptr::BorrowedRc<RffiCreateSessionDescriptionObserver>,
) {
    info!("Rust_createOffer():");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setLocalDescription(
    peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _ssd_observer: webrtc::ptr::BorrowedRc<RffiSetSessionDescriptionObserver>,
    _local_desc: webrtc::ptr::Owned<RffiSessionDescription>,
) {
    info!("Rust_setLocalDescription():");
    (*peer_connection.as_ptr()).set_local_description();
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createAnswer(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _csd_observer: webrtc::ptr::BorrowedRc<RffiCreateSessionDescriptionObserver>,
) {
    info!("Rust_createAnswer():");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setRemoteDescription(
    peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _ssd_observer: webrtc::ptr::BorrowedRc<RffiSetSessionDescriptionObserver>,
    _remote_desc: webrtc::ptr::Owned<RffiSessionDescription>,
) {
    info!("Rust_setRemoteDescription():");
    (*peer_connection.as_ptr()).set_remote_description();
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setOutgoingMediaEnabled(
    peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    enabled: bool,
) {
    info!("Rust_setOutgoingMediaEnabled({})", enabled);
    (*peer_connection.as_ptr()).set_outgoing_media_enabled(enabled);
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setIncomingMediaEnabled(
    peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    enabled: bool,
) -> bool {
    info!("Rust_setIncomingMediaEnabled({})", enabled);
    (*peer_connection.as_ptr()).set_incoming_media_enabled(enabled);
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setAudioPlayoutEnabled(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _enabled: bool,
) {
    info!("Rust_setAudioPlayoutEnabled:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setAudioRecordingEnabled(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _enabled: bool,
) {
    info!("Rust_setAudioRecordingEnabled:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_addIceCandidateFromSdp(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _sdp: webrtc::ptr::Borrowed<c_char>,
) -> bool {
    info!("Rust_addIceCandidateFromSdp():");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_addIceCandidateFromServer(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _ip: RffiIp,
    _port: u16,
    _tcp: bool,
) -> bool {
    info!("Rust_addIceCandidateFromServer():");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_removeIceCandidates(
    peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    removed_addresses_data: webrtc::ptr::Borrowed<RffiIpPort>,
    removed_addresses_len: usize,
) -> bool {
    info!("Rust_removeIceCandidates():");
    let removed_addresses =
        std::slice::from_raw_parts(removed_addresses_data.as_ptr(), removed_addresses_len)
            .iter()
            .map(|ip_port| ip_port.into());
    (*peer_connection.as_ptr()).remove_ice_candidates(removed_addresses);
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createSharedIceGatherer(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
) -> webrtc::ptr::OwnedRc<RffiIceGatherer> {
    info!("Rust_createSharedIceGatherer:");
    webrtc::ptr::OwnedRc::from_ptr(&FAKE_ICE_GATHERER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_useSharedIceGatherer(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _ice_gatherer: webrtc::ptr::BorrowedRc<RffiIceGatherer>,
) -> bool {
    info!("Rust_useSharedIceGatherer:");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getStats(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _stats_observer: webrtc::ptr::BorrowedRc<RffiStatsObserver>,
) {
    info!("Rust_getStats:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setSendBitrates(
    peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _min_bitrate_bps: i32,
    _start_bitrate_bps: i32,
    max_bitrate_bps: i32,
) {
    let mut state = (*peer_connection.as_ptr()).state.lock().unwrap();
    state.max_bitrate_bps = Some(max_bitrate_bps);
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_sendRtp(
    peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    pt: rtp::PayloadType,
    seqnum: rtp::SequenceNumber,
    timestamp: rtp::Timestamp,
    ssrc: rtp::Ssrc,
    payload_data: webrtc::ptr::Borrowed<u8>,
    payload_size: usize,
) -> bool {
    info!("Rust_sendRtp:");
    let mut state = (*peer_connection.as_ptr()).state.lock().unwrap();
    let payload = std::slice::from_raw_parts(payload_data.as_ptr(), payload_size);
    state.last_sent_rtp_data = Some(payload.to_vec());
    if let Some(rtp_packet_sink) = &state.rtp_packet_sink {
        let header = rtp::Header {
            pt,
            seqnum,
            timestamp,
            ssrc,
        };
        rtp_packet_sink(header, payload);
    }
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_receiveRtp(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _pt: rtp::PayloadType,
) -> bool {
    info!("Rust_receiveRtp:");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_configureAudioEncoders(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    _config: webrtc::ptr::Borrowed<RffiAudioEncoderConfig>,
) {
    info!("Rust_configureAudioEncoders:");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub fn Rust_getAudioLevels(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    captured_out: webrtc::ptr::Borrowed<RffiAudioLevel>,
    _received_out: webrtc::ptr::Borrowed<RffiReceivedAudioLevel>,
    _received_out_size: usize,
    received_size_out: webrtc::ptr::Borrowed<usize>,
) {
    info!("Rust_getAudioLevels:");
    unsafe {
        if let Some(captured_out) = captured_out.as_mut() {
            *captured_out = 0;
        }
        if let Some(received_size_out) = received_size_out.as_mut() {
            *received_size_out = 0;
        }
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_closePeerConnection(
    _peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
) {
    info!("Rust_closePeerConnection:");
}
