//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;

use std::os::raw::c_char;

extern "C" {
    pub fn Rust_setFieldTrials(field_trials_string: webrtc::ptr::Owned<c_char>);
}
