/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "rffi/api/stats_observer_intf.h"
#include "rffi/src/stats_observer.h"

#include "api/stats/rtcstats_objects.h"

namespace webrtc {
namespace rffi {

StatsObserverRffi::StatsObserverRffi(const rust_object             stats_observer,
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

  for (const auto& stat : outbound_stream_stats) {
    auto remote_stat = report->GetAs<RTCRemoteInboundRtpStreamStats>(*stat->remote_id);

    if (*stat->kind == "audio") {
      AudioSenderStatistics audio_sender = {0};

      audio_sender.ssrc = *stat->ssrc;
      audio_sender.packets_sent = *stat->packets_sent;
      audio_sender.bytes_sent = *stat->bytes_sent;

      if (remote_stat) {
        audio_sender.remote_packets_lost = *remote_stat->packets_lost;
        audio_sender.remote_jitter = *remote_stat->jitter;
        audio_sender.remote_round_trip_time = *remote_stat->round_trip_time;
      }

      this->audio_sender_statistics_.push_back(audio_sender);
    } else if (*stat->kind == "video") {
      VideoSenderStatistics video_sender = {0};

      video_sender.ssrc = *stat->ssrc;
      video_sender.packets_sent = *stat->packets_sent;
      video_sender.bytes_sent = *stat->bytes_sent;
      video_sender.frames_encoded = *stat->frames_encoded;
      video_sender.key_frames_encoded = *stat->key_frames_encoded;
      video_sender.total_encode_time = *stat->total_encode_time;
      video_sender.frame_width = *stat->frame_width;
      video_sender.frame_height = *stat->frame_height;
      video_sender.retransmitted_packets_sent = *stat->retransmitted_packets_sent;
      video_sender.retransmitted_bytes_sent = *stat->retransmitted_bytes_sent;
      video_sender.total_packet_send_delay = *stat->total_packet_send_delay;
      video_sender.nack_count = *stat->nack_count;
      video_sender.fir_count = *stat->fir_count;
      video_sender.pli_count = *stat->pli_count;
      if (*stat->quality_limitation_reason == "none") {
        video_sender.quality_limitation_reason = 0;
      } else if (*stat->quality_limitation_reason == "cpu") {
        video_sender.quality_limitation_reason = 1;
      } else if (*stat->quality_limitation_reason == "bandwidth") {
        video_sender.quality_limitation_reason = 2;
      } else {
        video_sender.quality_limitation_reason = 3;
      }
      video_sender.quality_limitation_resolution_changes = *stat->quality_limitation_resolution_changes;

      if (remote_stat) {
        video_sender.remote_packets_lost = *remote_stat->packets_lost;
        video_sender.remote_jitter = *remote_stat->jitter;
        video_sender.remote_round_trip_time = *remote_stat->round_trip_time;
      }

      this->video_sender_statistics_.push_back(video_sender);
    }
  }

  for (const auto& stat : inbound_stream_stats) {
    auto track_stat = report->GetAs<RTCMediaStreamTrackStats>(*stat->track_id);

    if (*stat->kind == "audio") {
      AudioReceiverStatistics audio_receiver = {0};

      audio_receiver.ssrc = *stat->ssrc;
      audio_receiver.packets_received = *stat->packets_received;
      audio_receiver.packets_lost = *stat->packets_lost;
      audio_receiver.bytes_received = *stat->bytes_received;
      audio_receiver.jitter = *stat->jitter;
      audio_receiver.frames_decoded = *stat->frames_decoded;
      audio_receiver.total_decode_time = *stat->total_decode_time;

      this->audio_receiver_statistics_.push_back(audio_receiver);
    } else if (*stat->kind == "video") {
      VideoReceiverStatistics video_receiver = {0};

      video_receiver.ssrc = *stat->ssrc;
      video_receiver.packets_received = *stat->packets_received;
      video_receiver.packets_lost = *stat->packets_lost;
      video_receiver.packets_repaired = *stat->packets_repaired;
      video_receiver.bytes_received = *stat->bytes_received;
      video_receiver.frames_decoded = *stat->frames_decoded;
      video_receiver.key_frames_decoded = *stat->key_frames_decoded;
      video_receiver.total_decode_time = *stat->total_decode_time;

      if (track_stat) {
        video_receiver.frame_width = *track_stat->frame_width;
        video_receiver.frame_height = *track_stat->frame_height;
      }

      this->video_receiver_statistics_.push_back(video_receiver);
    }
  }

  MediaStatistics media_statistics;
  media_statistics.audio_sender_statistics_size = this->audio_sender_statistics_.size();
  media_statistics.audio_sender_statistics = this->audio_sender_statistics_.data();
  media_statistics.video_sender_statistics_size = this->video_sender_statistics_.size();
  media_statistics.video_sender_statistics = this->video_sender_statistics_.data();
  media_statistics.audio_receiver_statistics_size = this->audio_receiver_statistics_.size();
  media_statistics.audio_receiver_statistics = this->audio_receiver_statistics_.data();
  media_statistics.video_receiver_statistics_count = this->video_receiver_statistics_.size();
  media_statistics.video_receiver_statistics = this->video_receiver_statistics_.data();

  // Pass media_statistics up to Rust, which will consume the data before returning.
  this->stats_observer_cbs_.OnStatsComplete(this->stats_observer_, &media_statistics);
}

RUSTEXPORT StatsObserverRffi*
Rust_createStatsObserver(const rust_object             stats_observer,
                         const StatsObserverCallbacks* stats_observer_cbs) {
  StatsObserverRffi* rffi_observer = new rtc::RefCountedObject<StatsObserverRffi>(stats_observer, stats_observer_cbs);
  rffi_observer->AddRef();

  // rffi_observer is now owned by caller. Must call Rust_releaseRef() eventually.
  return rffi_observer;
}

} // namespace rffi
} // namespace webrtc
