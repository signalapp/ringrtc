/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */
package org.signal.ringrtc;

import androidx.annotation.NonNull;
import android.media.MediaCodecInfo;
import android.media.MediaFormat;

import org.webrtc.EglBase;
import org.webrtc.HardwareVideoDecoderFactory;
import org.webrtc.HardwareVideoEncoderFactory;
import org.webrtc.Predicate;

import java.nio.ByteBuffer;
import java.util.Collection;
import java.util.UUID;

public final class Util {
    private static final String  TAG = "RingRtcUtil";

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

    private static boolean supportsMimeType(MediaCodecInfo info, String mimeType) {
        for (String supportedType : info.getSupportedTypes()) {
            if (supportedType.equals(mimeType)) {
                return true;
            }
        }
        return false;
    }

    public static boolean deviceSupportsHardwareEncoder(EglBase eglBase, String mimeType) {
        if (eglBase == null) {
            return false;
        }

        HardwareVideoEncoderFactory hwFactory = new HardwareVideoEncoderFactory(
            eglBase.getEglBaseContext(), true, true, includeMimeType(mimeType));

        if (hwFactory.getSupportedCodecs().length == 0) {
            Log.w(TAG, "No supported " + mimeType + " hardware encoder found");
            return false;
        }

        return true;
    }

    public static boolean deviceSupportsHardwareDecoder(EglBase eglBase, String mimeType) {
        if (eglBase == null) {
            return false;
        }

        HardwareVideoDecoderFactory hwFactory = new HardwareVideoDecoderFactory(
            eglBase.getEglBaseContext(), includeMimeType(mimeType));

        if (hwFactory.getSupportedCodecs().length == 0) {
            Log.w(TAG, "No supported " + mimeType + " hardware decoder found");
            return false;
        }

        return true;
    }

    private static Predicate<MediaCodecInfo> excludeMimeType(String mimeType) {
        return info -> !supportsMimeType(info, mimeType);
    }

    private static Predicate<MediaCodecInfo> includeMimeType(String mimeType) {
        return info -> supportsMimeType(info, mimeType);
    }

    /**
     * Builds the codec-allowed predicate for an encoder/decoder factory.
     * Returns null (allow everything the factory finds) when nothing needs filtering.
     */
    public static Predicate<MediaCodecInfo> codecFilter(boolean allowVp9, boolean allowH265) {
        Predicate<MediaCodecInfo> filter = null;
        if (!allowVp9) {
            filter = excludeMimeType(MediaFormat.MIMETYPE_VIDEO_VP9);
        }
        if (!allowH265) {
            Predicate<MediaCodecInfo> h265Filter = excludeMimeType(MediaFormat.MIMETYPE_VIDEO_HEVC);
            filter = (filter == null) ? h265Filter : filter.and(h265Filter);
        }
        return filter;
    }
}
