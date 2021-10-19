//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum LogSeverity {
    Verbose,
    Info,
    Warn,
    Error,
    None,
}

#[repr(C)]
#[allow(non_snake_case)]
pub struct LoggerCallbacks {
    pub onLogMessage: extern "C" fn(LogSeverity, webrtc::ptr::Borrowed<std::os::raw::c_char>),
}

extern "C" {
    #[allow(dead_code)]
    pub fn Rust_setLogger(
        callbacks: webrtc::ptr::Borrowed<LoggerCallbacks>,
        min_severity: LogSeverity,
    );
}
