/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
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