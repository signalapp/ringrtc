//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Peer Connection Observer Interface.

use std::marker::PhantomData;
use std::fmt;
use std::ptr;

use crate::common::{
    Result,
    DATA_CHANNEL_NAME,
};
use crate::core::call_connection::{
    CallConnectionInterface,
    CallConnectionHandle,
};
use crate::core::util::{
    RustObject,
    CppObject,
    get_object_ref_from_ptr,
};
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::ice_candidate::{
    CppIceCandidate,
    IceCandidate,
};
use crate::webrtc::peer_connection::RffiDataChannelInterface;
use crate::webrtc::media_stream::{
    MediaStream,
    RffiMediaStreamInterface,
};

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
#[allow(clippy::enum_variant_names)]
enum IceGatheringState {
    IceGatheringNew,
    IceGatheringGathering,
    IceGatheringComplete
}

/// Rust version of WebRTC RTCIceConnectionState enum
///
/// See [RTCIceConnectionState](https://w3c.github.io/webrtc-pc/#dom-rtciceconnectionstate)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
#[allow(clippy::enum_variant_names)]
enum IceConnectionState {
    IceConnectionNew,
    IceConnectionChecking,
    IceConnectionConnected,
    IceConnectionCompleted,
    IceConnectionFailed,
    IceConnectionDisconnected,
    IceConnectionClosed,
    IceConnectionMax,
}

/// PeerConnectionObserver OnIceCandidate() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnIceCandidate<T>(call_connection: *mut CallConnectionHandle<T>, candidate: *const CppIceCandidate)
where
    T: CallConnectionInterface,
{
    let ice_candidate = IceCandidate::from(unsafe {&*candidate});
    debug!("pc_observer_OnIceCandidate(): {:?}", ice_candidate);

    if let Ok(cc_handle) = get_object_ref_from_ptr(call_connection) {
        cc_handle.inject_local_ice_candidate(ice_candidate)
            .unwrap_or_else(|e| error!("Problems adding ice canddiate to fsm: {}", e));
    }
}

/// PeerConnectionObserver OnIceCandidateRemoved() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnIceCandidatesRemoved<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnIceCandidatesRemoved()");
}

/// PeerConnectionObserver OnSignalingChange() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnSignalingChange<T>(_call_connection: *mut CallConnectionHandle<T>, new_state: SignalingState)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnSignalingChange(): new_state: {:?}", new_state);
}

/// PeerConnectionObserver OnIceConnectionChange() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnIceConnectionChange<T>(call_connection: *mut CallConnectionHandle<T>, new_state: IceConnectionState)
where
    T: CallConnectionInterface,
{
    debug!("pc_observer_OnIceConnectionChange(): new_state: {:?}", new_state);
    if let Ok(cc_handle) = get_object_ref_from_ptr(call_connection) {
        use IceConnectionState::*;
        match new_state {
            IceConnectionCompleted | IceConnectionConnected => {
                cc_handle.inject_ice_connected()
                    .unwrap_or_else(|e| error!("Problems adding ice_connected to fsm: {}", e));
            },
            IceConnectionFailed => {
                cc_handle.inject_ice_connection_failed()
                    .unwrap_or_else(|e| error!("Problems adding ice_connection_failed to fsm: {}", e));
            },
            _ => {},
        }
    }
}

/// PeerConnectionObserver OnConnectionChange() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnConnectionChange<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnConnectionChange()");
}

/// PeerConnectionObserver OnIceConnectionReceivingChange() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnIceConnectionReceivingChange<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnIceConnectionReceivingChange()");
}

/// PeerConnectionObserver OnIceGatherChange() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnIceGatheringChange<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnIceGatheringChange()");
}

/// PeerConnectionObserver OnAddStream() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnAddStream<T>(call_connection: *mut CallConnectionHandle<T>,
                                       native_stream: *const RffiMediaStreamInterface)
where
    T: CallConnectionInterface,
{
    debug!("pc_observer_OnAddStream() -- {:p}", native_stream);
    let stream = MediaStream::new(native_stream);

    if let Ok(cc_handle) = get_object_ref_from_ptr(call_connection) {
        cc_handle.inject_on_add_stream(stream)
            .unwrap_or_else(|e| error!("Problems adding on_add_stream event to fsm: {}", e));
    }
}

/// PeerConnectionObserver OnRemoveStream() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnRemoveStream<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnRemoveStream()");
}

/// PeerConnectionObserver OnDataChannel() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnDataChannel<T>(call_connection:   *mut CallConnectionHandle<T>,
                                       rffi_dc_interface: *const RffiDataChannelInterface)
where
    T: CallConnectionInterface,
{
    debug!("pc_observer_OnDataChannel()");
    let data_channel = DataChannel::new(rffi_dc_interface);
    let label = data_channel.get_label();
    if label == DATA_CHANNEL_NAME {
        if let Ok(cc_handle) = get_object_ref_from_ptr(call_connection) {
            cc_handle.inject_on_data_channel(data_channel)
                .unwrap_or_else(|e| error!("Problems adding on_data_channel event to fsm: {}", e));
        }
    }
}

/// PeerConnectionObserver OnRenegotiationNeeded() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnRenegotiationNeeded<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnRenegotiationNeeded()");
}

/// PeerConnectionObserver OnAddTrack() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnAddTrack<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnAddTrack()");
}

/// PeerConnectionObserver OnTrack() callback.
#[allow(non_snake_case)]
extern fn pc_observer_OnTrack<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("pc_observer_OnTrack()");
}

/// PeerConnectionObserver callback function pointers.
///
/// A structure containing function pointers for each
/// PeerConnection event callback.
#[repr(C)]
#[allow(non_snake_case)]
pub struct PeerConnectionObserverCallbacks<T>
where
    T: CallConnectionInterface,
{
    onIceCandidate: extern fn(*mut CallConnectionHandle<T>, *const CppIceCandidate),
    onIceCandidatesRemoved: extern fn (*mut CallConnectionHandle<T>),
    onSignalingChange: extern fn (*mut CallConnectionHandle<T>, SignalingState),
    onIceConnectionChange: extern fn (*mut CallConnectionHandle<T>, IceConnectionState),
    onConnectionChange: extern fn (*mut CallConnectionHandle<T>),
    onIceConnectionReceivingChange: extern fn (*mut CallConnectionHandle<T>),
    onIceGatheringChange: extern fn (*mut CallConnectionHandle<T>),
    onAddStream: extern fn (*mut CallConnectionHandle<T>, *const RffiMediaStreamInterface),
    onRemoveStream: extern fn (*mut CallConnectionHandle<T>),
    onDataChannel: extern fn (*mut CallConnectionHandle<T>, *const RffiDataChannelInterface),
    onRenegotiationNeeded: extern fn (*mut CallConnectionHandle<T>),
    onAddTrack: extern fn (*mut CallConnectionHandle<T>),
    onTrack: extern fn (*mut CallConnectionHandle<T>),
}

/// Incomplete type for C++ PeerConnectionObserver.
#[repr(C)]
pub struct RffiPeerConnectionObserverInterface { _private: [u8; 0] }

/// Rust wrapper around WebRTC C++ PeerConnectionObserver object.
pub struct PeerConnectionObserver<T>
where
    T: CallConnectionInterface,
{
    /// Pointer to C++ webrtc::rffi::PeerConnectionObserverRffi.
    rffi_pc_observer: *const RffiPeerConnectionObserverInterface,
    connection_type:   PhantomData<T>,
}

impl<T> fmt::Display for PeerConnectionObserver<T>
where
    T: CallConnectionInterface,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "pc_observer: {:p}", self.rffi_pc_observer)
    }
}

impl<T> fmt::Debug for PeerConnectionObserver<T>
where
    T: CallConnectionInterface,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Default for PeerConnectionObserver<T>
where
    T: CallConnectionInterface,
{
    fn default() -> Self {
        Self {
            rffi_pc_observer:  ptr::null(),
            connection_type:   PhantomData::<T>,
        }
    }
}

impl<T> PeerConnectionObserver<T>
where
    T: CallConnectionInterface,
{

    /// Create a new Rust PeerConnectionObserver object.
    ///
    /// Creates a new WebRTC C++ PeerConnectionObserver object,
    /// registering the observer callbacks to this module, and wraps
    /// the result in a Rust PeerConnectionObserver object.
    pub fn new(call_connection_ptr: *mut CallConnectionHandle<T>) -> Result<Self>
    {
        debug!("create_pc_observer(): call_connection_ptr: {:p}", call_connection_ptr);
        let pc_observer_callbacks = PeerConnectionObserverCallbacks::<T> {
            onIceCandidate: pc_observer_OnIceCandidate::<T>,
            onIceCandidatesRemoved: pc_observer_OnIceCandidatesRemoved::<T>,
            onSignalingChange: pc_observer_OnSignalingChange::<T>,
            onIceConnectionChange: pc_observer_OnIceConnectionChange::<T>,
            onConnectionChange: pc_observer_OnConnectionChange::<T>,
            onIceConnectionReceivingChange: pc_observer_OnIceConnectionReceivingChange::<T>,
            onIceGatheringChange: pc_observer_OnIceGatheringChange::<T>,
            onAddStream: pc_observer_OnAddStream::<T>,
            onRemoveStream: pc_observer_OnRemoveStream::<T>,
            onDataChannel: pc_observer_OnDataChannel::<T>,
            onRenegotiationNeeded: pc_observer_OnRenegotiationNeeded::<T>,
            onAddTrack: pc_observer_OnAddTrack::<T>,
            onTrack: pc_observer_OnTrack::<T>,
        };
        let pc_observer_callbacks_ptr: *const PeerConnectionObserverCallbacks<T> = &pc_observer_callbacks;
        let rffi_pc_observer = unsafe {
            Rust_createPeerConnectionObserver(call_connection_ptr as RustObject,
                                              pc_observer_callbacks_ptr as CppObject)
        };

        if rffi_pc_observer.is_null() {
            Err(RingRtcError::CreatePeerConnectionObserver.into())
        } else {
            Ok(
                Self {
                    rffi_pc_observer,
                    connection_type:  PhantomData,
                }
            )
        }

    }

    /// Return the internal WebRTC C++ PeerConnectionObserver pointer.
    pub fn get_rffi_interface(&self) -> *const RffiPeerConnectionObserverInterface {
        self.rffi_pc_observer
    }

}

extern {
    fn Rust_createPeerConnectionObserver(call_connection: RustObject,
                                         pc_observer_cb:  CppObject)
                                         -> *const RffiPeerConnectionObserverInterface;
}
