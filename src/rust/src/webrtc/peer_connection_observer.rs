//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Peer Connection Observer

use std::ffi::CStr;
use std::fmt;
use std::marker::PhantomData;
use std::os::raw::c_char;
use std::slice;
use std::time::SystemTime;

use bytes::Bytes;
use libc::size_t;

use crate::common::{Result, RingBench};
use crate::core::signaling;
use crate::core::util::{CppObject, RustObject};
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::media::{AudioTrack, MediaStream, VideoTrack};
use crate::webrtc::media::{RffiAudioTrack, RffiMediaStream, RffiVideoTrack};
use crate::webrtc::peer_connection::RffiDataChannel;
use crate::webrtc::rtp;

/// Rust version of WebRTC RTCIceConnectionState enum
///
/// See [RTCIceConnectionState](https://w3c.github.io/webrtc-pc/#dom-rtciceconnectionstate)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum IceConnectionState {
    New,
    Checking,
    Connected,
    Completed,
    Failed,
    Disconnected,
    Closed,
    Max,
}

/// Ice Candidate structure passed between Rust and C++.
#[repr(C)]
#[derive(Debug)]
pub struct CppIceCandidate {
    sdp: *const c_char,
}

/// The callbacks from C++ will ultimately go to an impl of this.
/// I can't think of a better name :).
pub trait PeerConnectionObserverTrait {
    fn log_id(&self) -> &dyn std::fmt::Display;

    // ICE events
    fn handle_ice_candidate_gathered(
        &mut self,
        ice_candidate: signaling::IceCandidate,
        sdp_for_logging: &str,
    ) -> Result<()>;
    fn handle_ice_connection_state_changed(&mut self, new_state: IceConnectionState) -> Result<()>;

    // Media Events
    // Defaults allow an impl to choose between handling streams or tracks.
    // TODO: Replace handle_incoming_media_added with handle_incoming_audio_added + handle_incoming_video_added.
    fn handle_incoming_media_added(&mut self, _incoming_stream: MediaStream) -> Result<()> {
        Ok(())
    }
    fn handle_incoming_audio_added(&mut self, _incoming_track: AudioTrack) -> Result<()> {
        Ok(())
    }
    fn handle_incoming_video_added(&mut self, _incoming_track: VideoTrack) -> Result<()> {
        Ok(())
    }

    // Data channel events
    fn handle_signaling_data_channel_connected(&mut self, data_channel: DataChannel) -> Result<()>;
    fn handle_signaling_data_channel_message(&mut self, message: Bytes);
    fn handle_rtp_received(&mut self, _header: rtp::Header, _data: &[u8]) {}

    // Frame encryption
    // Defaults allow an impl to not support E2EE
    fn get_media_ciphertext_buffer_size(
        &mut self,
        _is_audio: bool,
        _plaintext_size: usize,
    ) -> usize {
        0
    }
    fn encrypt_media(
        &mut self,
        _is_audio: bool,
        _plaintext: &[u8],
        _ciphertext_buffer: &mut [u8],
    ) -> Result<usize> {
        Err(RingRtcError::FailedToEncrypt.into())
    }
    fn get_media_plaintext_buffer_size(
        &mut self,
        _track_id: u32,
        _is_audio: bool,
        _ciphertext_size: usize,
    ) -> usize {
        0
    }
    fn decrypt_media(
        &mut self,
        _track_id: u32,
        _is_audio: bool,
        _ciphertext: &[u8],
        _plaintext_buffer: &mut [u8],
    ) -> Result<usize> {
        Err(RingRtcError::FailedToDecrypt.into())
    }
}

/// PeerConnectionObserver OnIceCandidate() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceCandidate<T>(
    observer_ptr: *mut T,
    cpp_candidate: *const CppIceCandidate,
) where
    T: PeerConnectionObserverTrait,
{
    let observer = unsafe { &mut *observer_ptr };
    info!("pc_observer_OnIceCandidate: {}", observer.log_id());
    if !cpp_candidate.is_null() {
        let sdp = unsafe {
            CStr::from_ptr((*cpp_candidate).sdp)
                .to_string_lossy()
                .into_owned()
        };
        // ICE candidates are the same for V2 and V3 and V4.
        let ice_candidate = signaling::IceCandidate::from_v3_and_v2_sdp(sdp.clone());
        if let Ok(ice_candidate) = ice_candidate {
            observer
                .handle_ice_candidate_gathered(ice_candidate, sdp.as_str())
                .unwrap_or_else(|e| error!("Problems handling ice candidate: {}", e));
        } else {
            warn!("Failed to handle local ICE candidate SDP");
        }
    } else {
        warn!("Ignoring null IceCandidate pointer");
    }
}

/// PeerConnectionObserver OnIceConnectionChange() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceConnectionChange<T>(
    observer_ptr: *mut T,
    new_state: IceConnectionState,
) where
    T: PeerConnectionObserverTrait,
{
    let observer = unsafe { &mut *observer_ptr };
    ringbench!(
        RingBench::WebRtc,
        RingBench::Conn,
        format!(
            "ice_connection_change({:?})\t{}",
            new_state,
            observer.log_id()
        )
    );

    observer
        .handle_ice_connection_state_changed(new_state)
        .unwrap_or_else(|e| error!("Problems handling ICE connection state change: {}", e));
}

/// PeerConnectionObserver OnAddStream() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddStream<T>(observer_ptr: *mut T, rffi_stream: *const RffiMediaStream)
where
    T: PeerConnectionObserverTrait,
{
    let observer = unsafe { &mut *observer_ptr };
    info!(
        "pc_observer_OnAddStream(): {}, rffi_stream: {:p}",
        observer.log_id(),
        rffi_stream
    );
    let stream = MediaStream::new(rffi_stream);
    observer
        .handle_incoming_media_added(stream)
        .unwrap_or_else(|e| error!("Problems handling incoming media: {}", e));
}

/// PeerConnectionObserver OnAddTrack() callback for audio tracks.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddAudioRtpReceiver<T>(
    observer_ptr: *mut T,
    rffi_track: *const RffiAudioTrack,
) where
    T: PeerConnectionObserverTrait,
{
    let observer = unsafe { &mut *observer_ptr };
    info!(
        "pc_observer_OnAddAudioRtpReceiver(): {}, rffi_track: {:p}",
        observer.log_id(),
        rffi_track
    );
    let track = AudioTrack::owned(rffi_track);
    observer
        .handle_incoming_audio_added(track)
        .unwrap_or_else(|e| error!("Problems handling incoming audio: {}", e));
}

/// PeerConnectionObserver OnAddTrack() callback for video tracks.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddVideoRtpReceiver<T>(
    observer_ptr: *mut T,
    rffi_track: *const RffiVideoTrack,
) where
    T: PeerConnectionObserverTrait,
{
    let observer = unsafe { &mut *observer_ptr };
    info!(
        "pc_observer_OnAddVideoRtpReceiver(): {}, rffi_track: {:p}",
        observer.log_id(),
        rffi_track
    );
    let track = VideoTrack::owned(rffi_track);
    observer
        .handle_incoming_video_added(track)
        .unwrap_or_else(|e| error!("Problems handling incoming audio: {}", e));
}

/// PeerConnectionObserver OnSignalingDataChannel() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnSignalingDataChannel<T>(
    observer_ptr: *mut T,
    rffi_data_channel: *const RffiDataChannel,
) where
    T: PeerConnectionObserverTrait,
{
    let observer = unsafe { &mut *observer_ptr };
    info!(
        "pc_observer_OnSignalingDataChannel(): {}",
        observer.log_id()
    );
    let data_channel = unsafe { DataChannel::new(rffi_data_channel) };
    observer
        .handle_signaling_data_channel_connected(data_channel)
        .unwrap_or_else(|e| error!("Problems handling signaling data channel: {}", e));
}

/// PeerConnectionObserver OnDataChannelMessage() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnSignalingDataChannelMessage<T>(
    observer_ptr: *mut T,
    buffer: *const u8,
    length: size_t,
) where
    T: PeerConnectionObserverTrait,
{
    if buffer.is_null() {
        warn!("data channel message is null");
        return;
    }

    trace!("pc_observer_OnDataChannelMessage(): length: {}", length);

    let slice = unsafe { slice::from_raw_parts(buffer, length as usize) };
    let bytes = Bytes::from_static(slice);

    let observer = unsafe { &mut *observer_ptr };
    observer.handle_signaling_data_channel_message(bytes)
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_OnRtpReceived<T>(
    observer_ptr: *mut T,
    pt: u8,
    seqnum: u16,
    timestamp: u32,
    ssrc: u32,
    payload_data: *const u8,
    payload_size: size_t,
) where
    T: PeerConnectionObserverTrait,
{
    if payload_data.is_null() {
        return;
    }

    let observer = unsafe { &mut *observer_ptr };
    let header = rtp::Header {
        pt,
        seqnum,
        timestamp,
        ssrc,
    };
    let payload = unsafe { slice::from_raw_parts(payload_data, payload_size as usize) };
    observer.handle_rtp_received(header, payload)
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_GetMediaCiphertextBufferSize<T>(
    observer_ptr: *mut T,
    is_audio: bool,
    plaintext_size: size_t,
) -> size_t
where
    T: PeerConnectionObserverTrait,
{
    trace!(
        "pc_observer_GetMediaCiphertextBufferSize(): is_audio: {} plaintext_size: {}",
        is_audio,
        plaintext_size
    );

    let observer = unsafe { &mut *observer_ptr };
    observer.get_media_ciphertext_buffer_size(is_audio, plaintext_size)
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_EncryptMedia<T>(
    observer_ptr: *mut T,
    is_audio: bool,
    plaintext: *const u8,
    plaintext_size: size_t,
    ciphertext_buffer: *mut u8,
    ciphertext_buffer_size: size_t,
    ciphertext_size: *mut size_t,
) -> bool
where
    T: PeerConnectionObserverTrait,
{
    if plaintext.is_null() || ciphertext_buffer.is_null() || ciphertext_size.is_null() {
        error!("nulls passed into pc_observer_EncryptMedia");
        return false;
    }

    trace!(
        "pc_observer_EncryptMedia(): is_audio: {} plaintext_size: {}, ciphertext_buffer_size: {}",
        is_audio,
        plaintext_size,
        ciphertext_buffer_size
    );

    let observer = unsafe { &mut *observer_ptr };
    let plaintext = unsafe { slice::from_raw_parts(plaintext, plaintext_size as usize) };
    let ciphertext_buffer =
        unsafe { slice::from_raw_parts_mut(ciphertext_buffer, ciphertext_buffer_size as usize) };

    match observer.encrypt_media(is_audio, plaintext, ciphertext_buffer) {
        Ok(size) => {
            unsafe {
                *ciphertext_size = size;
            }
            true
        }
        Err(_e) => false,
    }
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_GetMediaPlaintextBufferSize<T>(
    observer_ptr: *mut T,
    track_id: u32,
    is_audio: bool,
    ciphertext_size: size_t,
) -> size_t
where
    T: PeerConnectionObserverTrait,
{
    trace!(
        "pc_observer_GetMediaPlaintextBufferSize(): track_id: {}, is_audio: {} ciphertext_size: {}",
        track_id,
        is_audio,
        ciphertext_size
    );

    let observer = unsafe { &mut *observer_ptr };
    observer.get_media_plaintext_buffer_size(track_id, is_audio, ciphertext_size)
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_DecryptMedia<T>(
    observer_ptr: *mut T,
    track_id: u32,
    is_audio: bool,
    ciphertext: *const u8,
    ciphertext_size: usize,
    plaintext_buffer: *mut u8,
    plaintext_buffer_size: size_t,
    plaintext_size: *mut size_t,
) -> bool
where
    T: PeerConnectionObserverTrait,
{
    if ciphertext.is_null() || plaintext_buffer.is_null() || plaintext_size.is_null() {
        return false;
    }

    let observer = unsafe { &mut *observer_ptr };
    let ciphertext = unsafe { slice::from_raw_parts(ciphertext, ciphertext_size as usize) };
    let plaintext_buffer =
        unsafe { slice::from_raw_parts_mut(plaintext_buffer, plaintext_buffer_size as usize) };

    match observer.decrypt_media(track_id, is_audio, ciphertext, plaintext_buffer) {
        Ok(size) => {
            unsafe {
                *plaintext_size = size;
            }
            true
        }
        Err(_e) => false,
    }
}

/// PeerConnectionObserver callback function pointers.
///
/// A structure containing function pointers for each
/// PeerConnection event callback.
#[repr(C)]
#[allow(non_snake_case)]
pub struct PeerConnectionObserverCallbacks<T>
where
    T: PeerConnectionObserverTrait,
{
    // ICE events
    onIceCandidate:        extern "C" fn(*mut T, *const CppIceCandidate),
    onIceConnectionChange: extern "C" fn(*mut T, IceConnectionState),

    // Media events
    onAddStream:           extern "C" fn(*mut T, *const RffiMediaStream),
    onAddAudioRtpReceiver: extern "C" fn(*mut T, *const RffiAudioTrack),
    onAddVideoRtpReceiver: extern "C" fn(*mut T, *const RffiVideoTrack),

    // Data channel events
    onSignalingDataChannel:        extern "C" fn(*mut T, *const RffiDataChannel),
    onSignalingDataChannelMessage: extern "C" fn(*mut T, *const u8, size_t),
    onRtpReceived:                 extern "C" fn(*mut T, u8, u16, u32, u32, *const u8, size_t),

    // Frame encryption
    getMediaCiphertextBufferSize: extern "C" fn(*mut T, bool, size_t) -> size_t,
    encryptMedia:
        extern "C" fn(*mut T, bool, *const u8, size_t, *mut u8, size_t, *mut size_t) -> bool,
    getMediaPlaintextBufferSize:  extern "C" fn(*mut T, u32, bool, size_t) -> size_t,
    decryptMedia:
        extern "C" fn(*mut T, u32, bool, *const u8, size_t, *mut u8, size_t, *mut size_t) -> bool,
}

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection_observer as pc_observer;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::peer_connection_observer::RffiPeerConnectionObserver;

#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection_observer as pc_observer;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::peer_connection_observer::RffiPeerConnectionObserver;

/// Rust wrapper around WebRTC C++ PeerConnectionObserver object.
pub struct PeerConnectionObserver<T>
where
    T: PeerConnectionObserverTrait,
{
    /// Pointer to C++ webrtc::rffi::RffiPeerConnectionObserver.
    rffi:          *const RffiPeerConnectionObserver,
    observer_type: PhantomData<T>,
}

impl<T> fmt::Display for PeerConnectionObserver<T>
where
    T: PeerConnectionObserverTrait,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rffi_peer_connection: {:p}", self.rffi)
    }
}

impl<T> fmt::Debug for PeerConnectionObserver<T>
where
    T: PeerConnectionObserverTrait,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> PeerConnectionObserver<T>
where
    T: PeerConnectionObserverTrait,
{
    /// Create a new Rust PeerConnectionObserver object.
    ///
    /// Creates a new WebRTC C++ PeerConnectionObserver object,
    /// registering the observer callbacks to this module, and wraps
    /// the result in a Rust PeerConnectionObserver object.

    pub fn new(observer_ptr: *mut T, enable_frame_encryption: bool) -> Result<Self> {
        debug!("create_pc_observer(): observer_ptr: {:p}", observer_ptr);

        let pc_observer_callbacks = PeerConnectionObserverCallbacks::<T> {
            // ICE events
            onIceCandidate:        pc_observer_OnIceCandidate::<T>,
            onIceConnectionChange: pc_observer_OnIceConnectionChange::<T>,

            // Media events
            onAddStream:           pc_observer_OnAddStream::<T>,
            onAddAudioRtpReceiver: pc_observer_OnAddAudioRtpReceiver::<T>,
            onAddVideoRtpReceiver: pc_observer_OnAddVideoRtpReceiver::<T>,

            // Data channel events
            onSignalingDataChannel:        pc_observer_OnSignalingDataChannel::<T>,
            onSignalingDataChannelMessage: pc_observer_OnSignalingDataChannelMessage::<T>,
            onRtpReceived:                 pc_observer_OnRtpReceived::<T>,

            // Frame encryption
            getMediaCiphertextBufferSize: pc_observer_GetMediaCiphertextBufferSize::<T>,
            encryptMedia:                 pc_observer_EncryptMedia::<T>,
            getMediaPlaintextBufferSize:  pc_observer_GetMediaPlaintextBufferSize::<T>,
            decryptMedia:                 pc_observer_DecryptMedia::<T>,
        };
        let pc_observer_callbacks_ptr: *const PeerConnectionObserverCallbacks<T> =
            &pc_observer_callbacks;
        let rffi = unsafe {
            pc_observer::Rust_createPeerConnectionObserver(
                observer_ptr as RustObject,
                pc_observer_callbacks_ptr as CppObject,
                enable_frame_encryption,
            )
        };

        if rffi.is_null() {
            Err(RingRtcError::CreatePeerConnectionObserver.into())
        } else {
            Ok(Self {
                rffi,
                observer_type: PhantomData,
            })
        }
    }

    /// Return the internal WebRTC C++ PeerConnectionObserver pointer.
    pub fn rffi(&self) -> *const RffiPeerConnectionObserver {
        self.rffi
    }
}
