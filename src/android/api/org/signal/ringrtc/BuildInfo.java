/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

public final class BuildInfo {

  private static final String TAG = Log.class.getSimpleName();
  public boolean debug;

  BuildInfo(boolean debug) {
    this.debug   = debug;
  }

}
