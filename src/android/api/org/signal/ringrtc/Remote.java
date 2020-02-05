/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
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
