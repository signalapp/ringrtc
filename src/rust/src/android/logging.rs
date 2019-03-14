//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Setup Android logging object

use std::ffi::CString;
use std::os::raw::{
    c_int,
    c_char,
};

use log::{
    Log,
    Level,
    Metadata,
    Record,
};

use crate::common::Result;

/// NDK logging facility priority numbers
///
/// See source code :
/// webrtc/src/third_party/android_ndk/sysroot/usr/include/android/log.h
#[repr(C)]
#[allow(non_camel_case_types)]
enum AndroidLogPriority {
    _ANDROID_LOG_UNKNOWN = 0, // unused here
    _ANDROID_LOG_DEFAULT,     // unused here
     ANDROID_LOG_VERBOSE,
     ANDROID_LOG_DEBUG,
     ANDROID_LOG_INFO,
     ANDROID_LOG_WARN,
     ANDROID_LOG_ERROR,
    _ANDROID_LOG_FATAL,       // unused here
    _ANDROID_LOG_SILENT,      // unused here
}

/// Log object for interfacing with existing Android logger.
struct AndroidLogger {
    level: Level,
}

impl Log for AndroidLogger {

    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {

        if self.enabled(record.metadata()) {

            let tag = match record.module_path() {
                Some(v) => v,
                None => "Unknown Rust module",
            };

            let ctag = match CString::new(tag) {
                Ok(v) => v,
                Err(_) => return,
            };

            let msg = match CString::new(format!("{}", record.args())) {
                Ok(v) => v,
                Err(_) => return,
            };

            let level = match record.level() {
                Level::Error => AndroidLogPriority::ANDROID_LOG_ERROR,
                Level::Warn  => AndroidLogPriority::ANDROID_LOG_WARN,
                Level::Info  => AndroidLogPriority::ANDROID_LOG_INFO,
                Level::Debug => AndroidLogPriority::ANDROID_LOG_DEBUG,
                Level::Trace => AndroidLogPriority::ANDROID_LOG_VERBOSE,
            };

            // Ignore the result here, can't do anything about it anyway.
            let _ = unsafe { __android_log_write(level as i32, ctag.as_ptr(), msg.as_ptr()) };
        }
    }

    fn flush(&self) {
    }
}

pub fn init_logging(level: Level) -> Result<()> {

    let logger = AndroidLogger {
        level,
    };

    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(level.to_level_filter());

    Ok(())
}

extern {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}
