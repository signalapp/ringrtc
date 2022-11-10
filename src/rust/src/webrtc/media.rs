//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;

pub use crate::webrtc::peer_connection_factory::RffiPeerConnectionFactoryOwner;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::media;

#[cfg(feature = "sim")]
use crate::webrtc::sim::media;

pub use media::{RffiAudioTrack, RffiMediaStream, RffiVideoFrameBuffer, RffiVideoTrack};

/// Rust wrapper around WebRTC C++ MediaStream object.
#[derive(Clone, Debug)]
pub struct MediaStream {
    /// Pointer to C++ webrtc::MediaStreamInterface object.
    rffi: webrtc::Arc<RffiMediaStream>,
}

impl MediaStream {
    // TODO: Figure out a way to pass in a PeerConnection as an owner.
    pub fn new(rffi: webrtc::Arc<media::RffiMediaStream>) -> Self {
        Self { rffi }
    }

    /// Return inner C++ MediaStream pointer.
    pub fn rffi(&self) -> &webrtc::Arc<media::RffiMediaStream> {
        &self.rffi
    }

    pub fn into_owned(self) -> webrtc::ptr::OwnedRc<media::RffiMediaStream> {
        self.rffi.into_owned()
    }
}

/// Rust wrapper around WebRTC C++ AudioTrackInterface object.
#[derive(Clone, Debug)]
pub struct AudioTrack {
    rffi: webrtc::Arc<media::RffiAudioTrack>,
    // We keep this around as an easy way to make sure the PeerConnectionFactory
    // outlives the AudioTrack.
    _owner: Option<webrtc::Arc<RffiPeerConnectionFactoryOwner>>,
}

impl Drop for AudioTrack {
    fn drop(&mut self) {
        // Delete the rffi before the _owner.
        self.rffi = webrtc::Arc::null();

        // Now it's safe to delete the _owner.
    }
}

impl AudioTrack {
    pub fn new(
        rffi: webrtc::Arc<media::RffiAudioTrack>,
        owner: Option<webrtc::Arc<RffiPeerConnectionFactoryOwner>>,
    ) -> Self {
        Self {
            rffi,
            _owner: owner,
        }
    }

    pub fn rffi(&self) -> &webrtc::Arc<media::RffiAudioTrack> {
        &self.rffi
    }

    pub fn set_enabled(&self, enabled: bool) {
        unsafe { media::Rust_setAudioTrackEnabled(self.rffi.as_borrowed(), enabled) }
    }
}

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
    pub height: u32,
    rotation: VideoRotation,
}

impl VideoFrameMetadata {
    #[must_use]
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

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoPixelFormat {
    I420,
    Nv12,
    Rgba,
}

impl VideoPixelFormat {
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(VideoPixelFormat::I420),
            1 => Some(VideoPixelFormat::Nv12),
            2 => Some(VideoPixelFormat::Rgba),
            _ => None,
        }
    }
}

pub struct VideoFrame {
    metadata: VideoFrameMetadata,
    rffi_buffer: webrtc::Arc<media::RffiVideoFrameBuffer>,
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

    #[must_use]
    pub fn apply_rotation(self) -> Self {
        if self.metadata.rotation == VideoRotation::None {
            return self;
        }
        Self {
            metadata: self.metadata.apply_rotation(),
            rffi_buffer: webrtc::Arc::from_owned(unsafe {
                media::Rust_copyAndRotateVideoFrameBuffer(
                    self.rffi_buffer.as_borrowed(),
                    self.metadata.rotation,
                )
            }),
        }
    }

    pub fn from_buffer(
        metadata: VideoFrameMetadata,
        rffi_buffer: webrtc::Arc<media::RffiVideoFrameBuffer>,
    ) -> Self {
        Self {
            metadata,
            rffi_buffer,
        }
    }

    pub fn copy_from_slice(
        width: u32,
        height: u32,
        pixel_format: VideoPixelFormat,
        buffer: &[u8],
    ) -> Self {
        let metadata = VideoFrameMetadata {
            width,
            height,
            rotation: VideoRotation::None,
        };
        let rffi_source = webrtc::ptr::Borrowed::from_ptr(buffer.as_ptr());
        let rffi_buffer = webrtc::Arc::from_owned(match pixel_format {
            VideoPixelFormat::I420 => unsafe {
                media::Rust_copyVideoFrameBufferFromI420(width, height, rffi_source)
            },
            VideoPixelFormat::Nv12 => unsafe {
                media::Rust_copyVideoFrameBufferFromNv12(width, height, rffi_source)
            },
            VideoPixelFormat::Rgba => unsafe {
                media::Rust_copyVideoFrameBufferFromRgba(width, height, rffi_source)
            },
        });
        Self::from_buffer(metadata, rffi_buffer)
    }

    pub fn to_rgba(&self, rgba_buffer: &mut [u8]) {
        unsafe {
            media::Rust_convertVideoFrameBufferToRgba(
                self.rffi_buffer.as_borrowed(),
                rgba_buffer.as_mut_ptr(),
            )
        }
    }
}

/// Rust wrapper around WebRTC C++ VideoTrackSourceInterface object.
#[derive(Clone, Debug)]
pub struct VideoSource {
    rffi: webrtc::Arc<media::RffiVideoSource>,
}

impl VideoSource {
    pub fn new(rffi: webrtc::Arc<media::RffiVideoSource>) -> Self {
        Self { rffi }
    }

    pub fn rffi(&self) -> &webrtc::Arc<media::RffiVideoSource> {
        &self.rffi
    }

    pub fn push_frame(&self, frame: VideoFrame) {
        unsafe {
            media::Rust_pushVideoFrame(self.rffi.as_borrowed(), frame.rffi_buffer.as_borrowed());
        }
    }
}

/// Rust wrapper around WebRTC C++ VideoTrackInterface object.
#[derive(Clone, Debug)]
pub struct VideoTrack {
    rffi: webrtc::Arc<RffiVideoTrack>,
    // We keep this around as an easy way to make sure the PeerConnectionFactory
    // outlives the VideoTrack.
    _owner: Option<webrtc::Arc<RffiPeerConnectionFactoryOwner>>,
}

impl Drop for VideoTrack {
    fn drop(&mut self) {
        // Delete the rffi before the _owner.
        self.rffi = webrtc::Arc::null();

        // Now it's safe to delete the _owner.
    }
}

impl VideoTrack {
    pub fn new(
        rffi: webrtc::Arc<media::RffiVideoTrack>,
        owner: Option<webrtc::Arc<RffiPeerConnectionFactoryOwner>>,
    ) -> Self {
        Self {
            rffi,
            _owner: owner,
        }
    }

    pub fn rffi(&self) -> &webrtc::Arc<media::RffiVideoTrack> {
        &self.rffi
    }

    pub fn set_enabled(&self, enabled: bool) {
        unsafe { media::Rust_setVideoTrackEnabled(self.rffi.as_borrowed(), enabled) }
    }

    pub fn set_content_hint(&self, is_screenshare: bool) {
        unsafe { media::Rust_setVideoTrackContentHint(self.rffi.as_borrowed(), is_screenshare) }
    }

    pub fn id(&self) -> Option<u32> {
        let id = unsafe { media::Rust_getTrackIdAsUint32(self.rffi.as_borrowed()) };
        if id == 0 {
            None
        } else {
            Some(id)
        }
    }
}

// You could have a non-Sync, non-Send VideoSink, but
// it's more convenient put those traits here than anywhere else.
pub trait VideoSink: Sync + Send {
    // Warning: this video frame's output buffer is shared with a video decoder,
    // and so must quickly be dropped (by copying it and dropping the original)
    // or the video decoder will soon stall and video will be choppy.
    fn on_video_frame(&self, track_id: u32, frame: VideoFrame);
    fn box_clone(&self) -> Box<dyn VideoSink>;
}

impl Clone for Box<dyn VideoSink> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

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
#[derive(Clone, Debug, PartialEq, Eq)]
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
            enable_vbr: i32::from(!config.enable_cbr),
            enable_dtx: i32::from(config.enable_dtx),
            enable_fec: i32::from(config.enable_fec),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
