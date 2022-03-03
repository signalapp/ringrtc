//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! For passing values to/from another language.

#[cfg(any(target_os = "ios", feature = "check-all"))]
pub mod ios {
    use libc::size_t;

    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_OptionalU16 {
        pub value: u16,
        pub valid: bool,
    }

    impl rtc_OptionalU16 {
        pub fn none() -> Self {
            Self {
                value: 0,
                valid: false,
            }
        }
    }

    impl From<u16> for rtc_OptionalU16 {
        fn from(value: u16) -> Self {
            Self { value, valid: true }
        }
    }

    /// Swift "UInt32?"
    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_OptionalU32 {
        pub value: u32,
        pub valid: bool,
    }

    impl rtc_OptionalU32 {
        pub fn none() -> Self {
            Self {
                value: 0,
                valid: false,
            }
        }
    }

    impl From<u32> for rtc_OptionalU32 {
        fn from(value: u32) -> Self {
            Self { value, valid: true }
        }
    }

    impl From<Option<u32>> for rtc_OptionalU32 {
        fn from(value: Option<u32>) -> Self {
            match value {
                Some(value) => Self::from(value),
                None => Self::none(),
            }
        }
    }

    /// Swift String/Data/[UInt8]/UnsafeBufferPointer<UInt8>
    #[repr(C)]
    #[derive(Debug)]
    pub struct rtc_Bytes<'a> {
        pub ptr: *const u8,
        pub count: size_t,
        phantom: std::marker::PhantomData<&'a u8>,
    }

    impl<'a> rtc_Bytes<'a> {
        pub fn empty() -> Self {
            Self {
                ptr: std::ptr::null(),
                count: 0,
                phantom: std::marker::PhantomData,
            }
        }
    }

    impl<'a> From<&'a [u8]> for rtc_Bytes<'a> {
        fn from(bytes: &[u8]) -> Self {
            Self {
                ptr: bytes.as_ptr(),
                count: bytes.len(),
                phantom: std::marker::PhantomData,
            }
        }
    }

    impl<'a> From<&'a str> for rtc_Bytes<'a> {
        fn from(s: &'a str) -> Self {
            Self::from(s.as_bytes())
        }
    }

    impl<'a, T> From<Option<T>> for rtc_Bytes<'a>
    where
        rtc_Bytes<'a>: From<T>,
    {
        fn from(maybe_bytes: Option<T>) -> Self {
            maybe_bytes.map(Self::from).unwrap_or_else(Self::empty)
        }
    }

    impl<'a> rtc_Bytes<'a> {
        pub fn as_slice(&self) -> &[u8] {
            if self.ptr.is_null() {
                return &[];
            }
            unsafe { std::slice::from_raw_parts(self.ptr, self.count as usize) }
        }

        pub fn to_vec(&self) -> Vec<u8> {
            self.as_slice().to_vec()
        }

        pub fn as_str(&self) -> Option<&str> {
            std::str::from_utf8(self.as_slice()).ok()
        }

        pub fn to_string(&self) -> Option<String> {
            Some(self.as_str()?.to_string())
        }
    }
}
