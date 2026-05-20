//
// Copyright 2019-2026 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

macro_rules! jni_arg {
    ( $arg:expr => boolean ) => {
        jni::objects::JValue::Bool($arg)
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

/// Builds a `(MethodSignature, [JValue; N])` tuple from a Java-style arg list.
macro_rules! jni_args {
    (
        (
            $( $arg:expr => $arg_base:tt $(. $arg_rest:ident)* $(:: $arg_nested:ident)* ),* $(,)?
        ) -> $ret_base:tt $(. $ret_rest:ident)* $(:: $ret_nested:ident)*
    ) => {
        (
            jni::jni_sig!(
                (
                    $( $arg_base $(. $arg_rest)* $(:: $arg_nested)* ),*
                ) -> $ret_base $(. $ret_rest)* $(:: $ret_nested)*
            ),
            [$(jni_arg!($arg => $arg_base)),*],
        )
    }
}

/// Calls `Env::call_method` with the given args. Returns `Result<JValueOwned>`.
///
/// Use trailing `.into_<type>` to unwrap a typed return.
macro_rules! jni_call_method {
    (
        $env:expr, $obj:expr, $name:expr,
        ( $($arg_list:tt)* ) -> $ret_base:tt $(. $ret_rest:ident)* $(:: $ret_nested:ident)*
    ) => {{
        let (sig, args) = jni_args!(
            ( $($arg_list)* ) -> $ret_base $(. $ret_rest)* $(:: $ret_nested)*
        );
        $env.call_method($obj, $name, &sig, &args)
    }};
}

/// Calls `Env::call_static_method` with the given args. Returns `Result<JValueOwned>`.
macro_rules! jni_call_static_method {
    (
        $env:expr, $class:expr, $name:expr,
        ( $($arg_list:tt)* ) -> $ret_base:tt $(. $ret_rest:ident)* $(:: $ret_nested:ident)*
    ) => {{
        let (sig, args) = jni_args!(
            ( $($arg_list)* ) -> $ret_base $(. $ret_rest)* $(:: $ret_nested)*
        );
        $env.call_static_method($class, $name, &sig, &args)
    }};
}

/// Calls `Env::new_object` with the given args. Returns `Result<JObject>`.
///
/// The constructor's JNI return type is always `void`; this macro embeds that.
macro_rules! jni_new_object {
    ( $env:expr, $class:expr, ( $($arg_list:tt)* ) $(,)? ) => {{
        let (sig, args) = jni_args!(( $($arg_list)* ) -> void);
        $env.new_object($class, &sig, &args)
    }};
}
