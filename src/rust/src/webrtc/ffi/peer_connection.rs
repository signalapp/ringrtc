//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Peer Connection Interface

use std::os::raw::c_char;

use crate::webrtc;
use crate::webrtc::ffi::ice_gatherer::RffiIceGatherer;
use crate::webrtc::media::RffiAudioEncoderConfig;
use crate::webrtc::network::{RffiIp, RffiIpPort};
use crate::webrtc::peer_connection::{RffiAudioLevel, RffiReceivedAudioLevel};
use crate::webrtc::rtp;
use crate::webrtc::sdp_observer::{
    RffiCreateSessionDescriptionObserver, RffiSessionDescription, RffiSetSessionDescriptionObserver,
};
use crate::webrtc::stats_observer::RffiStatsObserver;

/// Incomplete type for C++ PeerConnection.
#[repr(C)]
pub struct RffiPeerConnection {
    _private: [u8; 0],
}

// See "class PeerConnectionInterface: public rtc::RefCountInterface"
// in webrtc/api/peer_connection_interface.h
impl webrtc::RefCounted for RffiPeerConnection {}

extern "C" {
    pub fn Rust_createOffer(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        csd_observer: webrtc::ptr::BorrowedRc<RffiCreateSessionDescriptionObserver>,
    );

    pub fn Rust_setLocalDescription(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        ssd_observer: webrtc::ptr::BorrowedRc<RffiSetSessionDescriptionObserver>,
        local_description: webrtc::ptr::Owned<RffiSessionDescription>,
    );

    pub fn Rust_createAnswer(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        csd_observer: webrtc::ptr::BorrowedRc<RffiCreateSessionDescriptionObserver>,
    );

    pub fn Rust_setRemoteDescription(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        ssd_observer: webrtc::ptr::BorrowedRc<RffiSetSessionDescriptionObserver>,
        remote_description: webrtc::ptr::Owned<RffiSessionDescription>,
    );

    pub fn Rust_setOutgoingMediaEnabled(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        enabled: bool,
    );

    pub fn Rust_setIncomingMediaEnabled(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        enabled: bool,
    ) -> bool;

    pub fn Rust_setAudioPlayoutEnabled(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        enabled: bool,
    );

    pub fn Rust_setAudioRecordingEnabled(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        enabled: bool,
    );

    pub fn Rust_addIceCandidateFromSdp(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        sdp: webrtc::ptr::Borrowed<c_char>,
    ) -> bool;

    pub fn Rust_addIceCandidateFromServer(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        ip: RffiIp,
        port: u16,
        tcp: bool,
    ) -> bool;

    pub fn Rust_removeIceCandidates(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        removed_addresses_data: webrtc::ptr::Borrowed<RffiIpPort>,
        removed_addresses_len: usize,
    ) -> bool;

    pub fn Rust_createSharedIceGatherer(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
    ) -> webrtc::ptr::OwnedRc<RffiIceGatherer>;

    pub fn Rust_useSharedIceGatherer(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        ice_gatherer: webrtc::ptr::BorrowedRc<RffiIceGatherer>,
    ) -> bool;

    // The observer must live until it is called.
    pub fn Rust_getStats(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        stats_observer: webrtc::ptr::BorrowedRc<RffiStatsObserver>,
    );

    pub fn Rust_setSendBitrates(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        min_bitrate_bps: i32,
        start_bitrate_bps: i32,
        max_bitrate_bps: i32,
    );

    pub fn Rust_sendRtp(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        pt: rtp::PayloadType,
        seqnum: rtp::SequenceNumber,
        timestamp: rtp::Timestamp,
        ssrc: rtp::Ssrc,
        payload_data: webrtc::ptr::Borrowed<u8>,
        payload_size: usize,
    ) -> bool;

    pub fn Rust_receiveRtp(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        pt: rtp::PayloadType,
    ) -> bool;

    pub fn Rust_configureAudioEncoders(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        config: webrtc::ptr::Borrowed<RffiAudioEncoderConfig>,
    );

    pub fn Rust_getAudioLevels(
        peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>,
        captured_out: webrtc::ptr::Borrowed<RffiAudioLevel>,
        received_out: webrtc::ptr::Borrowed<RffiReceivedAudioLevel>,
        received_out_size: usize,
        received_size_out: webrtc::ptr::Borrowed<usize>,
    );

    pub fn Rust_closePeerConnection(peer_connection: webrtc::ptr::BorrowedRc<RffiPeerConnection>);
}
