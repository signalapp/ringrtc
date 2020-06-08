//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use crate::webrtc::ffi::media::{RffiAudioTrackInterface, RffiVideoTrackSourceInterface};
use crate::webrtc::ffi::peer_connection::RffiPeerConnectionInterface;
use crate::webrtc::ffi::peer_connection_observer::RffiPeerConnectionObserverInterface;
#[cfg(feature = "simnet")]
use crate::webrtc::injectable_network::RffiInjectableNetwork;
use crate::webrtc::peer_connection_factory::RffiIceServer;

/// Incomplete type for C++ PeerConnectionFactoryInterface.
#[repr(C)]
pub struct RffiPeerConnectionFactoryInterface {
    _private: [u8; 0],
}

/// Incomplete type for C++ RTCCerficate.
#[repr(C)]
pub struct RffiCertificate {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_createPeerConnectionFactory(
        use_injectable_network: bool,
    ) -> *const RffiPeerConnectionFactoryInterface;
    #[cfg(feature = "simnet")]
    pub fn Rust_getInjectableNetwork(
        factory: *const RffiPeerConnectionFactoryInterface,
    ) -> *const RffiInjectableNetwork;
    pub fn Rust_createPeerConnection(
        factory: *const RffiPeerConnectionFactoryInterface,
        observer: *const RffiPeerConnectionObserverInterface,
        certificate: *const RffiCertificate,
        hide_ip: bool,
        ice_server: RffiIceServer,
        outgoing_audio: *const RffiAudioTrackInterface,
        outgoing_video: *const RffiVideoTrackSourceInterface,
    ) -> *const RffiPeerConnectionInterface;
    pub fn Rust_createAudioTrack(
        factory: *const RffiPeerConnectionFactoryInterface,
    ) -> *const RffiAudioTrackInterface;
    pub fn Rust_createVideoSource(
        factory: *const RffiPeerConnectionFactoryInterface,
    ) -> *const RffiVideoTrackSourceInterface;
    pub fn Rust_generateCertificate() -> *const RffiCertificate;
}
