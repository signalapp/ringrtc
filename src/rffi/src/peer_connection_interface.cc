/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "api/data_channel_interface.h"
#include "api/ice_gatherer_interface.h"
#include "api/ice_transport_interface.h"
#include "api/peer_connection_interface.h"
#include "pc/session_description.h"
#include "sdk/media_constraints.h"
#include "rffi/api/peer_connection_interface_intf.h"
#include "rffi/src/sdp_observer.h"
#include "rffi/src/stats_observer.h"

#include <string>

namespace webrtc {
namespace rffi {

RUSTEXPORT void
Rust_createOffer(PeerConnectionInterface*              pc_interface,
                 CreateSessionDescriptionObserverRffi* csd_observer) {

  // No constraints are set
  MediaConstraints constraints = MediaConstraints();
  PeerConnectionInterface::RTCOfferAnswerOptions options;

  CopyConstraintsIntoOfferAnswerOptions(&constraints, &options);
  pc_interface->CreateOffer(csd_observer, options);
}

RUSTEXPORT void
Rust_setLocalDescription(PeerConnectionInterface*           pc_interface,
                         SetSessionDescriptionObserverRffi* ssd_observer,
                         SessionDescriptionInterface*       description) {
  pc_interface->SetLocalDescription(ssd_observer, description);
}

RUSTEXPORT const char*
Rust_toSdp(SessionDescriptionInterface* sdi) {

  std::string sdp;
  if (sdi->ToString(&sdp)) {
    return strdup(&sdp[0u]);
  }

  RTC_LOG(LS_ERROR) << "Unable to convert SessionDescription to SDP";
  return nullptr;
}

static SessionDescriptionInterface*
createSessionDescriptionInterface(SdpType type, const char* sdp) {

  if (sdp != nullptr) {
    std::string sdp_str = std::string(sdp);
    std::unique_ptr<SessionDescriptionInterface> session_desription =
      CreateSessionDescription(type, sdp_str);

    return session_desription.release();
  } else {
    return nullptr;
  }
}

RUSTEXPORT SessionDescriptionInterface*
Rust_answerFromSdp(const char* sdp) {
  return createSessionDescriptionInterface(SdpType::kAnswer, sdp);
}

RUSTEXPORT SessionDescriptionInterface*
Rust_offerFromSdp(const char* sdp) {
  return createSessionDescriptionInterface(SdpType::kOffer, sdp);
}

RUSTEXPORT bool 
Rust_replaceRtpDataChannelsWithSctp(webrtc::SessionDescriptionInterface* sdi) {
  if (!sdi) {
    return false;
  }

  std::string rtp_data_mid;
  cricket::SessionDescription* description = sdi->description();
  for (const cricket::ContentInfo& content : description->contents()) {
    if (content.type == cricket::MediaProtocolType::kRtp && 
        content.media_description() && content.media_description()->type() == cricket::MEDIA_TYPE_DATA) {
      rtp_data_mid = content.mid();
      break;
    }
  }
  if (rtp_data_mid.empty()) {
    // Couldn't find any RTP data channel, so nothing to change.
    return false;
  }

  description->RemoveContentByName(rtp_data_mid);

  // Mirror MediaSessionDescriptionFactory::AddSctpDataContentForOffer
  auto sctp = std::make_unique<cricket::SctpDataContentDescription>();
  sctp->set_protocol(cricket::kMediaProtocolUdpDtlsSctp);
  sctp->set_use_sctpmap(false);
  sctp->set_max_message_size(256 * 1024);
  // This shouldn't really be necessary, but just in case...
  sctp->set_rtcp_mux(true);
  description->AddContent(rtp_data_mid, cricket::MediaProtocolType::kSctp, std::move(sctp));
  return true;
}


RUSTEXPORT void
Rust_createAnswer(PeerConnectionInterface*              pc_interface,
                  CreateSessionDescriptionObserverRffi* csd_observer) {

  // No constraints are set
  MediaConstraints constraints = MediaConstraints();
  PeerConnectionInterface::RTCOfferAnswerOptions options;

  CopyConstraintsIntoOfferAnswerOptions(&constraints, &options);
  pc_interface->CreateAnswer(csd_observer, options);
}

RUSTEXPORT void
Rust_setRemoteDescription(PeerConnectionInterface*           pc_interface,
                          SetSessionDescriptionObserverRffi* ssd_observer,
                          SessionDescriptionInterface*       description) {
  pc_interface->SetRemoteDescription(ssd_observer, description);
}

RUSTEXPORT void
Rust_setOutgoingAudioEnabled(PeerConnectionInterface* pc_interface,
                             bool                     enabled) {
  // Note: calling SetAudioRecording(enabled) is deprecated and it's not clear
  // that it even does anything any more.
  int encodings_changed = 0;
  for (auto& sender : pc_interface->GetSenders()) {
    if (sender->media_type() == cricket::MediaType::MEDIA_TYPE_AUDIO) {
      RtpParameters parameters = sender->GetParameters();
      for (auto& encoding: parameters.encodings) {
        encoding.active = enabled;
        encodings_changed++;
      }
      sender->SetParameters(parameters);
    }
  }
  RTC_LOG(LS_INFO) << "Rust_setOutgoingAudioEnabled(" << enabled << ") for " << encodings_changed << " audio encodings.";
}

RUSTEXPORT bool
Rust_setIncomingRtpEnabled(PeerConnectionInterface* pc_interface,
                           bool                     enabled) {
  RTC_LOG(LS_INFO) << "Rust_setIncomingRtpEnabled(" << enabled << ")";
  return pc_interface->SetIncomingRtpEnabled(enabled);
}

RUSTEXPORT DataChannelInterface*
Rust_createDataChannel(PeerConnectionInterface*   pc_interface,
                       const char*                label,
                       const RffiDataChannelInit* config) {

  std::string dc_label = std::string(label);

  struct DataChannelInit dc_config;

  dc_config.reliable          = config->reliable;
  dc_config.ordered           = config->ordered;
  dc_config.maxRetransmitTime = config->maxRetransmitTime;
  dc_config.maxRetransmits    = config->maxRetransmits;
  dc_config.protocol          = std::string(config->protocol);
  dc_config.negotiated        = config->negotiated;
  dc_config.id                = config->id;

  rtc::scoped_refptr<DataChannelInterface> channel = pc_interface->CreateDataChannel(dc_label, &dc_config);

  // Channel is now owned by caller.  Must call Rust_releaseRef() eventually.
  return channel.release();
}

RUSTEXPORT bool
Rust_addIceCandidate(PeerConnectionInterface* pc_interface,
                     const char*              sdp) {
  // Since we always use bundle, we can always use index 0 and ignore the mid
  std::unique_ptr<IceCandidateInterface> candidate(
      CreateIceCandidate("", 0, std::string(sdp), nullptr));

  return pc_interface->AddIceCandidate(candidate.get());
}

RUSTEXPORT IceGathererInterface*
Rust_createSharedIceGatherer(PeerConnectionInterface* pc_interface) {
  rtc::scoped_refptr<IceGathererInterface> ice_gatherer = pc_interface->CreateSharedIceGatherer();

  // IceGatherer is now owned by caller.  Must call Rust_releaseRef() eventually.
  return ice_gatherer.release();
}

RUSTEXPORT bool
Rust_useSharedIceGatherer(PeerConnectionInterface* pc_interface,
                          IceGathererInterface* ice_gatherer) {
  return pc_interface->UseSharedIceGatherer(rtc::scoped_refptr<IceGathererInterface>(ice_gatherer));
}

RUSTEXPORT void
Rust_getStats(PeerConnectionInterface* pc_interface,
              StatsObserverRffi* stats_observer) {
    pc_interface->GetStats(stats_observer, nullptr, PeerConnectionInterface::kStatsOutputLevelStandard);
}

RUSTEXPORT void
Rust_setMaxSendBitrate(PeerConnectionInterface* pc_interface,
                       int32_t                  max_bitrate_bps) {
    struct BitrateSettings bitrate_settings;
    bitrate_settings.max_bitrate_bps = max_bitrate_bps;

    pc_interface->SetBitrate(bitrate_settings);
}

} // namespace rffi
} // namespace webrtc
