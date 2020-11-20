/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_STATS_OBSERVER_H__
#define RFFI_STATS_OBSERVER_H__

#include "api/peer_connection_interface.h"

namespace webrtc {
namespace rffi {

/**
 * Adapter between the C++ StatsObserver interface
 * and Rust. Wraps an instance of the Rust interface and dispatches
 * C++ callbacks to Rust.
 */

class StatsObserverRffi : public RTCStatsCollectorCallback {
public:
  StatsObserverRffi(const rust_object             stats_observer,
                    const StatsObserverCallbacks* stats_observer_cbs);
  ~StatsObserverRffi() override;

protected:
  void OnStatsDelivered(const rtc::scoped_refptr<const RTCStatsReport>& report) override;

private:
  const rust_object stats_observer_;
  StatsObserverCallbacks stats_observer_cbs_;

  std::vector<AudioSenderStatistics> audio_sender_statistics_;
  std::vector<VideoSenderStatistics> video_sender_statistics_;
  std::vector<AudioReceiverStatistics> audio_receiver_statistics_;
  std::vector<VideoReceiverStatistics> video_receiver_statistics_;
};

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_STATS_OBSERVER_H__ */
