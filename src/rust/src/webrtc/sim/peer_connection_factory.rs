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
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr::copy_nonoverlapping;

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

#[allow(non_snake_case, clippy::missing_safety_doc, clippy::too_many_arguments)]
pub unsafe fn Rust_createPeerConnection(
    _factory: *const RffiPeerConnectionFactoryInterface,
    _observer: *const RffiPeerConnectionObserverInterface,
    _certificate: *const RffiCertificate,
    _hide_ip: bool,
    _ice_server: RffiIceServer,
    _outgoing_audio: *const RffiAudioTrackInterface,
    _outgoing_video: *const RffiVideoTrackSourceInterface,
    _enable_dtls: bool,
    _enable_rtp_data_channel: bool,
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

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioPlayoutDevices(
    _factory: *const RffiPeerConnectionFactoryInterface,
) -> i16 {
    1
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioPlayoutDeviceName(
    _factory: *const RffiPeerConnectionFactoryInterface,
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
    _factory: *const RffiPeerConnectionFactoryInterface,
    index: u16,
) -> bool {
    index == 0
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioRecordingDevices(
    _factory: *const RffiPeerConnectionFactoryInterface,
) -> i16 {
    1
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getAudioRecordingDeviceName(
    _factory: *const RffiPeerConnectionFactoryInterface,
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
    _factory: *const RffiPeerConnectionFactoryInterface,
    index: u16,
) -> bool {
    index == 0
}

unsafe fn copy_to_c_buffer(string: &str, dest: *mut c_char) {
    let bytes = CString::new(string).unwrap();
    copy_nonoverlapping(bytes.as_ptr(), dest, string.len() + 1)
}
