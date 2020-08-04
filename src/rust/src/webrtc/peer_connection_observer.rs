//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Peer Connection Observer Interface.

use std::ffi::CStr;
use std::fmt;
use std::marker::PhantomData;
use std::os::raw::c_char;
use std::ptr;
use std::time::SystemTime;

use crate::common::{Result, RingBench, DATA_CHANNEL_NAME};
use crate::core::connection::Connection;
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::core::util::{ptr_as_mut, CppObject, RustObject};
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::media::MediaStream;
use crate::webrtc::media::RffiMediaStreamInterface;
use crate::webrtc::peer_connection::RffiDataChannelInterface;

/// Rust version of WebRTC RTCSignalingState enum
///
/// See [WebRTC
/// RTCSignalingState](https://www.w3.org/TR/webrtc/#rtcsignalingstate-enum)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum SignalingState {
    Stable,
    HaveLocalOffer,
    HaveLocalPrAnswer,
    HaveRemoteOffer,
    HaveRemotePrAnswer,
    Closed,
}

/// Rust version of WebRTC RTCIceGatheringState enum
///
/// See [WebRTC
/// RTCIceGatheringState](https://www.w3.org/TR/webrtc/#rtcicegatheringstate-enum)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum IceGatheringState {
    New,
    Gathering,
    Complete,
}

/// Rust version of WebRTC RTCPeerConnectionState enum
///
/// See [WebRTC
/// RTCPeerConnectionState](https://www.w3.org/TR/webrtc/#rtcpeerconnectionstate-enum)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum PeerConnectionState {
    New,
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Closed,
}

/// Rust version of WebRTC RTCIceConnectionState enum
///
/// See [RTCIceConnectionState](https://w3c.github.io/webrtc-pc/#dom-rtciceconnectionstate)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum IceConnectionState {
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

/// PeerConnectionObserver OnIceCandidate() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceCandidate<T>(
    connection_ptr: *mut Connection<T>,
    cpp_candidate: *const CppIceCandidate,
) where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!("pc_observer_OnIceCandidate: {}", connection.id());
        if !cpp_candidate.is_null() {
            let sdp = unsafe {
                CStr::from_ptr((*cpp_candidate).sdp)
                    .to_string_lossy()
                    .into_owned()
            };
            // ICE candidates are the same for V1 and V2, so this works for V1 as well.
            let ice_candidate = signaling::IceCandidate::from_v2_sdp(sdp);
            if let Ok(ice_candidate) = ice_candidate {
                let force_send = false;
                connection
                    .inject_local_ice_candidate(ice_candidate, force_send)
                    .unwrap_or_else(|e| error!("Problems adding ice canddiate to fsm: {}", e));
            } else {
                warn!("Failed to handle local ICE candidate SDP");
            }
        } else {
            warn!("Ignoring null IceCandidate pointer");
        }
    } else {
        warn!("pc_observer_OnIceCandidate(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnIceCandidateRemoved() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceCandidatesRemoved<T>(connection_ptr: *mut Connection<T>)
where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!("pc_observer_OnIceCandidatesRemoved(): {}", connection.id());
    } else {
        warn!("pc_observer_OnIceCandidatesRemoved(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnSignalingChange() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnSignalingChange<T>(
    connection_ptr: *mut Connection<T>,
    new_state: SignalingState,
) where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!("signaling_change({:?})\t{}", new_state, connection.id())
        );
    } else {
        warn!("pc_observer_OnSignalingChange(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnIceConnectionChange() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceConnectionChange<T>(
    connection_ptr: *mut Connection<T>,
    new_state: IceConnectionState,
) where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!(
                "ice_connection_change({:?})\t{}",
                new_state,
                connection.id()
            )
        );

        use IceConnectionState::*;
        match new_state {
            Completed | Connected => {
                connection
                    .inject_ice_connected()
                    .unwrap_or_else(|e| error!("Problems adding ice_connected to fsm: {}", e));
            }
            Failed => {
                connection
                    .inject_ice_failed()
                    .unwrap_or_else(|e| error!("Problems adding ice_failed to fsm: {}", e));
            }
            Disconnected => {
                connection
                    .inject_ice_disconnected()
                    .unwrap_or_else(|e| error!("Problems adding ice_disconnected to fsm: {}", e));
            }
            _ => {}
        }
    } else {
        warn!("pc_observer_OnIceConnectionChange(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnConnectionChange() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnConnectionChange<T>(
    connection_ptr: *mut Connection<T>,
    new_state: PeerConnectionState,
) where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!("connection_change({:?})\t{}", new_state, connection.id())
        );
    } else {
        warn!("pc_observer_OnConnectionChange(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnIceConnectionReceivingChange() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceConnectionReceivingChange<T>(connection_ptr: *mut Connection<T>)
where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!(
            "pc_observer_OnIceConnectionReceivingChange(): {}",
            connection.id()
        );
    } else {
        warn!("pc_observer_OnIceConnectionReceivingChange(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnIceGatheringChange() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnIceGatheringChange<T>(
    connection_ptr: *mut Connection<T>,
    new_state: IceGatheringState,
) where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!("ice_gathering_change({:?})\t{}", new_state, connection.id())
        );
    } else {
        warn!("pc_observer_OnIceGatheringChange(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnAddStream() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddStream<T>(
    connection_ptr: *mut Connection<T>,
    native_stream: *const RffiMediaStreamInterface,
) where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!(
            "pc_observer_OnAddStream(): {}, native_stream: {:p}",
            connection.id(),
            native_stream
        );
        let stream = MediaStream::new(native_stream);
        connection
            .inject_received_incoming_media(stream)
            .unwrap_or_else(|e| error!("Problems adding incoming media event to fsm: {}", e));
    } else {
        warn!("pc_observer_OnAddStream(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnRemoveStream() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnRemoveStream<T>(connection_ptr: *mut Connection<T>)
where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!("pc_observer_OnRemoveStream(): {}", connection.id());
    } else {
        warn!("pc_observer_OnRemoveStream(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnDataChannel() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnDataChannel<T>(
    connection_ptr: *mut Connection<T>,
    rffi_dc_interface: *const RffiDataChannelInterface,
) where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!("pc_observer_OnDataChannel(): {}", connection.id());
        let data_channel = unsafe { DataChannel::new(rffi_dc_interface) };
        let label = data_channel.get_label();
        if label == DATA_CHANNEL_NAME {
            connection
                .inject_received_data_channel(data_channel)
                .unwrap_or_else(|e| error!("Problems adding on_data_channel event to fsm: {}", e));
        } else {
            warn!(
                "pc_observer_OnDataChannel(): unexpected data channel label: {}",
                label
            );
        }
    } else {
        warn!("pc_observer_OnDataChannel(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnRenegotiationNeeded() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnRenegotiationNeeded<T>(connection_ptr: *mut Connection<T>)
where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!("pc_observer_OnRenegotiationNeeded(): {}", connection.id());
    } else {
        warn!("pc_observer_OnRenegotiationNeeded(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnAddTrack() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnAddTrack<T>(connection_ptr: *mut Connection<T>)
where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!("pc_observer_OnAddTrack(): {}", connection.id());
    } else {
        warn!("pc_observer_OnAddTrack(): ptr_as_mut() failed.");
    }
}

/// PeerConnectionObserver OnTrack() callback.
#[allow(non_snake_case)]
extern "C" fn pc_observer_OnTrack<T>(connection_ptr: *mut Connection<T>)
where
    T: Platform,
{
    let object = unsafe { ptr_as_mut(connection_ptr) };
    if let Ok(connection) = object {
        info!("pc_observer_OnTrack(): {}", connection.id());
    } else {
        warn!("pc_observer_OnTrack(): ptr_as_mut() failed.");
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
    T: Platform,
{
    onIceCandidate:                 extern "C" fn(*mut Connection<T>, *const CppIceCandidate),
    onIceCandidatesRemoved:         extern "C" fn(*mut Connection<T>),
    onSignalingChange:              extern "C" fn(*mut Connection<T>, SignalingState),
    onIceConnectionChange:          extern "C" fn(*mut Connection<T>, IceConnectionState),
    onConnectionChange:             extern "C" fn(*mut Connection<T>, PeerConnectionState),
    onIceConnectionReceivingChange: extern "C" fn(*mut Connection<T>),
    onIceGatheringChange:           extern "C" fn(*mut Connection<T>, IceGatheringState),
    onAddStream: extern "C" fn(*mut Connection<T>, *const RffiMediaStreamInterface),
    onRemoveStream:                 extern "C" fn(*mut Connection<T>),
    onDataChannel: extern "C" fn(*mut Connection<T>, *const RffiDataChannelInterface),
    onRenegotiationNeeded:          extern "C" fn(*mut Connection<T>),
    onAddTrack:                     extern "C" fn(*mut Connection<T>),
    onTrack:                        extern "C" fn(*mut Connection<T>),
}

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection_observer as pc_observer;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::peer_connection_observer::RffiPeerConnectionObserverInterface;

#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection_observer as pc_observer;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::peer_connection_observer::RffiPeerConnectionObserverInterface;

/// Rust wrapper around WebRTC C++ PeerConnectionObserver object.
pub struct PeerConnectionObserver<T>
where
    T: Platform,
{
    /// Pointer to C++ webrtc::rffi::PeerConnectionObserverRffi.
    rffi_pc_observer: *const RffiPeerConnectionObserverInterface,
    connection_type:  PhantomData<T>,
}

impl<T> fmt::Display for PeerConnectionObserver<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "pc_observer: {:p}", self.rffi_pc_observer)
    }
}

impl<T> fmt::Debug for PeerConnectionObserver<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Default for PeerConnectionObserver<T>
where
    T: Platform,
{
    fn default() -> Self {
        Self {
            rffi_pc_observer: ptr::null(),
            connection_type:  PhantomData::<T>,
        }
    }
}

impl<T> PeerConnectionObserver<T>
where
    T: Platform,
{
    /// Create a new Rust PeerConnectionObserver object.
    ///
    /// Creates a new WebRTC C++ PeerConnectionObserver object,
    /// registering the observer callbacks to this module, and wraps
    /// the result in a Rust PeerConnectionObserver object.
    pub fn new(connection_ptr: *mut Connection<T>) -> Result<Self> {
        debug!("create_pc_observer(): connection_ptr: {:p}", connection_ptr);
        let pc_observer_callbacks = PeerConnectionObserverCallbacks::<T> {
            onIceCandidate:                 pc_observer_OnIceCandidate::<T>,
            onIceCandidatesRemoved:         pc_observer_OnIceCandidatesRemoved::<T>,
            onSignalingChange:              pc_observer_OnSignalingChange::<T>,
            onIceConnectionChange:          pc_observer_OnIceConnectionChange::<T>,
            onConnectionChange:             pc_observer_OnConnectionChange::<T>,
            onIceConnectionReceivingChange: pc_observer_OnIceConnectionReceivingChange::<T>,
            onIceGatheringChange:           pc_observer_OnIceGatheringChange::<T>,
            onAddStream:                    pc_observer_OnAddStream::<T>,
            onRemoveStream:                 pc_observer_OnRemoveStream::<T>,
            onDataChannel:                  pc_observer_OnDataChannel::<T>,
            onRenegotiationNeeded:          pc_observer_OnRenegotiationNeeded::<T>,
            onAddTrack:                     pc_observer_OnAddTrack::<T>,
            onTrack:                        pc_observer_OnTrack::<T>,
        };
        let pc_observer_callbacks_ptr: *const PeerConnectionObserverCallbacks<T> =
            &pc_observer_callbacks;
        let rffi_pc_observer = unsafe {
            pc_observer::Rust_createPeerConnectionObserver(
                connection_ptr as RustObject,
                pc_observer_callbacks_ptr as CppObject,
            )
        };

        if rffi_pc_observer.is_null() {
            Err(RingRtcError::CreatePeerConnectionObserver.into())
        } else {
            Ok(Self {
                rffi_pc_observer,
                connection_type: PhantomData,
            })
        }
    }

    /// Return the internal WebRTC C++ PeerConnectionObserver pointer.
    pub fn rffi_interface(&self) -> *const RffiPeerConnectionObserverInterface {
        self.rffi_pc_observer
    }
}
