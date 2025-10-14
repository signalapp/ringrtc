//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::time::{Duration, SystemTime};

pub fn saturating_epoch_time(ts: SystemTime) -> Duration {
    ts.duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
}
