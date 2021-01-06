//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! webrtc::jni::JavaMediaStream Interface.

use jni::objects::GlobalRef;
use jni::sys::jobject;
use jni::JNIEnv;
use std::fmt;
use std::ptr;

use crate::android::error::AndroidError;
use crate::common::Result;
use crate::webrtc::media::{MediaStream, RffiMediaStream};

/// Incomplete type for C++ JavaMediaStream.
#[repr(C)]
pub struct RffiJavaMediaStream {
    _private: [u8; 0],
}

/// Rust wrapper around webrtc::jni::JavaMediaStream object.
pub struct JavaMediaStream {
    rffi: *const RffiJavaMediaStream,
}

unsafe impl Sync for JavaMediaStream {}
unsafe impl Send for JavaMediaStream {}

impl fmt::Debug for JavaMediaStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "jave_media_stream: {:p}", self.rffi)
    }
}

impl Drop for JavaMediaStream {
    fn drop(&mut self) {
        if !self.rffi.is_null() {
            info!("Dropping JavaMediastream");
            unsafe { Rust_freeJavaMediaStream(self.rffi) };
            self.rffi = ptr::null();
        }
    }
}

impl JavaMediaStream {
    /// Create a JavaMediaStream from a MediaStream object.
    pub fn new(stream: MediaStream) -> Result<Self> {
        let rffi = unsafe {
            // The JavaMediaStream constructor takes ownership of the
            // raw MediaStream pointer.
            Rust_createJavaMediaStream(stream.take_rffi())
        };
        if rffi.is_null() {
            return Err(AndroidError::CreateJavaMediaStream.into());
        }
        Ok(Self { rffi })
    }

    /// Return a JNI GlobalRef to the JavaMediaStream object
    pub fn global_ref(&self, env: &JNIEnv) -> Result<GlobalRef> {
        unsafe {
            let jobject = Rust_getJavaMediaStreamObject(self.rffi);
            Ok(env.new_global_ref(jobject)?)
        }
    }
}

extern "C" {
    fn Rust_createJavaMediaStream(
        rffi_media_stream: *const RffiMediaStream,
    ) -> *const RffiJavaMediaStream;

    fn Rust_freeJavaMediaStream(rffi_java_media_stream: *const RffiJavaMediaStream);

    fn Rust_getJavaMediaStreamObject(rffi_java_media_stream: *const RffiJavaMediaStream)
        -> jobject;
}
