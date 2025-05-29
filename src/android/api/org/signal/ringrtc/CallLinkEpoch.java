/*
 * Copyright 2019-2025 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;

/**
 * Represents a call link epoch. When a call link is created, the calling server will issue
 * a randomly-generated "epoch". This gets included in the URL and in requests to use the link.
 * If it is mismatched, the request will fail.
 *
 * Internally, the epoch is stored as a 32-bit integer.
 */
public final class CallLinkEpoch {
  private final String TAG = CallLinkEpoch.class.getSimpleName();

  private final int epoch;

  /**
   *
   * Create a new CallLinkEpoch from a raw integer value.
   *
   * @param epoch 32-bit epoch identifier.
   *
   */
  @CalledByNative
  private CallLinkEpoch(int epoch) {
    this.epoch = epoch;
  }

  /**
   *
   * Create a new CallLinkEpoch from a consonant base-16 encoded string.
   *
   * @param epoch Encoded string.
   *
   */
  public CallLinkEpoch(@NonNull String epoch) throws CallException {
    this.epoch = nativeParse(epoch);
  }

  public static final int SERIALIZED_SIZE = 4;

  /**
   *
   * Serializes this {@code CallLinkEpoch} into a byte array.
   *
   * @return Byte array.
   *
   */
  @NonNull
  public byte[] getBytes() {
    return new byte[]{
            (byte) ((epoch & 0x000000ff)),
            (byte) ((epoch & 0x0000ff00) >> 8),
            (byte) ((epoch & 0x00ff0000) >> 16),
            (byte) ((epoch & 0xff000000) >> 24)
    };
  }

  /**
   *
   * Deserializes a {@link CallLinkEpoch} from the given byte array.
   *
   * @param bytes Byte array
   * @throws IllegalArgumentException if {@code bytes} does not contain enough data.
   *
   * @return Instance of {@link CallLinkEpoch}.
   *
   */
  @NonNull
  public static CallLinkEpoch fromBytes(@NonNull byte[] bytes) {
    return fromBytes(bytes, 0);
  }

  /**
   *
   * Deserializes a {@link CallLinkEpoch} from the given byte array.
   *
   * @param bytes Byte array
   * @param from  Starting offset
   * @throws IllegalArgumentException if {@code bytes} does not contain enough data.
   *
   * @return Instance of {@link CallLinkEpoch}.
   *
   */
  @NonNull
  public static CallLinkEpoch fromBytes(@NonNull byte[] bytes, int from) {
    if (bytes.length - from < SERIALIZED_SIZE) {
      throw new IllegalArgumentException("length");
    }
    int epoch = (bytes[from] & 0xff)  |
            (bytes[from + 1] & 0xff)  << 8 |
            (bytes[from + 2] & 0xff) << 16 |
            (bytes[from + 3] & 0xff) << 24;
    return new CallLinkEpoch(epoch);
  }

  /**
   *
   * Creates a consonant base-16 encoded string representation of the epoch.
   *
   * @return String value.
   *
   */
  @NonNull
  @Override
  public String toString() {
    try {
      return nativeToFormattedString(epoch);
    } catch (CallException e) {
      throw new AssertionError(e);
    }
  }

  @Override
  public boolean equals(Object obj) {
    if (obj == this) {
      return true;
    }
    if (obj != null) {
      if (this.getClass() == obj.getClass()) {
        CallLinkEpoch that = (CallLinkEpoch)obj;
        return this.epoch == that.epoch;
      }
    }
    return false;
  }

  @Override
  public int hashCode() {
    return epoch;
  }

  private static native int nativeParse(String s) throws CallException;
  private static native String nativeToFormattedString(int v) throws CallException;
}
