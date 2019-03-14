//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Data Channel Observer Interface.

use std::slice;

use bytes::Bytes;
use libc::size_t;
use prost::Message;

use crate::common::{
    Result,
    CallId,
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
use crate::protobuf::data_channel::Data;

/// DataChannelObserver callback function pointers.
///
/// A structure containing function pointers for each
/// DataChannel event callback.
#[repr(C)]
#[allow(non_snake_case)]
struct DataChannelObserverCallbacks<T>
where
    T: CallConnectionInterface,
{
    onStateChange: extern fn(*mut CallConnectionHandle<T>),
    onBufferedAmountChange: extern fn (*mut CallConnectionHandle<T>, u64),
    onMessage: extern fn (*mut CallConnectionHandle<T>, *const u8, size_t, bool),
}

/// DataChannelObserver OnStateChange() callback.
#[allow(non_snake_case)]
extern fn dc_observer_OnStateChange<T>(_call_connection: *mut CallConnectionHandle<T>)
where
    T: CallConnectionInterface,
{
    info!("dc_observer_OnStateChange()");
}

/// DataChannelObserver OnBufferedAmountChange() callback.
#[allow(non_snake_case)]
extern fn dc_observer_OnBufferedAmountChange<T>(_call_connection: *mut CallConnectionHandle<T>, previous_amount: u64)
where
    T: CallConnectionInterface,
{
    info!("dc_observer_OnBufferedAmountChange(): previous_amount: {}", previous_amount);
}

/// DataChannelObserver OnMessage() callback.
#[allow(non_snake_case)]
extern fn dc_observer_OnMessage<T>(call_connection: *mut CallConnectionHandle<T>, buffer: *const u8, length: size_t, binary: bool)
where
    T: CallConnectionInterface,
{
    info!("dc_observer_OnMessage(): length: {}, binary: {}", length, binary);
    if buffer.is_null() {
        warn!("rx protobuf is null");
        return;
    }
    if length == 0 {
        warn!("rx protobuf has zero length");
        return;
    }

    let slice = unsafe { slice::from_raw_parts(buffer, length as usize) };
    let bytes = Bytes::from_static(slice);
    let message = match Data::decode(bytes) {
        Ok(v) => v,
        Err(_) => {
            warn!("unable to parse rx protobuf");
            return;
        },
    };

    info!("Found data channel message: {:?}", message);

    let cc_handle = match get_object_ref_from_ptr(call_connection) {
        Ok(v) => v,
        Err(e) => {
            warn!("unable to translate call_connection ptr: {}", e);
            return;
        },
    };

    // The data channel message could contain multiple message types
    if let Some(connected) = message.connected {
        cc_handle.inject_remote_connected(connected.id() as CallId)
            .unwrap_or_else(|e| warn!("unable to inject remote connected event: {}", e));
    }
    if let Some(hangup) = message.hangup {
        cc_handle.inject_remote_hangup(hangup.id() as CallId)
            .unwrap_or_else(|e| warn!("unable to inject remote hangup event: {}", e));
    }
    if let Some(video_status) = message.video_streaming_status {
        cc_handle.inject_remote_video_status(video_status.id() as CallId,
                                             video_status.enabled())
            .unwrap_or_else(|e| warn!("unable to inject remote video status event: {}", e));
    }

}

/// Incomplete type for C++ DataChannelObserver.
#[repr(C)]
pub struct RffiDataChannelObserverInterface { _private: [u8; 0] }

/// Rust wrapper around a WebRTC C++ DataChannelObserver object.
#[derive(Debug)]
pub struct DataChannelObserver<T>
where
    T: CallConnectionInterface,
{
    /// Pointer to C++ webrtc::rffi::DataChannelObserverRffi
    rffi_dc_observer:    *const RffiDataChannelObserverInterface,
    /// Pointer to Rust CallConnectionHandle object
    call_connection_ptr: *mut CallConnectionHandle<T>,
}

impl<T> Drop for DataChannelObserver<T>
where
    T: CallConnectionInterface,
{
    fn drop(&mut self) {
        if !self.call_connection_ptr.is_null() {
            debug!("DataChannelObserver(): drop");
            // Convert the raw CallConnection pointer back into a
            // Boxed object and let it go out of scope.
            let _cc_handle = unsafe { Box::from_raw(self.call_connection_ptr) };
        }
    }
}

impl<T> DataChannelObserver<T>
where
    T: CallConnectionInterface,
{
    /// Create a new Rust DataChannelObserver object.
    ///
    /// Creates a new WebRTC C++ DataChannelObserver object,
    /// registering the observer callbacks to this module, and wraps
    /// the result in a Rust DataChannelObserver object.
    pub fn new(cc_handle: CallConnectionHandle<T>) -> Result<Self>
    {
        let call_connection_ptr = cc_handle.create_call_connection_ptr();
        debug!("create_dc_observer_interface(): call_connection_ptr: {:p}", call_connection_ptr);
        let dc_observer_callbacks = DataChannelObserverCallbacks::<T> {
            onStateChange: dc_observer_OnStateChange::<T>,
            onBufferedAmountChange: dc_observer_OnBufferedAmountChange::<T>,
            onMessage: dc_observer_OnMessage::<T>,
        };
        let dc_observer_callbacks_ptr: *const DataChannelObserverCallbacks<T> = &dc_observer_callbacks;
        let rffi_dc_observer = unsafe {
            Rust_createDataChannelObserver(call_connection_ptr       as RustObject,
                                           dc_observer_callbacks_ptr as CppObject)
        };

        if rffi_dc_observer.is_null() {
            Err(RingRtcError::CreateDataChannelObserver.into())
        } else {
            Ok(
                Self {
                    rffi_dc_observer,
                    call_connection_ptr,
                }
            )
        }
    }

    /// Return the internal WebRTC C++ DataChannelObserver pointer.
    pub fn get_rffi_interface(&self) -> *const RffiDataChannelObserverInterface {
        self.rffi_dc_observer
    }

}

extern {
    fn Rust_createDataChannelObserver(call_connection: RustObject,
                                      dc_observer_cb:  CppObject)
                                      -> *const RffiDataChannelObserverInterface;
}
