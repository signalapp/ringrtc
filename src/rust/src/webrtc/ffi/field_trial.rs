//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::os::raw::c_char;

use crate::webrtc;

unsafe extern "C" {
    pub fn Rust_setFieldTrials(field_trials_string: webrtc::ptr::Owned<c_char>);
}
