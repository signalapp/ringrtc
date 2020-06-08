//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use crate::core::util::CppObject;
use crate::webrtc::ffi::logging::{LogSeverity, Rust_setLogger};

pub fn set_logger(filter: log::LevelFilter) {
    let cbs = LoggerCallbacks {
        onLogMessage: log_sink_OnLogMessage,
    };
    let cbs_ptr: *const LoggerCallbacks = &cbs;
    let min_severity = match filter {
        log::LevelFilter::Off => LogSeverity::None,
        log::LevelFilter::Error => LogSeverity::Error,
        log::LevelFilter::Warn => LogSeverity::Warn,
        log::LevelFilter::Info => LogSeverity::Info,
        log::LevelFilter::Debug => LogSeverity::Verbose,
        log::LevelFilter::Trace => LogSeverity::Verbose,
    };
    unsafe {
        Rust_setLogger(cbs_ptr as CppObject, min_severity);
    }
}

#[repr(C)]
#[allow(non_snake_case)]
struct LoggerCallbacks {
    onLogMessage: extern "C" fn(LogSeverity, *const std::os::raw::c_char),
}

#[allow(non_snake_case)]
extern "C" fn log_sink_OnLogMessage(severity: LogSeverity, c_message: *const std::os::raw::c_char) {
    let message = unsafe {
        std::ffi::CStr::from_ptr(c_message)
            .to_string_lossy()
            .into_owned()
    };
    match severity {
        LogSeverity::Error => error!("{}", message),
        LogSeverity::Warn => warn!("{}", message),
        LogSeverity::Info => info!("{}", message),
        LogSeverity::Verbose => debug!("{}", message),
        _ => {}
    };
}
