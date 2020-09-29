//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC FFI IceGatherer

/// Incomplete type for C++ IceGathererInterface.
#[repr(C)]
pub struct RffiIceGatherer {
    _private: [u8; 0],
}
