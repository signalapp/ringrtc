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

/// Cast opaque pointer back to concrete Rust type T.
pub fn get_object_from_cpp<T>(object: RustObject) -> Result<&'static mut T> {
    let object_ptr = object as *mut T;
    if object_ptr.is_null() {
        return Err(RingRtcError::NullPointer("get_object_from_cpp<T>()".to_string(),
                                             "object".to_string()).into());
    }

    let object = unsafe { &mut *object_ptr };
    Ok(object)
}

/// Cast opaque pointer back to concrete Rust type wrap in an Arc<>.
pub fn get_arc_from_ptr<T>(ptr: *mut T) -> Result<Arc<T>> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("get_arc_from_ptr<T>()".to_string(),
                                             "ptr".to_string()).into());
    }
    let arc = unsafe { Arc::from_raw(ptr) };
    Ok(arc)
}

/// Wrapper around an Arc pointer that prevents it from freeing its
/// contents when it goes out of scope.  Useful when translating a
/// Java long into an Arc, when you want the Arc to continue persist.
///
/// If you really want to consume the Arc use get_arc_from_XXX()
/// instead.
pub struct ArcPtr<T> {
    arc: Option<Arc<T>>,
}

impl<T> ArcPtr<T> {
    /// Creates a new ArcPtr<T>.
    pub fn new(ptr: *mut T) -> Self {
        ArcPtr {
            arc: Some(unsafe { Arc::<T>::from_raw(ptr as *mut T) }),
        }
    }

    /// Returns reference to the inner Arc<T>.
    pub fn get_arc(&self) -> &Arc<T> {
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

/// Returns a ArcPtr<T> from a raw pointer.
pub fn get_arc_ptr_from_ptr<T>(ptr: *mut T) -> Result<ArcPtr<T>> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("get_arc_ptr_from_ptr<T>()".to_string(),
                                             "ptr".to_string()).into());
    }
    Ok(ArcPtr::<T>::new(ptr))
}

/// Returns a <&T> object from a raw pointer.
pub fn get_object_ref_from_ptr<T>(ptr: *mut T) -> Result<&'static mut T> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("get_object_ref_from_ptr<T>()".to_string(),
                                             "ptr".to_string()).into());
    }

    let object = unsafe { &mut *ptr };
    Ok(object)
}

/// Returns a <Box<T>> object from a raw pointer.
pub fn get_object_from_ptr<T>(ptr: *mut T) -> Result<Box<T>> {
    if ptr.is_null() {
        return Err(RingRtcError::NullPointer("get_object_from_ptr<T>()".to_string(),
                                             "ptr".to_string()).into());
    }

    let object = unsafe { Box::from_raw(ptr) };
    Ok(object)
}
