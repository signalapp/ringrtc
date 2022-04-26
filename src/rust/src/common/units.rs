//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    cmp::{Ord, PartialEq, PartialOrd},
    ops::{Add, Div, Mul},
    time::Duration,
};

#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Default)]
pub struct DataRate {
    size_per_second: DataSize,
}

#[allow(dead_code)]
impl DataRate {
    pub const fn per_second(size_per_second: DataSize) -> Self {
        Self { size_per_second }
    }

    pub const fn from_bps(bps: u64) -> Self {
        Self::per_second(DataSize::from_bits(bps))
    }

    pub fn as_bps(self) -> u64 {
        self.size_per_second.as_bits()
    }

    pub const fn from_kbps(kbps: u64) -> Self {
        Self::per_second(DataSize::from_kilobits(kbps))
    }

    pub fn as_kbps(self) -> u64 {
        self.size_per_second.as_kilobits()
    }

    pub const fn from_mbps(mbps: u64) -> Self {
        Self::per_second(DataSize::from_megabits(mbps))
    }

    pub fn as_mbps(self) -> u64 {
        self.size_per_second.as_megabits()
    }

    // Only apply min if the other value is Some
    #[must_use]
    pub fn min_opt(self, other: Option<Self>) -> Self {
        if let Some(other) = other {
            self.min(other)
        } else {
            self
        }
    }
}

impl Mul<Duration> for DataRate {
    type Output = DataSize;

    fn mul(self, duration: Duration) -> DataSize {
        DataSize::from_bits(((self.as_bps() as f64) * duration.as_secs_f64()) as u64)
    }
}

#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Default)]
pub struct DataSize {
    bits: u64,
}

#[allow(dead_code)]
impl DataSize {
    pub const fn per_second(self) -> DataRate {
        DataRate::per_second(self)
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self { bits }
    }

    pub fn as_bits(self) -> u64 {
        self.bits
    }

    pub const fn from_bytes(bytes: u64) -> Self {
        Self::from_bits(bytes * 8)
    }

    pub fn as_bytes(self) -> u64 {
        self.as_bits() / 8
    }

    pub const fn from_kilobits(kbits: u64) -> Self {
        Self::from_bits(kbits * 1000)
    }

    pub fn as_kilobits(self) -> u64 {
        self.as_bits() / 1000
    }

    pub const fn from_kilobytes(kbytes: u64) -> Self {
        Self::from_bytes(kbytes * 1000)
    }

    pub fn as_kilobytes(self) -> u64 {
        self.as_bytes() / 1000
    }

    pub const fn from_megabits(mbits: u64) -> Self {
        Self::from_kilobits(mbits * 1000)
    }

    pub fn as_megabits(self) -> u64 {
        self.as_kilobits() / 1000
    }

    pub const fn from_megabytes(mbytes: u64) -> Self {
        Self::from_kilobytes(mbytes * 1000)
    }

    pub fn as_megabytes(self) -> u64 {
        self.as_kilobytes() / 1000
    }
}

impl Div<Duration> for DataSize {
    type Output = DataRate;

    fn div(self, duration: Duration) -> DataRate {
        DataRate::from_bps((self.as_bits() as f64 / duration.as_secs_f64()) as u64)
    }
}

impl Div<DataRate> for DataSize {
    type Output = Duration;

    fn div(self, rate: DataRate) -> Duration {
        Duration::from_secs_f64((self.as_bits() as f64) / (rate.as_bps() as f64))
    }
}

impl Add<DataSize> for DataSize {
    type Output = DataSize;

    fn add(self, other: DataSize) -> DataSize {
        DataSize::from_bits(self.bits + other.bits)
    }
}
