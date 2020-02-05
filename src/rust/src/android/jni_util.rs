//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Utility helpers for JNI access

use std::collections::HashMap;

use jni::objects::{GlobalRef, JClass, JList, JObject, JValue};
use jni::JNIEnv;

use crate::android::error::AndroidError;
use crate::common::Result;

/// Wrapper around JNIEnv::call_method() with logging.
pub fn jni_call_method<'a>(
    env: &'a JNIEnv,
    object: JObject,
    name: &str,
    sig: &str,
    args: &[JValue],
) -> Result<JValue<'a>> {
    match env.call_method(object, name, sig, args) {
        Ok(v) => Ok(v),
        Err(e) => Err(AndroidError::JniCallMethod(name.to_string(), sig.to_string(), e).into()),
    }
}

/// Wrapper around JNIEnv::call_static_method() with logging.
#[allow(dead_code)]
pub fn jni_call_static_method<'a>(
    env: &'a JNIEnv,
    class: &str,
    name: &str,
    sig: &str,
    args: &[JValue],
) -> Result<JValue<'a>> {
    match env.call_static_method(class, name, sig, args) {
        Ok(v) => Ok(v),
        Err(_) => Err(AndroidError::JniCallStaticMethod(
            class.to_string(),
            name.to_string(),
            sig.to_string(),
        )
        .into()),
    }
}

/// Wrapper around JNIEnv::new_object() with logging.
pub fn jni_new_object<'a>(
    env: &'a JNIEnv,
    class: &str,
    sig: &str,
    args: &[JValue],
) -> Result<JObject<'a>> {
    match env.new_object(class, sig, args) {
        Ok(v) => Ok(v),
        Err(_) => Err(AndroidError::JniCallConstructor(class.to_string(), sig.to_string()).into()),
    }
}

/// Wrapper around JNIEnv::get_field() with logging.
pub fn jni_get_field<'a>(
    env: &'a JNIEnv,
    obj: JObject,
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
    const LINKED_LIST_CLASS: &str = "java/util/LinkedList";
    const LINKED_LIST_CLASS_SIG: &str = "()V";
    let list = jni_new_object(env, LINKED_LIST_CLASS, LINKED_LIST_CLASS_SIG, &[])?;
    Ok(env.get_list(list)?)
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
