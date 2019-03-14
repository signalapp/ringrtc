/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_SDP_OBSERVER_INTF_H__
#define RFFI_API_SDP_OBSERVER_INTF_H__

#include "api/peer_connection_interface.h"
#include "rffi/api/rffi_defs.h"

/**
 * Rust friendly wrapper for creating objects that implement the
 * webrtc::CreateSessionDescriptionObserver and
 * webrtc::SetSessionDescriptionObserver interfaces.
 *
 */

namespace webrtc {
namespace rffi {
  class CreateSessionDescriptionObserverRffi;
  class SetSessionDescriptionObserverRffi;
} // namespace rffi
} // namespace webrtc

/* Create Session Description Observer callback function pointers */
typedef struct {
  void (*onSuccess)(rust_object, webrtc::SessionDescriptionInterface*);
  void (*onFailure)(rust_object, const char* err_message, int32_t err_type);
} CreateSessionDescriptionObserverCallbacks;

RUSTEXPORT webrtc::rffi::CreateSessionDescriptionObserverRffi*
Rust_createCreateSessionDescriptionObserver(const rust_object                                csd_observer,
                                            const CreateSessionDescriptionObserverCallbacks* csd_observer_cbs);

/* Set Session Description Observer callback function pointers */
typedef struct {
  void (*onSuccess)(rust_object);
  void (*onFailure)(rust_object, const char* err_message, int32_t err_type);
} SetSessionDescriptionObserverCallbacks;

RUSTEXPORT webrtc::rffi::SetSessionDescriptionObserverRffi*
Rust_createSetSessionDescriptionObserver(const rust_object                             ssd_observer,
                                         const SetSessionDescriptionObserverCallbacks* ssd_observer_cbs);

#endif /* RFFI_API_SDP_OBSERVER_INTF_H__ */
