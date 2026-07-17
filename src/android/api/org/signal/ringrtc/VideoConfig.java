/*
 * Copyright 2026 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import java.util.Objects;

public class VideoConfig {
  public boolean enableHardwareVp9Encode;
  public boolean enableHardwareVp9Decode;
  public boolean enableSoftwareVp9Encode;
  public boolean enableSoftwareVp9Decode;

  public VideoConfig() {
    this.enableHardwareVp9Encode = false;
    this.enableHardwareVp9Decode = false;
    this.enableSoftwareVp9Encode = false;
    this.enableSoftwareVp9Decode = false;
  }

  @Override
  public String toString() {
    return "VideoConfig{" +
           "enableHardwareVp9Encode=" + enableHardwareVp9Encode +
           ", enableHardwareVp9Decode=" + enableHardwareVp9Decode +
           ", enableSoftwareVp9Encode=" + enableSoftwareVp9Encode +
           ", enableSoftwareVp9Encode=" + enableSoftwareVp9Decode +
           "}";
  }

  @Override
  public boolean equals(Object o) {
    if (this == o) return true;
    if (o == null || getClass() != o.getClass()) return false;
    VideoConfig that = (VideoConfig) o;
    return enableHardwareVp9Encode == that.enableHardwareVp9Encode &&
           enableHardwareVp9Decode == that.enableHardwareVp9Decode &&
           enableSoftwareVp9Encode == that.enableSoftwareVp9Encode &&
           enableSoftwareVp9Decode == that.enableSoftwareVp9Decode;
  }

  @Override
  public int hashCode() {
    return Objects.hash(enableHardwareVp9Encode, enableHardwareVp9Decode, enableSoftwareVp9Encode, enableSoftwareVp9Decode);
  }
}
