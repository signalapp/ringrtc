//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation IceGatherer Interface.

/// Simulation type for DataChannelInterface.
pub type RffiIceGathererInterface = u32;

pub static FAKE_ICE_GATHERER: RffiIceGathererInterface = 20;
