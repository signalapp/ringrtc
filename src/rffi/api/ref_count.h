/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

/*
 * Rust friendly wrappers for:
 *
 *   rtc::RefCountInterface::Release();
 *   rtc::RefCountInterface::AddRef();
 */

#ifndef RFFI_API_SCOPED_REFPTR_H__
#define RFFI_API_SCOPED_REFPTR_H__

#include "rffi/api/rffi_defs.h"
#include "rtc_base/ref_count.h"

// Decrements the ref count of a ref-counted object.
// If the ref count goes to zero, the object is deleted.
RUSTEXPORT void
Rust_decRc(rtc::RefCountInterface* owned_rc);

// Increments the ref count of a ref-counted object.
// The borrowed RC becomes an owned RC.
RUSTEXPORT void
Rust_incRc(rtc::RefCountInterface* borrowed_rc);

#endif /* RFFI_API_SCOPED_REFPTR_H__ */
