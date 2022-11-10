//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Peer Connection Observer

use libc::size_t;
use std::ffi::CStr;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::os::raw::c_char;
use std::slice;
use std::time::SystemTime;

use crate::common::{Result, RingBench};
use crate::core::signaling;
use crate::error::RingRtcError;
use crate::webrtc;
use crate::webrtc::media::{
    AudioTrack, MediaStream, RffiAudioTrack, RffiMediaStream, RffiVideoFrameBuffer, RffiVideoTrack,
    VideoFrame, VideoFrameMetadata, VideoTrack,
};
use crate::webrtc::network::RffiIpPort;
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

/// Stays in sync with the C++ value in rffi_defs.h.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportProtocol {
    Udp,
    Tcp,
    Tls,
    Unknown,
}

/// Ice Candidate structure passed between Rust and C++.
#[repr(C)]
#[derive(Debug)]
pub struct CppIceCandidate {
    sdp: webrtc::ptr::Borrowed<c_char>,
    is_relayed: bool,
    // Will be Unknown if !is_relayed, but may be Unknown for other reasons, so don't use that to check.
    relay_protocol: TransportProtocol,
}

/// Rust version of WebRTC AdapterType
///
/// See webrtc/rtc_base/network_constants.h
// Despite how it looks, this is not an option set.
// A network adapter type can only be one of the listed values.
// And there are a few oddities to note:
// - Cellular means we don't know if it's 2G, 3G, 4G, 5G, ...
//   If we know, it will be one of those corresponding enum values.
//   This means to know if something is cellular or not, you must
//   check all of those values.
// - Default means we don't know the adapter type (like Unknown)
//   but it's because we bound to the default IP address (0.0.0.0)
//   so it's probably the default adapter (wifi if available, for example)
//   This is unlikely to happen in practice.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NetworkAdapterType {
    Unknown = 0,
    Ethernet = 1 << 0,
    Wifi = 1 << 1,
    Cellular = 1 << 2,
    Vpn = 1 << 3,
    Loopback = 1 << 4,
    Default = 1 << 5,
    Cellular2G = 1 << 6,
    Cellular3G = 1 << 7,
    Cellular4G = 1 << 8,
    Cellular5G = 1 << 9,
}

/// Ice Network Route structure passed between Rust and C++.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct NetworkRoute {
    pub local_adapter_type: NetworkAdapterType,
    pub local_adapter_type_under_vpn: NetworkAdapterType,
    pub local_relayed: bool,
    pub local_relay_protocol: TransportProtocol,
    pub remote_relayed: bool,
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
        relay_protocol: Option<webrtc::peer_connection_observer::TransportProtocol>,
    ) -> Result<()>;
    fn handle_ice_candidates_removed(&mut self, removed_addresses: Vec<SocketAddr>) -> Result<()>;
    fn handle_ice_connection_state_changed(&mut self, new_state: IceConnectionState) -> Result<()>;
    fn handle_ice_network_route_changed(&mut self, network_route: NetworkRoute) -> Result<()>;

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
    fn handle_incoming_video_frame(
        &mut self,
        _track_id: u32,
        _video_frame_metadata: VideoFrameMetadata,
        _video_frame: Option<VideoFrame>,
    ) -> Result<()> {
        Ok(())
    }

    // RTP data events
    // Warning: this runs on the WebRTC network thread, so doing anything that
    // would block is dangerous, especially taking a lock that is also taken
    // while calling something that blocks on the network thread.
    fn handle_rtp_received(&mut self, header: rtp::Header, data: &[u8]);

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
    observer: webrtc::ptr::Borrowed<T>,
    cpp_candidate: webrtc::ptr::Borrowed<CppIceCandidate>,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        info!("pc_observer_OnIceCandidate: {}", observer.log_id());
        // Safe because the candidate should still be alive (it was just passed to us)
        if let Some(cpp_candidate) = unsafe { cpp_candidate.as_ref() } {
            let sdp = unsafe {
                CStr::from_ptr(cpp_candidate.sdp.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            };
            // ICE candidates are the same for V2 and V3 and V4.
            let ice_candidate = signaling::IceCandidate::from_v3_sdp(sdp.clone());
            let relay_protocol = if cpp_candidate.is_relayed {
                Some(cpp_candidate.relay_protocol)
            } else {
                None
            };
            if let Ok(ice_candidate) = ice_candidate {
                observer
                    .handle_ice_candidate_gathered(ice_candidate, sdp.as_str(), relay_protocol)
                    .unwrap_or_else(|e| error!("Problems handling ice candidate: {}", e));
            } else {
                warn!("Failed to handle local ICE candidate SDP");
            }
        } else {
            error!("pc_observer_OnIceCandidate called with null candidate");
        }
    } else {
        error!("pc_observer_OnIceCandidate called with null observer");
    }
}

/// PeerConnectionObserver OnIceCandidatesRemoved() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceCandidatesRemoved<T>(
    observer: webrtc::ptr::Borrowed<T>,
    removed_addresses: webrtc::ptr::Borrowed<RffiIpPort>,
    length: size_t,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        info!("pc_observer_OnIceCandidatesRemoved: {}", observer.log_id());

        if removed_addresses.is_null() {
            if length > 0 {
                warn!("ICE candidates removed is null");
            }
            return;
        }

        trace!("pc_observer_OnIceCandidatesRemoved(): length: {}", length);

        let removed_addresses =
            unsafe { slice::from_raw_parts(removed_addresses.as_ptr(), length) }
                .iter()
                .map(|address| address.into())
                .collect();

        observer
            .handle_ice_candidates_removed(removed_addresses)
            .unwrap_or_else(|e| error!("Problems handling ice candidates removed: {}", e));
    } else {
        error!("pc_observer_OnIceCandidatesRemoved called with null observer");
    }
}

/// PeerConnectionObserver OnIceConnectionChange() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceConnectionChange<T>(
    observer: webrtc::ptr::Borrowed<T>,
    new_state: IceConnectionState,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
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
    } else {
        error!("pc_observer_OnIceConnectionChange called with null observer");
    }
}

/// PeerConnectionObserver OnIceSelectedCandidatePairChanged() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceNetworkRouteChange<T>(
    observer: webrtc::ptr::Borrowed<T>,
    network_route: NetworkRoute,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        ringbench!(
            RingBench::WebRtc,
            RingBench::Conn,
            format!(
                "ice_network_route_change({:?})\t{}",
                network_route,
                observer.log_id()
            )
        );

        observer
            .handle_ice_network_route_changed(network_route)
            .unwrap_or_else(|e| error!("Problems handling ICE network route change: {}", e));
    } else {
        error!("pc_observer_OnIceNetworkRouteChange called with null observer");
    }
}

/// PeerConnectionObserver OnAddStream() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddStream<T>(
    observer: webrtc::ptr::Borrowed<T>,
    rffi_stream: webrtc::ptr::OwnedRc<RffiMediaStream>,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        info!(
            "pc_observer_OnAddStream(): {}, rffi_stream: {:p}",
            observer.log_id(),
            rffi_stream.as_ptr()
        );
        let stream = MediaStream::new(webrtc::Arc::from_owned(rffi_stream));
        observer
            .handle_incoming_media_added(stream)
            .unwrap_or_else(|e| error!("Problems handling incoming media: {}", e));
    } else {
        error!("pc_observer_OnAddStream called with null observer");
    }
}

/// PeerConnectionObserver OnAddTrack() callback for audio tracks.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddAudioRtpReceiver<T>(
    observer: webrtc::ptr::Borrowed<T>,
    rffi_track: webrtc::ptr::OwnedRc<RffiAudioTrack>,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        info!(
            "pc_observer_OnAddAudioRtpReceiver(): {}, rffi_track: {:p}",
            observer.log_id(),
            rffi_track.as_ptr()
        );
        // TODO: Figure out how to pass in a PeerConnection as an owner.
        let track = AudioTrack::new(webrtc::Arc::from_owned(rffi_track), None);
        observer
            .handle_incoming_audio_added(track)
            .unwrap_or_else(|e| error!("Problems handling incoming audio: {}", e));
    } else {
        error!("pc_observer_OnAddAudioRtpReceiver called with null observer");
    }
}

/// PeerConnectionObserver OnAddTrack() callback for video tracks.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddVideoRtpReceiver<T>(
    observer: webrtc::ptr::Borrowed<T>,
    rffi_track: webrtc::ptr::OwnedRc<RffiVideoTrack>,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        info!(
            "pc_observer_OnAddVideoRtpReceiver(): {}, rffi_track: {:p}",
            observer.log_id(),
            rffi_track.as_ptr()
        );
        // TODO: Figure out how to pass in a PeerConnection as an owner.
        let track = VideoTrack::new(webrtc::Arc::from_owned(rffi_track), None);
        observer
            .handle_incoming_video_added(track)
            .unwrap_or_else(|e| error!("Problems handling incoming audio: {}", e));
    } else {
        error!("pc_observer_OnAddVideoRtpReceiver called with null observer");
    }
}

/// PeerConnectionObserver OnVideoFrame() callback for video frames.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnVideoFrame<T>(
    observer: webrtc::ptr::Borrowed<T>,
    track_id: u32,
    metadata: VideoFrameMetadata,
    rffi_buffer: webrtc::ptr::OwnedRc<RffiVideoFrameBuffer>,
) where
    T: PeerConnectionObserverTrait,
{
    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        debug!("pc_observer_OnVideoFrame(): track_id: {}", track_id,);
        // TODO: Figure out how to pass in a PeerConnection as an owner.
        let frame = if !rffi_buffer.is_null() {
            Some(VideoFrame::from_buffer(
                metadata,
                webrtc::Arc::from_owned(rffi_buffer),
            ))
        } else {
            None
        };
        observer
            .handle_incoming_video_frame(track_id, metadata, frame)
            .unwrap_or_else(|e| error!("Problems handling incoming video frame: {}", e));
    } else {
        error!("pc_observer_OnVideoFrame called with null observer");
    }
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_OnRtpReceived<T>(
    observer: webrtc::ptr::Borrowed<T>,
    pt: u8,
    seqnum: u16,
    timestamp: u32,
    ssrc: u32,
    payload_data: webrtc::ptr::Borrowed<u8>,
    payload_size: size_t,
) where
    T: PeerConnectionObserverTrait,
{
    if payload_data.is_null() {
        return;
    }

    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        let header = rtp::Header {
            pt,
            seqnum,
            timestamp,
            ssrc,
        };
        let payload = unsafe { slice::from_raw_parts(payload_data.as_ptr(), payload_size) };
        observer.handle_rtp_received(header, payload)
    } else {
        error!("pc_observer_OnRtpReceived called with null observer");
    }
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_GetMediaCiphertextBufferSize<T>(
    observer: webrtc::ptr::Borrowed<T>,
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

    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        observer.get_media_ciphertext_buffer_size(is_audio, plaintext_size)
    } else {
        error!("pc_observer_GetMediaCiphertextBufferSize called with null observer");
        0
    }
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_EncryptMedia<T>(
    observer: webrtc::ptr::Borrowed<T>,
    is_audio: bool,
    plaintext: webrtc::ptr::Borrowed<u8>,
    plaintext_size: size_t,
    ciphertext_out: *mut u8,
    ciphertext_out_size: size_t,
    ciphertext_size_out: *mut size_t,
) -> bool
where
    T: PeerConnectionObserverTrait,
{
    if plaintext.is_null() || ciphertext_out.is_null() || ciphertext_size_out.is_null() {
        error!("nulls passed into pc_observer_EncryptMedia");
        return false;
    }

    trace!(
        "pc_observer_EncryptMedia(): is_audio: {} plaintext_size: {}, ciphertext_out_size: {}",
        is_audio,
        plaintext_size,
        ciphertext_out_size
    );

    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        let plaintext = unsafe { slice::from_raw_parts(plaintext.as_ptr(), plaintext_size) };
        let ciphertext = unsafe { slice::from_raw_parts_mut(ciphertext_out, ciphertext_out_size) };

        match observer.encrypt_media(is_audio, plaintext, ciphertext) {
            Ok(size) => {
                unsafe {
                    *ciphertext_size_out = size;
                }
                true
            }
            Err(_e) => false,
        }
    } else {
        error!("pc_observer_EncryptMedia called with null observer");
        false
    }
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_GetMediaPlaintextBufferSize<T>(
    observer: webrtc::ptr::Borrowed<T>,
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

    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        observer.get_media_plaintext_buffer_size(track_id, is_audio, ciphertext_size)
    } else {
        error!("pc_observer_GetMediaPlaintextBufferSize called with null observer");
        0
    }
}

#[allow(non_snake_case)]
extern "C" fn pc_observer_DecryptMedia<T>(
    observer: webrtc::ptr::Borrowed<T>,
    track_id: u32,
    is_audio: bool,
    ciphertext: webrtc::ptr::Borrowed<u8>,
    ciphertext_size: usize,
    plaintext_out: *mut u8,
    plaintext_out_size: size_t,
    plaintext_size_out: *mut size_t,
) -> bool
where
    T: PeerConnectionObserverTrait,
{
    if ciphertext.is_null() || plaintext_out.is_null() || plaintext_size_out.is_null() {
        return false;
    }

    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        let ciphertext = unsafe { slice::from_raw_parts(ciphertext.as_ptr(), ciphertext_size) };
        let plaintext = unsafe { slice::from_raw_parts_mut(plaintext_out, plaintext_out_size) };

        match observer.decrypt_media(track_id, is_audio, ciphertext, plaintext) {
            Ok(size) => {
                unsafe {
                    *plaintext_size_out = size;
                }
                true
            }
            Err(_e) => false,
        }
    } else {
        error!("pc_observer_DecryptMedia called with null observer");
        false
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
    onIceCandidate: extern "C" fn(webrtc::ptr::Borrowed<T>, webrtc::ptr::Borrowed<CppIceCandidate>),
    onIceCandidatesRemoved:
        extern "C" fn(webrtc::ptr::Borrowed<T>, webrtc::ptr::Borrowed<RffiIpPort>, size_t),
    onIceConnectionChange: extern "C" fn(webrtc::ptr::Borrowed<T>, IceConnectionState),
    onIceNetworkRouteChange: extern "C" fn(webrtc::ptr::Borrowed<T>, NetworkRoute),

    // Media events
    onAddStream: extern "C" fn(webrtc::ptr::Borrowed<T>, webrtc::ptr::OwnedRc<RffiMediaStream>),
    onAddAudioRtpReceiver:
        extern "C" fn(webrtc::ptr::Borrowed<T>, webrtc::ptr::OwnedRc<RffiAudioTrack>),
    onAddVideoRtpReceiver:
        extern "C" fn(webrtc::ptr::Borrowed<T>, webrtc::ptr::OwnedRc<RffiVideoTrack>),
    onVideoFrame: extern "C" fn(
        webrtc::ptr::Borrowed<T>,
        track_id: u32,
        VideoFrameMetadata,
        webrtc::ptr::OwnedRc<RffiVideoFrameBuffer>,
    ),

    // RTP data events
    onRtpReceived: extern "C" fn(
        webrtc::ptr::Borrowed<T>,
        u8,
        u16,
        u32,
        u32,
        webrtc::ptr::Borrowed<u8>,
        size_t,
    ),

    // Frame encryption
    getMediaCiphertextBufferSize: extern "C" fn(webrtc::ptr::Borrowed<T>, bool, size_t) -> size_t,
    encryptMedia: extern "C" fn(
        webrtc::ptr::Borrowed<T>,
        bool,
        webrtc::ptr::Borrowed<u8>,
        size_t,
        *mut u8,
        size_t,
        *mut size_t,
    ) -> bool,
    getMediaPlaintextBufferSize:
        extern "C" fn(webrtc::ptr::Borrowed<T>, u32, bool, size_t) -> size_t,
    decryptMedia: extern "C" fn(
        webrtc::ptr::Borrowed<T>,
        u32,
        bool,
        webrtc::ptr::Borrowed<u8>,
        size_t,
        *mut u8,
        size_t,
        *mut size_t,
    ) -> bool,
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
    rffi: webrtc::ptr::Unique<RffiPeerConnectionObserver>,
    observer_type: PhantomData<T>,
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
    pub fn new(
        observer: webrtc::ptr::Borrowed<T>,
        enable_frame_encryption: bool,
        enable_video_frame_event: bool,
        enable_video_frame_content: bool,
    ) -> Result<Self> {
        debug!(
            "create_pc_observer(): observer_ptr: {:p}",
            observer.as_ptr()
        );

        let pc_observer_callbacks = PeerConnectionObserverCallbacks::<T> {
            // ICE events
            onIceCandidate: pc_observer_OnIceCandidate::<T>,
            onIceCandidatesRemoved: pc_observer_OnIceCandidatesRemoved::<T>,
            onIceConnectionChange: pc_observer_OnIceConnectionChange::<T>,
            onIceNetworkRouteChange: pc_observer_OnIceNetworkRouteChange::<T>,

            // Media events
            // Triggered by PeerConnection::SetRemoteDescription when audio or video tracks are added.
            // Used for 1:1 calls.
            onAddStream: pc_observer_OnAddStream::<T>,
            // Triggered by PeerConnection::SetRemoteDescription when audio or video tracks are added.
            // Used for group calls.
            onAddAudioRtpReceiver: pc_observer_OnAddAudioRtpReceiver::<T>,
            // Triggered by PeerConnection::SetRemoteDescription when audio or video tracks are added.
            // Used for group calls.
            onAddVideoRtpReceiver: pc_observer_OnAddVideoRtpReceiver::<T>,
            onVideoFrame: pc_observer_OnVideoFrame::<T>,

            // RTP data events
            onRtpReceived: pc_observer_OnRtpReceived::<T>,

            // Frame encryption
            getMediaCiphertextBufferSize: pc_observer_GetMediaCiphertextBufferSize::<T>,
            encryptMedia: pc_observer_EncryptMedia::<T>,
            getMediaPlaintextBufferSize: pc_observer_GetMediaPlaintextBufferSize::<T>,
            decryptMedia: pc_observer_DecryptMedia::<T>,
        };
        let pc_observer_callbacks_ptr: *const PeerConnectionObserverCallbacks<T> =
            &pc_observer_callbacks;
        let rffi = webrtc::ptr::Unique::from(unsafe {
            pc_observer::Rust_createPeerConnectionObserver(
                observer.to_void(),
                webrtc::ptr::Borrowed::from_ptr(pc_observer_callbacks_ptr).to_void(),
                enable_frame_encryption,
                enable_video_frame_event,
                enable_video_frame_content,
            )
        });

        if rffi.is_null() {
            Err(RingRtcError::CreatePeerConnectionObserver.into())
        } else {
            Ok(Self {
                rffi,
                observer_type: PhantomData,
            })
        }
    }

    pub fn into_rffi(mut self) -> webrtc::ptr::Unique<RffiPeerConnectionObserver> {
        self.rffi.take()
    }
}
