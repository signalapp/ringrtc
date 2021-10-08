//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! iOS wrapper around an RTCMediaStream

use std::ffi::c_void;
use std::fmt;

use crate::ios::api::call_manager_interface::AppMediaStreamInterface;
use crate::ios::error::IosError;

use crate::common::Result;

use crate::webrtc::media::MediaStream;

/// Rust wrapper around application stream interface and RTCMediaStream object.
pub struct IosMediaStream {
    app_media_stream_interface: AppMediaStreamInterface,
    app_media_stream: *mut c_void,
}

unsafe impl Sync for IosMediaStream {}

unsafe impl Send for IosMediaStream {}

impl fmt::Debug for IosMediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "app_media_stream_interface: {}, app_media_stream: {:p}",
            self.app_media_stream_interface, self.app_media_stream
        )
    }
}

impl Drop for IosMediaStream {
    fn drop(&mut self) {
        // @note The raw app_media_stream might be released elsewhere...

        // @todo At least drop the interface. Is it automatically dropped then?
        //self.app_media_stream_interface
    }
}

impl IosMediaStream {
    pub fn new(
        app_media_stream_interface: AppMediaStreamInterface,
        stream: MediaStream,
    ) -> Result<Self> {
        // Create the application's RTCMediaStream object pointer using the native stream pointer.
        let app_media_stream = (app_media_stream_interface.createMediaStream)(
            app_media_stream_interface.object,
            stream.take_rffi() as *mut c_void,
        );
        if app_media_stream.is_null() {
            return Err(IosError::CreateIosMediaStream.into());
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
        //            .ok_or::<anyhow::Error>(RingRtcError::CallConnectionMemberNotSet("app_call_connection".to_string()).into())?;
        Ok(self.app_media_stream)
    }
}
