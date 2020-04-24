//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation Peer Connection Observer Interface.

use crate::core::util::{CppObject, RustObject};

/// Simulation type for PeerConnectionObserver.
pub type RffiPeerConnectionObserverInterface = u32;

static FAKE_OBSERVER: u32 = 7;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createPeerConnectionObserver(
    _cc_ptr: RustObject,
    _pc_observer_cb: CppObject,
) -> *const RffiPeerConnectionObserverInterface {
    info!("Rust_createPeerConnectionObserver():");
    &FAKE_OBSERVER
}
