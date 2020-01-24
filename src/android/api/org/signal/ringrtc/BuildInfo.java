/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

package org.signal.ringrtc;

public final class BuildInfo {

  private static final String TAG = Log.class.getSimpleName();
  public boolean debug;

  BuildInfo(boolean debug) {
    this.debug   = debug;
  }

}
