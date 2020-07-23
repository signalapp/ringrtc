/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_DATA_CHANNEL_OBSERVER_INTF_H__
#define RFFI_API_DATA_CHANNEL_OBSERVER_INTF_H__

#include <cstdint>
#include <stddef.h>
#include "api/data_channel_interface.h"
#include "rffi/api/rffi_defs.h"

/**
 * Rust friendly wrapper for working with objects that implement the
 * webrtc::DataChannelInterface and webrtc::DataChannelObserver
 * interfaces.
 *
 */

namespace webrtc {
namespace rffi {
  class DataChannelObserverRffi;
} // namespace rffi
} // namespace webrtc

/* Data Channel Observer callback function pointers */
typedef struct {
  void (*onStateChange)(rust_object);
  void (*onBufferedAmountChange)(rust_object, uint64_t);
  void (*onMessage)(rust_object, const uint8_t*, size_t, bool);
} DataChannelObserverCallbacks;

RUSTEXPORT webrtc::rffi::DataChannelObserverRffi*
Rust_createDataChannelObserver(const rust_object                   call_connection,
                               const DataChannelObserverCallbacks* dc_observer_cbs);

RUSTEXPORT void
Rust_registerDataChannelObserver(webrtc::DataChannelInterface*          data_channel,
                                 webrtc::rffi::DataChannelObserverRffi* data_channel_observer);

RUSTEXPORT void
Rust_unregisterDataChannelObserver(webrtc::DataChannelInterface*          data_channel,
                                   webrtc::rffi::DataChannelObserverRffi* data_channel_observer);

RUSTEXPORT bool
Rust_dataChannelSend(webrtc::DataChannelInterface* data_channel,
                     const uint8_t*                buf,
                     size_t                        len,
                     bool                          binary);

RUSTEXPORT const char*
Rust_dataChannelGetLabel(webrtc::DataChannelInterface* data_channel);

RUSTEXPORT bool
Rust_dataChannelIsReliable(webrtc::DataChannelInterface* data_channel);

#endif /* RFFI_API_DATA_CHANNEL_OBSERVER_INTF_H__ */
