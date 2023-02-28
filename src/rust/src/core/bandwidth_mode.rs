//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Modes of operation when working with different bandwidth environments.

use std::fmt;

use crate::common::units;

pub const MINIMUM_BITRATE_BPS: u64 = 30_000;
pub const MAXIMUM_BITRATE_BPS: u64 = 2_000_001;

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BandwidthMode {
    /// Intended for low bitrate video calls. Useful to reduce
    /// bandwidth costs, especially on mobile networks.
    Low = 0,
    /// (Default) No specific constraints, but keep a relatively
    /// high bitrate to ensure good quality.
    Normal,
}

impl fmt::Display for BandwidthMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl BandwidthMode {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => BandwidthMode::Low,
            1 => BandwidthMode::Normal,
            _ => {
                // Log but otherwise assume normal if not valid.
                warn!("Invalid bandwidth_mode: {}", value);
                BandwidthMode::Normal
            }
        }
    }

    /// Return the maximum bitrate (for all media) allowed for the mode.
    pub fn max_bitrate(&self) -> units::DataRate {
        match self {
            BandwidthMode::Low => units::DataRate::from_kbps(300),
            BandwidthMode::Normal => units::DataRate::from_kbps(2_000),
        }
    }

    pub fn audio_encoder_config(&self) -> crate::webrtc::media::AudioEncoderConfig {
        let (start_bitrate_bps, min_bitrate_bps, max_bitrate_bps) = match self {
            BandwidthMode::Low => (28_000, 16_000, 28_000),
            BandwidthMode::Normal => (32_000, 20_000, 32_000),
        };
        crate::webrtc::media::AudioEncoderConfig {
            start_bitrate_bps,
            min_bitrate_bps,
            max_bitrate_bps,
            ..Default::default()
        }
    }
}
