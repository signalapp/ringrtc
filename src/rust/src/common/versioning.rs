/*
 * Copyright 2026 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

use std::fmt::Display;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SemanticVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl SemanticVersion {
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl Display for SemanticVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}.{}.{}", self.major, self.minor, self.patch))
    }
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.major.cmp(&other.major) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        self.patch.cmp(&other.patch)
    }
}

impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<&str> for SemanticVersion {
    type Error = SemanticVersionParsingError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut splits = value.split(".");
        let major = splits
            .next()
            .map(str::parse)
            .ok_or(SemanticVersionParsingError)?
            .or(Err(SemanticVersionParsingError))?;
        let minor = splits
            .next()
            .map(str::parse)
            .ok_or(SemanticVersionParsingError)?
            .or(Err(SemanticVersionParsingError))?;
        let patch = splits
            .next()
            .map(str::parse)
            .ok_or(SemanticVersionParsingError)?
            .or(Err(SemanticVersionParsingError))?;
        Ok(Self::new(major, minor, patch))
    }
}

#[derive(thiserror::Error, Debug)]
pub struct SemanticVersionParsingError;

impl Display for SemanticVersionParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SemanticVersionParsingError")
    }
}
