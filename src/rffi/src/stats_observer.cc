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

void StatsObserverRffi::OnComplete(const StatsReports& reports) {
  StatsObserverValues values;

  for (size_t i = 0; i < reports.size(); ++i) {
    const StatsReport* report = reports[i];
    StatsReport::StatsType type = report->type();

    if (type == StatsReport::kStatsReportTypeSsrc) {
      const StatsReport::Value* media_type = report->FindValue(
          StatsReport::StatsValueName::kStatsValueNameMediaType);
      if (media_type && strcmp(media_type->static_string_val(), "audio") == 0) {
        if (report->FindValue(StatsReport::StatsValueName::kStatsValueNameBytesSent)) {
          // Audio Sender...
          const webrtc::StatsReport::Value *audio_packets_sent = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNamePacketsSent);
          if (audio_packets_sent) {
            values.audio_packets_sent = audio_packets_sent->int_val();
          }

          const webrtc::StatsReport::Value *audio_packets_sent_lost = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNamePacketsLost);
          if (audio_packets_sent_lost) {
            values.audio_packets_sent_lost = audio_packets_sent_lost->int_val();
          }

          const webrtc::StatsReport::Value *audio_rtt = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNameRtt);
          if (audio_rtt) {
            values.audio_rtt = audio_rtt->int64_val();
          }
        } else {
          // Audio Receiver...
          const webrtc::StatsReport::Value *audio_expand_rate = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNameExpandRate);
          if (audio_expand_rate) {
            values.audio_expand_rate = audio_expand_rate->float_val();
          }

          const webrtc::StatsReport::Value *audio_accelerate_rate = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNameAccelerateRate);
          if (audio_accelerate_rate) {
            values.audio_accelerate_rate = audio_accelerate_rate->float_val();
          }

          const webrtc::StatsReport::Value *audio_preemptive_rate = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNamePreemptiveExpandRate);
          if (audio_preemptive_rate) {
            values.audio_preemptive_rate = audio_preemptive_rate->float_val();
          }

          const webrtc::StatsReport::Value *audio_speech_expand_rate = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNameSpeechExpandRate);
          if (audio_speech_expand_rate) {
            values.audio_speech_expand_rate = audio_speech_expand_rate->float_val();
          }

          const webrtc::StatsReport::Value *audio_preferred_buffer_size_ms = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNamePreferredJitterBufferMs);
          if (audio_preferred_buffer_size_ms) {
            values.audio_preferred_buffer_size_ms = audio_preferred_buffer_size_ms->int_val();
          }

          const webrtc::StatsReport::Value *audio_packets_received = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNamePacketsReceived);
          if (audio_packets_received) {
            values.audio_packets_received = audio_packets_received->int_val();
          }

          const webrtc::StatsReport::Value *audio_packets_received_lost = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNamePacketsLost);
          if (audio_packets_received_lost) {
            values.audio_packets_received_lost = audio_packets_received_lost->int_val();
          }

          const webrtc::StatsReport::Value *audio_jitter_received = report->FindValue(
              StatsReport::StatsValueName::kStatsValueNameJitterReceived);
          if (audio_jitter_received) {
            values.audio_jitter_received = audio_jitter_received->int_val();
          }
        }
      } else if (media_type && strcmp(media_type->static_string_val(), "video") == 0) {
        if (report->FindValue(StatsReport::StatsValueName::kStatsValueNameBytesSent)) {
          // Video Sender...
        } else {
          // Video Receiver...
        }
      }
    } else if (type == StatsReport::kStatsReportTypeBwe) {
      // Video Bandwidth...
    }
  }

  // values is expected to be consumed before the callback returns.
  this->stats_observer_cbs_.OnStatsComplete(this->stats_observer_, &values);
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
