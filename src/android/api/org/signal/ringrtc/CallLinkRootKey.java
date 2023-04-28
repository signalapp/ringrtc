/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;

public class CallLinkRootKey {
  @NonNull
  private final byte[] rawKey;

  public CallLinkRootKey(@NonNull String keyString) throws CallException {
    this.rawKey = nativeParseKeyString(keyString);
  }

  public CallLinkRootKey(@NonNull byte[] keyBytes) throws CallException {
    nativeValidateKeyBytes(keyBytes);
    this.rawKey = keyBytes;
  }

  @NonNull
  public static native CallLinkRootKey generate();

  @NonNull
  public static native byte[] generateAdminPasskey();

  @NonNull
  public byte[] deriveRoomId() {
    try {
      return nativeDeriveRoomId(rawKey);
    } catch (CallException e) {
      throw new AssertionError(e);
    }
  }

  /** Returns the internal storage, so don't modify it! */
  @NonNull
  public byte[] getKeyBytes() {
    return rawKey;
  }

  @NonNull @Override
  public String toString() {
    try {
      return nativeToFormattedString(rawKey);
    } catch (CallException e) {
      throw new AssertionError(e);
    }
  }

  // Native-only methods.
  private static native byte[] nativeParseKeyString(String keyString) throws CallException;
  private static native void nativeValidateKeyBytes(byte[] keyBytes) throws CallException;
  private static native byte[] nativeDeriveRoomId(byte[] keyBytes) throws CallException;
  private static native String nativeToFormattedString(byte[] keyBytes) throws CallException;
}