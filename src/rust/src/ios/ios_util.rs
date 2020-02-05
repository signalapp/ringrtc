//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS utility helpers

use libc::size_t;

/// Structure for passing buffers (such as strings) to Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppByteSlice {
    pub bytes: *const u8,
    pub len:   size_t,
}
