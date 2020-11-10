/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;

/**
 *
 * Represents a Http header name/value pair.
 */
public final class HttpHeader {
  @NonNull private final String name;
  @NonNull private final String value;

  public HttpHeader(String name, String value) {
    this.name = name;
    this.value = value;
  }

  @NonNull
  public String getName() {
    return name;
  }

  @NonNull
  public String getValue() {
    return value;
  }
}