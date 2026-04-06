//
// Copyright 2026 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SliceError {
    #[error("Source and destination have mistmatched lengths")]
    MismatchedLengths,
}

/// Wraps functions that panic when dealing with slices
pub trait SafeSlicing<T: Copy> {
    /// returns [SliceError::MismatchedLengths] when `core::slice::safe_copy_from_slice` would panic
    fn safe_copy_from_slice(&mut self, src: &[T]) -> Result<(), SliceError>;
}

/// Copies src array to the front of the dest array. Validates array sizes at compile time.
#[allow(clippy::disallowed_methods)]
pub fn safe_copy_array<const N: usize, const M: usize, T: Copy>(src: &[T; N], dest: &mut [T; M]) {
    const { assert!(N <= M) }
    dest[..N].copy_from_slice(src);
}

impl<T: Copy, const N: usize> SafeSlicing<T> for [T; N] {
    #![allow(clippy::disallowed_methods)]
    fn safe_copy_from_slice(&mut self, src: &[T]) -> Result<(), SliceError> {
        if N == src.len() {
            self.copy_from_slice(src);
            Ok(())
        } else {
            Err(SliceError::MismatchedLengths)
        }
    }
}

impl<T: Copy> SafeSlicing<T> for Vec<T> {
    #![allow(clippy::disallowed_methods)]
    fn safe_copy_from_slice(&mut self, src: &[T]) -> Result<(), SliceError> {
        if self.len() == src.len() {
            self.copy_from_slice(src);
            Ok(())
        } else {
            Err(SliceError::MismatchedLengths)
        }
    }
}

impl<T: Copy> SafeSlicing<T> for &mut [T] {
    #![allow(clippy::disallowed_methods)]
    fn safe_copy_from_slice(&mut self, src: &[T]) -> Result<(), SliceError> {
        if self.len() == src.len() {
            self.copy_from_slice(src);
            Ok(())
        } else {
            Err(SliceError::MismatchedLengths)
        }
    }
}

impl<T: Copy> SafeSlicing<T> for [T] {
    #![allow(clippy::disallowed_methods)]
    fn safe_copy_from_slice(&mut self, src: &[T]) -> Result<(), SliceError> {
        if self.len() == src.len() {
            self.copy_from_slice(src);
            Ok(())
        } else {
            Err(SliceError::MismatchedLengths)
        }
    }
}
