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
  void (*onIceCandidatesRemoved)(rust_object);
  void (*onSignalingChange)(rust_object, webrtc::PeerConnectionInterface::SignalingState);
  void (*onIceConnectionChange)(rust_object, webrtc::PeerConnectionInterface::IceConnectionState);
  void (*onConnectionChange)(rust_object, webrtc::PeerConnectionInterface::PeerConnectionState);
  void (*onIceConnectionReceivingChange)(rust_object);
  void (*onIceGatheringChange)(rust_object, webrtc::PeerConnectionInterface::IceGatheringState);
  void (*onAddStream)(rust_object, webrtc::MediaStreamInterface*);
  void (*onRemoveStream)(rust_object);
  void (*onDataChannel)(rust_object, webrtc::DataChannelInterface*);
  void (*onRenegotiationNeeded)(rust_object);
  void (*onAddTrack)(rust_object);
  void (*onTrack)(rust_object);
} PeerConnectionObserverCallbacks;

RUSTEXPORT webrtc::rffi::PeerConnectionObserverRffi*
Rust_createPeerConnectionObserver(const rust_object                      call_connection,
                                  const PeerConnectionObserverCallbacks* pc_observer_cbs);

#endif /* RFFI_API_PEER_CONNECTION_OBSERVER_INTF_H__ */
