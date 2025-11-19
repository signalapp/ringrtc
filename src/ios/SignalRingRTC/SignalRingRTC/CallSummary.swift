//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/// Overall call quality statistics.
///
/// Contains connection-level metrics and separate audio/video quality stats.
public struct QualityStats {
    /// Median connection RTT in milliseconds calculated via STUN/ICE,
    /// or nil if unavailable.
    public let rttMedianConnectionMillis: Float32?
    /// Audio quality statistics.
    public let audioStats: MediaQualityStats
    /// Video quality statistics.
    public let videoStats: MediaQualityStats
}

/// Media quality statistics for audio or video streams.
///
/// Contains network quality metrics including RTT, jitter, and packet loss.
public struct MediaQualityStats {
    /// Median RTT in milliseconds calculated via RTP/RTCP, or nil if unavailable.
    public let rttMedianMillis: Float32?
    /// Median jitter for sent packets as reported by remote peer in milliseconds,
    /// or nil if unavailable.
    public let jitterMedianSendMillis: Float32?
    /// Median jitter for received packets in milliseconds, or nil if unavailable.
    public let jitterMedianReceiveMillis: Float32?
    /// Packet loss fraction for sent packets as reported by remote peer,
    /// or nil if unavailable.
    public let packetLossFractionSend: Float32?
    /// Packet loss fraction for received packets, or nil if unavailable.
    public let packetLossFractionReceive: Float32?
}

/// Summary of call telemetry data providing a synopsis of call quality.
///
/// Statistics are captured when the call ends and are available for reporting.
public struct CallSummary {
    /// Call start timestamp in milliseconds since January 1, 1970 00:00:00 UTC.
    public let startTime: UInt64
    /// Call end timestamp in milliseconds since January 1, 1970 00:00:00 UTC.
    public let endTime: UInt64
    /// High-level call quality statistics with cumulative metrics for the entire
    /// call session, including connection-level stats and separate audio/video
    /// quality stats.
    public let qualityStats: QualityStats
    /// Raw call telemetry data containing periodic internal/opaque values for the
    /// last few seconds of the call, or nil if unavailable.
    public let rawStats: Data?
    /// Textual description of raw telemetry data, or nil if unavailable.
    public let rawStatsText: String?
    /// Textual representation of the call end reason.
    public let callEndReasonText: String
    /// Whether the call is eligible for user survey (i.e., the call actually
    /// connected).
    public let isSurveyCandidate: Bool

    static func fromRtc(_ summary: UnsafePointer<rtc_callsummary_CallSummary>) -> Self {
        let summary = summary.pointee

        let qualityStats = QualityStats(
            rttMedianConnectionMillis: summary.rtt_median_connection.asFloat32() ,
            audioStats: MediaQualityStats(
                rttMedianMillis: summary.audio_rtt_median_media.asFloat32(),
                jitterMedianSendMillis: summary.audio_jitter_median_send.asFloat32(),
                jitterMedianReceiveMillis: summary.audio_jitter_median_recv.asFloat32(),
                packetLossFractionSend: summary.audio_packet_loss_fraction_send.asFloat32(),
                packetLossFractionReceive: summary.audio_packet_loss_fraction_recv.asFloat32()
            ),
            videoStats: MediaQualityStats(
                rttMedianMillis: summary.video_rtt_median_media.asFloat32(),
                jitterMedianSendMillis: summary.video_jitter_median_send.asFloat32(),
                jitterMedianReceiveMillis: summary.video_jitter_median_recv.asFloat32(),
                packetLossFractionSend: summary.video_packet_loss_fraction_send.asFloat32(),
                packetLossFractionReceive: summary.video_packet_loss_fraction_recv.asFloat32()
            )
        )

        let rawStats = summary.raw_stats.map { raw_stats in
            Data(bytes: raw_stats, count: Int(summary.raw_stats_len))
        }

        let rawStatsText = summary.raw_stats_text.map { raw_stats_text in
            String(cString: raw_stats_text)
        }

        let callEndReasonText = String(cString: summary.raw_call_end_reason_text)

        return Self(
            startTime: summary.start_time,
            endTime: summary.end_time,
            qualityStats: qualityStats,
            rawStats: rawStats,
            rawStatsText: rawStatsText,
            callEndReasonText: callEndReasonText,
            isSurveyCandidate: summary.is_survey_candidate
        )
    }
}
