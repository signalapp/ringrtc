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

RUSTEXPORT bool
Rust_dataChannelSend(webrtc::DataChannelInterface* data_channel,
                     const uint8_t*                buf,
                     size_t                        len,
                     bool                          binary);

RUSTEXPORT bool
Rust_dataChannelIsReliable(webrtc::DataChannelInterface* data_channel);

#endif /* RFFI_API_DATA_CHANNEL_OBSERVER_INTF_H__ */
