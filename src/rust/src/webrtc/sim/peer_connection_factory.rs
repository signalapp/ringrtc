//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;
use crate::webrtc::peer_connection_factory::RffiIceServer;
use crate::webrtc::sim::media::{
    RffiAudioTrack, RffiVideoSource, RffiVideoTrack, FAKE_AUDIO_TRACK, FAKE_VIDEO_SOURCE,
    FAKE_VIDEO_TRACK,
};
use crate::webrtc::sim::peer_connection::RffiPeerConnection;
use crate::webrtc::sim::peer_connection_observer::RffiPeerConnectionObserver;
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr::copy_nonoverlapping;

pub type RffiPeerConnectionFactoryOwner = u32;

impl webrtc::RefCounted for RffiPeerConnectionFactoryOwner {}

pub static FAKE_PEER_CONNECTION_FACTORY: RffiPeerConnectionFactoryOwner = 10;

pub type RffiPeerConnectionFactoryInterface = u32;

pub type RffiCertificate = u32;

pub static FAKE_CERTIFICATE: RffiCertificate = 11;

pub type RffiAudioDeviceModule = u32;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createPeerConnectionFactory(
    _use_new_audio_device_module: bool,
    _use_injectable_network: bool,
) -> webrtc::ptr::OwnedRc<RffiPeerConnectionFactoryOwner> {
    info!("Rust_createPeerConnectionFactory()");
    webrtc::ptr::OwnedRc::from_ptr(&FAKE_PEER_CONNECTION_FACTORY)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createPeerConnectionFactoryWrapper(
    _interface: webrtc::ptr::BorrowedRc<RffiPeerConnectionFactoryInterface>,
) -> webrtc::ptr::OwnedRc<RffiPeerConnectionFactoryOwner> {
    panic!("no interface to wrap in sim!")
}

#[allow(non_snake_case, clippy::missing_safety_doc, clippy::too_many_arguments)]
pub unsafe fn Rust_createPeerConnection(
    _factory: *const RffiPeerConnectionFactoryOwner,
    _observer: *const RffiPeerConnectionObserver,
    _certificate: *const RffiCertificate,
    _hide_ip: bool,
    _ice_server: RffiIceServer,
    _outgoing_audio_track: *const RffiAudioTrack,
    _outgoing_video_track: *const RffiVideoTrack,
    _enable_dtls: bool,
    _enable_rtp_data_channel: bool,
) -> webrtc::ptr::OwnedRc<RffiPeerConnection> {
    info!("Rust_createPeerConnection()");
    webrtc::ptr::OwnedRc::from_ptr(Box::leak(Box::new(RffiPeerConnection::new())))
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createAudioTrack(
    _factory: webrtc::ptr::BorrowedRc<RffiPeerConnectionFactoryOwner>,
) -> webrtc::ptr::OwnedRc<RffiAudioTrack> {
    info!("Rust_createVideoSource()");
    webrtc::ptr::OwnedRc::from_ptr(&FAKE_AUDIO_TRACK)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createVideoSource(
    _factory: *const RffiPeerConnectionFactoryOwner,
) -> *const RffiVideoSource {
    info!("Rust_createVideoSource()");
    &FAKE_VIDEO_SOURCE
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createVideoTrack(
    _factory: *const RffiPeerConnectionFactoryOwner,
    _source: *const RffiVideoSource,
) -> *const RffiVideoTrack {
    info!("Rust_createVideoTrack()");
    &FAKE_VIDEO_TRACK
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_generateCertificate() -> *const RffiCertificate {
    info!("Rust_generateCertificate()");
    &FAKE_CERTIFICATE
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_computeCertificateFingerprintSha256(
    _cert: *const RffiCertificate,
    _fingerprint: *mut [u8; 32],
) -> bool {
    info!("Rust_computeCertificateFingerprintSha256()");
    true
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioPlayoutDevices(_factory: *const RffiPeerConnectionFactoryOwner) -> i16 {
    1
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioPlayoutDeviceName(
    _factory: *const RffiPeerConnectionFactoryOwner,
    index: u16,
    out_name: *mut c_char,
    out_uuid: *mut c_char,
) -> i32 {
    if index != 0 {
        return -1;
    }
    copy_to_c_buffer("FakeSpeaker", out_name);
    copy_to_c_buffer("FakeSpeakerUuid", out_uuid);
    0
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setAudioPlayoutDevice(
    _factory: *const RffiPeerConnectionFactoryOwner,
    index: u16,
) -> bool {
    index == 0
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioRecordingDevices(
    _factory: *const RffiPeerConnectionFactoryOwner,
) -> i16 {
    1
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioRecordingDeviceName(
    _factory: *const RffiPeerConnectionFactoryOwner,
    index: u16,
    out_name: *mut c_char,
    out_uuid: *mut c_char,
) -> i32 {
    if index != 0 {
        return -1;
    }
    copy_to_c_buffer("FakeMicrophone", out_name);
    copy_to_c_buffer("FakeMicrophoneUuid", out_uuid);
    0
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setAudioRecordingDevice(
    _factory: *const RffiPeerConnectionFactoryOwner,
    index: u16,
) -> bool {
    index == 0
}

unsafe fn copy_to_c_buffer(string: &str, dest: *mut c_char) {
    let bytes = CString::new(string).unwrap();
    copy_nonoverlapping(bytes.as_ptr(), dest, string.len() + 1)
}
