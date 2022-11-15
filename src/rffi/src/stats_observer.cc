/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "rffi/api/stats_observer_intf.h"
#include "rffi/src/ptr.h"
#include "rffi/src/stats_observer.h"
#include "api/stats/rtcstats_objects.h"

namespace webrtc {
namespace rffi {

StatsObserverRffi::StatsObserverRffi(void*                         stats_observer,
                                     const StatsObserverCallbacks* stats_observer_cbs)
        : stats_observer_(stats_observer), stats_observer_cbs_(*stats_observer_cbs)
{
  RTC_LOG(LS_INFO) << "StatsObserverRffi:ctor(): " << this->stats_observer_;
}

StatsObserverRffi::~StatsObserverRffi() {
  RTC_LOG(LS_INFO) << "StatsObserverRffi:dtor(): " << this->stats_observer_;
}

void StatsObserverRffi::OnStatsDelivered(const rtc::scoped_refptr<const RTCStatsReport>& report) {
//  RTC_LOG(LS_INFO) << report->ToJson();

  this->audio_sender_statistics_.clear();
  this->video_sender_statistics_.clear();
  this->audio_receiver_statistics_.clear();
  this->video_receiver_statistics_.clear();

  auto outbound_stream_stats = report->GetStatsOfType<RTCOutboundRTPStreamStats>();
  auto inbound_stream_stats = report->GetStatsOfType<RTCInboundRTPStreamStats>();
  auto candidate_pair_stats = report->GetStatsOfType<RTCIceCandidatePairStats>();

  for (const auto& stat : outbound_stream_stats) {
    if (*stat->kind == "audio") {
      AudioSenderStatistics audio_sender = {0};

      audio_sender.ssrc = stat->ssrc.ValueOrDefault(0);
      audio_sender.packets_sent = stat->packets_sent.ValueOrDefault(0);
      audio_sender.bytes_sent = stat->bytes_sent.ValueOrDefault(0);

      if (stat->remote_id.is_defined()) {
        auto remote_stat = report->GetAs<RTCRemoteInboundRtpStreamStats>(*stat->remote_id);
        if (remote_stat) {
          audio_sender.remote_packets_lost = remote_stat->packets_lost.ValueOrDefault(0);
          audio_sender.remote_jitter = remote_stat->jitter.ValueOrDefault(0.0);
          audio_sender.remote_round_trip_time = remote_stat->round_trip_time.ValueOrDefault(0.0);
        }
      }

      if (stat->media_source_id.is_defined()) {
        auto audio_source_stat = report->GetAs<RTCAudioSourceStats>(*stat->media_source_id);
        if (audio_source_stat) {
          audio_sender.total_audio_energy = audio_source_stat->total_audio_energy.ValueOrDefault(0.0);
          audio_sender.echo_likelihood = audio_source_stat->echo_likelihood.ValueOrDefault(0.0);
        }
      }

      this->audio_sender_statistics_.push_back(audio_sender);
    } else if (*stat->kind == "video") {
      VideoSenderStatistics video_sender = {0};

      video_sender.ssrc = stat->ssrc.ValueOrDefault(0);
      video_sender.packets_sent = stat->packets_sent.ValueOrDefault(0);
      video_sender.bytes_sent = stat->bytes_sent.ValueOrDefault(0);
      video_sender.frames_encoded = stat->frames_encoded.ValueOrDefault(0);
      video_sender.key_frames_encoded = stat->key_frames_encoded.ValueOrDefault(0);
      video_sender.total_encode_time = stat->total_encode_time.ValueOrDefault(0.0);
      video_sender.frame_width = stat->frame_width.ValueOrDefault(0);
      video_sender.frame_height = stat->frame_height.ValueOrDefault(0);
      video_sender.retransmitted_packets_sent = stat->retransmitted_packets_sent.ValueOrDefault(0);
      video_sender.retransmitted_bytes_sent = stat->retransmitted_bytes_sent.ValueOrDefault(0);
      video_sender.total_packet_send_delay = stat->total_packet_send_delay.ValueOrDefault(0.0);
      video_sender.nack_count = stat->nack_count.ValueOrDefault(0);
      video_sender.pli_count = stat->pli_count.ValueOrDefault(0);
      if (stat->quality_limitation_reason.is_defined()) {
        // "none" = 0 (the default)
        if (*stat->quality_limitation_reason == "cpu") {
          video_sender.quality_limitation_reason = 1;
        } else if (*stat->quality_limitation_reason == "bandwidth") {
          video_sender.quality_limitation_reason = 2;
        } else {
          video_sender.quality_limitation_reason = 3;
        }
      }
      video_sender.quality_limitation_resolution_changes = stat->quality_limitation_resolution_changes.ValueOrDefault(0);

      if (stat->remote_id.is_defined()) {
        auto remote_stat = report->GetAs<RTCRemoteInboundRtpStreamStats>(*stat->remote_id);
        if (remote_stat) {
          video_sender.remote_packets_lost = remote_stat->packets_lost.ValueOrDefault(0);
          video_sender.remote_jitter = remote_stat->jitter.ValueOrDefault(0.0);
          video_sender.remote_round_trip_time = remote_stat->round_trip_time.ValueOrDefault(0.0);
        }
      }

      this->video_sender_statistics_.push_back(video_sender);
    }
  }

  for (const auto& stat : inbound_stream_stats) {
    if (*stat->kind == "audio") {
      AudioReceiverStatistics audio_receiver = {0};

      audio_receiver.ssrc = stat->ssrc.ValueOrDefault(0);
      audio_receiver.packets_received = stat->packets_received.ValueOrDefault(0);
      audio_receiver.packets_lost = stat->packets_lost.ValueOrDefault(0);
      audio_receiver.bytes_received = stat->bytes_received.ValueOrDefault(0);
      audio_receiver.jitter = stat->jitter.ValueOrDefault(0.0);

      if (stat->track_id.is_defined()) {
        auto track_stat = report->GetAs<RTCMediaStreamTrackStats>(*stat->track_id);
        if (track_stat) {
          audio_receiver.total_audio_energy = track_stat->total_audio_energy.ValueOrDefault(0.0);
        }
      }

      this->audio_receiver_statistics_.push_back(audio_receiver);
    } else if (*stat->kind == "video") {
      VideoReceiverStatistics video_receiver = {0};

      video_receiver.ssrc = stat->ssrc.ValueOrDefault(0);
      video_receiver.packets_received = stat->packets_received.ValueOrDefault(0);
      video_receiver.packets_lost = stat->packets_lost.ValueOrDefault(0);
      video_receiver.bytes_received = stat->bytes_received.ValueOrDefault(0);
      video_receiver.frames_decoded = stat->frames_decoded.ValueOrDefault(0);
      video_receiver.key_frames_decoded = stat->key_frames_decoded.ValueOrDefault(0);
      video_receiver.total_decode_time = stat->total_decode_time.ValueOrDefault(0.0);

      if (stat->track_id.is_defined()) {
        auto track_stat = report->GetAs<RTCMediaStreamTrackStats>(*stat->track_id);
        if (track_stat) {
          video_receiver.frame_width = track_stat->frame_width.ValueOrDefault(0);
          video_receiver.frame_height = track_stat->frame_height.ValueOrDefault(0);
        }
      }

      this->video_receiver_statistics_.push_back(video_receiver);
    }
  }

  ConnectionStatistics connection_statistics = {0};
  uint64_t highest_priority = 0;

  for (const auto& stat : candidate_pair_stats) {
    // We'll only look at the pair that is nominated with the highest priority, usually
    // that has useful values (there does not seem to be a 'in_use' type of flag).
    uint64_t current_priority = stat->priority.ValueOrDefault(0);
    if (*stat->nominated && stat->priority.ValueOrDefault(0) > highest_priority) {
      highest_priority = current_priority;
      connection_statistics.current_round_trip_time = stat->current_round_trip_time.ValueOrDefault(0.0);
      connection_statistics.available_outgoing_bitrate = stat->available_outgoing_bitrate.ValueOrDefault(0.0);
    }
  }

  MediaStatistics media_statistics;
  media_statistics.timestamp_us = report->timestamp_us();
  media_statistics.audio_sender_statistics_size = this->audio_sender_statistics_.size();
  media_statistics.audio_sender_statistics = this->audio_sender_statistics_.data();
  media_statistics.video_sender_statistics_size = this->video_sender_statistics_.size();
  media_statistics.video_sender_statistics = this->video_sender_statistics_.data();
  media_statistics.audio_receiver_statistics_size = this->audio_receiver_statistics_.size();
  media_statistics.audio_receiver_statistics = this->audio_receiver_statistics_.data();
  media_statistics.video_receiver_statistics_count = this->video_receiver_statistics_.size();
  media_statistics.video_receiver_statistics = this->video_receiver_statistics_.data();
  media_statistics.connection_statistics = connection_statistics;

  // Pass media_statistics up to Rust, which will consume the data before returning.
  this->stats_observer_cbs_.OnStatsComplete(this->stats_observer_, &media_statistics);
}

// Returns an owned RC.
// Pass-in values must outlive the returned value.
RUSTEXPORT StatsObserverRffi*
Rust_createStatsObserver(void*                         stats_observer_borrowed,
                         const StatsObserverCallbacks* stats_observer_cbs_borrowed) {
  return take_rc(rtc::make_ref_counted<StatsObserverRffi>(stats_observer_borrowed, stats_observer_cbs_borrowed));
}

} // namespace rffi
} // namespace webrtc
