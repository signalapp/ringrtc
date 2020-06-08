//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! webrtc::jni::JavaMediaStream Interface.

use jni::objects::GlobalRef;
use jni::sys::jobject;
use jni::JNIEnv;
use std::fmt;
use std::ptr;

use crate::android::error::AndroidError;
use crate::common::Result;
use crate::webrtc::media::{MediaStream, RffiMediaStreamInterface};

/// Incomplete type for C++ JavaMediaStream.
#[repr(C)]
pub struct RffiJavaMediaStream {
    _private: [u8; 0],
}

/// Rust wrapper around webrtc::jni::JavaMediaStream object.
pub struct JavaMediaStream {
    rffi_jms_interface: *const RffiJavaMediaStream,
}

unsafe impl Sync for JavaMediaStream {}
unsafe impl Send for JavaMediaStream {}

impl fmt::Debug for JavaMediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "jave_media_stream: {:p}", self.rffi_jms_interface)
    }
}

impl Drop for JavaMediaStream {
    fn drop(&mut self) {
        if !self.rffi_jms_interface.is_null() {
            info!("Dropping JavaMediastream");
            unsafe { Rust_freeJavaMediaStream(self.rffi_jms_interface) };
            self.rffi_jms_interface = ptr::null();
        }
    }
}

impl JavaMediaStream {
    /// Create a JavaMediaStream from a MediaStream object.
    pub fn new(mut stream: MediaStream) -> Result<Self> {
        let rffi_jms_interface = unsafe {
            // The JavaMediaStream constructor takes ownership of the
            // raw MediaStreamInterface pointer.
            Rust_createJavaMediaStream(stream.own_rffi_interface())
        };
        if rffi_jms_interface.is_null() {
            return Err(AndroidError::CreateJavaMediaStream.into());
        }
        Ok(Self { rffi_jms_interface })
    }

    /// Return a JNI GlobalRef to the JavaMediaStream object
    pub fn global_ref(&self, env: &JNIEnv) -> Result<GlobalRef> {
        unsafe {
            let jobject = Rust_getObjectJavaMediaStream(self.rffi_jms_interface);
            Ok(env.new_global_ref(jobject.into())?)
        }
    }
}

extern "C" {
    fn Rust_createJavaMediaStream(
        media_stream_interface: *const RffiMediaStreamInterface,
    ) -> *const RffiJavaMediaStream;

    fn Rust_freeJavaMediaStream(rffi_jms_interface: *const RffiJavaMediaStream);

    fn Rust_getObjectJavaMediaStream(rffi_jms_interface: *const RffiJavaMediaStream) -> jobject;
}
