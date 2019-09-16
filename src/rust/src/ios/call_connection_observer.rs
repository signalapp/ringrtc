//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS CallConnectionObserver Implementation.

use std::ffi::c_void;

use libc::size_t;

use crate::ios::ios_util::*;
use crate::common::{
    CallId,
    Result,
};
use crate::core::call_connection_observer::{
    ClientEvent,
    CallConnectionObserver,
};

/// Observer object for interfacing with Swift.
#[repr(C)]
#[allow(non_snake_case)]
/// iOS CallConnectionObserver
///
/// Wrapper around a Swift object.
pub struct IOSObserver {
    /// Raw Swift object pointer.
    pub object: *mut c_void,
    /// Swift object clean up method.
    pub destroy: extern fn(object: *mut c_void),
    /// Swift call event callback method.
    pub onCallEvent: extern fn(object: *mut c_void, callId: u64, callEvent: i32),
    /// Swift call error callback method.
    pub onCallError: extern fn(object: *mut c_void, callId: u64, errorString: IOSByteSlice),
    /// Swift add stream callback method.
    pub onAddStream: extern fn(object: *mut c_void, callId: u64, stream: *mut c_void),
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for IOSObserver {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for IOSObserver {}

// Rust owns the observer object from swift. Drop it when it goes out of
// scope.
impl Drop for IOSObserver {
    fn drop(&mut self) {
        (self.destroy)(self.object);
    }
}

/// iOS CallConnectionObserver
pub struct IOSCallConnectionObserver {
    /// Swift object wrapper.
    app_observer: IOSObserver,
    /// call identifier.
    call_id: CallId
}

impl IOSCallConnectionObserver {

    /// Creates a new IOSCallConnectionObserver
    pub fn new(app_observer: IOSObserver, call_id: CallId) -> Self {
        Self {
            app_observer,
            call_id,
        }
    }

    /// Send the client application a notification via the observer.
    fn notify(&self, event: ClientEvent) -> Result<()> {
        (self.app_observer.onCallEvent)(self.app_observer.object, self.call_id, event as i32);

        Ok(())
    }

    /// Send an error message to the client application via the observer.
    fn error(&self, error: failure::Error) -> Result<()> {
        // Create an error message containing the string
        // representation of the error code.
        let msg = format!("{}", error);

        let byte_slice = IOSByteSlice {
            bytes: msg.as_ptr(),
            len: msg.len() as size_t,
        };

        // Invoke the function in Swift to actually handle the log
        // message.
        // @note We assume lifetime is that byte_slice will be
        // copied or consumed by the time the function returns.
        (self.app_observer.onCallError)(self.app_observer.object, self.call_id, byte_slice);

        Ok(())
    }

    /// Send an onAddStream message to the client application.
    fn on_add_stream(&self, stream: *mut c_void) -> Result<()> {
        (self.app_observer.onAddStream)(self.app_observer.object, self.call_id, stream);

        Ok(())
    }
}

impl CallConnectionObserver for IOSCallConnectionObserver {

    type AppMediaStream = *mut c_void;

    fn notify_event(&self, event: ClientEvent) {
        info!("notify_event: {}", event);
        self.notify(event)
            .unwrap_or_else(|e| error!("notify() failed: {}", e));
    }

    fn notify_error(&self, error: failure::Error) {
        info!("notify_error: {}", error);
        self.error(error)
            .unwrap_or_else(|e| error!("error() failed: {}", e));
    }

    fn notify_on_add_stream(&self, stream: Self::AppMediaStream) {
        info!("notify_on_add_stream()");
        self.on_add_stream(stream)
            .unwrap_or_else(|e| error!("on_add_stream() failed: {}", e));
    }

}
