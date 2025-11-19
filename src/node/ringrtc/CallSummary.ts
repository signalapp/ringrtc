//
// Copyright 2019-2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/**
 * Media quality statistics for audio or video streams.
 *
 * Contains network quality metrics including RTT, jitter, and packet loss.
 */

export class MediaQualityStats {
  /**
   * Median RTT in milliseconds calculated via RTP/RTCP, or undefined if unavailable.
   */
  readonly rttMedianMillis: number | undefined;
  /**
   * Median jitter for sent packets as reported by remote peer in milliseconds,
   * or undefined if unavailable.
   */
  readonly jitterMedianSendMillis: number | undefined;
  /**
   * Median jitter for received packets in milliseconds, or undefined if unavailable.
   */
  readonly jitterMedianRecvMillis: number | undefined;
  /**
   * Packet loss percentage for sent packets as reported by remote peer,
   * or undefined if unavailable.
   */
  readonly packetLossPercentageSend: number | undefined;
  /**
   * Packet loss percentage for received packets, or undefined if unavailable.
   */
  readonly packetLossPercentageRecv: number | undefined;
}

/**
 * Overall call quality statistics.
 *
 * Contains connection-level metrics and separate audio/video quality stats.
 */
export class QualityStats {
  /**
   * Median connection RTT in milliseconds calculated via STUN/ICE,
   * or undefined if unavailable.
   */
  readonly rttMedianConnection: number | undefined;
  /** Audio quality statistics. */
  readonly audioStats!: MediaQualityStats;
  /** Video quality statistics. */
  readonly videoStats!: MediaQualityStats;
}

/**
 * Summary of call telemetry data providing a synopsis of call quality.
 *
 * Statistics are captured when the call ends and are available for reporting.
 */
export class CallSummary {
  /**
   * Call start timestamp in milliseconds since January 1, 1970 00:00:00 UTC.
   */
  readonly startTime!: number;
  /**
   * Call end timestamp in milliseconds since January 1, 1970 00:00:00 UTC.
   */
  readonly endTime!: number;
  /**
   * High-level call quality statistics with cumulative metrics for the entire
   * call session, including connection-level stats and separate audio/video
   * quality stats.
   */
  readonly qualityStats!: QualityStats;
  /**
   * Raw call telemetry data containing periodic internal/opaque values for the
   * last few seconds of the call, or undefined if unavailable.
   */
  readonly rawStats: Uint8Array | undefined;
  /**
   * Textual description of raw telemetry data, or undefined if unavailable.
   */
  readonly rawStatsText: string | undefined;
  /**
   * Textual representation of the call end reason.
   */
  readonly callEndReasonText!: string;
  /**
   * Whether the call is eligible for user survey (i.e., the call actually connected).
   */
  readonly isSurveyCandidate!: boolean;
}
