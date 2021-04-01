//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

pub use crate::webrtc::media::VideoRotation;

pub type RffiMediaStream = u32;

pub type RffiAudioTrack = u32;

pub static FAKE_AUDIO_TRACK: u32 = 21;

pub type RffiVideoSource = u32;

pub static FAKE_VIDEO_SOURCE: RffiVideoSource = 22;

pub type RffiVideoTrack = u32;

pub static FAKE_VIDEO_TRACK: RffiVideoSource = 23;

pub type RffiVideoFrameBuffer = u32;

pub static FAKE_VIDEO_FRAME_BUFFER: RffiVideoFrameBuffer = 24;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getTrackIdAsUint32(_track: *const RffiVideoTrack) -> u32 {
    info!("Rust_getTrackIdAsUint32()");
    1
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setAudioTrackEnabled(_track: *const RffiAudioTrack, _enabled: bool) {
    info!("Rust_setAudioTrackEnabled()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setVideoTrackEnabled(_track: *const RffiVideoTrack, _enabled: bool) {
    info!("Rust_setVideoTrackEnabled()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setVideoTrackContentHint(_track: *const RffiVideoTrack, _is_screenshare: bool) {
    info!("Rust_setVideoTrackContentHint()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getFirstVideoTrack(_stream: *const RffiMediaStream) -> *const RffiVideoTrack {
    info!("Rust_getFirstVideoTrack()");
    &FAKE_VIDEO_TRACK
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_pushVideoFrame(
    _source: *const RffiVideoSource,
    _buffer: *const RffiVideoFrameBuffer,
) {
    info!("Rust_pushVideoFrame()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createVideoFrameBufferFromRgba(
    _width: u32,
    _height: u32,
    _rgba_buffer: *const u8,
) -> *const RffiVideoFrameBuffer {
    info!("Rust_createVideoFrameBufferFromRgba()");
    &FAKE_VIDEO_FRAME_BUFFER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_convertVideoFrameBufferToRgba(
    _buffer: *const RffiVideoFrameBuffer,
    _rgba_buffer: *mut u8,
) {
    info!("Rust_convertVideoFrameBufferToRgba()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_copyAndRotateVideoFrameBuffer(
    _buffer: *const RffiVideoFrameBuffer,
    _rotation: VideoRotation,
) -> *const RffiVideoFrameBuffer {
    info!("Rust_copyAndRotateVideoFrameBuffer()");
    &FAKE_VIDEO_FRAME_BUFFER
}
