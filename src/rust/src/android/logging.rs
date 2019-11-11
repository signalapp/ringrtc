//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Setup Android logging object

use std::sync::{
    mpsc::channel,
    mpsc::Receiver,
    mpsc::Sender,
    Arc,
    Mutex,
};
use std::thread;

use jni::JNIEnv;
use jni::objects::{
    JClass,
    JObject,
};

use log::{
    Log,
    Level,
    LevelFilter,
    Metadata,
    Record,
};

use crate::android::error::AndroidError;
use crate::common::Result;

type LogMessage = (i32, String, String, Sender<()>);

/// Log object for interfacing with existing Android logger.
struct AndroidLogger {
    level:  Level,
    tx:     Arc<Mutex<Sender<LogMessage>>>,
}

// Method name and signature required of Java logger class
// void log(int level, String tag, String message)
const LOGGER_CLASS:  &str = "org/signal/ringrtc/Log";
const LOGGER_METHOD: &str = "log";
const LOGGER_SIG:    &str = "(ILjava/lang/String;Ljava/lang/String;)V";

impl Log for AndroidLogger {

    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {

        if self.enabled(record.metadata()) {
            // Ignore the result here, can't do anything about it
            // anyway.
            if let Ok(tx) = self.tx.lock() {
                let path = match record.module_path() {
                    Some(v) => v,
                    None    => "unknown",
                };
                let (sync_tx, sync_rx) = channel();
                let log_msg = (record.level() as i32,
                               #[cfg(debug_assertions)]
                               format!("{}:{:?}", path, thread::current().id()),
                               #[cfg(not(debug_assertions))]
                               path.to_owned(),
                               format!("{}", record.args()),
                               sync_tx);
                let _ = tx.send(log_msg);
                // wait for logger thread to write out
                let _ = sync_rx.recv();
            }
        }
    }

    fn flush(&self) {
    }
}

pub fn init_logging(env: &JNIEnv, level: Level) -> Result<()> {

    // Check if the Logger class contains a good logger method and signature
    if env.get_static_method_id(LOGGER_CLASS, LOGGER_METHOD, LOGGER_SIG).is_err() {
        return Err(AndroidError::JniStaticMethodLookup(String::from(LOGGER_CLASS),
                                                       String::from(LOGGER_METHOD),
                                                       String::from(LOGGER_SIG)).into());
    }

    // JNI cannot lookup classes by name from threads other than the
    // main thread, so stash a global ref to the class now, while
    // we're on the main thread.
    let logger_class = env.find_class(LOGGER_CLASS)?;
    let logger       = env.new_global_ref(JObject::from(logger_class))?;

    // `JNIEnv` cannot be sent across thread boundaries. To be able to use JNI
    // functions in other threads, we must first obtain the `JavaVM` interface
    // which, unlike `JNIEnv` is `Send`.
    let jvm = env.get_java_vm()?;

    // Create a logging thread that remains attached to the JVM for
    // the life of the application.
    let (tx, rx) : (Sender<LogMessage>, Receiver<LogMessage>) = channel();
    let _ = thread::Builder::new().
        name("ringrtc-logger".into()).spawn(move || {
            // disable logging as we attch to the JVM
            log::set_max_level(LevelFilter::Off);
            if let Ok(env) = jvm.attach_current_thread_as_daemon() {
                log::set_max_level(level.to_level_filter());
                while let Ok((level, module_path, args, sync_tx)) = rx.recv() {
                    // As we are permanently attached to the JVM,
                    // there is no opportunity to automatically clean
                    // up local references.  To clean up as we go,
                    // allocate the local references within a "local
                    // frame", which cleans up the local references
                    // allocated within the scope of the frame.
                    let _ = env.with_local_frame(5, || {
                        let tag = match env.new_string(module_path) {
                            Ok(v)  => JObject::from(v),
                            Err(_) => return Ok(JObject::null()),
                        };

                        let msg = match env.new_string(args) {
                            Ok(v) => JObject::from(v),
                            Err(_) => return Ok(JObject::null()),
                        };

                        let values = [level.into(), tag.into(), msg.into()];

                        // Ignore the result here, can't do anything about it anyway.
                        let _ = env.call_static_method(JClass::from(logger.as_obj()),
                                                       LOGGER_METHOD,
                                                       LOGGER_SIG,
                                                       &values);
                        Ok(JObject::null())
                    });
                    // notify loggee, log operation is complete
                    let _ = sync_tx.send(());
                }
            }
        });

    let logger = AndroidLogger {
        level,
        tx: Arc::new(Mutex::new(tx)),
    };

    log::set_boxed_logger(Box::new(logger))?;
    log::set_max_level(level.to_level_filter());

    Ok(())
}
