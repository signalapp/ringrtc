//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

#[cfg(feature = "native")]
use crate::core::util::{CppObject, RustObject};
pub use crate::webrtc::media::VideoRotation;

/// Incomplete type for WebRTC C++ MediaStreamInterface.
#[repr(C)]
pub struct RffiMediaStreamInterface {
    _private: [u8; 0],
}

/// Incomplete type for C++ AudioTrackInterface.
#[repr(C)]
#[cfg(feature = "native")]
pub struct RffiAudioTrackInterface {
    _private: [u8; 0],
}

/// Incomplete type for C++ VideoTrackSourceInterface.
#[repr(C)]
#[cfg(feature = "native")]
pub struct RffiVideoTrackSourceInterface {
    _private: [u8; 0],
}

/// Incomplete type for C++ VideoTrackInterface.
#[repr(C)]
#[cfg(feature = "native")]
pub struct RffiVideoTrackInterface {
    _private: [u8; 0],
}

/// Incomplete type for C++ webrtc::VideoFrameBuffer.
#[repr(C)]
#[cfg(feature = "native")]
pub struct RffiVideoFrameBuffer {
    _private: [u8; 0],
}

#[cfg(feature = "native")]
extern "C" {
    pub fn Rust_setAudioTrackEnabled(track: *const RffiAudioTrackInterface, enabled: bool);
    pub fn Rust_getFirstVideoTrack(
        stream: *const RffiMediaStreamInterface,
    ) -> *const RffiVideoTrackInterface;
    pub fn Rust_addVideoSink(track: *const RffiVideoTrackInterface, obj: RustObject, cb: CppObject);
    pub fn Rust_pushVideoFrame(
        source: *const RffiVideoTrackSourceInterface,
        buffer: *const RffiVideoFrameBuffer,
    );
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
