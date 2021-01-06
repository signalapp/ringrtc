//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Simulation IceGatherer

/// Simulation type for IceGatherer.
pub type RffiIceGatherer = u32;

pub static FAKE_ICE_GATHERER: RffiIceGatherer = 20;
