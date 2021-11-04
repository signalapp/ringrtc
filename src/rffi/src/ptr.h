/*
 * Copyright 2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_POINTERS_H__
#define RFFI_POINTERS_H__

#include "api/scoped_refptr.h"

namespace webrtc {
namespace rffi {

// This just makes it easier to read.
// Calling the rtc::scoped_refptr constructor doesn't make it very clear that
// it increments the ref count.
template <typename T>
rtc::scoped_refptr<T> inc_rc(T* borrowed_rc) {
    return rtc::scoped_refptr<T>(borrowed_rc);
}

// This just makes it easier to read.
// Calling the rtc::scoped_refptr::release() doesn't make it very clear that
// it prevents decrementing the RC.
// The caller now owns an RC.
template <typename T>
T* take_rc(rtc::scoped_refptr<T> scoped) {
    return scoped.release();
}

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_PEER_CONNECTION_OBSERVER_H__ */
