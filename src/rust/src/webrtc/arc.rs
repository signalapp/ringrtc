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

use crate::webrtc;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count;
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count;

pub struct Arc<T: webrtc::RefCounted> {
    owned: webrtc::ptr::OwnedRc<T>,
}

impl<T: webrtc::RefCounted> fmt::Debug for Arc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("webrtc::Arc({:p})", self.owned.as_ptr()))
    }
}

impl<T: webrtc::RefCounted> Arc<T> {
    // Takes ownership of one reference.
    // Does not increment the ref count.
    // Should be called with a pointer returned from scoped_refptr<T>::release().
    // or from "auto t = new rtc::RefCountedObject<T>(...); t->AddRef()"
    pub fn from_owned(owned: webrtc::ptr::OwnedRc<T>) -> Self {
        Self { owned }
    }

    /// Convenience function which is the same as Arc::from_owned(webrtc::ptr::OwnedRc::from_ptr(stc::ptr::null()))
    pub fn null() -> Self {
        // Safe because a dropped null will do nothing.
        Self::from_owned(webrtc::ptr::OwnedRc::null())
    }

    /// Clones ownership (increments the ref count).
    /// # Safety
    /// The pointee must be alive.
    pub unsafe fn from_borrowed(borrowed: webrtc::ptr::BorrowedRc<T>) -> Self {
        Self::from_owned(ref_count::inc(borrowed))
    }

    pub fn as_borrowed(&self) -> webrtc::ptr::BorrowedRc<T> {
        self.owned.borrow()
    }

    pub fn take_owned(&mut self) -> webrtc::ptr::OwnedRc<T> {
        std::mem::replace(&mut self.owned, webrtc::ptr::OwnedRc::null())
    }

    pub fn into_owned(mut self) -> webrtc::ptr::OwnedRc<T> {
        self.take_owned()
    }

    /// Convenience function which is the same as self.as_borrowed().is_null()
    pub fn is_null(&self) -> bool {
        self.owned.is_null()
    }

    /// Convenience function which is the same as self.as_borrowed().as_ref()
    /// # Safety
    /// Just as safe as any pointer deref.
    pub unsafe fn as_ref(&self) -> Option<&T> {
        self.owned.as_ref()
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
        ref_count::dec(self.owned.take());
    }
}

unsafe impl<T: webrtc::RefCounted + Send + Sync> Send for Arc<T> {}
unsafe impl<T: webrtc::RefCounted + Sync> Sync for Arc<T> {}
