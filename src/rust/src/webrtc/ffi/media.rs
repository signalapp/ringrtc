//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;
pub use crate::webrtc::media::VideoRotation;

/// Incomplete type for WebRTC C++ MediaStream.
#[repr(C)]
pub struct RffiMediaStream {
    _private: [u8; 0],
}

// See "class MediaStreamInterface : public rtc::RefCountInterface"
// in webrtc/api/media_stream_interface.h
impl webrtc::RefCounted for RffiMediaStream {}

/// Incomplete type for C++ AudioTrack.
#[repr(C)]
#[allow(dead_code)]
pub struct RffiAudioTrack {
    _private: [u8; 0],
}

// See "class MediaStreamTrackInterface : public rtc::RefCountInterface"
// and "class AudioTrackInterface: public MediaStreamTrackInterface"
// in webrtc/api/media_stream_interface.h
impl webrtc::RefCounted for RffiAudioTrack {}

/// Incomplete type for C++ VideoSource.
#[repr(C)]
#[allow(dead_code)]
pub struct RffiVideoSource {
    _private: [u8; 0],
}

// See "class MediaSourceInterface : public rtc::RefCountInterface"
// and "class VideoSourceInterface: public MediaSourceInterface"
// in webrtc/api/media_stream_interface.h
impl webrtc::RefCounted for RffiVideoSource {}

/// Incomplete type for C++ VideoTrack.
#[repr(C)]
#[allow(dead_code)]
pub struct RffiVideoTrack {
    _private: [u8; 0],
}

// See "class MediaStreamTrackInterface : public rtc::RefCountInterface"
// and "class VideoTrackInterface: public MediaStreamTrackInterface"
// in webrtc/api/media_stream_interface.h
impl webrtc::RefCounted for RffiVideoTrack {}

/// Incomplete type for C++ webrtc::VideoFrameBuffer.
#[repr(C)]
pub struct RffiVideoFrameBuffer {
    _private: [u8; 0],
}

// See "class VideoFrameBuffer : public rtc::RefCountInterface"
// in webrtc/api/video/video_frame_buffer.h
impl webrtc::RefCounted for RffiVideoFrameBuffer {}

extern "C" {
    pub fn Rust_getTrackIdAsUint32(track: webrtc::ptr::BorrowedRc<RffiVideoTrack>) -> u32;
    pub fn Rust_setAudioTrackEnabled(track: webrtc::ptr::BorrowedRc<RffiAudioTrack>, enabled: bool);
    pub fn Rust_setVideoTrackEnabled(track: webrtc::ptr::BorrowedRc<RffiVideoTrack>, enabled: bool);
    pub fn Rust_setVideoTrackContentHint(
        track: webrtc::ptr::BorrowedRc<RffiVideoTrack>,
        is_screenshare: bool,
    );
    pub fn Rust_pushVideoFrame(
        source: webrtc::ptr::BorrowedRc<RffiVideoSource>,
        buffer: webrtc::ptr::BorrowedRc<RffiVideoFrameBuffer>,
    );
    pub fn Rust_copyVideoFrameBufferFromI420(
        width: u32,
        height: u32,
        src: webrtc::ptr::Borrowed<u8>,
    ) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer>;
    pub fn Rust_copyVideoFrameBufferFromNv12(
        width: u32,
        height: u32,
        src: webrtc::ptr::Borrowed<u8>,
    ) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer>;
    pub fn Rust_copyVideoFrameBufferFromRgba(
        width: u32,
        height: u32,
        src: webrtc::ptr::Borrowed<u8>,
    ) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer>;
    pub fn Rust_convertVideoFrameBufferToRgba(
        buffer: webrtc::ptr::BorrowedRc<RffiVideoFrameBuffer>,
        rgba_out: *mut u8,
    );
    pub fn Rust_copyAndRotateVideoFrameBuffer(
        buffer: webrtc::ptr::BorrowedRc<RffiVideoFrameBuffer>,
        rotation: VideoRotation,
    ) -> webrtc::ptr::OwnedRc<RffiVideoFrameBuffer>;
}
