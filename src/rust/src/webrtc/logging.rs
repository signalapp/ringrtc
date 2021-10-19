//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc::{
    self,
    ffi::logging::{LogSeverity, LoggerCallbacks, Rust_setLogger},
};

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
        Rust_setLogger(webrtc::ptr::Borrowed::from_ptr(cbs_ptr), min_severity);
    }
}

#[allow(non_snake_case)]
extern "C" fn log_sink_OnLogMessage(
    severity: LogSeverity,
    c_message: webrtc::ptr::Borrowed<std::os::raw::c_char>,
) {
    let message = unsafe {
        std::ffi::CStr::from_ptr(c_message.as_ptr())
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
