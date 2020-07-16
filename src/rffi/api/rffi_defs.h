/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_DEFS_H__
#define RFFI_API_DEFS_H__

/**
 * Common definitions used throughout the Rust RFFI API.
 *
 */

// Public interfaces exported to Rust as "extern C".
#define RUSTEXPORT extern "C" __attribute__((visibility("default")))

// Opaque pointer to a Rust object.
typedef void* rust_object;

/* Ice Update Message structure passed between Rust and c++ */
typedef struct {
  const char* sdp;
} RustIceCandidate;

#endif /* RFFI_API_DEFS_H__ */
