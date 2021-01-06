/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;

import org.webrtc.CapturerObserver;

/**
 *
 * An interface for controlling the camera devices
 *
 */
public interface CameraControl {

  public boolean hasCapturer();

  public void initCapturer(@NonNull CapturerObserver observer);

  public void setEnabled(boolean enable);

  public void flip();

}
