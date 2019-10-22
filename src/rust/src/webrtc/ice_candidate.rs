//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC ICE Candidate Interface.

use std::ffi::CStr;
use std::fmt;
use std::os::raw::c_char;

use crate::core::util::redact_string;

/// Ice Candidate structure passed between Rust and C++.
#[repr(C)]
#[derive(Debug)]
pub struct CppIceCandidate {
    sdp_mid:         *const c_char,
    sdp_mline_index: i32,
    sdp:             *const c_char,
}

/// Ice Candiate structure passed around within Rust only.
#[derive(Clone)]
pub struct IceCandidate {
    pub sdp_mid:         String,
    pub sdp_mline_index: i32,
    pub sdp:             String,
}

impl fmt::Display for IceCandidate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let text = format!("spd_mid: {}, spd_mline: {}, sdp: {}",
                           self.sdp_mid,
                           self.sdp_mline_index,
                           self.sdp);
        write!(f, "{}", redact_string(&text))
    }
}

impl fmt::Debug for IceCandidate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl IceCandidate {
    /// Create a new IceCandidate.
    pub fn new(sdp_mid: String, sdp_mline_index: i32, sdp: String) -> Self {
        Self {
            sdp_mid,
            sdp_mline_index,
            sdp,
        }
    }
}

impl From<&CppIceCandidate> for IceCandidate {
    fn from(item: &CppIceCandidate) -> Self {
        IceCandidate::new(
            unsafe { CStr::from_ptr(item.sdp_mid).to_string_lossy().into_owned() },
            item.sdp_mline_index,
            unsafe { CStr::from_ptr(item.sdp).to_string_lossy().into_owned() }
            )
    }
}
