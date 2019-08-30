//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Application direct access functions for the
//! RTCMediaStream object.

use std::fmt;
use std::ptr;

use std::ffi::c_void;

use crate::ios::error::iOSError;
use crate::common::Result;
use crate::webrtc::media_stream::{
    MediaStream,
};

/// Rust wrapper around Application RTCMediaStream object.
pub struct AppMediaStream {
    app_call_connection: *const c_void,
    ams_interface: *mut c_void,
}

impl fmt::Debug for AppMediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ams_interface: {:p}", self.ams_interface)
    }
}

impl Drop for AppMediaStream {
    fn drop(&mut self) {

        debug!("app_call_connection: {:?}", self.app_call_connection);
        debug!("ams_interface: {:?}", self.ams_interface);

        if !self.ams_interface.is_null() {
            unsafe { appReleaseStream(self.app_call_connection, self.ams_interface) };
            self.ams_interface = ptr::null_mut();
        }
    }
}

impl AppMediaStream {
    /// Create an Application RTCMediaStream from a MediaStream object.
    pub fn new(app_call_connection: *const c_void,
                        mut stream: MediaStream) -> Result<Self> {
        let ams_interface = unsafe {
            appCreateStreamFromNative(app_call_connection, stream.own_rffi_interface() as *mut c_void)
        };

        debug!("app_call_connection: {:?}", app_call_connection);
        debug!("ams_interface: {:?}", ams_interface);
        debug!("stream: {:?}", stream);

        if ams_interface.is_null() {
            return Err(iOSError::CreateAppMediaStream.into());
        }

        Ok(
            Self {
                app_call_connection,
                ams_interface,
            }
        )
    }

    /// Return a reference to the Application RTCMediaStream object.
    pub fn get_ref(&self) -> Result<*const c_void> {
//        let app_call_connection = self.app_call_connection.as_ref()
//            .ok_or::<failure::Error>(RingRtcError::CallConnectionMemberNotSet("app_call_connection".to_string()).into())?;
        Ok(self.ams_interface)
    }
}

extern "C" {
    #[allow(non_snake_case)]
    pub fn appCreateStreamFromNative(appCallConnection: *const c_void,
                                          nativeStream: *mut c_void) -> *mut c_void;

    #[allow(non_snake_case)]
    pub fn appReleaseStream(appCallConnection: *const c_void,
                                    appStream: *mut c_void);                                
}
