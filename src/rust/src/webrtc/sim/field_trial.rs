//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::os::raw::c_char;

use crate::webrtc;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_setFieldTrials(_field_trials_string: webrtc::ptr::Owned<c_char>) {
    info!("Rust_setFieldTrials()");
}
