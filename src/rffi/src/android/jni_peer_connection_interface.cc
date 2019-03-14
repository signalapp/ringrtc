/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "sdk/android/src/jni/pc/peer_connection.h"
#include "rffi/api/android/peer_connection_interface_intf.h"

#include <string>

namespace webrtc {
namespace rffi {

RUSTEXPORT PeerConnectionInterface*
Rust_getPeerConnectionInterface(jlong owned_peer_connection) {
  return reinterpret_cast<jni::OwnedPeerConnection*>(owned_peer_connection)->pc();
}

} // namespace rffi
} // namespace webrtc
