/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "rffi/api/data_channel.h"
#include "rffi/api/data_channel_intf.h"
#include "rffi/src/data_channel_observer.h"
#include "rtc_base/logging.h"

namespace webrtc {
namespace rffi {

DataChannelObserverRffi::DataChannelObserverRffi(const rust_object call_connection,
                                                 const DataChannelObserverCallbacks* dc_observer_cbs)
  : call_connection_(call_connection), dc_observer_cbs_(*dc_observer_cbs)
{
  RTC_LOG(LS_INFO) << "DataChannelObserverRffi:ctor(): " << this->call_connection_;
}

DataChannelObserverRffi::~DataChannelObserverRffi() {
  RTC_LOG(LS_INFO) << "DataChannelObserverRffi:dtor(): " << this->call_connection_;
}

void DataChannelObserverRffi::OnBufferedAmountChange(uint64_t previous_amount) {
  dc_observer_cbs_.onBufferedAmountChange(call_connection_, previous_amount);
}

void DataChannelObserverRffi::OnStateChange() {
  dc_observer_cbs_.onStateChange(call_connection_);
}

void DataChannelObserverRffi::OnMessage(const DataBuffer& buffer) {
  dc_observer_cbs_.onMessage(call_connection_, buffer.data.cdata(), buffer.size(), buffer.binary);
}

RUSTEXPORT DataChannelObserverRffi*
Rust_createDataChannelObserver(const rust_object call_connection,
                               const DataChannelObserverCallbacks* dc_observer_cbs) {
  return new DataChannelObserverRffi(call_connection, dc_observer_cbs);
}

RUSTEXPORT void
Rust_registerDataChannelObserver(DataChannelInterface*    data_channel,
                                 DataChannelObserverRffi* data_channel_observer) {
  data_channel->RegisterObserver(data_channel_observer);
}

RUSTEXPORT void
Rust_unregisterDataChannelObserver(DataChannelInterface*    data_channel,
                                   DataChannelObserverRffi* data_channel_observer) {
  data_channel->UnregisterObserver();
  delete data_channel_observer;
}


} // namespace rffi
} // namespace webrtc
