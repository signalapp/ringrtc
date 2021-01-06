/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef ANDROID_PEER_CONNECTION_H__
#define ANDROID_PEER_CONNECTION_H__

#include "rffi/api/rffi_defs.h"
#include <jni.h>

/**
 * Rust friendly wrapper to return the underlying
 * PeerConnectionInterface object from a Java jni::OwnedPeerConnection
 * object.
 *
 */
RUSTEXPORT webrtc::PeerConnectionInterface*
Rust_getPeerConnectionFromJniOwnedPeerConnection(jlong owned_peer_connection);

#endif /* ANDROID_PEER_CONNECTION_H__ */
