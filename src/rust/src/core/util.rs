//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Foreign Function Interface utility helpers and types.

use std::ffi::c_void;
use std::mem;
use std::sync::{
    Arc,
    Mutex,
    Condvar,
};

use crate::common::Result;
use crate::error::RingRtcError;

/// Generic Mutex/Condvar pair for signaling async event completion.
pub type FutureResult<T> = Arc<(Mutex<(bool, T)>, Condvar)>;

/// Opaque pointer type for an object of C++ origin.
pub type CppObject = *const c_void;

/// Opaque pointer type for an object of Rust origin.
pub type RustObject = *const c_void;

/// Dereferences raw *mut T into an Arc<Mutex<T>>.
pub unsafe fn ptr_as_arc_mutex<T>(ptr: *mut T) -> Result<Arc<Mutex<T>>> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("ptr_as_arc_mutex<T>()".to_string(),
                                             "ptr".to_string()).into());
    }
    let arc = Arc::from_raw(ptr as *mut Mutex<T>);
    Ok(arc)
}

/// Wrapper around an Arc<Mutex<T>> pointer that prevents it from
/// freeing its contents when it goes out of scope.  Useful when
/// translating a Java long into an Arc, when you want the Arc to
/// continue persist.
///
/// If you really want to consume the Arc use ptr_as_arc_mutex()
/// instead.
pub struct ArcPtr<T> {
    arc: Option<Arc<Mutex<T>>>,
}

impl<T> ArcPtr<T> {
    /// Creates a new ArcPtr<T>.
    pub unsafe fn new(ptr: *mut T) -> Self {
        ArcPtr {
            arc: Some(Arc::<Mutex<T>>::from_raw(ptr as *mut Mutex<T>)),
        }
    }

    /// Returns reference to the inner Arc<Mutex<T>>.
    pub fn get_arc(&self) -> &Arc<Mutex<T>> {
        match self.arc {
            Some(ref v) => v,
            None => panic!("Empty ArcPtr"),
        }
    }

}

impl<T> Drop for ArcPtr<T> {
    fn drop(&mut self) {
        let mut swap = None;
        mem::swap(&mut swap, &mut self.arc);
        if let Some(arc) = swap {
            let _ = Arc::into_raw(arc);
        }
    }
}

/// Dereferences raw *mut T into an ArcPtr<T>.
pub unsafe fn ptr_as_arc_ptr<T>(ptr: *mut T) -> Result<ArcPtr<T>> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("ptr_as_arc_ptr<T>()".to_string(),
                                             "ptr".to_string()).into());
    }
    Ok(ArcPtr::<T>::new(ptr))
}

/// Casts a raw *mut T into a &T.
pub unsafe fn ptr_as_ref<T>(ptr: *mut T) -> Result<&'static T> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("ptr_as_ref<T>()".to_string(),
                                             "ptr".to_string()).into());
    }

    let object = & *ptr;
    Ok(object)
}

/// Casts a raw *mut T into a &mut T.
pub unsafe fn ptr_as_mut<T>(ptr: *mut T) -> Result<&'static mut T> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("ptr_as_mut<T>()".to_string(),
                                             "ptr".to_string()).into());
    }

    let object = &mut *ptr;
    Ok(object)
}

/// Dereferences raw *mut T into a Box<T>.
pub unsafe fn ptr_as_box<T>(ptr: *mut T) -> Result<Box<T>> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("ptr_as_box<T>()".to_string(),
                                             "ptr".to_string()).into());
    }

    let object = Box::from_raw(ptr);
    Ok(object)
}

/// Scrubs off sensitive information from the SDP string for public
/// logging purposes.
#[cfg(not(debug_assertions))]
pub fn sanitize_sdp(sdp: &str) -> String {
    let mut lines = sdp.lines().collect::<Vec<&str>>();

    for line in lines.iter_mut() {
        // Redact entire line as needed to mask Ice Password.
        if line.find("ice-pwd").is_some() {
            *line = "a=ice-pwd:[ REDACTED ]";
        }
    }

    lines.join("\n")
}

#[cfg(debug_assertions)]
pub fn sanitize_sdp(sdp: &str) -> String {
    sdp.to_string()
}
