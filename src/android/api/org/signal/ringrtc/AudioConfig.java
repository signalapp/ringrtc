/*
 * Copyright 2025 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import java.util.Objects;

public class AudioConfig {
  public boolean useOboe;
  public boolean useSoftwareAec;
  public boolean useSoftwareNs;
  public boolean useInputLowLatency;
  public boolean useInputVoiceComm;

  public AudioConfig() {
    this.useOboe = false;
    this.useSoftwareAec = false;
    this.useSoftwareNs = false;
    this.useInputLowLatency = true;
    this.useInputVoiceComm = true;
  }

  @Override
  public String toString() {
    return "AudioConfig{" +
           "useOboe=" + useOboe +
           ", useSoftwareAec=" + useSoftwareAec +
           ", useSoftwareNs=" + useSoftwareNs +
           ", useInputLowLatency=" + useInputLowLatency +
           ", useInputVoiceComm=" + useInputVoiceComm +
           "}";
  }

  @Override
  public boolean equals(Object o) {
    if (this == o) return true;
    if (o == null || getClass() != o.getClass()) return false;
    AudioConfig that = (AudioConfig) o;
    return useOboe == that.useOboe &&
           useSoftwareAec == that.useSoftwareAec &&
           useSoftwareNs == that.useSoftwareNs &&
           useInputLowLatency == that.useInputLowLatency &&
           useInputVoiceComm == that.useInputVoiceComm;
  }

  @Override
  public int hashCode() {
    return Objects.hash(useOboe, useSoftwareAec, useSoftwareNs, useInputLowLatency, useInputVoiceComm);
  }
}
