//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS logging wrapper

use std::env;
use std::ffi::c_void;
use std::ptr;

use libc::size_t;

use log::{LevelFilter, Log, Metadata, Record};

use crate::common::Result;
use crate::ios::error::IOSError;
use crate::ios::ios_util::*;

/// Log object for interfacing with swift.
#[repr(C)]
pub struct IOSLogger {
    pub object:  *mut c_void,
    pub destroy: extern "C" fn(object: *mut c_void),
    pub log: extern "C" fn(
        object: *mut c_void,
        message: AppByteSlice,
        file: AppByteSlice,
        function: AppByteSlice,
        line: i32,
        level: i8,
    ),
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for IOSLogger {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for IOSLogger {}

// Rust owns the log object from Swift. Drop it when it goes out of
// scope.
impl Drop for IOSLogger {
    fn drop(&mut self) {
        (self.destroy)(self.object);
    }
}

/// Implement the Log trait for our IOSLogger.
impl Log for IOSLogger {
    // This logger is always enabled as filtering is controlled by the
    // application level logger.
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // Ignore tokio and mio logs. For our purposes they are just noise.
            if record.target().contains("tokio") || record.target().contains("mio::") {
                return;
            }

            let message_string = format!("{}", record.args());
            let message_byte_slice = AppByteSlice {
                bytes: message_string.as_ptr(),
                len:   message_string.len() as size_t,
            };

            let file_byte_slice = match record.file() {
                Some(v) => AppByteSlice {
                    bytes: v.as_ptr(),
                    len:   v.len() as size_t,
                },
                None => AppByteSlice {
                    bytes: ptr::null_mut(),
                    len:   0 as size_t,
                },
            };

            let function_byte_slice = AppByteSlice {
                bytes: record.target().as_ptr(),
                len:   record.target().len() as size_t,
            };

            // Invoke the function in Swift to actually handle the log
            // message.
            // @note We assume lifetime is that byte_slice will be
            // copied or consumed by the time the function returns.
            (self.log)(
                self.object,
                message_byte_slice,
                file_byte_slice,
                function_byte_slice,
                record.line().unwrap() as i32,
                record.level() as i8,
            );
        }
    }

    fn flush(&self) {}
}

/// Initialize the global logging system. Rust will take ownership of
/// the Swift object passed down in the IOSLogger structure.
pub fn init_logging(log_object: IOSLogger) -> Result<()> {
    match log::set_boxed_logger(Box::new(log_object)) {
        Ok(v) => v,
        Err(_e) => return Err(IOSError::InitializeLogging.into()),
    }

    log::set_max_level(LevelFilter::Trace);

    env::set_var("RUST_BACKTRACE", "1");

    debug!("RingRTC logging system initialized!");

    Ok(())
}
