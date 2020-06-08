//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS wrapper around an RTCMediaStream

use std::ffi::c_void;
use std::fmt;

use crate::ios::api::call_manager_interface::AppMediaStreamInterface;
use crate::ios::error::IOSError;

use crate::common::Result;

use crate::webrtc::media::MediaStream;

/// Rust wrapper around application stream interface and RTCMediaStream object.
pub struct IOSMediaStream {
    app_media_stream_interface: AppMediaStreamInterface,
    app_media_stream:           *mut c_void,
}

unsafe impl Sync for IOSMediaStream {}

unsafe impl Send for IOSMediaStream {}

impl fmt::Debug for IOSMediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "app_media_stream_interface: {}, app_media_stream: {:p}",
            self.app_media_stream_interface, self.app_media_stream
        )
    }
}

impl Drop for IOSMediaStream {
    fn drop(&mut self) {
        // @note The raw app_media_stream might be released elsewhere...

        // @todo At least drop the interface. Is it automatically dropped then?
        //self.app_media_stream_interface
    }
}

impl IOSMediaStream {
    pub fn new(
        app_media_stream_interface: AppMediaStreamInterface,
        mut stream: MediaStream,
    ) -> Result<Self> {
        // Create the application's RTCMediaStream object pointer using the native stream pointer.
        let app_media_stream = (app_media_stream_interface.createMediaStream)(
            app_media_stream_interface.object,
            stream.own_rffi_interface() as *mut c_void,
        );
        if app_media_stream.is_null() {
            return Err(IOSError::CreateIOSMediaStream.into());
        }

        debug!(
            "app_media_stream_interface: {}, app_media_stream: {:p}",
            app_media_stream_interface, app_media_stream
        );

        Ok(Self {
            app_media_stream_interface,
            app_media_stream,
        })
    }

    /// Return a reference to the Application RTCMediaStream object.
    pub fn get_ref(&self) -> Result<*const c_void> {
        //        let app_call_connection = self.app_call_connection.as_ref()
        //            .ok_or::<failure::Error>(RingRtcError::CallConnectionMemberNotSet("app_call_connection".to_string()).into())?;
        Ok(self.app_media_stream)
    }
}
