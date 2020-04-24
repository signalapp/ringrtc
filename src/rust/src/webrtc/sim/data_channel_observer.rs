//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation Data Channel Observer Interface.

use crate::core::util::{CppObject, RustObject};

/// Simulation type for DataChannelObserver.
pub type RffiDataChannelObserverInterface = u32;

static FAKE_OBSERVER: u32 = 5;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createDataChannelObserver(
    _call_connection: RustObject,
    _dc_observer_cb: CppObject,
) -> *const RffiDataChannelObserverInterface {
    info!("Rust_createDataChannelObserver():");
    &FAKE_OBSERVER
}
