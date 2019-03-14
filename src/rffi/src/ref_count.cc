/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "rffi/api/ref_count.h"

namespace webrtc {
namespace rffi {

/*
 * Release reference counted objects
 */
RUSTEXPORT void
Rust_releaseRef(rtc::RefCountInterface* ref_counted_ptr) {
  ref_counted_ptr->Release();
}

/*
 * Add reference to reference counted objects
 */
RUSTEXPORT void
Rust_addRef(rtc::RefCountInterface* ref_counted_ptr) {
  ref_counted_ptr->AddRef();
}

}
}
