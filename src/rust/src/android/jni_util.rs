//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Utility helpers for JNI access

use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::marker::PhantomData;

use jni::objects::{GlobalRef, JClass, JList, JObject, JValue};
use jni::JNIEnv;

use crate::android::error::AndroidError;
use crate::common::Result;

pub use crate::jni_class_name;
pub use crate::jni_signature;

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
        jni::objects::JValue::Object($arg.into())
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
pub struct JniArgs<'a, R, const LEN: usize> {
    pub sig: &'static str,
    pub args: [JValue<'a>; LEN],
    pub _return: PhantomReturnType<R>,
}

impl<'a, const LEN: usize> JniArgs<'a, (), LEN> {
    pub fn returning_void(sig: &'static str, args: [JValue<'a>; LEN]) -> Self {
        Self {
            sig,
            args,
            _return: PhantomData,
        }
    }
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
pub fn jni_call_method<'a, R, const ARG_LEN: usize>(
    env: &'a JNIEnv,
    object: JObject<'a>,
    name: &str,
    args: JniArgs<'a, R, ARG_LEN>,
) -> Result<R>
where
    R: TryFrom<JValue<'a>, Error = jni::errors::Error>,
{
    env.call_method(object, name, args.sig, &args.args)
        .and_then(|v| v.try_into())
        .map_err(|e| AndroidError::JniCallMethod(name.to_string(), args.sig.to_string(), e).into())
}

/// Wrapper around JNIEnv::call_static_method() with logging.
#[allow(dead_code)]
pub fn jni_call_static_method<'a, R, const ARG_LEN: usize>(
    env: &'a JNIEnv,
    class: &str,
    name: &str,
    args: JniArgs<'a, R, ARG_LEN>,
) -> Result<R>
where
    R: TryFrom<JValue<'a>, Error = jni::errors::Error>,
{
    env.call_static_method(class, name, args.sig, &args.args)
        .and_then(|v| v.try_into())
        .map_err(|e| AndroidError::JniCallMethod(name.to_string(), args.sig.to_string(), e).into())
}

/// Wrapper around JNIEnv::new_object() with logging.
pub fn jni_new_object<'a, const ARG_LEN: usize>(
    env: &'a JNIEnv,
    class: &str,
    args: JniArgs<'a, (), ARG_LEN>,
) -> Result<JObject<'a>> {
    match env.new_object(class, args.sig, &args.args) {
        Ok(v) => Ok(v),
        Err(_) => {
            Err(AndroidError::JniCallConstructor(class.to_string(), args.sig.to_string()).into())
        }
    }
}

/// Wrapper around JNIEnv::get_field() with logging.
pub fn jni_get_field<'a>(
    env: &'a JNIEnv,
    obj: JObject<'a>,
    name: &str,
    ty: &str,
) -> Result<JValue<'a>> {
    match env.get_field(obj, name, ty) {
        Ok(v) => Ok(v),
        Err(_) => Err(AndroidError::JniGetField(name.to_string(), ty.to_string()).into()),
    }
}

/// Creates a new java.util.LinkedList object
pub fn jni_new_linked_list<'a>(env: &'a JNIEnv) -> Result<JList<'a, 'a>> {
    // create empty java linked list object
    let list = jni_new_object(
        env,
        jni_class_name!(java.util.LinkedList),
        jni_args!(() -> void),
    )?;
    Ok(env.get_list(list)?)
}

/// Prints local and global references to the log.
#[allow(dead_code)]
pub fn dump_references(env: &JNIEnv) {
    let _ = env.with_local_frame(5, || {
        info!("Dumping references ->");
        let _ = env.call_static_method(
            jni_class_name!(dalvik.system.VMDebug),
            "dumpReferenceTables",
            jni_signature!(() -> void),
            &[],
        );
        info!("<- Done with references");

        Ok(JObject::null())
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
    pub fn add_class(&mut self, env: &JNIEnv, class_name: &str) -> Result<()> {
        let class_string = String::from(class_name);
        if self.map.contains_key(&class_string) {
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
    pub fn get_class(&self, class_name: &str) -> Result<JClass> {
        let class_string = String::from(class_name);
        if let Some(class_ref) = self.map.get(&class_string) {
            Ok(JClass::from(class_ref.as_obj()))
        } else {
            Err(AndroidError::ClassCacheLookup(class_name.to_string()).into())
        }
    }
}
