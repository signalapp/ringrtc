/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

/**
 *
 * Represents the currently joined users and other transient state in a group call.
 */
public final class PeekInfo {
  // These "synthetic" status codes match up with Rust's lite::http::ResponseStatus.

  /**
   * As a peek result, indicates that a call link has expired or been revoked.
   */
  public static final short EXPIRED_CALL_LINK_STATUS = 703;

  /**
   * As a peek result, indicates that a call link is invalid.
   *
   * It may have expired a long time ago.
   */
  public static final short INVALID_CALL_LINK_STATUS = 704;

  @NonNull
  private static final String TAG = PeekInfo.class.getSimpleName();

  @NonNull
  private final List<UUID> joinedMembers;
  @Nullable
  private final UUID       creator;
  @Nullable
  private final String     eraId;
  @Nullable
  private final Long       maxDevices;

  private final long       deviceCountIncludingPendingDevices;

  private final long       deviceCountExcludingPendingDevices;
  @NonNull
  private final List<UUID> pendingUsers;

  public PeekInfo(
    @NonNull  List<UUID> joinedMembers,
    @Nullable UUID       creator,
    @Nullable String     eraId,
    @Nullable Long       maxDevices,
              long       deviceCountIncludingPendingDevices,
              long       deviceCountExcludingPendingDevices,
    @NonNull  List<UUID> pendingUsers
  ) {
    this.joinedMembers = joinedMembers;
    this.creator = creator;
    this.eraId = eraId;
    this.maxDevices = maxDevices;
    this.deviceCountIncludingPendingDevices = deviceCountIncludingPendingDevices;
    this.deviceCountExcludingPendingDevices = deviceCountExcludingPendingDevices;
    this.pendingUsers = pendingUsers;
  }

  @CalledByNative
  private static PeekInfo fromNative(
    @NonNull  List<byte[]> rawJoinedMembers,
    @Nullable byte[]       creator,
    @Nullable String       eraId,
    @Nullable Long         maxDevices,
              long         deviceCountIncludingPendingDevices,
              long         deviceCountExcludingPendingDevices,
    @NonNull  List<byte[]> rawPendingUsers
  ) {
    Log.i(TAG, "fromNative(): joinedMembers.size = " + rawJoinedMembers.size());

    // Create the collections, converting each provided byte[] to a UUID.
    List<UUID> joinedMembers = new ArrayList<UUID>(rawJoinedMembers.size());
    for (byte[] joinedMember : rawJoinedMembers) {
        joinedMembers.add(Util.getUuidFromBytes(joinedMember));
    }
    List<UUID> pendingUsers = new ArrayList<UUID>(rawPendingUsers.size());
    for (byte[] pendingUser : rawPendingUsers) {
        pendingUsers.add(Util.getUuidFromBytes(pendingUser));
    }

    return new PeekInfo(joinedMembers, creator == null ? null : Util.getUuidFromBytes(creator), eraId, maxDevices, deviceCountIncludingPendingDevices, deviceCountExcludingPendingDevices, pendingUsers);
  }

  @NonNull
  public List<UUID> getJoinedMembers() {
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

  /** @deprecated Use {@link #getDeviceCountIncludingPendingDevices()} or {@link #getDeviceCountExcludingPendingDevices()} as appropriate */
  @Deprecated
  public long getDeviceCount() {
    return deviceCountIncludingPendingDevices;
  }

  public long getDeviceCountIncludingPendingDevices() {
    return deviceCountIncludingPendingDevices;
  }

  public long getDeviceCountExcludingPendingDevices() {
    return deviceCountExcludingPendingDevices;
  }

  @NonNull
  public List<UUID> getPendingUsers() {
    return pendingUsers;
  }
}
