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

RUSTEXPORT void
Rust_releaseRef(rtc::RefCountInterface *ref_counted_ptr);

RUSTEXPORT void
Rust_addRef(rtc::RefCountInterface *ref_counted_ptr);

#endif /* RFFI_API_SCOPED_REFPTR_H__ */
