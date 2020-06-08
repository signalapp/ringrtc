//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use crate::webrtc::peer_connection_factory::RffiIceServer;
use crate::webrtc::sim::media::{
    RffiAudioTrackInterface,
    RffiVideoTrackSourceInterface,
    FAKE_AUDIO_TRACK,
    FAKE_VIDEO_SOURCE,
};
use crate::webrtc::sim::peer_connection::RffiPeerConnectionInterface;
use crate::webrtc::sim::peer_connection_observer::RffiPeerConnectionObserverInterface;

pub type RffiPeerConnectionFactoryInterface = u32;

pub static FAKE_PEER_CONNECTION_FACTORY: RffiPeerConnectionFactoryInterface = 10;

pub type RffiCertificate = u32;

pub static FAKE_CERTIFICATE: RffiCertificate = 11;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createPeerConnectionFactory(
    _use_injectable_network: bool,
) -> *const RffiPeerConnectionFactoryInterface {
    info!("Rust_createPeerConnectionFactory()");
    &FAKE_PEER_CONNECTION_FACTORY
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createPeerConnection(
    _factory: *const RffiPeerConnectionFactoryInterface,
    _observer: *const RffiPeerConnectionObserverInterface,
    _certificate: *const RffiCertificate,
    _hide_ip: bool,
    _ice_server: RffiIceServer,
    _outgoing_audio: *const RffiAudioTrackInterface,
    _outgoing_video: *const RffiVideoTrackSourceInterface,
) -> *const RffiPeerConnectionInterface {
    info!("Rust_createPeerConnection()");
    &RffiPeerConnectionInterface::new()
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createAudioTrack(
    _factory: *const RffiPeerConnectionFactoryInterface,
) -> *const RffiAudioTrackInterface {
    info!("Rust_createVideoSource()");
    &FAKE_AUDIO_TRACK
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createVideoSource(
    _factory: *const RffiPeerConnectionFactoryInterface,
) -> *const RffiVideoTrackSourceInterface {
    info!("Rust_createVideoSource()");
    &FAKE_VIDEO_SOURCE
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_generateCertificate() -> *const RffiCertificate {
    info!("Rust_generateCertificate()");
    &FAKE_CERTIFICATE
}
