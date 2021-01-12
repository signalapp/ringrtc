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
Rust_dataChannelSend(DataChannelInterface* data_channel,
                     const uint8_t*        buf,
                     size_t                len,
                     bool                  binary) {
  bool ret = data_channel->Send(DataBuffer(rtc::CopyOnWriteBuffer(buf, len), binary));
  return ret;
}

} // namespace rffi
} // namespace webrtc
