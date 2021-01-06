/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

public class Log {

  private static final String TAG = Log.class.getSimpleName();

  // Log levels corresponding to rust's Log::Level values
  private static final int LL_ERROR = 1;
  private static final int LL_WARN  = 2;
  private static final int LL_INFO  = 3;
  private static final int LL_DEBUG = 4;
  private static final int LL_TRACE = 5;

  private static Log.Logger logger;

  public static void initialize(Log.Logger logger) {
    Log.logger = logger;
  }

  public static void log(int level, String tag, String message) {

    switch(level) {
    case LL_ERROR:
      e(tag, message); break;
    case LL_WARN:
      w(tag, message); break;
    case LL_INFO:
      i(tag, message); break;
    case LL_DEBUG:
      d(tag, message); break;
    case LL_TRACE:
      v(tag, message); break;
    default:
      w(TAG, "Unknown log level: " + tag + ", " + message);
    }

  }

  public static void v(String tag, String message) {
    v(tag, message, null);
  }

  public static void d(String tag, String message) {
    d(tag, message, null);
  }

  public static void i(String tag, String message) {
    i(tag, message, null);
  }

  public static void w(String tag, String message) {
    w(tag, message, null);
  }

  public static void e(String tag, String message) {
    e(tag, message, null);
  }

  public static void v(String tag, Throwable t) {
    v(tag, null, t);
  }

  public static void d(String tag, Throwable t) {
    d(tag, null, t);
  }

  public static void i(String tag, Throwable t) {
    i(tag, null, t);
  }

  public static void w(String tag, Throwable t) {
    w(tag, null, t);
  }

  public static void e(String tag, Throwable t) {
    e(tag, null, t);
  }

  public static void v(String tag, String message, Throwable t) {
    if (logger != null) {
      logger.v(tag, message, t);
    } else {
      android.util.Log.v(tag, message, t);
    }
  }

  public static void d(String tag, String message, Throwable t) {
    if (logger != null) {
      logger.d(tag, message, t);
    } else {
      android.util.Log.d(tag, message, t);
    }
  }

  public static void i(String tag, String message, Throwable t) {
    if (logger != null) {
      logger.i(tag, message, t);
    } else {
      android.util.Log.i(tag, message, t);
    }
  }

  public static void w(String tag, String message, Throwable t) {
    if (logger != null) {
      logger.w(tag, message, t);
    } else {
      android.util.Log.w(tag, message, t);
    }
  }

  public static void e(String tag, String message, Throwable t) {
    if (logger != null) {
      logger.e(tag, message, t);
    } else {
      android.util.Log.e(tag, message, t);
    }
  }

  public interface Logger {
    void v(String tag, String message, Throwable t);

    void d(String tag, String message, Throwable t);

    void i(String tag, String message, Throwable t);

    void w(String tag, String message, Throwable t);

    void e(String tag, String message, Throwable t);
  }

}
