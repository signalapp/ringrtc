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

use crate::webrtc::{self, media::MediaStream};

/// Rust wrapper around application stream interface and RTCMediaStream object.
// IosMediaStream (Rust) wraps an AppMediaStreamInterface (Rust)
// which wraps a ConnectionMediaStream (Swift)
// which wraps an RTCMediaStream (Objective-C)
// which wraps a MediaStream RC (C++).
//
// To initialize, IosPlatform::create_incoming_media (Rust) calls
// AppInterface.onCreateMediaStreamInterface (Rust)
// which is equal to callManagerInterfaceOnCreateMediaStreamInterface (Swift),
// which calls ConnectionMediaStream(...).getWrapper() (Swift),
// which creates an AppMediaStreamInterface with a null RTCMediaStream (Objective-C).
// IosPlatform::create_incoming_media then wraps that in an IosMediaStream,
// but IosMediaStream::new (Rust) first calls AppMediaStreamInterface.createMediaStream
// which is equal to connectionMediaStreamCreateMediaStream
// which wraps the MediaStream RC (C++) in an RTCMediaStream (Objective-C)
// and then sets ConnectionMediaStream.mediaStream (Swift) to that
// RTCMediaStream (Objective-C) wrapper.
// Now, IosMediaStream (Rust) effectively wraps the MediaStream RC (C++)
// passed into IosPlatform::create_incoming_media (Rust).
//
// If that weren't enough, AppMediaStreamInterface.createMediaStream (Rust)
// (AKA connectionMediaStreamCreateMediaStream (Swift)) returns an unretained
// (borrowed) pointer to the RTCMediaStream that is stored in
// ConnectionMediaStream.mediaStream (Swift).  This doesn't own anything
// but is used to pass a reference to the RTCMediaStream into
// AppInterface.onConnectMedia.
//
// To destroy,
// IosMediaStream::drop (Rust) calls AppMediaStreamInterface::Drop (Rust)
// which calls connectionMediaStreamDestroy (Swift),
// which destroys the ConnectionMediaStream (Swift),
// which destroys the RTCMediaStream (Objective-C),
// which decrements the native MediaStream RC (C++).
pub struct IosMediaStream {
    app_media_stream_interface: AppMediaStreamInterface,
    // Really an RTCMediaStream, which wraps a NativeMediaStream.
    app_media_stream: webrtc::ptr::Borrowed<c_void>,
}

unsafe impl Sync for IosMediaStream {}

unsafe impl Send for IosMediaStream {}

impl fmt::Debug for IosMediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "app_media_stream_interface: {}, app_media_stream: {:p}",
            self.app_media_stream_interface,
            self.app_media_stream.as_ptr()
        )
    }
}

impl IosMediaStream {
    pub fn new(
        app_media_stream_interface: AppMediaStreamInterface,
        stream: MediaStream,
    ) -> Result<Self> {
        // Create the application's RTCMediaStream object pointer using the native stream pointer.
        let app_media_stream = webrtc::ptr::Borrowed::from_ptr((app_media_stream_interface
            .createMediaStream)(
            app_media_stream_interface.object,
            // Takes a borrowed RC.
            stream.rffi().as_borrowed().as_ptr() as *mut c_void,
        ));
        if app_media_stream.is_null() {
            return Err(IosError::CreateIosMediaStream.into());
        }

        debug!(
            "app_media_stream_interface: {}, app_media_stream: {:p}",
            app_media_stream_interface,
            app_media_stream.as_ptr()
        );

        Ok(Self {
            app_media_stream_interface,
            app_media_stream,
        })
    }

    /// Return a reference to the Application RTCMediaStream object.
    pub fn get_ref(&self) -> Result<webrtc::ptr::Borrowed<c_void>> {
        Ok(self.app_media_stream)
    }
}
