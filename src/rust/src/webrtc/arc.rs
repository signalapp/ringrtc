//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Rust-friendly wrapper around rtc::RefCountInterface, similar to
//! WebRTC's scoped_refptr.

use std::{
    fmt,
    marker::{Send, Sync},
};

use crate::core::util::CppObject;
use crate::webrtc;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count::{add_ref, release_ref};
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count::{add_ref, release_ref};

pub struct Arc<T: webrtc::RefCounted> {
    ptr: *const T,
}

impl<T: webrtc::RefCounted> fmt::Debug for Arc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("webrtc::Arc({:p})", self.ptr))
    }
}

impl<T: webrtc::RefCounted> Arc<T> {
    // Takes ownership of one reference.
    // Does not increment the ref count.
    // Should be called with a pointer returned from scoped_refptr<T>::release().
    // or from "auto t = new rtc::RefCountedObject<T>(...); t->AddRef()"
    pub fn from_owned(owned: webrtc::ptr::OwnedRc<T>) -> Self {
        Self {
            ptr: owned.borrow().as_ptr(),
        }
    }

    /// Clones ownership (increments the ref count).
    /// # Safety
    /// The pointee must be alive.
    pub unsafe fn from_borrowed(borrowed: webrtc::ptr::BorrowedRc<T>) -> Self {
        let ptr = borrowed.as_ptr();
        if !ptr.is_null() {
            add_ref(ptr as CppObject);
        }
        Self::from_owned(webrtc::ptr::OwnedRc::from_ptr(ptr))
    }

    pub fn as_borrowed(&self) -> webrtc::ptr::BorrowedRc<T> {
        webrtc::ptr::BorrowedRc::from_ptr(self.ptr)
    }

    pub fn as_borrowed_ptr(&self) -> *const T {
        self.as_borrowed().as_ptr()
    }

    // Convenience function which is the same as self.as_borrowed_ptr().is_null()
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}

impl<T: webrtc::RefCounted> Clone for Arc<T> {
    fn clone(&self) -> Self {
        // Safe because from_borrowed is only unsafe because the passed-in BorrowedRc
        // might not longer be alive, but in this case we know it's still alive.
        unsafe { Self::from_borrowed(self.as_borrowed()) }
    }
}

impl<T: webrtc::RefCounted> Drop for Arc<T> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            release_ref(self.ptr as CppObject)
        }
    }
}

unsafe impl<T: webrtc::RefCounted + Send + Sync> Send for Arc<T> {}
unsafe impl<T: webrtc::RefCounted + Sync> Sync for Arc<T> {}
