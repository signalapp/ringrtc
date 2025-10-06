//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Utility helpers for JNI access

use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    marker::PhantomData,
};

use jni::{
    JNIEnv,
    objects::{GlobalRef, JClass, JObject, JValue, JValueOwned},
};

use crate::{android::error::AndroidError, common::Result, core::util::try_scoped};
pub use crate::{jni_class_name, jni_signature};

macro_rules! jni_arg {
    ( $arg:expr => boolean ) => {
        <jni::objects::JValue as From<bool>>::from($arg)
    };
    // jni_signature will reject this, but having it do something reasonable avoids multiple errors.
    ( $arg:expr => bool ) => {
        <jni::objects::JValue as From<bool>>::from($arg)
    };
    ( $arg:expr => byte ) => {
        jni::objects::JValue::Byte($arg)
    };
    ( $arg:expr => char ) => {
        jni::objects::JValue::Char($arg)
    };
    ( $arg:expr => short ) => {
        jni::objects::JValue::Short($arg)
    };
    ( $arg:expr => int ) => {
        jni::objects::JValue::Int($arg)
    };
    ( $arg:expr => long ) => {
        jni::objects::JValue::Long($arg)
    };
    ( $arg:expr => float ) => {
        jni::objects::JValue::Float($arg)
    };
    ( $arg:expr => double ) => {
        jni::objects::JValue::Double($arg)
    };
    // Assume anything else is an object. This includes arrays and classes.
    ( $arg:expr => $($_:tt)+) => {
        jni::objects::JValue::Object($arg.as_ref())
    };
}

macro_rules! jni_return_type {
    // Unfortunately there's not a conversion directly from JValue to bool, only jboolean.
    (boolean) => {
        jni::sys::jboolean
    };
    // jni_signature will reject this, but having it do something reasonable avoids multiple errors.
    (bool) => {
        jni::sys::jboolean
    };
    (byte) => {
        jni::sys::jbyte
    };
    (char) => {
        jni::sys::jchar
    };
    (short) => {
        jni::sys::jshort
    };
    (int) => {
        jni::sys::jint
    };
    (long) => {
        jni::sys::jlong
    };
    (float) => {
        jni::sys::jfloat
    };
    (double) => {
        jni::sys::jdouble
    };
    (void) => {
        ()
    };
    // Assume anything else is an object. This includes arrays and classes.
    ($($_:tt)+) => {
        jni::objects::JObject
    };
}

/// Represents a return type, used by [`JniArgs`].
///
/// This is an implementation detail of [`jni_args`] and [`JniArgs`]. Using a function type makes
/// `JniArgs` covariant, which allows the compiler to be less strict about the lifetime marker.
pub type PhantomReturnType<R> = PhantomData<fn() -> R>;

/// A JNI argument list, type-checked with its signature.
#[derive(Debug, Clone, Copy)]
pub struct JniArgs<'local, 'obj_ref, R, const LEN: usize> {
    pub sig: &'static str,
    pub args: [JValue<'local, 'obj_ref>; LEN],
    pub _return: PhantomReturnType<R>,
}

/// Produces a JniArgs struct from the given arguments and return type.
///
/// # Example
///
/// ```
/// let args = jni_args!((name => java.lang.String, 0x3FFF => short) -> void);
/// assert_eq!(args.sig, "(Ljava/lang/String;S)V");
/// assert_eq!(args.args.len(), 2);
/// ```
macro_rules! jni_args {
    (
        (
            $( $arg:expr => $arg_base:tt $(. $arg_rest:ident)* $(:: $arg_nested:ident)* ),* $(,)?
        ) -> $ret_base:tt $(. $ret_rest:ident)* $(:: $ret_nested:ident)*
    ) => {
        JniArgs {
            sig: jni_signature!(
                (
                    $( $arg_base $(. $arg_rest)* $(:: $arg_nested)* ),*
                ) -> $ret_base $(. $ret_rest)* $(:: $ret_nested)*
            ),
            args: [$(jni_arg!($arg => $arg_base)),*],
            _return: PhantomReturnType::<jni_return_type!($ret_base)> {},
        }
    }
}

/// Wrapper around JNIEnv::call_method() with logging.
pub fn jni_call_method<
    'input,
    'output,
    O: AsRef<JObject<'input>>,
    R: TryFrom<JValueOwned<'output>, Error = jni::errors::Error>,
    const LEN: usize,
>(
    env: &mut JNIEnv<'output>,
    object: O,
    name: &'static str,
    args: JniArgs<R, LEN>,
) -> Result<R> {
    env.call_method(object, name, args.sig, &args.args)
        .and_then(|v| v.try_into())
        .map_err(|e| AndroidError::JniCallMethod(name.to_string(), args.sig.to_string(), e).into())
}

/// Wrapper around JNIEnv::call_static_method() with logging.
#[allow(dead_code)]
pub fn jni_call_static_method<
    'output,
    R: TryFrom<JValueOwned<'output>, Error = jni::errors::Error>,
    const LEN: usize,
>(
    env: &mut JNIEnv<'output>,
    class: &'static str,
    name: &'static str,
    args: JniArgs<R, LEN>,
) -> Result<R> {
    env.call_static_method(class, name, args.sig, &args.args)
        .and_then(|v| v.try_into())
        .map_err(|e| AndroidError::JniCallMethod(name.to_string(), args.sig.to_string(), e).into())
}

/// Wrapper around JNIEnv::new_object() with logging.
pub fn jni_new_object<'output, const LEN: usize>(
    env: &mut JNIEnv<'output>,
    class: &'static str,
    args: JniArgs<(), LEN>,
) -> Result<JObject<'output>> {
    match env.new_object(class, args.sig, &args.args) {
        Ok(v) => Ok(v),
        Err(_) => {
            Err(AndroidError::JniCallConstructor(class.to_string(), args.sig.to_string()).into())
        }
    }
}

/// Wrapper around JNIEnv::get_field() with logging.
pub fn jni_get_field<'input, 'output, O: AsRef<JObject<'input>>>(
    env: &mut JNIEnv<'output>,
    object: O,
    name: &'static str,
    ty: &'static str,
) -> Result<JValueOwned<'output>> {
    env.get_field(object, name, ty)
        .map_err(|_| AndroidError::JniGetField(name.to_string(), ty.to_string()).into())
}

/// Creates a new java.util.ArrayList object
pub fn jni_new_arraylist<'output>(
    env: &mut JNIEnv<'output>,
    initial_capacity: usize,
) -> Result<JObject<'output>> {
    jni_new_object(
        env,
        jni_class_name!(java.util.ArrayList),
        jni_args!((initial_capacity.try_into().expect("too big for Java") => int) -> void),
    )
}

/// Prints local and global references to the log.
#[allow(dead_code)]
pub fn dump_references(env: &mut JNIEnv) {
    let _ = env.with_local_frame(5, |env| -> Result<()> {
        info!("Dumping references ->");
        let _ = env.call_static_method(
            jni_class_name!(dalvik.system.VMDebug),
            "dumpReferenceTables",
            jni_signature!(() -> void),
            &[],
        );
        info!("<- Done with references");

        Ok(())
    });
}

/// A cache of Java class objects
///
/// JNI cannot lookup classes by name from threads other than the main
/// thread.  See this FAQ for background:
/// https://developer.android.com/training/articles/perf-jni#faq:-why-didnt-findclass-find-my-class
///
/// The solution here is to look up the class objects at init time on
/// the main thread and cache a global reference to the object for
/// later use.
#[derive(Clone)]
pub struct ClassCache {
    /// HashMap mapping the class name (String) to Java object
    map: HashMap<String, GlobalRef>,
}

impl ClassCache {
    /// Returns an empty cache
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Look up the class specified by `class_name` and store a global
    /// reference to the class object result in the cache.
    ///
    /// * Adding the same class twice is treated as an error.
    /// * If the class lookup fails, return an error.
    pub fn add_class(&mut self, env: &mut JNIEnv, class_name: &str) -> Result<()> {
        if self.map.contains_key(class_name) {
            return Err(AndroidError::ClassCacheDuplicate(class_name.to_string()).into());
        }

        let class_object = match env.find_class(class_name) {
            Ok(v) => v,
            Err(_) => return Err(AndroidError::ClassCacheNotFound(class_name.to_string()).into()),
        };

        let class_ref = env.new_global_ref(JObject::from(class_object))?;
        self.map.insert(String::from(class_name), class_ref);
        Ok(())
    }

    /// Retrieve the class object specified by `class_name` and return it.
    ///
    /// * If the class is not in the cache, return an error.
    pub fn get_class(&self, class_name: &str) -> Result<&JClass<'_>> {
        if let Some(class_ref) = self.map.get(class_name) {
            let object = class_ref.as_obj();
            Ok(<&JClass>::from(object))
        } else {
            Err(AndroidError::ClassCacheLookup(class_name.to_string()).into())
        }
    }
}

/// A wrapper around [`JNIEnv`] that reports uncaught exceptions on destruction.
///
/// Normally JNI handles uncaught exceptions when a native thread is "detached" from the JVM, but
/// RingRTC treats all its callback threads as "daemon" threads that are only detached when the
/// native thread exits. This regains that functionality.
///
/// Because `ExceptionCheckingJNIEnv` implements `Deref`, it should be a drop-in replacement for
/// most uses of JNIEnv as a value. References to JNIEnv should continue as references.
pub struct ExceptionCheckingJNIEnv<'a>(JNIEnv<'a>);

impl Drop for ExceptionCheckingJNIEnv<'_> {
    fn drop(&mut self) {
        let result = try_scoped(|| {
            let exception = self.exception_occurred()?;
            if exception.is_null() {
                return Ok(());
            }
            self.exception_clear()?;
            let thread = jni_call_static_method(
                self,
                jni_class_name!(java.lang.Thread),
                "currentThread",
                jni_args!(() -> java.lang.Thread),
            )?;
            let handler = jni_call_method(
                self,
                &thread,
                "getUncaughtExceptionHandler",
                jni_args!(() -> java.lang.Thread::UncaughtExceptionHandler),
            )?;
            jni_call_method(
                self,
                handler,
                "uncaughtException",
                jni_args!((thread => java.lang.Thread, exception => java.lang.Throwable) -> void),
            )?;
            Ok(())
        });
        match result {
            Ok(()) => {}
            Err(e) => {
                error!("unable to rethrow exception: {e}");
            }
        }
    }
}

impl<'a> From<JNIEnv<'a>> for ExceptionCheckingJNIEnv<'a> {
    fn from(env: JNIEnv<'a>) -> Self {
        Self(env)
    }
}

impl<'a> std::ops::Deref for ExceptionCheckingJNIEnv<'a> {
    type Target = JNIEnv<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ExceptionCheckingJNIEnv<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
