/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

/**
 *
 * An interface that represents the remote side of a peer-to-peer call
 * connection.
 *
 */
public interface Remote {
  public boolean recipientEquals(Remote remote);
}
