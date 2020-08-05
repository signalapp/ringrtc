//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use crate::core::util::{CppObject, RustObject};
pub use crate::webrtc::media::VideoRotation;

pub type RffiMediaStreamInterface = u32;

pub type RffiAudioTrackInterface = u32;

pub static FAKE_AUDIO_TRACK: u32 = 21;

pub type RffiVideoTrackSourceInterface = u32;

pub static FAKE_VIDEO_SOURCE: RffiVideoTrackSourceInterface = 22;

pub type RffiVideoTrackInterface = u32;

pub static FAKE_VIDEO_TRACK: RffiVideoTrackSourceInterface = 23;

pub type RffiVideoFrameBuffer = u32;

pub static FAKE_VIDEO_FRAME_BUFFER: RffiVideoFrameBuffer = 24;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setAudioTrackEnabled(_track: *const RffiAudioTrackInterface, _enabled: bool) {
    info!("Rust_setAudioTrackEnabled()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_getFisrtVideoTrack(
    _stream: *const RffiMediaStreamInterface,
) -> *const RffiVideoTrackInterface {
    info!("Rust_setAudioTrackEnabled()");
    &FAKE_VIDEO_TRACK
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_addVideoSink(
    _track: *const RffiVideoTrackInterface,
    _obj: RustObject,
    _cb: CppObject,
) {
    info!("Rust_addVideoSink()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_pushVideoFrame(
    _source: *const RffiVideoTrackSourceInterface,
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
    info!("Rust_rotateAndConvertVideoFrameBufferToRgba()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_copyAndRotateVideoFrameBuffer(
    _buffer: *const RffiVideoFrameBuffer,
    _rotation: VideoRotation,
) -> *const RffiVideoFrameBuffer {
    info!("Rust_createVideoFrameBufferWithRotationApplied()");
    &FAKE_VIDEO_FRAME_BUFFER
}
