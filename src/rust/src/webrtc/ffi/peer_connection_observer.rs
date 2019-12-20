//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC FFI Peer Connection Observer Interface.

use crate::core::util::{CppObject, RustObject};

/// Incomplete type for C++ PeerConnectionObserver.
#[repr(C)]
pub struct RffiPeerConnectionObserverInterface {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_createPeerConnectionObserver(
        cc_ptr: RustObject,
        pc_observer_cb: CppObject,
    ) -> *const RffiPeerConnectionObserverInterface;
}
