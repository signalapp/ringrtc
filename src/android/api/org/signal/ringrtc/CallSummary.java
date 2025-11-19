/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

/**
 * Summary of call telemetry data providing a synopsis of call quality.
 *
 * Statistics are captured when the call ends and are available for reporting.
 */
public class CallSummary {
    /**
     * Media quality statistics for audio or video streams.
     *
     * Contains network quality metrics including RTT, jitter, and packet loss.
     */
    public static class MediaQualityStats {
        @Nullable
        private final Float rttMedianMillis;
        @Nullable
        private final Float jitterMedianSendMillis;
        @Nullable
        private final Float jitterMedianRecvMillis;
        @Nullable
        private final Float packetLossPercentageSend;
        @Nullable
        private final Float packetLossPercentageRecv;

        /**
         * Creates media quality statistics.
         *
         * @param rttMedianMillis median RTT in milliseconds calculated via RTP/RTCP
         * @param jitterMedianSendMillis median jitter for sent packets as reported by
         *                               remote peer in milliseconds
         * @param jitterMedianRecvMillis median jitter for received packets in milliseconds
         * @param packetLossPercentageSend packet loss percentage for sent packets as
         *                                 reported by remote peer
         * @param packetLossPercentageRecv packet loss percentage for received packets
         */
        @CalledByNative
        MediaQualityStats(
            @Nullable Float rttMedianMillis,
            @Nullable Float jitterMedianSendMillis,
            @Nullable Float jitterMedianRecvMillis,
            @Nullable Float packetLossPercentageSend,
            @Nullable Float packetLossPercentageRecv
        ) {
            this.rttMedianMillis = rttMedianMillis;
            this.jitterMedianSendMillis = jitterMedianSendMillis;
            this.jitterMedianRecvMillis = jitterMedianRecvMillis;
            this.packetLossPercentageSend = packetLossPercentageSend;
            this.packetLossPercentageRecv = packetLossPercentageRecv;
        }

        /**
         * @return Median RTT in milliseconds calculated via RTP/RTCP, or {@code null} if
         *         unavailable.
         */
        @Nullable
        public Float getRttMedianMillis() {
            return rttMedianMillis;
        }

        /**
         * @return Median jitter for sent packets as reported by remote peer in milliseconds,
         *         or {@code null} if unavailable.
         */
        @Nullable
        public Float getJitterMedianSendMillis() {
            return jitterMedianSendMillis;
        }

        /**
         * @return Median jitter for received packets in milliseconds, or {@code null} if
         *         unavailable.
         */
        @Nullable
        public Float getJitterMedianRecvMillis() {
            return jitterMedianRecvMillis;
        }

        /**
         * @return Packet loss percentage for sent packets as reported by remote peer,
         *         or {@code null} if unavailable.
         */
        @Nullable
        public Float getPacketLossPercentageSend() {
            return packetLossPercentageSend;
        }

        /**
         * @return Packet loss percentage for received packets, or {@code null} if
         *         unavailable.
         */
        @Nullable
        public Float getPacketLossPercentageRecv() {
            return packetLossPercentageRecv;
        }
    }

    /**
     * Overall call quality statistics.
     *
     * Contains connection-level metrics and separate audio/video quality stats.
     */
    public static class QualityStats {
        private final @Nullable Float             rttMedianConnectionMillis;
        private final @NonNull  MediaQualityStats audioStats;
        private final @NonNull  MediaQualityStats videoStats;

        /**
         * Creates quality statistics.
         *
         * @param rttMedianConnectionMillis median connection RTT in milliseconds
         *                                  calculated via STUN/ICE
         * @param audioStats audio quality statistics
         * @param videoStats video quality statistics
         */
        @CalledByNative
        QualityStats(
                @Nullable Float             rttMedianConnectionMillis,
                @NonNull  MediaQualityStats audioStats,
                @NonNull  MediaQualityStats videoStats
        ) {
            this.rttMedianConnectionMillis = rttMedianConnectionMillis;
            this.audioStats = audioStats;
            this.videoStats = videoStats;
        }

        /**
         * @return Median connection RTT in milliseconds calculated via STUN/ICE,
         *         or {@code null} if unavailable.
         */
        @Nullable
        public Float getRttMedianConnectionMillis() {
            return rttMedianConnectionMillis;
        }

        /** @return Audio quality statistics. */
        @NonNull
        public MediaQualityStats getAudioStats() {
            return audioStats;
        }

        /** @return Video quality statistics. */
        @NonNull
        public MediaQualityStats getVideoStats() {
            return videoStats;
        }
    }

    private final long              startTime;

    private final long              endTime;

    @NonNull
    private final QualityStats      qualityStats;

    @Nullable
    private final byte[]            rawStats;

    @Nullable
    private final String            rawStatsText;

    @NonNull
    private final String            callEndReasonText;

    private final boolean           isSurveyCandidate;

    /**
     * Creates a call summary.
     *
     * @param startTime call start timestamp in milliseconds since January 1, 1970
     *                  00:00:00 UTC
     * @param endTime call end timestamp in milliseconds since January 1, 1970
     *                00:00:00 UTC
     * @param qualityStats call quality statistics
     * @param rawStats raw telemetry data
     * @param rawStatsText textual description of raw telemetry data
     * @param callEndReasonText textual representation of call end reason
     * @param isSurveyCandidate whether the call is eligible for survey (i.e., the call
     *                          actually connected)
     */
    @CalledByNative
    CallSummary(
                  long          startTime,
                  long          endTime,
        @NonNull  QualityStats  qualityStats,
        @Nullable byte[]        rawStats,
        @Nullable String        rawStatsText,
        @NonNull  String        callEndReasonText,
                  boolean       isSurveyCandidate
    ) {
        this.startTime = startTime;
        this.endTime = endTime;
        this.qualityStats = qualityStats;
        this.rawStats = rawStats;
        this.rawStatsText = rawStatsText;
        this.callEndReasonText = callEndReasonText;
        this.isSurveyCandidate = isSurveyCandidate;
    }

    /**
     * @return Call start timestamp in milliseconds since January 1, 1970 00:00:00 UTC.
     */
    public long getStartTime() {
        return startTime;
    }

    /**
     * @return Call end timestamp in milliseconds since January 1, 1970 00:00:00 UTC.
     */
    public long getEndTime() {
        return endTime;
    }

    /**
     * @return High-level call quality statistics with cumulative metrics for the entire
     *         call session, including connection-level stats and separate audio/video
     *         quality stats.
     */
    @NonNull
    public QualityStats getQualityStats() {
        return qualityStats;
    }

    /**
     * @return Raw call telemetry data containing periodic internal/opaque values for the
     *         last few seconds of the call, or {@code null} if unavailable.
     */
    @Nullable
    public byte[] getRawStats() {
        return rawStats;
    }

    /**
     * @return Textual description of raw telemetry data, or {@code null} if unavailable.
     */
    @Nullable
    public String getRawStatsText() {
        return rawStatsText;
    }

    /**
     * @return Textual representation of the call end reason.
     */
    @NonNull
    public String getCallEndReasonText() {
        return callEndReasonText;
    }

    /**
     * @return {@code true} if the call is eligible for user survey (i.e., the call
     *         actually connected).
     */
    public boolean isSurveyCandidate() {
        return isSurveyCandidate;
    }
}
