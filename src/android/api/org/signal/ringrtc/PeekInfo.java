/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import java.util.Collection;
import java.util.UUID;

/**
 *
 * Represents the currently joined users and other transient state in a group call.
 */
public final class PeekInfo {
  @NonNull
  private final String TAG = PeekInfo.class.getSimpleName();

  @NonNull
  private final Collection<UUID> joinedMembers;
  @Nullable
  private final UUID             creator;
  @Nullable
  private final String           eraId;
  @Nullable
  private final Long             maxDevices;

  private final long             deviceCount;

  public PeekInfo(
    @NonNull  Collection<UUID> joinedMembers,
    @Nullable UUID             creator,
    @Nullable String           eraId,
    @Nullable Long             maxDevices,
              long             deviceCount
  ) {
    this.joinedMembers = joinedMembers;
    this.creator = creator;
    this.eraId = eraId;
    this.maxDevices = maxDevices;
    this.deviceCount = deviceCount;
  }

  @NonNull
  public Collection<UUID> getJoinedMembers() {
    return joinedMembers;
  }

  @Nullable
  public UUID getCreator() {
    return creator;
  }

  @Nullable
  public String getEraId() {
    return eraId;
  }

  @Nullable
  public Long getMaxDevices() {
    return maxDevices;
  }

  public long getDeviceCount() {
    return deviceCount;
  }
}
