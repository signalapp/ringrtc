/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_STATS_OBSERVER_INTF_H__
#define RFFI_API_STATS_OBSERVER_INTF_H__

#include "api/peer_connection_interface.h"
#include "rffi/api/rffi_defs.h"

/**
 * Rust friendly wrapper for creating objects that implement the
 * webrtc::StatsObserver interface.
 *
 */

namespace webrtc {
namespace rffi {
  class StatsObserverRffi;
} // namespace rffi
} // namespace webrtc

typedef struct {
  int32_t audio_packets_sent = {-1};
  int32_t audio_packets_sent_lost = {-1};
  int64_t audio_rtt = {-1};
  int32_t audio_packets_received = {-1};
  int32_t audio_packets_received_lost = {-1};
  int32_t audio_jitter_received = {-1};
  float_t audio_expand_rate = {-1.0};
  float_t audio_accelerate_rate = {-1.0};
  float_t audio_preemptive_rate = {-1.0};
  float_t audio_speech_expand_rate = {-1.0};
  int32_t audio_preferred_buffer_size_ms = {-1};
} StatsObserverValues;

/* Stats Observer Callback callback function pointers */
typedef struct {
  void (*OnStatsComplete)(rust_object, StatsObserverValues *values);
} StatsObserverCallbacks;

RUSTEXPORT webrtc::rffi::StatsObserverRffi*
Rust_createStatsObserver(const rust_object             stats_observer,
                         const StatsObserverCallbacks* stats_observer_cbs);

#endif /* RFFI_API_STATS_OBSERVER_INTF_H__ */
