/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_PEER_CONNECTION_OBSERVER_INTF_H__
#define RFFI_API_PEER_CONNECTION_OBSERVER_INTF_H__

#include "api/peer_connection_interface.h"
#include "rffi/api/rffi_defs.h"

/**
 * Rust friendly wrapper around a custom class that implements the
 * webrtc::PeerConnectionObserver interface.
 *
 */

namespace webrtc {
namespace rffi {
  class PeerConnectionObserverRffi;
} // namespace rffi
} // namespace webrtc

/* Peer Connection Observer callback function pointers */
typedef struct {
  void (*onIceCandidate)(rust_object, const RustIceCandidate*);
  void (*onIceConnectionChange)(rust_object, webrtc::PeerConnectionInterface::IceConnectionState);
  void (*onAddStream)(rust_object, webrtc::MediaStreamInterface*);
  void (*onSignalingDataChannel)(rust_object, webrtc::DataChannelInterface*);
  void (*onSignalingDataChannelMessage)(rust_object, const uint8_t*, size_t);
} PeerConnectionObserverCallbacks;

RUSTEXPORT webrtc::rffi::PeerConnectionObserverRffi*
Rust_createPeerConnectionObserver(const rust_object                      observer,
                                  const PeerConnectionObserverCallbacks* callbacks);

#endif /* RFFI_API_PEER_CONNECTION_OBSERVER_INTF_H__ */
