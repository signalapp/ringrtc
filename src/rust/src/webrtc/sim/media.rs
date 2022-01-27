//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;

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
pub unsafe fn Rust_getTrackIdAsUint32(_track: webrtc::ptr::BorrowedRc<RffiVideoTrack>) -> u32 {
    info!("Rust_getTrackIdAsUint32()");
    1
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setAudioTrackEnabled(
    _track: webrtc::ptr::BorrowedRc<RffiVideoTrack>,
    _enabled: bool,
) {
    info!("Rust_setAudioTrackEnabled()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setVideoTrackEnabled(
    _track: webrtc::ptr::BorrowedRc<RffiVideoTrack>,
    _enabled: bool,
) {
    info!("Rust_setVideoTrackEnabled()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setVideoTrackContentHint(
    _track: webrtc::ptr::BorrowedRc<RffiVideoTrack>,
    _is_screenshare: bool,
) {
    info!("Rust_setVideoTrackContentHint()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_pushVideoFrame(
    _source: webrtc::ptr::BorrowedRc<RffiVideoSource>,
    _buffer: webrtc::ptr::BorrowedRc<RffiVideoFrameBuffer>,
) {
    info!("Rust_pushVideoFrame()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_copyVideoFrameBufferFromI420(
    _width: u32,
    _height: u32,
    _src: webrtc::ptr::Borrowed<u8>,
) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer> {
    info!("Rust_copyVideoFrameBufferFromI420()");
    webrtc::ptr::OwnedRc::from_ptr(&FAKE_VIDEO_FRAME_BUFFER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_copyVideoFrameBufferFromNv12(
    _width: u32,
    _height: u32,
    _src: webrtc::ptr::Borrowed<u8>,
) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer> {
    info!("Rust_copyVideoFrameBufferFromNv12()");
    webrtc::ptr::OwnedRc::from_ptr(&FAKE_VIDEO_FRAME_BUFFER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_copyVideoFrameBufferFromRgba(
    _width: u32,
    _height: u32,
    _src: webrtc::ptr::Borrowed<u8>,
) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer> {
    info!("Rust_copyVideoFrameBufferFromRgba()");
    webrtc::ptr::OwnedRc::from_ptr(&FAKE_VIDEO_FRAME_BUFFER)
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_convertVideoFrameBufferToRgba(
    _buffer: webrtc::ptr::BorrowedRc<RffiVideoFrameBuffer>,
    _rgba_out: *mut u8,
) {
    info!("Rust_convertVideoFrameBufferToRgba()");
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_copyAndRotateVideoFrameBuffer(
    _buffer: webrtc::ptr::BorrowedRc<RffiVideoFrameBuffer>,
    _rotation: VideoRotation,
) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer> {
    info!("Rust_copyAndRotateVideoFrameBuffer()");
    webrtc::ptr::OwnedRc::from_ptr(&FAKE_VIDEO_FRAME_BUFFER)
}
