//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Wrapper around rtc::RefCountInterface

use crate::core::util::CppObject;

/// Rust wrapper around RefCountInterface::AddRef()
pub fn add_ref(ref_counted_pointer: CppObject) {
    unsafe { Rust_addRef(ref_counted_pointer) };
}

/// Rust wrapper around RefCountInterface::Release()
pub fn release_ref(ref_counted_pointer: CppObject) {
    unsafe { Rust_releaseRef(ref_counted_pointer) };
}

extern "C" {

    fn Rust_addRef(ref_counted_pointer: CppObject);

    fn Rust_releaseRef(ref_counted_pointer: CppObject);

}
