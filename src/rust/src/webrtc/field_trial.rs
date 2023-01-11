//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::ffi::CString;

use libc::strdup;

use crate::common::Result;
use crate::webrtc;
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::field_trial;
#[cfg(feature = "sim")]
use crate::webrtc::sim::field_trial;

pub fn init(field_trials_string: &str) -> Result<()> {
    let c_str = CString::new(field_trials_string)?;
    unsafe {
        field_trial::Rust_setFieldTrials(webrtc::ptr::Owned::from_ptr(strdup(c_str.as_ptr())));
    }

    Ok(())
}
