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
use std::os::raw::c_char;

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
    #[allow(clippy::too_many_arguments)]
    pub fn Rust_createPeerConnection(
        factory: *const RffiPeerConnectionFactoryInterface,
        observer: *const RffiPeerConnectionObserverInterface,
        certificate: *const RffiCertificate,
        hide_ip: bool,
        ice_server: RffiIceServer,
        outgoing_audio: *const RffiAudioTrackInterface,
        outgoing_video: *const RffiVideoTrackSourceInterface,
        enable_dtls: bool,
        enable_rtp_data_channel: bool,
    ) -> *const RffiPeerConnectionInterface;
    pub fn Rust_createAudioTrack(
        factory: *const RffiPeerConnectionFactoryInterface,
    ) -> *const RffiAudioTrackInterface;
    pub fn Rust_createVideoSource(
        factory: *const RffiPeerConnectionFactoryInterface,
    ) -> *const RffiVideoTrackSourceInterface;
    pub fn Rust_generateCertificate() -> *const RffiCertificate;
    pub fn Rust_getAudioPlayoutDevices(factory: *const RffiPeerConnectionFactoryInterface) -> i16;
    pub fn Rust_getAudioPlayoutDeviceName(
        factory: *const RffiPeerConnectionFactoryInterface,
        index: u16,
        out_name: *mut c_char,
        out_uuid: *mut c_char,
    ) -> i32;
    pub fn Rust_setAudioPlayoutDevice(
        factory: *const RffiPeerConnectionFactoryInterface,
        index: u16,
    ) -> bool;
    pub fn Rust_getAudioRecordingDevices(factory: *const RffiPeerConnectionFactoryInterface)
        -> i16;
    pub fn Rust_getAudioRecordingDeviceName(
        factory: *const RffiPeerConnectionFactoryInterface,
        index: u16,
        out_name: *mut c_char,
        out_uuid: *mut c_char,
    ) -> i32;
    pub fn Rust_setAudioRecordingDevice(
        factory: *const RffiPeerConnectionFactoryInterface,
        index: u16,
    ) -> bool;
}
