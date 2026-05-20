//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Setup Android logging object

use jni::{
    Env, JavaVM, jni_sig, jni_str,
    objects::{Global, JObject},
    signature::MethodSignature,
    strings::JNIStr,
};
use log::{Level, Log, Metadata, Record};

use crate::{android::error::AndroidError, common::Result};

/// Log object for interfacing with existing Android logger.
struct AndroidLogger {
    level: Level,
    jvm: JavaVM,
    logger_class: Global<jni::objects::JClass<'static>>,
}

// Method name and signature required of Java logger class
// void log(int level, String message)
const LOGGER_CLASS: &JNIStr = jni_str!("org/signal/ringrtc/Log");
const LOGGER_METHOD: &JNIStr = jni_str!("log");
const LOGGER_SIG: MethodSignature<'static, 'static> = jni_sig!((int, java.lang.String) -> void);

impl Log for AndroidLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // Skip annoying jni module debug messages
            if record.level() == Level::Debug
                && record.module_path().is_some_and(|v| v.starts_with("jni::"))
            {
                return;
            }

            let get_file_name = || -> Option<&str> {
                let file = record.file()?;
                file.split(std::path::MAIN_SEPARATOR_STR).last()
            };

            let message = match (get_file_name(), record.line()) {
                (Some(file), Some(line)) => {
                    format!("{}:{}: {}", file, line, record.args())
                }
                (_, _) => {
                    format!("{}", record.args())
                }
            };

            let level = record.level() as i32;

            let _ = self.jvm.attach_current_thread(|env| -> Result<()> {
                // Attempt to clear any exception before we log anything.
                // We'll rethrow it after logging.
                let exception = env.exception_occurred();
                let had_exception = exception.is_some();
                if had_exception {
                    env.exception_clear();
                }

                let _ = env.with_local_frame(5, |env| -> Result<()> {
                    let msg = match env.new_string(&message) {
                        Ok(v) => JObject::from(v),
                        Err(_) => return Ok(()),
                    };

                    let values = [level.into(), (&msg).into()];

                    // Ignore the result here, can't do anything about it anyway.
                    let _ = env.call_static_method(
                        &self.logger_class,
                        LOGGER_METHOD,
                        &LOGGER_SIG,
                        &values,
                    );
                    Ok(())
                });

                // If we put an exception "on hold" earlier, try to throw it again now.
                if let Some(exception) = exception
                    && had_exception
                {
                    // But check that there hasn't been *another* exception thrown.
                    if env.exception_occurred().is_none() {
                        let _ = env.throw(exception);
                    }
                }

                Ok(())
            });
        }
    }

    fn flush(&self) {}
}

pub fn init_logging(env: &mut Env, level: Level) -> Result<()> {
    // Check if the Logger class contains a good logger method and signature
    if env
        .get_static_method_id(LOGGER_CLASS, LOGGER_METHOD, &LOGGER_SIG)
        .is_err()
    {
        return Err(AndroidError::JniStaticMethodLookup(
            LOGGER_CLASS.to_string(),
            LOGGER_METHOD.to_string(),
            LOGGER_SIG.sig().to_string(),
        )
        .into());
    }

    // JNI cannot lookup classes by name from threads other than the
    // main thread, so stash a global ref to the class now, while
    // we're on the main thread.
    let logger_class = env.find_class(LOGGER_CLASS)?;
    let logger_class = env.new_global_ref(logger_class)?;

    // `Env` cannot be sent across thread boundaries. To be able to use JNI
    // functions in other threads, we must first obtain the `JavaVM` interface
    // which, unlike `Env` is `Send`.
    let jvm = env.get_java_vm()?;
    let logger = AndroidLogger {
        level,
        jvm,
        logger_class,
    };

    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(level.to_level_filter());

    Ok(())
}
