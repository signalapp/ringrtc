/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_DATA_CHANNEL_OBSERVER_H__
#define RFFI_DATA_CHANNEL_OBSERVER_H__

#include "api/data_channel_interface.h"

/**
 * Adapter between the C++ DataChannelObserver interface and the Rust
 * data_channel_observer interface.  Wraps an instance of the Rust
 * interface and dispatches C++ callbacks to Rust.
 */

namespace webrtc {
namespace rffi {

class DataChannelObserverRffi : public DataChannelObserver {
 public:
  DataChannelObserverRffi(const rust_object                   call_connection,
                          const DataChannelObserverCallbacks* dc_observer_cbs);
  ~DataChannelObserverRffi() override;

  // Implementation of DataChannelObserver interface, which propagates
  // the callbacks to the Rust observer.
  void OnBufferedAmountChange(uint64_t previous_amount) override;
  void OnStateChange() override;
  void OnMessage(const DataBuffer& buffer) override;

 private:
  const rust_object call_connection_;
  DataChannelObserverCallbacks dc_observer_cbs_;

};

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_DATA_CHANNEL_OBSERVER_H__ */
