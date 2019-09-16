//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android CallConnectionObserver Implementation.

use jni::{
    JNIEnv,
    JavaVM,
};
use jni::objects::{
    GlobalRef,
    JObject,
    JValue,
};

use jni::sys::jlong;

use crate::android::jni_util::*;
use crate::android::error::AndroidError;
use crate::common::{
    CallId,
    Result,
};
use crate::core::call_connection_observer::{
    ClientEvent,
    CallConnectionObserver,
};

const RINGRTC_PACKAGE:       &str = "org/signal/ringrtc";
const CALL_CONNECTION_CLASS: &str = "CallConnection";
const CALL_EXCEPTION_CLASS:  &str = "CallException";

/// Android CallConnectionObserver
pub struct AndroidCallConnectionObserver {
    /// Java JVM object
    jvm:          JavaVM,
    /// Java object that implements org.signal.ringrtc.CallConnection$Observer
    jcc_observer: GlobalRef,
    /// Java object that implements org.signal.ringrtc.SignalMessageRecipient
    jrecipient:   GlobalRef,
    /// Unique identifier for the call
    call_id:      CallId,
    /// Cache of Java classes needed at runtime
    class_cache:  ClassCache,
}

impl AndroidCallConnectionObserver {

    /// Creates a new AndroidCallConnectionObserver
    pub fn new(env: &JNIEnv, jcc_observer: GlobalRef, call_id: CallId, jrecipient: GlobalRef) -> Result<Self> {

        let mut class_cache = ClassCache::new();
        for class in &[
            "CallConnection$CallEvent",
            "CallConnection$CallError",
            "CallException",
        ] {
            class_cache.add_class(env, &format!("{}/{}", RINGRTC_PACKAGE, class))?;
        }

        let jvm = env.get_java_vm()?;

        Ok(
            Self {
                jvm,
                jcc_observer,
                jrecipient,
                call_id,
                class_cache,
            }
        )

    }

    /// Returns the Java JNIEnv
    ///
    /// Attaches the JVM to the current thread if necessary.
    fn get_java_env(&self) -> Result <JNIEnv> {

        match self.jvm.get_env() {
            Ok(v) => Ok(v),
            Err(_e) => Ok(self.jvm.attach_current_thread_as_daemon()?),
        }

    }

    /// Send the client application a notification via the observer
    fn notify(&self, event: ClientEvent) -> Result<()> {

        let class = "CallEvent";
        let class_path = format!("{}/{}${}", RINGRTC_PACKAGE, CALL_CONNECTION_CLASS, class);
        let class_object = self.class_cache.get_class(&class_path)?;

        let env = self.get_java_env()?;

        // 1. convert rust item into Java enum
        const ENUM_FROM_NATIVE_INDEX_METHOD: &str = "fromNativeIndex";
        let method_signature = format!("(I)L{};", class_path);
        let args = [ JValue::from(event as i32) ];
        let jitem = match env.call_static_method(class_object,
                                                 ENUM_FROM_NATIVE_INDEX_METHOD,
                                                 &method_signature,
                                                 &args) {
            Ok(v) => v.l()?,
            Err(_) => return Err(AndroidError::JniCallStaticMethod(class_path,
                                                                   ENUM_FROM_NATIVE_INDEX_METHOD.to_string(),
                                                                   method_signature.to_string()).into()),
        };

        // 2. invoke observer.onCallEvent(recipient, call_id, event)
        let call_id_jlong = self.call_id as jlong;
        let method = format!("on{}", class);
        let method_signature = format!("(Lorg/signal/ringrtc/SignalMessageRecipient;JL{};)V", class_path);
        let args = [ self.jrecipient.as_obj().into(), call_id_jlong.into(), jitem.into() ];
        let _ = jni_call_method(&env,
                                self.jcc_observer.as_obj(),
                                &method,
                                &method_signature,
                                &args)?;
        Ok(())
    }


    /// Create a Java CallException using `message` as the contents
    pub fn call_exception<'a>(&self, env: &'a JNIEnv, message: String) -> Result<JObject<'a>> {

        let class_path = format!("{}/{}", RINGRTC_PACKAGE, CALL_EXCEPTION_CLASS);
        let class_object = self.class_cache.get_class(&class_path)?;

        let args = [
            JObject::from(env.new_string(message)?).into(),
        ];

        static CALL_EXCEPTION_CLASS_SIG: &str = "(Ljava/lang/String;)V";
        let exception = match env.new_object(class_object,
                                             CALL_EXCEPTION_CLASS_SIG,
                                             &args) {
            Ok(v) => v,
            Err(_) => return Err(AndroidError::JniCallConstructor(class_path,
                                                                  CALL_EXCEPTION_CLASS_SIG.to_string()).into()),
        };

        Ok(exception)
    }

    /// Invoke observer.onCallError(recipient, call_id, exception)
    fn send_exception(&self, env: & JNIEnv, error: JObject) -> Result<()> {

        let call_id_jlong = self.call_id as jlong;
        let method: &str = "onCallError";
        let method_signature: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;JLjava/lang/Exception;)V";
        let args = [ self.jrecipient.as_obj().into(), call_id_jlong.into(), error.into() ];
        jni_call_method(&env, self.jcc_observer.as_obj(),
                        &method, &method_signature,
                        &args)?;
        Ok(())

    }

    /// Send an error message to the client application via the observer
    fn error(&self, error: failure::Error) -> Result<()> {
        let env = self.get_java_env()?;

        // We pass a Java exception to the client error callback
        match error.downcast() {
            Ok(android_error) => {
                match android_error {
                    AndroidError::JniServiceFailure(mut e) => {
                        // This error type contains an exception to
                        // pass up to the application.
                        let global_ref = e.get_global_ref()?;
                        self.send_exception(&env, global_ref.as_obj())?;
                    },
                    _ => {
                        // Create an exception containing the string
                        // represenation of the error code.
                        self.send_exception(&env, self.call_exception(&env, format!("{}", android_error))?)?
                    },
                }
            },
            Err(e) => {
                // Create an exception containing the string
                // represenation of the error code.
                self.send_exception(&env, self.call_exception(&env, format!("{}", e))?)?
            },
        };

        Ok(())
    }

    /// Send an onAddStream message to the client application
    fn on_add_stream(&self, stream: GlobalRef) -> Result<()> {
        let env = self.get_java_env()?;

        // invoke observer.onAddStream(recipient, call_id, stream)
        let call_id_jlong = self.call_id as jlong;
        let method: &str = "onAddStream";
        let method_signature: &str = "(Lorg/signal/ringrtc/SignalMessageRecipient;JLorg/webrtc/MediaStream;)V";
        let args = [ self.jrecipient.as_obj().into(), call_id_jlong.into(), stream.as_obj().into() ];
        let _ = jni_call_method(&env, self.jcc_observer.as_obj(),
                                &method, &method_signature,
                                &args)?;
        Ok(())
    }
}

impl CallConnectionObserver for AndroidCallConnectionObserver {

    type AppMediaStream = GlobalRef;

    fn notify_event(&self, event: ClientEvent) {
        info!("notify_event: {}", event);
        self.notify(event)
            .unwrap_or_else(|e| error!("notify() failed: {}", e));
    }

    fn notify_error(&self, error: failure::Error) {
        info!("notify_error: {}", error);
        self.error(error)
            .unwrap_or_else(|e| error!("error() failed: {}", e));
    }

    fn notify_on_add_stream(&self, stream: Self::AppMediaStream) {
        info!("notify_on_add_stream()");
        self.on_add_stream(stream as GlobalRef)
            .unwrap_or_else(|e| error!("on_add_stream() failed: {}", e));
    }

}
