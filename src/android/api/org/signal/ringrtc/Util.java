/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */
package org.signal.ringrtc;

import androidx.annotation.NonNull;

import java.nio.ByteBuffer;
import java.util.Collection;
import java.util.UUID;

public final class Util {
    // Based on https://gist.github.com/jeffjohnson9046/c663dd22bbe6bb0b3f5e.
    public static byte[] getBytesFromUuid(UUID uuid) {
        ByteBuffer bytes = ByteBuffer.wrap(new byte[16]);
        bytes.putLong(uuid.getMostSignificantBits());
        bytes.putLong(uuid.getLeastSignificantBits());

        return bytes.array();
    }

    public static UUID getUuidFromBytes(byte[] bytes) {
        ByteBuffer byteBuffer = ByteBuffer.wrap(bytes);
        Long high = byteBuffer.getLong();
        Long low = byteBuffer.getLong();

        return new UUID(high, low);
    }

    // Convert an array of GroupMemberInfo classes to a byte[] using 32-byte chunks.
    public static byte[] serializeFromGroupMemberInfo(@NonNull Collection<GroupCall.GroupMemberInfo> groupMembers) {
        if (groupMembers != null && groupMembers.size() > 0) {
            // Serialize 16-byte UUID and 65-byte cipher to a byte[] as uuid|cipher|uuid|...
            byte[] serializedGroupMembers = new byte[groupMembers.size() * 81];
            int position = 0;

            for (GroupCall.GroupMemberInfo member : groupMembers) {
                // Copy in the userId UUID as a byte[].
                System.arraycopy(getBytesFromUuid(member.userId), 0, serializedGroupMembers, position, 16);
                position += 16;

                // Copy in the ciphertext.
                System.arraycopy(member.userIdCipherText, 0, serializedGroupMembers, position, 65);
                position += 65;
            }

            return serializedGroupMembers;
        } else {
            // Return an empty array.
            return new byte[0];
        }
    }
}
