//
// Copyright 2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::fmt;

// Wrappers for pointers to make it clear what type of pointer we have.
// On the C++ side of the FFI, these will be pointers.
// On the Rust side, they will be one of these.

// A marker trait for WebRTC types that can be passed to rtc::scoped_refptr,
// such as rtc::RefCountedObject.  Notable examples:
// - PeerConnectionFactory (Sync and Send because it's wrapped in a Proxy)
// - AudioDeviceImpl
// - AudioMixerImpl
// - PeerConnection (Sync and Send because it's wrapped in a Proxy)
// - RtpSender (Sync and Send because it's wrapped in a Proxy)
// - RtpReceiver (Sync and Send because it's wrapped in a Proxy)
// - MediaStream (Sync and Send because it's wrapped in a Proxy)
// - AudioTrack (Sync and Send because it's wrapped in a Proxy)
// - VideoTrack (Sync and Send because it's wrapped in a Proxy)
// - VideoTrackSource (Sync and Send because it's wrapped in a Proxy)
// - I420Buffer
// We use this to tell if something can be wrapped with
// OwnedRc, BorrowedRc, and webrtc::Arc.
// Technically unsafe because we're reinterpreting the pointee based on the
// presence of this trait, given that all the adopting types are placeholder
// types anyway, adding unsafe won't make anything more clear.
pub trait RefCounted {}

pub trait Borrow<T> {
    fn borrow(&self) -> Borrowed<T>;
}

#[repr(transparent)]
pub struct Owned<T: ?Sized>(*const T);

impl<T> Owned<T> {
    /// # Safety
    /// The pointee must be owned.
    pub unsafe fn from_ptr(ptr: *const T) -> Self {
        Self(ptr)
    }

    pub fn as_ptr(&self) -> *const T {
        self.0
    }

    pub fn borrow(&self) -> Borrowed<T> {
        Borrowed::from_ptr(self.as_ptr())
    }

    // All convenience methods below

    /// Null-out the existing value.
    #[must_use]
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::null())
    }

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }

    pub fn as_ref(&self) -> Option<&T> {
        // Safe because we own it and a null ptr will become a None
        unsafe { self.as_ptr().as_ref() }
    }

    pub fn as_mut(&self) -> Option<&mut T> {
        // Safe because we own it
        unsafe { (self.as_ptr() as *mut T).as_mut() }
    }
}

impl<T> fmt::Debug for Owned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Owned({:p})", self.0)
    }
}

impl<T> Borrow<T> for Owned<T> {
    fn borrow(&self) -> Borrowed<T> {
        Borrowed::from_ptr(self.as_ptr())
    }
}

#[repr(transparent)]
pub struct Borrowed<T>(*const T);

impl<T> Borrowed<T> {
    /// Safe because we don't do anything with it other than turn it back into a pointer.
    pub fn from_ptr(ptr: *const T) -> Self {
        Self(ptr)
    }

    pub fn as_ptr(&self) -> *const T {
        self.0
    }

    // All convenience methods below

    pub fn to_void(&self) -> Borrowed<std::ffi::c_void> {
        Borrowed::from_ptr(self.as_ptr() as *const std::ffi::c_void)
    }

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }

    /// # Safety
    /// It's as safe as any pointer deref.
    pub unsafe fn as_ref(&self) -> Option<&T> {
        self.as_ptr().as_ref()
    }

    /// # Safety
    /// It's as safe as any pointer deref.
    pub unsafe fn as_mut(&self) -> Option<&mut T> {
        (self.as_ptr() as *mut T).as_mut()
    }
}

impl<T> Copy for Borrowed<T> {}

impl<T> Clone for Borrowed<T> {
    fn clone(&self) -> Self {
        Self::from_ptr(self.as_ptr())
    }
}

impl<T> fmt::Debug for Borrowed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Borrowed({:p})", self.0)
    }
}

impl<T> Borrow<T> for Borrowed<T> {
    fn borrow(&self) -> Borrowed<T> {
        *self
    }
}

#[repr(transparent)]
pub struct OwnedRc<T: RefCounted>(*const T);

impl<T: RefCounted> OwnedRc<T> {
    /// # Safety
    /// The pointee must own a ref count.
    pub unsafe fn from_ptr(ptr: *const T) -> Self {
        Self(ptr)
    }

    pub fn as_ptr(&self) -> *const T {
        self.0
    }

    pub fn borrow(&self) -> BorrowedRc<T> {
        BorrowedRc::from_ptr(self.as_ptr())
    }

    // All convenience methods below

    /// Null-out the existing value.
    #[must_use]
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::null())
    }

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }

    pub fn as_ref(&self) -> Option<&T> {
        // Safe because we own it and a null ptr will become a None
        unsafe { self.as_ptr().as_ref() }
    }

    pub fn as_mut(&self) -> Option<&mut T> {
        // Safe because we own it
        unsafe { (self.as_ptr() as *mut T).as_mut() }
    }
}

impl<T: RefCounted> fmt::Debug for OwnedRc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OwnedRc({:p})", self.0)
    }
}

#[repr(transparent)]
pub struct BorrowedRc<T: RefCounted>(*const T);

impl<T: RefCounted> BorrowedRc<T> {
    /// Safe because we don't do anything with it other than turn it back into a pointer.
    pub fn from_ptr(ptr: *const T) -> Self {
        Self(ptr)
    }

    pub fn as_ptr(&self) -> *const T {
        self.0
    }

    // All convenience methods below

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }

    /// # Safety
    /// It's as safe as any pointer deref.
    pub unsafe fn as_ref(&self) -> Option<&T> {
        self.as_ptr().as_ref()
    }

    /// # Safety
    /// It's as safe as any pointer deref.
    pub unsafe fn as_mut(&self) -> Option<&mut T> {
        (self.as_ptr() as *mut T).as_mut()
    }
}

impl<T: RefCounted> Copy for BorrowedRc<T> {}

impl<T: RefCounted> Clone for BorrowedRc<T> {
    fn clone(&self) -> Self {
        Self::from_ptr(self.as_ptr())
    }
}

impl<T: RefCounted> fmt::Debug for BorrowedRc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BorrowedRc({:p})", self.0)
    }
}

// A pointer that can be deleted.
pub trait Delete {
    fn delete(owned: Owned<Self>);
}

// A wrapper for Owned that makes sure we delete it.
// Similar to C++ std::unique_ptr.
pub struct Unique<T: Delete>(Owned<T>);

impl<T: Delete> Unique<T> {
    pub fn from(owned: Owned<T>) -> Self {
        Self(owned)
    }

    /// Null-out the existing value.
    pub fn take_owned(&mut self) -> Owned<T> {
        self.0.take()
    }

    pub fn borrow(&self) -> Borrowed<T> {
        self.0.borrow()
    }

    // All convenience methods below

    /// Null-out the existing value.
    #[must_use]
    pub fn take(&mut self) -> Self {
        Self::from(self.take_owned())
    }

    pub fn into_owned(mut self) -> Owned<T> {
        self.0.take()
    }

    pub fn null() -> Self {
        Self::from(Owned::null())
    }

    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    pub fn as_ref(&self) -> Option<&T> {
        self.0.as_ref()
    }

    pub fn as_mut(&self) -> Option<&mut T> {
        self.0.as_mut()
    }
}

impl<T: Delete> Drop for Unique<T> {
    fn drop(&mut self) {
        if !self.is_null() {
            Delete::delete(self.take_owned())
        }
    }
}

impl<T: Delete> fmt::Debug for Unique<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unique({:p})", self.borrow().as_ptr())
    }
}

impl<T: Delete> Borrow<T> for Unique<T> {
    fn borrow(&self) -> Borrowed<T> {
        self.0.borrow()
    }
}
