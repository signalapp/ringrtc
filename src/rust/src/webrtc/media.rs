//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
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

pub use media::{RffiAudioTrack, RffiMediaStream, RffiVideoTrack};

/// Rust wrapper around WebRTC C++ MediaStream object.
pub struct MediaStream {
    /// Pointer to C++ webrtc::MediaStreamInterface object.
    rffi: *const RffiMediaStream,
}

// Send and Sync needed to share *const pointer types across threads.
unsafe impl Send for MediaStream {}
unsafe impl Sync for MediaStream {}

impl fmt::Display for MediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rffi_media_stream: {:p}", self.rffi)
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
            rffi: std::ptr::null(),
        }
    }
}

impl Drop for MediaStream {
    fn drop(&mut self) {
        if !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as CppObject);
        }
    }
}

impl MediaStream {
    /// Create new MediaStream object from C++ MediaStream.
    pub fn new(rffi: *const media::RffiMediaStream) -> Self {
        Self { rffi }
    }

    /// Return inner C++ MediaStream pointer.
    pub fn rffi(&self) -> *const media::RffiMediaStream {
        self.rffi
    }

    /// Take ownership of the MediaStream pointer.
    pub fn take_rffi(mut self) -> *const media::RffiMediaStream {
        let rffi = self.rffi;
        self.rffi = std::ptr::null();
        rffi
    }

    pub fn first_video_track(&self) -> Option<VideoTrack> {
        let track_rffi = unsafe { media::Rust_getFirstVideoTrack(self.rffi) };
        if track_rffi.is_null() {
            return None;
        }
        Some(VideoTrack::owned(track_rffi))
    }
}

/// Rust wrapper around WebRTC C++ AudioTrackInterface object.
pub struct AudioTrack {
    rffi: *const media::RffiAudioTrack,
    // If owned, release ref count when Dropped
    owned: bool,
}

impl AudioTrack {
    pub fn unowned(rffi: *const media::RffiAudioTrack) -> Self {
        let owned = false;
        Self { rffi, owned }
    }

    pub fn owned(rffi: *const media::RffiAudioTrack) -> Self {
        let owned = true;
        Self { rffi, owned }
    }

    pub fn rffi(&self) -> *const media::RffiAudioTrack {
        self.rffi
    }

    pub fn set_enabled(&self, enabled: bool) {
        unsafe { media::Rust_setAudioTrackEnabled(self.rffi, enabled) }
    }
}

impl fmt::Display for AudioTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AudioSource: {:p}", self.rffi)
    }
}

impl fmt::Debug for AudioTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for AudioTrack {
    fn drop(&mut self) {
        if self.owned && !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as CppObject);
        }
    }
}

impl Clone for AudioTrack {
    fn clone(&self) -> Self {
        ref_count::add_ref(self.rffi as CppObject);
        Self::owned(self.rffi)
    }
}

unsafe impl Send for AudioTrack {}

unsafe impl Sync for AudioTrack {}

/// cbindgen:prefix-with-name=true
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoRotation {
    None = 0,
    Clockwise90 = 90,
    Clockwise180 = 180,
    Clockwise270 = 270,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VideoFrameMetadata {
    width: u32,
    height: u32,
    rotation: VideoRotation,
}

impl VideoFrameMetadata {
    pub fn apply_rotation(&self) -> Self {
        match self.rotation {
            VideoRotation::None | VideoRotation::Clockwise180 => Self {
                width: self.width,
                height: self.height,
                rotation: VideoRotation::None,
            },
            VideoRotation::Clockwise90 | VideoRotation::Clockwise270 => Self {
                width: self.height,
                height: self.width,
                rotation: VideoRotation::None,
            },
        }
    }
}

pub struct VideoFrame {
    metadata: VideoFrameMetadata,
    // Owns this
    rffi_buffer: *const media::RffiVideoFrameBuffer,
}

impl VideoFrame {
    pub fn metadata(&self) -> VideoFrameMetadata {
        self.metadata
    }

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
            metadata: self.metadata.apply_rotation(),
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
            metadata: VideoFrameMetadata {
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

impl Drop for VideoFrame {
    fn drop(&mut self) {
        debug!("VideoFrame::drop()");
        if !self.rffi_buffer.is_null() {
            ref_count::release_ref(self.rffi_buffer as crate::core::util::CppObject);
        }
    }
}

impl fmt::Display for VideoFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VideoFrame({}x{})", self.width(), self.height())
    }
}

impl fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

unsafe impl Send for VideoFrame {}

unsafe impl Sync for VideoFrame {}

/// Rust wrapper around WebRTC C++ VideoTrackSourceInterface object.
pub struct VideoSource {
    rffi: *const media::RffiVideoSource,
}

impl VideoSource {
    pub fn new(rffi: *const media::RffiVideoSource) -> Self {
        Self { rffi }
    }

    pub fn rffi(&self) -> *const media::RffiVideoSource {
        self.rffi
    }

    pub fn push_frame(&self, frame: VideoFrame) {
        unsafe {
            media::Rust_pushVideoFrame(self.rffi, frame.rffi_buffer);
        }
    }
}

impl fmt::Display for VideoSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VideoSource: {:p}", self.rffi)
    }
}

impl fmt::Debug for VideoSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for VideoSource {
    fn drop(&mut self) {
        debug!("VideoSource::drop()");
        if !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as CppObject);
        }
    }
}

impl Clone for VideoSource {
    fn clone(&self) -> Self {
        debug!("VideoSource::clone() {}", self.rffi as u64);
        if !self.rffi.is_null() {
            ref_count::add_ref(self.rffi as CppObject);
        }
        Self::new(self.rffi)
    }
}

unsafe impl Send for VideoSource {}

unsafe impl Sync for VideoSource {}

/// Rust wrapper around WebRTC C++ VideoTrackInterface object.
pub struct VideoTrack {
    rffi: *const media::RffiVideoTrack,
    // If owned, release ref count when Dropped
    owned: bool,
}

impl VideoTrack {
    pub fn unowned(rffi: *const media::RffiVideoTrack) -> Self {
        let owned = false;
        Self { rffi, owned }
    }

    pub fn owned(rffi: *const media::RffiVideoTrack) -> Self {
        let owned = true;
        Self { rffi, owned }
    }

    pub fn rffi(&self) -> *const media::RffiVideoTrack {
        self.rffi
    }

    pub fn set_enabled(&self, enabled: bool) {
        unsafe { media::Rust_setVideoTrackEnabled(self.rffi, enabled) }
    }

    pub fn set_content_hint(&self, is_screenshare: bool) {
        unsafe { media::Rust_setVideoTrackContentHint(self.rffi, is_screenshare) }
    }

    pub fn id(&self) -> Option<u32> {
        let id = unsafe { media::Rust_getTrackIdAsUint32(self.rffi) };
        if id == 0 {
            None
        } else {
            Some(id)
        }
    }

    #[cfg(feature = "native")]
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

#[cfg(feature = "native")]
#[repr(C)]
#[allow(non_snake_case)]
struct VideoSinkCallbacks {
    onVideoFrame:
        extern "C" fn(*mut RffiVideoSink, VideoFrameMetadata, *mut media::RffiVideoFrameBuffer),
}

#[cfg(feature = "native")]
#[allow(non_snake_case)]
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

impl fmt::Display for VideoTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VideoTrack: {:p}", self.rffi)
    }
}

impl fmt::Debug for VideoTrack {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for VideoTrack {
    fn drop(&mut self) {
        if self.owned && !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as crate::core::util::CppObject);
        }
    }
}

impl Clone for VideoTrack {
    fn clone(&self) -> Self {
        ref_count::add_ref(self.rffi as CppObject);
        Self::owned(self.rffi)
    }
}

unsafe impl Send for VideoTrack {}

unsafe impl Sync for VideoTrack {}

// Same as webrtc::AudioEncoder::Config in api/audio_codecs/audio_encoder.h.
// Very OPUS-specific
#[repr(C)]
#[derive(Clone, Debug)]
pub struct RffiAudioEncoderConfig {
    packet_size_ms: u32,

    bandwidth: i32,
    start_bitrate_bps: i32,
    min_bitrate_bps: i32,
    max_bitrate_bps: i32,
    complexity: i32,
    enable_vbr: i32,
    enable_dtx: i32,
    enable_fec: i32,
}

// A nice form of RffiAudioEncoderConfig
#[derive(Clone, Debug)]
pub struct AudioEncoderConfig {
    // AKA ptime or frame size
    // Valid sizes: 10, 20, 40, 60, 120
    // Default is 20ms
    pub packet_size_ms: u32,

    // Default in Auto
    pub bandwidth: AudioBandwidth,

    // Valid range: 500-192000
    // Default is to start at 40000 and move between 16000 and 40000.
    pub start_bitrate_bps: u16,
    pub min_bitrate_bps: u16,
    pub max_bitrate_bps: u16,
    // Valid range: 0-9 (9 must complex)
    // Default is 9
    pub complexity: u16,
    // Default is true.
    pub enable_cbr: bool,
    // Default in false.
    pub enable_dtx: bool,
    // Default in true.
    pub enable_fec: bool,
}

impl Default for AudioEncoderConfig {
    fn default() -> Self {
        Self {
            packet_size_ms: 20,

            bandwidth: AudioBandwidth::Auto,

            start_bitrate_bps: 40000,
            min_bitrate_bps: 16000,
            max_bitrate_bps: 40000,
            complexity: 9,
            enable_cbr: true,
            enable_dtx: false,
            enable_fec: true,
        }
    }
}

impl From<&AudioEncoderConfig> for RffiAudioEncoderConfig {
    fn from(config: &AudioEncoderConfig) -> Self {
        Self {
            packet_size_ms: config.packet_size_ms,

            bandwidth: config.bandwidth as i32,
            start_bitrate_bps: config.start_bitrate_bps as i32,
            min_bitrate_bps: config.min_bitrate_bps as i32,
            max_bitrate_bps: config.max_bitrate_bps as i32,
            complexity: config.complexity as i32,
            enable_vbr: if config.enable_cbr { 0 } else { 1 },
            enable_dtx: if config.enable_dtx { 1 } else { 0 },
            enable_fec: if config.enable_fec { 1 } else { 0 },
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(i32)]
pub enum AudioBandwidth {
    // Constants in libopus
    Auto = -1000,
    Full = 1105,
    SuperWide = 1104,
    Wide = 1103,
    Medium = 1102,
    Narrow = 1101,
}
