//
// Copyright 2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Macros to generate JNI signature strings.
//!
//! Located outside of the `android` module so that the tests get run.

#![allow(unused_macros)]

/// Takes a Java-esque class name of the form `org.signal.Outer::Inner` and turns it into a
/// JNI-style name `org/signal/Outer$Inner`.
#[macro_export]
macro_rules! jni_class_name {
    ( $arg_base:tt $(. $arg_rest:ident)+ $(:: $arg_nested:ident)* ) => {
        concat!(
            stringify!($arg_base),
            $("/", stringify!($arg_rest),)+
            $("$", stringify!($arg_nested),)*
        )
    }
}

#[test]
fn test_jni_class_name() {
    assert_eq!(jni_class_name!(foo.bar), "foo/bar");
    assert_eq!(jni_class_name!(foo.bar.baz), "foo/bar/baz");
    assert_eq!(jni_class_name!(foo.bar.baz::garply), "foo/bar/baz$garply");
    assert_eq!(
        jni_class_name!(foo.bar.baz::garply::qux),
        "foo/bar/baz$garply$qux"
    );
}

/// Converts a function or type signature to a JNI signature string.
///
/// This macro uses Rust function syntax `(Foo, Bar) -> Baz`, and uses Rust syntax for Java arrays
/// `[Foo]`, but otherwise uses Java names for types: `boolean`, `byte`, `void`. Like
/// [`jni_class_name`], inner classes are indicated with `::` rather than `.`.
#[macro_export]
macro_rules! jni_signature {
    ( boolean ) => ("Z");
    ( bool ) => (compile_error!("use Java type 'boolean'"));
    ( byte ) => ("B");
    ( char ) => ("C");
    ( short ) => ("S");
    ( int ) => ("I");
    ( long ) => ("J");
    ( float ) => ("F");
    ( double ) => ("D");
    ( void ) => ("V");

    // Escape hatch: provide a literal string.
    ( $x:literal ) => ($x);

    // Arrays
    ( [$($contents:tt)+] ) => {
        concat!("[", jni_signature!($($contents)+))
    };

    // Classes
    ( $arg_base:tt $(. $arg_rest:ident)+ $(:: $arg_nested:ident)* ) => {
        concat!(
            "L",
            jni_class_name!($arg_base $(. $arg_rest)+ $(:: $arg_nested)*),
            ";"
        )
    };

    // Functions
    (
        (
            $( $arg_base:tt $(. $arg_rest:ident)* $(:: $arg_nested:ident)* ),* $(,)?
        ) -> $ret_base:tt $(. $ret_rest:ident)* $(:: $ret_nested:ident)*
    ) => {
        concat!(
            "(",
            $( jni_signature!($arg_base $(. $arg_rest)* $(:: $arg_nested)*), )*
            ")",
            jni_signature!($ret_base $(. $ret_rest)* $(:: $ret_nested)*)
        )
    };
}

#[test]
fn test_jni_signature() {
    // Literals
    assert_eq!(jni_signature!("Lfoo/bar;"), "Lfoo/bar;");

    // Classes
    assert_eq!(jni_signature!(foo.bar), "Lfoo/bar;");
    assert_eq!(jni_signature!(foo.bar.baz), "Lfoo/bar/baz;");
    assert_eq!(jni_signature!(foo.bar.baz::garply), "Lfoo/bar/baz$garply;");
    assert_eq!(
        jni_signature!(foo.bar.baz::garply::qux),
        "Lfoo/bar/baz$garply$qux;"
    );

    // Arrays
    assert_eq!(jni_signature!([byte]), "[B");
    assert_eq!(jni_signature!([[byte]]), "[[B");
    assert_eq!(jni_signature!([foo.bar]), "[Lfoo/bar;");
    assert_eq!(
        jni_signature!([foo.bar.baz::garply::qux]),
        "[Lfoo/bar/baz$garply$qux;"
    );

    // Functions
    assert_eq!(jni_signature!(() -> void), "()V");
    assert_eq!(jni_signature!((byte, int) -> float), "(BI)F");
    assert_eq!(
        jni_signature!(([byte], foo.bar, foo.bar.baz::garply::qux) -> [byte]),
        "([BLfoo/bar;Lfoo/bar/baz$garply$qux;)[B"
    );
    assert_eq!(jni_signature!(() -> foo.bar), "()Lfoo/bar;");
    assert_eq!(
        jni_signature!(() -> foo.bar.baz::garply::qux),
        "()Lfoo/bar/baz$garply$qux;"
    );
}
