/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
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
  // Passed-in observer must live as long as the StatsObserverRffi.
  StatsObserverRffi(void*                         stats_observer_borrowed,
                    const StatsObserverCallbacks* stats_observer_cbs_borrowed);
  ~StatsObserverRffi() override;

protected:
  void OnStatsDelivered(const rtc::scoped_refptr<const RTCStatsReport>& report) override;

private:
  void* stats_observer_;
  StatsObserverCallbacks stats_observer_cbs_;

  std::vector<AudioSenderStatistics> audio_sender_statistics_;
  std::vector<VideoSenderStatistics> video_sender_statistics_;
  std::vector<AudioReceiverStatistics> audio_receiver_statistics_;
  std::vector<VideoReceiverStatistics> video_receiver_statistics_;
};

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_STATS_OBSERVER_H__ */
