//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Setup Android logging object

use jni::objects::{GlobalRef, JClass, JObject};
use jni::{JNIEnv, JavaVM};

use log::{Level, Log, Metadata, Record};

use crate::android::error::AndroidError;
use crate::android::jni_util::*;
use crate::common::Result;

/// Log object for interfacing with existing Android logger.
struct AndroidLogger {
    level: Level,
    jvm: JavaVM,
    logger: GlobalRef,
}

// Method name and signature required of Java logger class
// void log(int level, String tag, String message)
const LOGGER_CLASS: &str = jni_class_name!(org.signal.ringrtc.Log);
const LOGGER_METHOD: &str = "log";
const LOGGER_SIG: &str = jni_signature!((int, java.lang.String, java.lang.String) -> void);

impl AndroidLogger {
    fn get_java_env(&self) -> Result<JNIEnv> {
        match self.jvm.get_env() {
            Ok(v) => Ok(v),
            Err(_e) => Ok(self.jvm.attach_current_thread_as_daemon()?),
        }
    }
}

impl Log for AndroidLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // Skip annoying jni module debug messages
            if record.level() == Level::Debug
                && record
                    .module_path()
                    .map_or(false, |v| v.starts_with("jni::"))
            {
                return;
            }

            // Use the `JavaVM` interface to attach a `JNIEnv` to the current thread.
            let env = match self.get_java_env() {
                Ok(v) => v,
                Err(_) => return,
            };

            let path = record.module_path().unwrap_or("unknown");

            let level = record.level() as i32;

            let _ = env.with_local_frame(5, || {
                let tag = match env.new_string(path) {
                    Ok(v) => JObject::from(v),
                    Err(_) => return Ok(JObject::null()),
                };

                let msg = match env.new_string(format!("{}", record.args())) {
                    Ok(v) => JObject::from(v),
                    Err(_) => return Ok(JObject::null()),
                };

                let values = [level.into(), tag.into(), msg.into()];

                // Ignore the result here, can't do anything about it
                // anyway.
                let _ = env.call_static_method(
                    JClass::from(self.logger.as_obj()),
                    LOGGER_METHOD,
                    LOGGER_SIG,
                    &values,
                );
                Ok(JObject::null())
            });
        }
    }

    fn flush(&self) {}
}

pub fn init_logging(env: &JNIEnv, level: Level) -> Result<()> {
    // Check if the Logger class contains a good logger method and signature
    if env
        .get_static_method_id(LOGGER_CLASS, LOGGER_METHOD, LOGGER_SIG)
        .is_err()
    {
        return Err(AndroidError::JniStaticMethodLookup(
            String::from(LOGGER_CLASS),
            String::from(LOGGER_METHOD),
            String::from(LOGGER_SIG),
        )
        .into());
    }

    // JNI cannot lookup classes by name from threads other than the
    // main thread, so stash a global ref to the class now, while
    // we're on the main thread.
    let logger_class = env.find_class(LOGGER_CLASS)?;
    let logger = env.new_global_ref(JObject::from(logger_class))?;

    // `JNIEnv` cannot be sent across thread boundaries. To be able to use JNI
    // functions in other threads, we must first obtain the `JavaVM` interface
    // which, unlike `JNIEnv` is `Send`.
    let jvm = env.get_java_vm()?;
    let logger = AndroidLogger { level, jvm, logger };

    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(level.to_level_filter());

    Ok(())
}
