//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;
use crate::webrtc::ffi::media::{RffiAudioTrack, RffiVideoSource, RffiVideoTrack};
use crate::webrtc::ffi::peer_connection::RffiPeerConnection;
use crate::webrtc::ffi::peer_connection_observer::RffiPeerConnectionObserver;
#[cfg(feature = "simnet")]
use crate::webrtc::injectable_network::RffiInjectableNetwork;
use crate::webrtc::peer_connection_factory::RffiIceServer;
#[cfg(feature = "native")]
use std::os::raw::c_char;

/// Incomplete type for C++ PeerConnectionFactoryOwner.
#[repr(C)]
pub struct RffiPeerConnectionFactoryOwner {
    _private: [u8; 0],
}

impl webrtc::RefCounted for RffiPeerConnectionFactoryOwner {}

/// Incomplete type for C++ PeerConnectionFactoryInterface.
#[repr(C)]
pub struct RffiPeerConnectionFactoryInterface {
    _private: [u8; 0],
}

impl webrtc::RefCounted for RffiPeerConnectionFactoryInterface {}

/// Incomplete type for C++ RTCCertificate.
#[repr(C)]
pub struct RffiCertificate {
    _private: [u8; 0],
}

/// Incomplete type for C++ AudioDeviceModule.
#[repr(C)]
pub struct RffiAudioDeviceModule {
    _private: [u8; 0],
}

// See "class AudioDeviceModule : public rtc::RefCountInterface"
// in webrtc/modules/audio_device/include/audio_device.h
impl webrtc::RefCounted for RffiAudioDeviceModule {}

extern "C" {
    pub fn Rust_createPeerConnectionFactory(
        use_new_audio_device_module: bool,
        use_injectable_network: bool,
    ) -> *const RffiPeerConnectionFactoryOwner;
    pub fn Rust_createPeerConnectionFactoryWrapper(
        factory: *const RffiPeerConnectionFactoryInterface,
    ) -> *const RffiPeerConnectionFactoryOwner;
    #[cfg(feature = "simnet")]
    pub fn Rust_getInjectableNetwork(
        factory: *const RffiPeerConnectionFactoryOwner,
    ) -> *const RffiInjectableNetwork;
    #[allow(clippy::too_many_arguments)]
    pub fn Rust_createPeerConnection(
        factory: *const RffiPeerConnectionFactoryOwner,
        observer: *const RffiPeerConnectionObserver,
        certificate: *const RffiCertificate,
        hide_ip: bool,
        ice_server: RffiIceServer,
        outgoing_audio_track: *const RffiAudioTrack,
        outgoing_video_track: *const RffiVideoTrack,
        enable_dtls: bool,
        enable_rtp_data_channel: bool,
    ) -> *const RffiPeerConnection;
    pub fn Rust_createAudioTrack(
        factory: *const RffiPeerConnectionFactoryOwner,
    ) -> *const RffiAudioTrack;
    pub fn Rust_createVideoSource(
        factory: *const RffiPeerConnectionFactoryOwner,
    ) -> *const RffiVideoSource;
    // PeerConnectionFactory::CreateVideoTrack increments the refcount on source,
    // So there's no need to retain extra references to the source.
    pub fn Rust_createVideoTrack(
        factory: *const RffiPeerConnectionFactoryOwner,
        source: *const RffiVideoSource,
    ) -> *const RffiVideoTrack;
    pub fn Rust_generateCertificate() -> *const RffiCertificate;
    pub fn Rust_computeCertificateFingerprintSha256(
        cert: *const RffiCertificate,
        fingerprint: *mut [u8; 32],
    ) -> bool;
    #[cfg(feature = "native")]
    pub fn Rust_getAudioPlayoutDevices(factory: *const RffiPeerConnectionFactoryOwner) -> i16;
    #[cfg(feature = "native")]
    pub fn Rust_getAudioPlayoutDeviceName(
        factory: *const RffiPeerConnectionFactoryOwner,
        index: u16,
        out_name: *mut c_char,
        out_uuid: *mut c_char,
    ) -> i32;
    #[cfg(feature = "native")]
    pub fn Rust_setAudioPlayoutDevice(
        factory: *const RffiPeerConnectionFactoryOwner,
        index: u16,
    ) -> bool;
    #[cfg(feature = "native")]
    pub fn Rust_getAudioRecordingDevices(factory: *const RffiPeerConnectionFactoryOwner) -> i16;
    #[cfg(feature = "native")]
    pub fn Rust_getAudioRecordingDeviceName(
        factory: *const RffiPeerConnectionFactoryOwner,
        index: u16,
        out_name: *mut c_char,
        out_uuid: *mut c_char,
    ) -> i32;
    #[cfg(feature = "native")]
    pub fn Rust_setAudioRecordingDevice(
        factory: *const RffiPeerConnectionFactoryOwner,
        index: u16,
    ) -> bool;
}
