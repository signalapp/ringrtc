//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI IceGatherer

use crate::webrtc;

/// Incomplete type for C++ IceGathererInterface.
#[repr(C)]
pub struct RffiIceGatherer {
    _private: [u8; 0],
}

// See "class IceGathererInterface : public rtc::RefCountInterface"
// in webrtc/api/ice_gatherer_interface.h
// (in RingRTC's forked version of WebRTC).
impl webrtc::RefCounted for RffiIceGatherer {}
