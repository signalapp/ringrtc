/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;

import java.lang.Comparable;

/**
 *
 * Represents a unique call identifier.
 *
 * Internally the call identifier is stored as a 64-bit integer.
 *
 */
public final class CallId implements Comparable<CallId> {
  private final String TAG = CallId.class.getSimpleName();

  @NonNull private final Long callId;

  /**
   *
   * Create a new CallId from an existing Long object.
   *
   * @param callId  64-bit call identifier.
   */
  public CallId(Long callId) {
    this.callId = callId;
  }

  /**
   *
   * Returns a Long object representation of the CallId
   *
   * @return  The internal Long representation.
   */
  public Long longValue() {
    return callId;
  }

  /**
   *
   * Formats the CallId with an additional integer appended
   *
   * @param   id  Integer representing a remote device Id
   * @return  A String representation of CallId plus the deviceId.
   */
  public String format(Integer id) {
    return this + "-" + id;
  }

  @Override
  public String toString() {
    return "0x" + Long.toHexString(callId);
  }

  @Override
  public boolean equals(Object obj) {
    if (obj == this) {
      return true;
    }
    if (obj != null) {
      if (this.getClass() == obj.getClass()) {
        CallId  that = (CallId)obj;
        return this.compareTo(that) == 0;
      }
    }
    return false;
  }

  @Override
  public int hashCode() {
    return callId.hashCode();
  }

  @Override
  public int compareTo(CallId obj) {
    return callId.compareTo(obj.callId);
  }

}
