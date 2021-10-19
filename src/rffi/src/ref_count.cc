/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "rffi/api/ref_count.h"
#include "rtc_base/logging.h"

namespace webrtc {
namespace rffi {

// Decrements the ref count of a ref-counted object.
// If the ref count goes to zero, the object is deleted.
RUSTEXPORT void
Rust_decRc(rtc::RefCountInterface* owned_rc) {
  if (!owned_rc) {
    return;
  }

  auto result = owned_rc->Release();
  RTC_LOG(LS_VERBOSE) << "Did it get deleted? " << (result == rtc::RefCountReleaseStatus::kDroppedLastRef);
}

// Increments the ref count of a ref-counted object.
// The borrowed RC becomes an owned RC.
RUSTEXPORT void
Rust_incRc(rtc::RefCountInterface* borrowed_rc) {
  if (!borrowed_rc) {
    return;
  }

  borrowed_rc->AddRef();
}

}
}
