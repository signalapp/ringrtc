//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI IceGatherer

/// Incomplete type for C++ IceGathererInterface.
#[repr(C)]
pub struct RffiIceGatherer {
    _private: [u8; 0],
}
