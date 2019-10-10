//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC FFI Data Channel Observer Interface.

use crate::core::util::{
    RustObject,
    CppObject,
};

/// Incomplete type for C++ DataChannelObserver.
#[repr(C)]
pub struct RffiDataChannelObserverInterface { _private: [u8; 0] }

extern {
    pub fn Rust_createDataChannelObserver(call_connection: RustObject,
                                          dc_observer_cb:  CppObject)
                                          -> *const RffiDataChannelObserverInterface;
}
