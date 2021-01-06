/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.Nullable;

/**
 *
 * Represents an ICE candiate used for signaling.
 */
public final class IceCandidate {
  private final String TAG = IceCandidate.class.getSimpleName();

  @Nullable private final byte[] opaque;
  @Nullable private final String sdp;

  public IceCandidate(byte[] opaque, String sdp) {
    this.opaque = opaque;
    this.sdp = sdp;
  }

  @Nullable
  public byte[] getOpaque() {
    return opaque;
  }

  @Nullable
  public String getSdp() {
    return sdp;
  }
}