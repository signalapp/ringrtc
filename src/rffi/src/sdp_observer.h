/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_SDP_OBSERVER_H__
#define RFFI_SDP_OBSERVER_H__

#include "api/peer_connection_interface.h"

namespace webrtc {
namespace rffi {

/**
 * Adapter between the C++ CreateSessionDescriptionObserver interface
 * and Rust.  Wraps an instance of the Rust interface and dispatches
 * C++ callbacks to Rust.
 */

class CreateSessionDescriptionObserverRffi : public CreateSessionDescriptionObserver {
 public:
  CreateSessionDescriptionObserverRffi(const rust_object                                csd_observer,
                                       const CreateSessionDescriptionObserverCallbacks* csd_observer_cbs);
  ~CreateSessionDescriptionObserverRffi() override;

  // MediaConstraintsInterface* constraints() { return constraints_.get(); }

  void OnSuccess(SessionDescriptionInterface* desc) override;
  void OnFailure(RTCError error) override;

 private:
  const rust_object csd_observer_;
  CreateSessionDescriptionObserverCallbacks csd_observer_cbs_;

};

/**
 * Adapter between the C++ SetSessionDescriptionObserver interface and
 * Rust.  Wraps an instance of the Rust interface and dispatches C++
 * callbacks to Rust.
 */

class SetSessionDescriptionObserverRffi : public SetSessionDescriptionObserver {
 public:
  SetSessionDescriptionObserverRffi(const rust_object                             ssd_observer,
                                    const SetSessionDescriptionObserverCallbacks* ssd_observer_cbs);
  ~SetSessionDescriptionObserverRffi() override;

  void OnSuccess() override;
  void OnFailure(RTCError error) override;

 private:
  const rust_object ssd_observer_;
  SetSessionDescriptionObserverCallbacks ssd_observer_cbs_;

};

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_SDP_OBSERVER_H__ */
