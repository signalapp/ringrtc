//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! webrtc::jni::JavaMediaStream Interface.

use jni::objects::GlobalRef;
use jni::sys::jobject;
use jni::JNIEnv;

use crate::android::error::AndroidError;
use crate::common::Result;
use crate::webrtc::{
    self,
    media::{MediaStream, RffiMediaStream},
};

/// Incomplete type for C++ JavaMediaStream.
#[repr(C)]
pub struct RffiJavaMediaStream {
    _private: [u8; 0],
}

/// Rust wrapper around webrtc::jni::JavaMediaStream object.
pub struct JavaMediaStream {
    rffi: webrtc::ptr::Unique<RffiJavaMediaStream>,
}

unsafe impl Sync for JavaMediaStream {}
unsafe impl Send for JavaMediaStream {}

impl webrtc::ptr::Delete for RffiJavaMediaStream {
    fn delete(owned: webrtc::ptr::Owned<Self>) {
        unsafe { Rust_deleteJavaMediaStream(owned) };
    }
}

impl JavaMediaStream {
    /// Create a JavaMediaStream from a MediaStream object.
    pub fn new(stream: MediaStream) -> Result<Self> {
        let rffi =
            webrtc::ptr::Unique::from(unsafe { Rust_createJavaMediaStream(stream.into_owned()) });
        if rffi.is_null() {
            return Err(AndroidError::CreateJavaMediaStream.into());
        }
        Ok(Self { rffi })
    }

    /// Return a JNI GlobalRef to the JavaMediaStream object
    pub fn global_ref(&self, env: &JNIEnv) -> Result<GlobalRef> {
        unsafe {
            let jobject = Rust_getJavaMediaStreamObject(self.rffi.borrow());
            Ok(env.new_global_ref(jobject)?)
        }
    }
}

extern "C" {
    fn Rust_createJavaMediaStream(
        rffi_media_stream: webrtc::ptr::OwnedRc<RffiMediaStream>,
    ) -> webrtc::ptr::Owned<RffiJavaMediaStream>;

    fn Rust_deleteJavaMediaStream(rffi_java_media_stream: webrtc::ptr::Owned<RffiJavaMediaStream>);

    fn Rust_getJavaMediaStreamObject(
        rffi_java_media_stream: webrtc::ptr::Borrowed<RffiJavaMediaStream>,
    ) -> jobject;
}
