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
use crate::ios::ios_util::AppCallConnection;
use crate::common::Result;
use crate::webrtc::media_stream::{
    MediaStream,
};

/// Rust wrapper around Application RTCMediaStream object.
pub struct IOSMediaStream {
    app_call_connection: *const AppCallConnection,
    ams_interface: *mut c_void,
}

unsafe impl Sync for IOSMediaStream {}
unsafe impl Send for IOSMediaStream {}

impl fmt::Debug for IOSMediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ams_interface: {:p}", self.ams_interface)
    }
}

impl Drop for IOSMediaStream {
    fn drop(&mut self) {

        debug!("app_call_connection: {:?}", self.app_call_connection);
        debug!("ams_interface: {:?}", self.ams_interface);

        if !self.ams_interface.is_null() {
            unsafe { appReleaseStream(self.app_call_connection as *const c_void, self.ams_interface) };
            self.ams_interface = ptr::null_mut();
        }
    }
}

impl IOSMediaStream {
    /// Create an Application RTCMediaStream from a MediaStream object.
    pub fn new(app_call_connection: *const AppCallConnection,
                        mut stream: MediaStream) -> Result<Self> {
        let ams_interface = unsafe {
            appCreateStreamFromNative(app_call_connection as *const c_void, stream.own_rffi_interface() as *mut c_void)
        };

        debug!("app_call_connection: {:p}", app_call_connection);
        debug!("ams_interface: {:?}", ams_interface);
        debug!("stream: {:?}", stream);

        if ams_interface.is_null() {
            return Err(iOSError::CreateIOSMediaStream.into());
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
