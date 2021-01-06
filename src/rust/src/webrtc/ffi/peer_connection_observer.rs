//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI Peer Connection Observer Interface.

use crate::core::util::{CppObject, RustObject};

/// Incomplete type for C++ PeerConnectionObserver.
#[repr(C)]
pub struct RffiPeerConnectionObserver {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_createPeerConnectionObserver(
        cc_ptr: RustObject,
        pc_observer_cb: CppObject,
        enable_frame_encryption: bool,
    ) -> *const RffiPeerConnectionObserver;
}
