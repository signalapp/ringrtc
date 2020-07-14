//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use std::{
    cmp::{Ord, PartialEq, PartialOrd},
    ops::{Add, Div, Mul},
    time::Duration,
};

#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct DataRate {
    size_per_second: DataSize,
}

#[allow(dead_code)]
impl DataRate {
    pub fn per_second(size_per_second: DataSize) -> Self {
        Self { size_per_second }
    }

    pub fn from_bps(bps: u64) -> Self {
        Self::per_second(DataSize::from_bits(bps))
    }

    pub fn as_bps(self) -> u64 {
        self.size_per_second.as_bits()
    }

    pub fn from_kbps(kbps: u64) -> Self {
        Self::per_second(DataSize::from_kilobits(kbps))
    }

    pub fn as_kbps(self) -> u64 {
        self.size_per_second.as_kilobits()
    }

    pub fn from_mbps(kbps: u64) -> Self {
        Self::per_second(DataSize::from_megabits(kbps))
    }

    pub fn as_mbps(self) -> u64 {
        self.size_per_second.as_megabits()
    }
}

impl Mul<Duration> for DataRate {
    type Output = DataSize;

    fn mul(self, duration: Duration) -> DataSize {
        DataSize::from_bits(((self.as_bps() as f64) * duration.as_secs_f64()) as u64)
    }
}

#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct DataSize {
    bits: u64,
}

#[allow(dead_code)]
impl DataSize {
    pub fn per_second(self) -> DataRate {
        DataRate::per_second(self)
    }

    pub fn from_bits(bits: u64) -> Self {
        Self { bits }
    }

    pub fn as_bits(self) -> u64 {
        self.bits
    }

    pub fn from_bytes(bytes: u64) -> Self {
        Self::from_bits(bytes * 8)
    }

    pub fn as_bytes(self) -> u64 {
        self.as_bits() / 8
    }

    pub fn from_kilobits(kbits: u64) -> Self {
        Self::from_bits(kbits * 1024)
    }

    pub fn as_kilobits(self) -> u64 {
        self.as_bits() / 1024
    }

    pub fn from_kilobytes(kbytes: u64) -> Self {
        Self::from_bytes(kbytes * 1024)
    }

    pub fn as_kilobytes(self) -> u64 {
        self.as_bytes() / 1024
    }

    pub fn from_megabits(mbits: u64) -> Self {
        Self::from_kilobits(mbits * 1024)
    }

    pub fn as_megabits(self) -> u64 {
        self.as_kilobits() / 1024
    }

    pub fn from_megabytes(mbytes: u64) -> Self {
        Self::from_kilobytes(mbytes * 1024)
    }

    pub fn as_megabytes(self) -> u64 {
        self.as_kilobytes() / 1024
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
