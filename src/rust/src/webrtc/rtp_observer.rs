//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC RTP Observer

use std::{marker::PhantomData, slice};

use libc::size_t;

use crate::{common::Result, error::RingRtcError, webrtc, webrtc::rtp};

/// The callbacks from C++ will ultimately go to an impl of this.
pub trait RtpObserverTrait {
    /// Warning: this runs on the WebRTC network thread, so doing anything that
    /// would block is dangerous, especially taking a lock that is also taken
    /// while calling something that blocks on the network thread.
    fn handle_rtp_received(&mut self, header: rtp::Header, data: &[u8]);
}

/// RtpObserver OnRtpReceived() callback.
#[allow(non_snake_case)]
extern "C" fn rtp_observer_OnRtpReceived<T>(
    mut observer: webrtc::ptr::Borrowed<T>,
    pt: u8,
    seqnum: u16,
    timestamp: u32,
    ssrc: u32,
    payload_data: webrtc::ptr::Borrowed<u8>,
    payload_size: size_t,
) where
    T: RtpObserverTrait,
{
    if payload_data.is_null() {
        return;
    }

    // Safe because the observer should still be alive (it was just passed to us)
    if let Some(observer) = unsafe { observer.as_mut() } {
        let header = rtp::Header {
            pt,
            seqnum,
            timestamp,
            ssrc,
        };
        let payload = unsafe { slice::from_raw_parts(payload_data.as_ptr(), payload_size) };
        observer.handle_rtp_received(header, payload);
    } else {
        error!("rtp_observer_OnRtpReceived called with null observer");
    }
}

/// RtpObserver callback function pointers.
#[repr(C)]
#[allow(non_snake_case)]
pub struct RtpObserverCallbacks<T>
where
    T: RtpObserverTrait,
{
    onRtpReceived: extern "C" fn(
        webrtc::ptr::Borrowed<T>,
        u8,
        u16,
        u32,
        u32,
        webrtc::ptr::Borrowed<u8>,
        size_t,
    ),
}

#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::rtp_observer::RffiRtpObserver;
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::rtp_observer::Rust_createRtpObserver;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::rtp_observer::RffiRtpObserver;
#[cfg(feature = "sim")]
use crate::webrtc::sim::rtp_observer::Rust_createRtpObserver;

/// Rust wrapper around WebRTC C++ RtpObserver object.
pub struct RtpObserver<T>
where
    T: RtpObserverTrait,
{
    /// Pointer to C++ webrtc::rffi::RtpObserverRffi.
    rffi: webrtc::ptr::Unique<RffiRtpObserver>,
    observer_type: PhantomData<T>,
}

impl<T> RtpObserver<T>
where
    T: RtpObserverTrait,
{
    /// Create a new Rust RtpObserver object.
    ///
    /// Creates a new WebRTC C++ RtpObserver object,
    /// registering the observer callbacks to this module, and wraps
    /// the result in a Rust RtpObserver object.
    pub fn new(observer: webrtc::ptr::Borrowed<T>) -> Result<Self> {
        debug!(
            "create_rtp_observer(): observer_ptr: {:p}",
            observer.as_ptr()
        );

        let rtp_observer_callbacks = RtpObserverCallbacks::<T> {
            onRtpReceived: rtp_observer_OnRtpReceived::<T>,
        };
        let rtp_observer_callbacks_ptr: *const RtpObserverCallbacks<T> = &rtp_observer_callbacks;
        let rffi = webrtc::ptr::Unique::from(unsafe {
            Rust_createRtpObserver(
                observer.to_void(),
                webrtc::ptr::Borrowed::from_ptr(rtp_observer_callbacks_ptr).to_void(),
            )
        });

        if rffi.is_null() {
            Err(RingRtcError::CreateRtpObserver.into())
        } else {
            Ok(Self {
                rffi,
                observer_type: PhantomData,
            })
        }
    }

    pub fn into_rffi(self) -> webrtc::ptr::Unique<RffiRtpObserver> {
        self.rffi
    }
}
