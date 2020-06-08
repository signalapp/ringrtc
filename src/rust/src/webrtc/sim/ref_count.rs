//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Simulation of Wrapper around rtc::RefCountInterface

use crate::core::util::CppObject;

/// Rust wrapper around RefCountInterface::AddRef()
pub fn add_ref(_ref_counted_pointer: CppObject) {
    info!("add_ref()");
}

/// Rust wrapper around RefCountInterface::Release()
pub fn release_ref(_ref_counted_pointer: CppObject) {
    info!("release_ref()");
}
