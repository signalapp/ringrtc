/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_API_PEER_CONNECTION_OBSERVER_INTF_H__
#define RFFI_API_PEER_CONNECTION_OBSERVER_INTF_H__

#include "api/peer_connection_interface.h"
#include "rffi/api/rffi_defs.h"
#include "rffi/api/network.h"
#include "rtc_base/network_constants.h"

/**
 * Rust friendly wrapper around a custom class that implements the
 * webrtc::PeerConnectionObserver interface.
 *
 */

namespace webrtc {
namespace rffi {
  class PeerConnectionObserverRffi;

  /* NetworkRoute structure passed between Rust and C++ */
  typedef struct {
     rtc::AdapterType local_adapter_type;
  } NetworkRoute;
} // namespace rffi
} // namespace webrtc

/* Peer Connection Observer callback function pointers */
typedef struct {
  // ICE events
  void (*onIceCandidate)(rust_object, const RustIceCandidate*);
  void (*onIceCandidatesRemoved)(rust_object, const webrtc::rffi::IpPort*, size_t);
  void (*onIceConnectionChange)(rust_object, webrtc::PeerConnectionInterface::IceConnectionState);
  void (*onIceNetworkRouteChange)(rust_object, webrtc::rffi::NetworkRoute);

  // Media events
  void (*onAddStream)(rust_object, webrtc::MediaStreamInterface*);
  void (*onAddAudioRtpReceiver)(rust_object, webrtc::MediaStreamTrackInterface*);
  void (*onAddVideoRtpReceiver)(rust_object, webrtc::MediaStreamTrackInterface*);

  // Data Channel events
  void (*onSignalingDataChannel)(rust_object, webrtc::DataChannelInterface*);
  void (*onSignalingDataChannelMessage)(rust_object, const uint8_t*, size_t);
  void (*onRtpReceived)(rust_object, uint8_t, uint16_t, uint32_t, uint32_t, const uint8_t*, size_t);

  // Frame encryption
  size_t (*getMediaCiphertextBufferSize)(rust_object, bool, size_t);
  bool (*encryptMedia)(rust_object, bool, const uint8_t*, size_t, uint8_t*, size_t, size_t*);
  size_t (*getMediaPlaintextBufferSize)(rust_object, uint32_t, bool, size_t);
  bool (*decryptMedia)(rust_object, uint32_t, bool, const uint8_t*, size_t, uint8_t*, size_t, size_t*);
} PeerConnectionObserverCallbacks;

RUSTEXPORT webrtc::rffi::PeerConnectionObserverRffi*
Rust_createPeerConnectionObserver(const rust_object                      observer,
                                  const PeerConnectionObserverCallbacks* callbacks,
                                  bool enable_frame_encryption);

#endif /* RFFI_API_PEER_CONNECTION_OBSERVER_INTF_H__ */
