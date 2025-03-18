//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Call Mutex
///
/// Wrapper around std::mpsc::Mutex::lock() that on error consumes
/// the poisoned mutex and returns a simple error code.
///
use std::sync::{Mutex, MutexGuard};

use crate::{common::Result, error::RingRtcError};

pub struct CallMutex<T: ?Sized> {
    /// Human readable label for the mutex
    label: String,
    /// The actual mutex
    mutex: Mutex<T>,
}

unsafe impl<T: ?Sized + Send> Send for CallMutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for CallMutex<T> {}

impl<T> CallMutex<T> {
    /// Creates a new CallMutex
    pub fn new(t: T, label: &str) -> CallMutex<T> {
        CallMutex {
            mutex: Mutex::new(t),
            label: label.to_string(),
        }
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, T>> {
        match self.mutex.lock() {
            Ok(v) => Ok(v),
            Err(_) => Err(RingRtcError::MutexPoisoned(self.label.clone()).into()),
        }
    }

    pub fn lock_or_reset(&self, reset: impl FnOnce(&str) -> T) -> MutexGuard<'_, T> {
        self.mutex.lock().unwrap_or_else(|mut poison_error| {
            let new_value = reset(&self.label);
            **poison_error.get_mut() = new_value;
            poison_error.into_inner()
        })
    }
}
