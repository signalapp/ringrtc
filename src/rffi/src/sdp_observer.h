/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
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
  // Passed-in observer must live as long as the CreateSessionDescriptionRffi.
  CreateSessionDescriptionObserverRffi(void*                                            csd_observer,
                                       const CreateSessionDescriptionObserverCallbacks* csd_observer_cbs);
  ~CreateSessionDescriptionObserverRffi() override;

  // MediaConstraintsInterface* constraints() { return constraints_.get(); }

  void OnSuccess(SessionDescriptionInterface* session_description) override;
  void OnFailure(RTCError error) override;

 private:
  void* csd_observer_;
  CreateSessionDescriptionObserverCallbacks csd_observer_cbs_;

};

/**
 * Adapter between the C++ SetSessionDescriptionObserver interface and
 * Rust.  Wraps an instance of the Rust interface and dispatches C++
 * callbacks to Rust.
 */

class SetSessionDescriptionObserverRffi : public SetSessionDescriptionObserver {
 public:
  // Passed-in observer must live as long as the SetSessionDescriptionRffi.
  SetSessionDescriptionObserverRffi(void*                                         ssd_observer,
                                    const SetSessionDescriptionObserverCallbacks* ssd_observer_cbs);
  ~SetSessionDescriptionObserverRffi() override;

  void OnSuccess() override;
  void OnFailure(RTCError error) override;

 private:
  void* ssd_observer_;
  SetSessionDescriptionObserverCallbacks ssd_observer_cbs_;

};

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_SDP_OBSERVER_H__ */
