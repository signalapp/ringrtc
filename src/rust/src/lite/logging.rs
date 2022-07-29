//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Make calls to the platform to do logging

#[cfg(any(target_os = "ios", feature = "check-all"))]
pub mod ios {
    use crate::lite::ffi::ios::{rtc_OptionalU32, rtc_String, FromOrDefault};

    #[repr(C)]
    pub struct rtc_log_Record<'a> {
        message: rtc_String<'a>,
        file: rtc_String<'a>,
        function: rtc_String<'a>,
        line: rtc_OptionalU32,
        level: u8,
    }

    #[repr(C)]
    pub struct rtc_log_Delegate {
        pub log: extern "C" fn(record: rtc_log_Record),
    }

    #[no_mangle]
    pub extern "C" fn rtc_log_init(delegate: rtc_log_Delegate, max_level: u8) {
        if log::set_boxed_logger(Box::new(delegate)).is_err() {
            warn!("Logging already initialized");
            return;
        }

        let max_level_filter = match max_level {
            level if level == (log::LevelFilter::Off as u8) => Some(log::LevelFilter::Off),
            level if level == (log::LevelFilter::Error as u8) => Some(log::LevelFilter::Error),
            level if level == (log::LevelFilter::Warn as u8) => Some(log::LevelFilter::Warn),
            level if level == (log::LevelFilter::Info as u8) => Some(log::LevelFilter::Info),
            level if level == (log::LevelFilter::Debug as u8) => Some(log::LevelFilter::Debug),
            level if level == (log::LevelFilter::Trace as u8) => Some(log::LevelFilter::Trace),
            _ => None,
        };

        if let Some(max_level_filter) = max_level_filter {
            log::set_max_level(max_level_filter);
        } else {
            log::set_max_level(log::LevelFilter::Debug);
            warn!("Invalid max log level = {:?}.  Using Debug", max_level);
        }

        std::panic::set_hook(Box::new(|panic_info| {
            error!("Critical error: {}", panic_info);
        }));

        debug!("RingRTC logging system initialized!");
    }

    impl log::Log for rtc_log_Delegate {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            if !self.enabled(record.metadata()) {
                return;
            }

            // Ignore tokio and mio logs. For our purposes they are just noise.
            if record.target().contains("tokio") || record.target().contains("mio::") {
                return;
            }

            let message = format!("{}", record.args());

            (self.log)(rtc_log_Record {
                message: rtc_String::from(&message),
                file: rtc_String::from_or_default(record.file()),
                function: rtc_String::from(record.target()),
                line: rtc_OptionalU32::from_or_default(record.line()),
                level: record.level() as u8,
            });
        }

        fn flush(&self) {}
    }
}
