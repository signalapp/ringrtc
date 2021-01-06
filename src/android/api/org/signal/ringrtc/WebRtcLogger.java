/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import org.webrtc.Logging.Severity;

public final class WebRtcLogger implements org.webrtc.Loggable {

  private static final String TAG = Log.class.getSimpleName();

  @Override
  public void onLogMessage(String message, Severity severity, String tag) {

    switch(severity) {
    case LS_NONE:
      // eat it
      break;
    case LS_ERROR:
      Log.e(tag, message); break;
    case LS_WARNING:
      Log.w(tag, message); break;
    case LS_INFO:
      Log.i(tag, message); break;
    case LS_VERBOSE:
      Log.d(tag, message); break;
    default:
      Log.w(TAG, "Unknown log level: " + tag + ", " + message);
    }

  }

}
