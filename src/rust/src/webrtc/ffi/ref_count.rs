//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Wrapper around rtc::RefCountInterface

use crate::core::util::CppObject;

/// Rust wrapper around RefCountInterface::AddRef()
#[cfg(feature = "native")]
pub fn add_ref(ref_counted_pointer: CppObject) {
    unsafe { Rust_addRef(ref_counted_pointer) };
}

/// Rust wrapper around RefCountInterface::Release()
pub fn release_ref(ref_counted_pointer: CppObject) {
    unsafe { Rust_releaseRef(ref_counted_pointer) };
}

extern "C" {

    #[cfg(feature = "native")]
    fn Rust_addRef(ref_counted_pointer: CppObject);

    fn Rust_releaseRef(ref_counted_pointer: CppObject);

}
