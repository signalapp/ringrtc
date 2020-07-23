/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
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

RUSTEXPORT const char*
Rust_dataChannelGetLabel(DataChannelInterface* data_channel) {
  std::string label = data_channel->label();
  return strdup(&label[0u]);
}

RUSTEXPORT bool
Rust_dataChannelIsReliable(DataChannelInterface* data_channel) {
  return data_channel->reliable();
}

} // namespace rffi
} // namespace webrtc
