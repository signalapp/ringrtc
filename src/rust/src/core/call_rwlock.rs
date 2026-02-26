//
// Copyright 2026 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Call RwLock
///
/// Wrapper around std::mpsc::RwLock::lock() that on error consumes
/// the poisoned rwlock and returns a simple error code.
///
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{common::Result, error::RingRtcError};

#[derive(Debug)]
pub struct CallRwLock<T: ?Sized> {
    /// Human readable label for the rwlock
    label: String,
    /// The actual rwlock
    rwlock: RwLock<T>,
}

unsafe impl<T: ?Sized + Send> Send for CallRwLock<T> {}
unsafe impl<T: ?Sized + Send> Sync for CallRwLock<T> {}

impl<T> CallRwLock<T> {
    /// Creates a new CallRwLock
    pub fn new(t: T, label: &str) -> CallRwLock<T> {
        CallRwLock {
            rwlock: RwLock::new(t),
            label: label.to_string(),
        }
    }

    pub fn read(&self) -> Result<RwLockReadGuard<'_, T>> {
        match self.rwlock.read() {
            Ok(v) => Ok(v),
            Err(_) => Err(RingRtcError::RwLockPoisoned(self.label.clone()).into()),
        }
    }

    pub fn write(&self) -> Result<RwLockWriteGuard<'_, T>> {
        match self.rwlock.write() {
            Ok(v) => Ok(v),
            Err(_) => Err(RingRtcError::RwLockPoisoned(self.label.clone()).into()),
        }
    }
}
