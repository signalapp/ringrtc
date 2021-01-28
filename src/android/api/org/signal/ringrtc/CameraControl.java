/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

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

  /**
   *
   * Sets the orientation of the camera to a rotation defined by the
   * application. The orientation (how the device is rotated) can be
   * any of the integer degree values (i.e. 0, 90, 180, 270) defined
   * by: CameraCharacteristics::SENSOR_ORIENTATION
   *
   * If not called or if null is set as the orientation, the default
   * behavior will be used (CameraSession.getDeviceOrientation()).
   *
   * @param orientation  the current rotation value of the device
   *
   */
  public void setOrientation(@Nullable Integer orientation);

}
