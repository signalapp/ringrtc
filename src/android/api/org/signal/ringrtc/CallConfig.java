/*
 * Copyright 2026 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import java.util.Objects;

public class CallConfig {
  public int dataMode;
  public byte dredDuration;
  public boolean enableVp9Encode;
  public boolean enableVp9Decode;
  public boolean enableH265Encode;
  public boolean enableH265Decode;
  public Integer statsIntervalSecs;

  public CallConfig(int dataMode,
      byte dredDuration,
      boolean enableVp9Encode,
      boolean enableVp9Decode,
      boolean enableH265Encode,
      boolean enableH265Decode,
      Integer statsIntervalSecs) {
    this.dataMode = dataMode;
    this.dredDuration = dredDuration;
    this.enableVp9Encode = enableVp9Encode;
    this.enableVp9Decode = enableVp9Decode;
    this.enableH265Encode = enableH265Encode;
    this.enableH265Decode = enableH265Decode;
    this.statsIntervalSecs = statsIntervalSecs;
  }

  @Override
  public String toString() {
    return "CallConfig{" +
        "dataMode=" + dataMode +
        ", dredDuration=" + dredDuration +
        ", enableVp9Encode=" + enableVp9Encode +
        ", enableVp9Decode=" + enableVp9Decode +
        ", enableH265Encode=" + enableH265Encode +
        ", enableH265Decode=" + enableH265Decode +
        ", statsIntervalSecs=" + statsIntervalSecs +
        "}";
  }

  @Override
  public boolean equals(Object o) {
    if (this == o)
      return true;
    if (o == null || getClass() != o.getClass())
      return false;
    CallConfig that = (CallConfig) o;
    return dataMode == that.dataMode &&
        dredDuration == that.dredDuration &&
        enableVp9Encode == that.enableVp9Encode &&
        enableVp9Decode == that.enableVp9Decode &&
        enableH265Encode == that.enableH265Encode &&
        enableH265Decode == that.enableH265Decode &&
        Objects.equals(statsIntervalSecs, that.statsIntervalSecs);
  }

  @Override
  public int hashCode() {
    return Objects.hash(dataMode, dredDuration, enableVp9Encode, enableVp9Decode, enableH265Encode, enableH265Decode, statsIntervalSecs);
  }
}
