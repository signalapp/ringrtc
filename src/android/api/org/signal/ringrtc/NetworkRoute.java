/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import org.webrtc.PeerConnection;

/**
 *
 * Information about the network route being used for sending audio/video/data
 *
 */
public class NetworkRoute {
  PeerConnection.AdapterType localAdapterType;

  public NetworkRoute() {
    this.localAdapterType = PeerConnection.AdapterType.UNKNOWN;
  }

  public NetworkRoute(PeerConnection.AdapterType localAdapterType) {
    this.localAdapterType = localAdapterType;
  }

  public PeerConnection.AdapterType getLocalAdapterType() {
    return this.localAdapterType;
  }
}