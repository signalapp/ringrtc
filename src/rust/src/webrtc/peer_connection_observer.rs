//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Peer Connection Observer

use std::ffi::CStr;
use std::fmt;
use std::marker::PhantomData;
use std::os::raw::c_char;
use std::ptr;
use std::slice;
use std::time::SystemTime;

use bytes::Bytes;
use libc::size_t;

use crate::common::{Result, RingBench};
use crate::core::signaling;
use crate::core::util::{CppObject, RustObject};
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::media::MediaStream;
use crate::webrtc::media::RffiMediaStream;
use crate::webrtc::peer_connection::RffiDataChannel;

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
    fn handle_ice_candidate_gathered(
        &mut self,
        ice_candidate: signaling::IceCandidate,
    ) -> Result<()>;
    fn handle_ice_connection_state_changed(&mut self, new_state: IceConnectionState) -> Result<()>;
    fn handle_incoming_media_added(&mut self, stream: MediaStream) -> Result<()>;
    fn handle_signaling_data_channel_connected(&mut self, data_channel: DataChannel) -> Result<()>;
    fn handle_signaling_data_channel_message(&mut self, message: Bytes);
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
        // ICE candidates are the same for V1 and V2, so this works for V1 as well.
        let ice_candidate = signaling::IceCandidate::from_v3_and_v2_and_v1_sdp(sdp);
        if let Ok(ice_candidate) = ice_candidate {
            observer
                .handle_ice_candidate_gathered(ice_candidate)
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
        RingBench::WebRTC,
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

    debug!("pc_observer_OnDataChannelMessage(): length: {}", length);

    let slice = unsafe { slice::from_raw_parts(buffer, length as usize) };
    let bytes = Bytes::from_static(slice);

    let observer = unsafe { &mut *observer_ptr };
    observer.handle_signaling_data_channel_message(bytes)
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
    onIceCandidate:                extern "C" fn(*mut T, *const CppIceCandidate),
    onIceConnectionChange:         extern "C" fn(*mut T, IceConnectionState),
    onAddStream:                   extern "C" fn(*mut T, *const RffiMediaStream),
    onSignalingDataChannel:        extern "C" fn(*mut T, *const RffiDataChannel),
    onSignalingDataChannelMessage: extern "C" fn(*mut T, *const u8, size_t),
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

impl<T> Default for PeerConnectionObserver<T>
where
    T: PeerConnectionObserverTrait,
{
    fn default() -> Self {
        Self {
            rffi:          ptr::null(),
            observer_type: PhantomData::<T>,
        }
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

    pub fn new(observer_ptr: *mut T) -> Result<Self> {
        debug!("create_pc_observer(): observer_ptr: {:p}", observer_ptr);

        let pc_observer_callbacks = PeerConnectionObserverCallbacks::<T> {
            onIceCandidate:                pc_observer_OnIceCandidate::<T>,
            onIceConnectionChange:         pc_observer_OnIceConnectionChange::<T>,
            onAddStream:                   pc_observer_OnAddStream::<T>,
            onSignalingDataChannel:        pc_observer_OnSignalingDataChannel::<T>,
            onSignalingDataChannelMessage: pc_observer_OnSignalingDataChannelMessage::<T>,
        };
        let pc_observer_callbacks_ptr: *const PeerConnectionObserverCallbacks<T> =
            &pc_observer_callbacks;
        let rffi = unsafe {
            pc_observer::Rust_createPeerConnectionObserver(
                observer_ptr as RustObject,
                pc_observer_callbacks_ptr as CppObject,
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
