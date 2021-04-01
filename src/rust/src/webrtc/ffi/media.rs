//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

#[cfg(feature = "native")]
use crate::core::util::{CppObject, RustObject};
pub use crate::webrtc::media::VideoRotation;

/// Incomplete type for WebRTC C++ MediaStream.
#[repr(C)]
pub struct RffiMediaStream {
    _private: [u8; 0],
}

/// Incomplete type for C++ AudioTrack.
#[repr(C)]
#[allow(dead_code)]
pub struct RffiAudioTrack {
    _private: [u8; 0],
}

/// Incomplete type for C++ VideoSource.
#[repr(C)]
#[allow(dead_code)]
pub struct RffiVideoSource {
    _private: [u8; 0],
}

/// Incomplete type for C++ VideoTrack.
#[repr(C)]
#[allow(dead_code)]
pub struct RffiVideoTrack {
    _private: [u8; 0],
}

/// Incomplete type for C++ webrtc::VideoFrameBuffer.
#[repr(C)]
pub struct RffiVideoFrameBuffer {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_getTrackIdAsUint32(track: *const RffiVideoTrack) -> u32;
    pub fn Rust_setAudioTrackEnabled(track: *const RffiAudioTrack, enabled: bool);
    pub fn Rust_setVideoTrackEnabled(track: *const RffiVideoTrack, enabled: bool);
    pub fn Rust_setVideoTrackContentHint(track: *const RffiVideoTrack, is_screenshare: bool);
    pub fn Rust_getFirstVideoTrack(stream: *const RffiMediaStream) -> *const RffiVideoTrack;
    #[cfg(feature = "native")]
    pub fn Rust_addVideoSink(track: *const RffiVideoTrack, obj: RustObject, cb: CppObject);
    pub fn Rust_pushVideoFrame(source: *const RffiVideoSource, buffer: *const RffiVideoFrameBuffer);
    pub fn Rust_createVideoFrameBufferFromRgba(
        width: u32,
        height: u32,
        rgba_buffer: *const u8,
    ) -> *const RffiVideoFrameBuffer;
    pub fn Rust_convertVideoFrameBufferToRgba(
        buffer: *const RffiVideoFrameBuffer,
        rgba_buffer: *mut u8,
    );
    pub fn Rust_copyAndRotateVideoFrameBuffer(
        buffer: *const RffiVideoFrameBuffer,
        rotation: VideoRotation,
    ) -> *const RffiVideoFrameBuffer;
}
