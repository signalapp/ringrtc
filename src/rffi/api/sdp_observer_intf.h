/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
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
  void (*onSuccess)(void* csd_observer_borrowed, webrtc::SessionDescriptionInterface* session_description_owned_rc);
  void (*onFailure)(void* csd_observer_borrowed, const char* err_message_borrowed, int32_t err_type);
} CreateSessionDescriptionObserverCallbacks;

RUSTEXPORT webrtc::rffi::CreateSessionDescriptionObserverRffi*
Rust_createCreateSessionDescriptionObserver(void*                                            csd_observer_borrowed,
                                            const CreateSessionDescriptionObserverCallbacks* csd_observer_cbs_borrowed);

/* Set Session Description Observer callback function pointers */
typedef struct {
  void (*onSuccess)(void* ssd_observer_borrowed);
  void (*onFailure)(void* ssd_observer_borrowed, const char* err_message_borrowed, int32_t err_type);
} SetSessionDescriptionObserverCallbacks;

RUSTEXPORT webrtc::rffi::SetSessionDescriptionObserverRffi*
Rust_createSetSessionDescriptionObserver(void*                                         ssd_observer_borrowed,
                                         const SetSessionDescriptionObserverCallbacks* ssd_observer_cbs_borrowed);

#endif /* RFFI_API_SDP_OBSERVER_INTF_H__ */
