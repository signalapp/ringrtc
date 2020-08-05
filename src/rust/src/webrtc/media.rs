//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use std::fmt;
use std::marker::Send;

use crate::core::util::CppObject;

#[cfg(feature = "native")]
use crate::core::util::RustObject;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::media;
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count;

#[cfg(feature = "sim")]
use crate::webrtc::sim::media;
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count;

pub use media::RffiMediaStreamInterface;

/// Rust wrapper around WebRTC C++ MediaStreamInterface object.
pub struct MediaStream {
    /// Pointer to C++ webrtc::MediaStreamInterface object.
    rffi_ms_interface: *const RffiMediaStreamInterface,
}

// Send and Sync needed to share *const pointer types across threads.
unsafe impl Send for MediaStream {}
unsafe impl Sync for MediaStream {}

impl fmt::Display for MediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ms_interface: {:p}", self.rffi_ms_interface)
    }
}

impl fmt::Debug for MediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Default for MediaStream {
    fn default() -> Self {
        Self {
            rffi_ms_interface: std::ptr::null(),
        }
    }
}

impl Drop for MediaStream {
    fn drop(&mut self) {
        if !self.rffi_ms_interface.is_null() {
            ref_count::release_ref(self.rffi_ms_interface as CppObject);
        }
    }
}

impl MediaStream {
    /// Create new MediaStream object from C++ MediaStreamInterface.
    pub fn new(rffi_ms_interface: *const media::RffiMediaStreamInterface) -> Self {
        Self { rffi_ms_interface }
    }

    /// Return inner C++ MediaStreamInterface pointer.
    pub fn rffi_interface(&self) -> *const media::RffiMediaStreamInterface {
        self.rffi_ms_interface
    }

    /// Take ownership of the MediaStreamInterface pointer.
    pub fn own_rffi_interface(&mut self) -> *const media::RffiMediaStreamInterface {
        let rffi_ms_interface = self.rffi_ms_interface;
        self.rffi_ms_interface = std::ptr::null();
        rffi_ms_interface
    }

    #[cfg(feature = "native")]
    pub fn first_video_track(&self) -> Option<VideoTrack> {
        let track_rffi = unsafe { media::Rust_getFirstVideoTrack(self.rffi_ms_interface) };
        if track_rffi.is_null() {
            return None;
        }
        Some(VideoTrack::new(track_rffi))
    }
}

/// Rust wrapper around WebRTC C++ AudioTrackInterface object.
#[cfg(any(feature = "native", feature = "sim"))]
pub struct AudioTrack {
    rffi: *const media::RffiAudioTrackInterface,
}

#[cfg(any(feature = "native", feature = "sim"))]
impl AudioTrack {
    pub fn new(rffi: *const media::RffiAudioTrackInterface) -> Self {
        Self { rffi }
    }

    pub fn rffi(&self) -> *const media::RffiAudioTrackInterface {
        self.rffi
    }

    pub fn set_enabled(&self, enabled: bool) {
        unsafe { media::Rust_setAudioTrackEnabled(self.rffi, enabled) }
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl fmt::Display for AudioTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AudioSource: {:p}", self.rffi)
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl fmt::Debug for AudioTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl Drop for AudioTrack {
    fn drop(&mut self) {
        ref_count::release_ref(self.rffi as CppObject);
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl Clone for AudioTrack {
    fn clone(&self) -> Self {
        ref_count::add_ref(self.rffi as CppObject);
        Self::new(self.rffi)
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
unsafe impl Send for AudioTrack {}

#[cfg(any(feature = "native", feature = "sim"))]
unsafe impl Sync for AudioTrack {}

/// cbindgen:prefix-with-name=true
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoRotation {
    None         = 0,
    Clockwise90  = 90,
    Clockwise180 = 180,
    Clockwise270 = 270,
}

#[repr(C)]
#[derive(Debug)]
pub struct VideoFrameMetadata {
    width:    u32,
    height:   u32,
    rotation: VideoRotation,
}

impl VideoFrameMetadata {
    pub fn apply_rotation(&self) -> Self {
        match self.rotation {
            VideoRotation::None | VideoRotation::Clockwise180 => Self {
                width:    self.width,
                height:   self.height,
                rotation: VideoRotation::None,
            },
            VideoRotation::Clockwise90 | VideoRotation::Clockwise270 => Self {
                width:    self.height,
                height:   self.width,
                rotation: VideoRotation::None,
            },
        }
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
pub struct VideoFrame {
    metadata:    VideoFrameMetadata,
    // Owns this
    rffi_buffer: *const media::RffiVideoFrameBuffer,
}

#[cfg(any(feature = "native", feature = "sim"))]
impl VideoFrame {
    pub fn width(&self) -> u32 {
        self.metadata.width
    }

    pub fn height(&self) -> u32 {
        self.metadata.height
    }

    pub fn apply_rotation(self) -> Self {
        if self.metadata.rotation == VideoRotation::None {
            return self;
        }
        Self {
            metadata:    self.metadata.apply_rotation(),
            rffi_buffer: unsafe {
                media::Rust_copyAndRotateVideoFrameBuffer(self.rffi_buffer, self.metadata.rotation)
            },
        }
    }

    pub fn from_owned_buffer(
        metadata: VideoFrameMetadata,
        rffi_buffer: *mut media::RffiVideoFrameBuffer,
    ) -> Self {
        Self {
            metadata,
            rffi_buffer,
        }
    }

    pub fn from_rgba(width: u32, height: u32, rgba_buffer: &[u8]) -> Self {
        Self {
            metadata:    VideoFrameMetadata {
                width,
                height,
                rotation: VideoRotation::None,
            },
            rffi_buffer: unsafe {
                media::Rust_createVideoFrameBufferFromRgba(width, height, rgba_buffer.as_ptr())
            },
        }
    }

    pub fn to_rgba(&self, rgba_buffer: &mut [u8]) {
        unsafe {
            media::Rust_convertVideoFrameBufferToRgba(self.rffi_buffer, rgba_buffer.as_mut_ptr())
        }
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl Drop for VideoFrame {
    fn drop(&mut self) {
        debug!("VideoFrame::drop()");
        if !self.rffi_buffer.is_null() {
            ref_count::release_ref(self.rffi_buffer as crate::core::util::CppObject);
        }
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl fmt::Display for VideoFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VideoFrame({}x{})", self.width(), self.height())
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
unsafe impl Send for VideoFrame {}

#[cfg(any(feature = "native", feature = "sim"))]
unsafe impl Sync for VideoFrame {}

/// Rust wrapper around WebRTC C++ VideoTrackSourceInterface object.
#[cfg(any(feature = "native", feature = "sim"))]
pub struct VideoSource {
    rffi: *const media::RffiVideoTrackSourceInterface,
}

#[cfg(any(feature = "native", feature = "sim"))]
impl VideoSource {
    pub fn new(rffi: *const media::RffiVideoTrackSourceInterface) -> Self {
        Self { rffi }
    }

    pub fn rffi(&self) -> *const media::RffiVideoTrackSourceInterface {
        self.rffi
    }

    pub fn push_frame(&self, frame: VideoFrame) {
        unsafe {
            media::Rust_pushVideoFrame(self.rffi, frame.rffi_buffer);
        }
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl fmt::Display for VideoSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VideoSource: {:p}", self.rffi)
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl fmt::Debug for VideoSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl Drop for VideoSource {
    fn drop(&mut self) {
        debug!("VideoSource::drop()");
        if !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as CppObject);
        }
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
impl Clone for VideoSource {
    fn clone(&self) -> Self {
        debug!("VideoSource::clone() {}", self.rffi as u64);
        if !self.rffi.is_null() {
            ref_count::add_ref(self.rffi as CppObject);
        }
        Self::new(self.rffi)
    }
}

#[cfg(any(feature = "native", feature = "sim"))]
unsafe impl Send for VideoSource {}

#[cfg(any(feature = "native", feature = "sim"))]
unsafe impl Sync for VideoSource {}

/// Rust wrapper around WebRTC C++ VideoTrackInterface object.
#[cfg(feature = "native")]
pub struct VideoTrack {
    rffi: *const media::RffiVideoTrackInterface,
}

#[cfg(feature = "native")]
impl VideoTrack {
    pub fn new(rffi: *const media::RffiVideoTrackInterface) -> Self {
        Self { rffi }
    }

    pub fn rffi(&self) -> *const media::RffiVideoTrackInterface {
        self.rffi
    }

    pub fn add_sink(&self, sink: &dyn VideoSink) {
        let sink_ptr = Box::into_raw(Box::new(RffiVideoSink { sink })) as RustObject;
        let cbs_ptr = &VideoSinkCallbacks {
            onVideoFrame: video_sink_OnVideoFrame,
        } as *const VideoSinkCallbacks as CppObject;
        unsafe {
            media::Rust_addVideoSink(self.rffi, sink_ptr, cbs_ptr);
        }
    }
}

#[cfg(feature = "native")]
pub trait VideoSink {
    // If not enabled, ignore new frames and clear old frames.
    fn set_enabled(&self, enabled: bool);
    // Warning: this video frame's output buffer is shared with a video decoder,
    // and so must quickly be dropped (by copying it and dropping the original)
    // or the video decoder will soon stall and video will be choppy.
    fn on_video_frame(&self, frame: VideoFrame);
}

// Since dyn pointers aren't safe to send over FFI (they are double-sized fat pointers),
// we have to wrap them in something that can have a normal pointer.
#[cfg(feature = "native")]
struct RffiVideoSink<'sink> {
    sink: &'sink dyn VideoSink,
}

#[repr(C)]
#[allow(non_snake_case)]
#[cfg(feature = "native")]
struct VideoSinkCallbacks {
    onVideoFrame:
        extern "C" fn(*mut RffiVideoSink, VideoFrameMetadata, *mut media::RffiVideoFrameBuffer),
}

#[allow(non_snake_case)]
#[cfg(feature = "native")]
extern "C" fn video_sink_OnVideoFrame(
    rffi_sink: *mut RffiVideoSink,
    metadata: VideoFrameMetadata,
    rffi_buffer: *mut media::RffiVideoFrameBuffer,
) {
    let rffi_sink = unsafe { &*rffi_sink };
    rffi_sink
        .sink
        .on_video_frame(VideoFrame::from_owned_buffer(metadata, rffi_buffer));
}

#[cfg(feature = "native")]
impl fmt::Display for VideoTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VideoTrack: {:p}", self.rffi)
    }
}

#[cfg(feature = "native")]
impl fmt::Debug for VideoTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[cfg(feature = "native")]
impl Drop for VideoTrack {
    fn drop(&mut self) {
        ref_count::release_ref(self.rffi as CppObject);
    }
}

#[cfg(feature = "native")]
impl Clone for VideoTrack {
    fn clone(&self) -> Self {
        ref_count::add_ref(self.rffi as CppObject);
        Self::new(self.rffi)
    }
}

#[cfg(feature = "native")]
unsafe impl Send for VideoTrack {}

#[cfg(feature = "native")]
unsafe impl Sync for VideoTrack {}
