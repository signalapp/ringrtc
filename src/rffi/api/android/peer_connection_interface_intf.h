/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
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
Rust_getPeerConnectionInterface(jlong owned_peer_connection);

#endif /* ANDROID_PEER_CONNECTION_H__ */
