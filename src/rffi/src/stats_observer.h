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

class StatsObserverRffi : public StatsObserver {
public:
  StatsObserverRffi(const rust_object             stats_observer,
                    const StatsObserverCallbacks* stats_observer_cbs);
  ~StatsObserverRffi() override;

protected:
  void OnComplete(const StatsReports& reports) override;

private:
  const rust_object stats_observer_;
  StatsObserverCallbacks stats_observer_cbs_;
};

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_STATS_OBSERVER_H__ */
