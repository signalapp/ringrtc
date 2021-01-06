//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation PeerConnectionObserver

use crate::core::util::{CppObject, RustObject};

/// Simulation type for PeerConnectionObserver.
pub type RffiPeerConnectionObserver = u32;

static FAKE_OBSERVER: u32 = 7;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createPeerConnectionObserver(
    _cc_ptr: RustObject,
    _pc_observer_cb: CppObject,
    _enable_frame_encryption: bool,
) -> *const RffiPeerConnectionObserver {
    info!("Rust_createPeerConnectionObserver():");
    &FAKE_OBSERVER
}
