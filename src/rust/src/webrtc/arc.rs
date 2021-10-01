//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Rust-friendly wrapper around rtc::RefCountInterface, similar to 
//! WebRTC's scoped_refptr.

use std::{fmt, marker::{Send, Sync}};

use crate::core::util::CppObject;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count::{add_ref, release_ref};
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count::{add_ref, release_ref};


// A marker trait for WebRTC types that can be passed to rtc::scoped_refptr,
// such as rtc::RefCountedObject.  Notable examples:
// - PeerConnectionFactory (Sync and Send because it's wrapped in a Proxy)
// - AudioDeviceImpl
// - AudioMixerImpl
// - PeerConnection (Sync and Send because it's wrapped in a Proxy)
// - RTCCertificate
// - RtpSender (Sync and Send because it's wrapped in a Proxy)
// - RtpReceiver (Sync and Send because it's wrapped in a Proxy)
// - DataChannel (Sync and Send because it's wrapped in a Proxy)
// - MediaStream (Sync and Send because it's wrapped in a Proxy)
// - AudioTrack (Sync and Send because it's wrapped in a Proxy)
// - VideoTrack (Sync and Send because it's wrapped in a Proxy)
// - VideoTrackSource (Sync and Send because it's wrapped in a Proxy)
// - I420Buffer
pub trait RefCounted {}

pub struct Arc<T: RefCounted> {
    ptr: *const T,
}

impl<T: RefCounted> fmt::Debug for Arc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("webrtc::Arc({:p})", self.ptr))
    }
}

impl<T: RefCounted> Arc<T> {
    // Takes ownership of one reference.
    // Does not increment the ref count.
    // Should be called with a pointer returned from scoped_refptr<T>::release().
    // or from "auto t = new rtc::RefCountedObject<T>(...); t->AddRef()"
    pub fn from_owned_ptr(ptr: *const T) -> Self {
        Self {
            ptr,
        }
    }

    // Increments the ref count.
    // Should be called with a pointer returned from scoped_refptr<T>::get().
    // or from "auto t = new rtc::RefCountedObject<T>(...)"
    pub fn from_borrowed_ptr(ptr: *const T) -> Self {
        if !ptr.is_null() {
            add_ref(ptr as CppObject);
        }
        Self::from_owned_ptr(ptr)
    }

    // Caller takes ownership of one reference.
    // After this, Arc::drop will *not* decrement the ref count.
    // To avoid a memory leak, someone must call ReleaseRef,
    // Should be used with something like
    // "auto t = scoped_refptr<T>(owned_ptr); owned_ptr->Release()".
    pub fn take_owned_ptr(mut self) -> *const T {
        std::mem::replace(&mut self.ptr, std::ptr::null())
    }

    // Gives a borrow ptr.  Either use it during the lifetime
    // of the Arc or increment the ref count using something like
    // "auto t = scoped_refptr<T>(owned_ptr)".
    pub fn as_borrowed_ptr(&self) -> *const T {
        self.ptr
    }

    // Convenience function which is the same as self.as_borrowed_ptr().is_null()
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}

impl<T: RefCounted> Clone for Arc<T> {
    fn clone(&self) -> Self {
        Self::from_borrowed_ptr(self.as_borrowed_ptr())
    }
}

impl<T: RefCounted> Drop for Arc<T> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            release_ref(self.ptr as CppObject)
        }
    }
}

unsafe impl<T: RefCounted + Send + Sync> Send for Arc<T> {}
unsafe impl<T: RefCounted + Sync> Sync for Arc<T> {}

