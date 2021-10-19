/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "api/data_channel_interface.h"
#include "rtc_base/logging.h"

#include "rffi/api/data_channel_intf.h"

#include <string>

namespace webrtc {
namespace rffi {

RUSTEXPORT bool
Rust_dataChannelSend(DataChannelInterface* data_channel_borrowed_rc,
                     const uint8_t*        buf_borrowed,
                     size_t                len,
                     bool                  binary) {
  return data_channel_borrowed_rc->Send(DataBuffer(rtc::CopyOnWriteBuffer(buf_borrowed, len), binary));
}

} // namespace rffi
} // namespace webrtc
