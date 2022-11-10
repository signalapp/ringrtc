//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! For passing values to/from another language.

#[cfg(any(target_os = "ios", feature = "check-all"))]
pub mod ios {
    use libc::size_t;

    pub trait FromOrDefault<S>: From<S> + Default {
        fn from_or_default(maybe: Option<S>) -> Self;
    }

    impl<S, T: From<S> + Default> FromOrDefault<S> for T {
        fn from_or_default(maybe: Option<S>) -> Self {
            maybe.map(Self::from).unwrap_or_default()
        }
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct rtc_OptionalU16 {
        pub value: u16,
        pub valid: bool,
    }

    impl From<u16> for rtc_OptionalU16 {
        fn from(value: u16) -> Self {
            Self { value, valid: true }
        }
    }

    /// Swift "UInt32?"
    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct rtc_OptionalU32 {
        pub value: u32,
        pub valid: bool,
    }

    impl From<u32> for rtc_OptionalU32 {
        fn from(value: u32) -> Self {
            Self { value, valid: true }
        }
    }

    /// Swift Data/[UInt8]/UnsafeBufferPointer<UInt8>
    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_Bytes<'a> {
        pub ptr: *const u8,
        pub count: size_t,
        phantom: std::marker::PhantomData<&'a u8>,
    }

    impl<'a> Default for rtc_Bytes<'a> {
        fn default() -> Self {
            Self {
                ptr: std::ptr::null(),
                count: 0,
                phantom: std::marker::PhantomData,
            }
        }
    }

    impl<'a, T: AsRef<[u8]> + ?Sized> From<&'a T> for rtc_Bytes<'a> {
        fn from(bytes: &'a T) -> Self {
            let bytes = bytes.as_ref();
            Self {
                ptr: bytes.as_ptr(),
                count: bytes.len(),
                phantom: std::marker::PhantomData,
            }
        }
    }

    impl<'a> rtc_Bytes<'a> {
        pub fn as_slice(&self) -> &[u8] {
            if self.ptr.is_null() {
                return &[];
            }
            unsafe { std::slice::from_raw_parts(self.ptr, self.count) }
        }

        pub fn to_vec(&self) -> Vec<u8> {
            self.as_slice().to_vec()
        }
    }

    /// Swift String
    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_String<'a> {
        pub ptr: *const u8,
        pub count: size_t,
        phantom: std::marker::PhantomData<&'a u8>,
    }

    impl<'a> Default for rtc_String<'a> {
        fn default() -> Self {
            Self {
                ptr: std::ptr::null(),
                count: 0,
                phantom: std::marker::PhantomData,
            }
        }
    }

    impl<'a, T: AsRef<str> + ?Sized> From<&'a T> for rtc_String<'a> {
        fn from(s: &'a T) -> Self {
            let s = s.as_ref();
            Self {
                ptr: s.as_ptr(),
                count: s.len(),
                phantom: std::marker::PhantomData,
            }
        }
    }

    impl<'a> rtc_String<'a> {
        pub fn as_str(&self) -> Option<&str> {
            if self.ptr.is_null() {
                return None;
            }
            let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.count) };
            std::str::from_utf8(bytes).ok()
        }

        pub fn to_string(&self) -> Option<String> {
            Some(self.as_str()?.to_string())
        }
    }
}
