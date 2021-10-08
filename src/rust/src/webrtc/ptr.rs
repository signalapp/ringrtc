//
// Copyright 2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// Wrappers for pointers to make it clear what type of pointer we have.
// On the C++ side of the FFI, these will be pointers.
// On the Rust side, they will be one of these.

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
// We use this to tell if something can be wrapped with
// OwnedRc, BorrowedRc, and webrtc::Arc.
// Technically unsafe because we're reinterpreting the pointee based on the
// presence of this trait, given that all the adopting types are placeholder
// types anyway, adding unsafe won't make anything more clear.
pub trait RefCounted {}

#[derive(Debug)]
#[repr(transparent)]
pub struct Owned<T>(*const T);

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

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }
}

#[derive(Clone, Copy, Debug)]
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

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }
}

#[derive(Debug)]
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

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }
}

#[derive(Clone, Copy, Debug)]
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

    pub fn null() -> Self {
        Self(std::ptr::null())
    }

    pub fn is_null(&self) -> bool {
        self.as_ptr().is_null()
    }
}
