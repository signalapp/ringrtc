/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "api/data_channel_interface.h"
#include "api/ice_gatherer_interface.h"
#include "api/ice_transport_interface.h"
#include "api/jsep_session_description.h"
#include "api/peer_connection_interface.h"
#include "media/base/h264_profile_level_id.h"
#include "modules/rtp_rtcp/source/rtp_header_extensions.h"
#include "p2p/base/port.h"
#include "pc/media_session.h"
#include "pc/sdp_utils.h"
#include "pc/session_description.h"
#include "sdk/media_constraints.h"
#include "rffi/api/peer_connection_intf.h"
#include "rffi/src/sdp_observer.h"
#include "rffi/src/stats_observer.h"
#include "rtc_base/message_digest.h"
#include "rtc_base/string_encode.h"
#include "rtc_base/third_party/base64/base64.h"

#include <string>

namespace webrtc {
namespace rffi {

RUSTEXPORT void
Rust_createOffer(PeerConnectionInterface*              peer_connection,
                 CreateSessionDescriptionObserverRffi* csd_observer) {

  // No constraints are set
  MediaConstraints constraints = MediaConstraints();
  PeerConnectionInterface::RTCOfferAnswerOptions options;

  CopyConstraintsIntoOfferAnswerOptions(&constraints, &options);
  peer_connection->CreateOffer(csd_observer, options);
}

// Warning!  This takes ownership of the local description
RUSTEXPORT void
Rust_setLocalDescription(PeerConnectionInterface*           peer_connection,
                         SetSessionDescriptionObserverRffi* ssd_observer,
                         SessionDescriptionInterface*       local_description) {
  peer_connection->SetLocalDescription(ssd_observer, local_description);
}

RUSTEXPORT const char*
Rust_toSdp(SessionDescriptionInterface* session_description) {

  std::string sdp;
  if (session_description->ToString(&sdp)) {
    return strdup(&sdp[0u]);
  }

  RTC_LOG(LS_ERROR) << "Unable to convert SessionDescription to SDP";
  return nullptr;
}

static SessionDescriptionInterface*
createSessionDescriptionInterface(SdpType type, const char* sdp) {

  if (sdp != nullptr) {
    std::string sdp_str = std::string(sdp);
    std::unique_ptr<SessionDescriptionInterface> session_description =
      CreateSessionDescription(type, sdp_str);

    return session_description.release();
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
Rust_disableDtlsAndSetSrtpKey(webrtc::SessionDescriptionInterface* session_description,
                              int                                  crypto_suite,
                              const char*                          key_ptr,
                              size_t                               key_len,
                              const char*                          salt_ptr,
                              size_t                               salt_len) {
  if (!session_description) {
    return false;
  }

  cricket::SessionDescription* session = session_description->description();
  if (!session) {
    return false;
  }

  cricket::CryptoParams crypto_params;
  crypto_params.cipher_suite = rtc::SrtpCryptoSuiteToName(crypto_suite);

  std::string key(key_ptr, key_len);
  std::string salt(salt_ptr, salt_len);
  crypto_params.key_params = "inline:" + rtc::Base64::Encode(key + salt);

  // Disable DTLS
  for (cricket::TransportInfo& transport : session->transport_infos()) {
    transport.description.connection_role = cricket::CONNECTIONROLE_NONE;
    transport.description.identity_fingerprint = nullptr;
  }

  // Set SRTP key
  for (cricket::ContentInfo& content : session->contents()) {
    cricket::MediaContentDescription* media = content.media_description();
    if (media) {
      media->set_protocol(cricket::kMediaProtocolSavpf);
      std::vector<cricket::CryptoParams> cryptos;
      cryptos.push_back(crypto_params);
      media->set_cryptos(cryptos);
    }
  }

  return true;
}

RUSTEXPORT RffiConnectionParametersV4*
Rust_sessionDescriptionToV4(const webrtc::SessionDescriptionInterface* session_description) {
  if (!session_description) {
    return nullptr;
  }

  const cricket::SessionDescription* session = session_description->description();
  if (!session) {
    return nullptr;
  }

  // Get ICE ufrag + pwd
  if (session->transport_infos().empty()) {
    return nullptr;
  }

  auto v4 = std::make_unique<ConnectionParametersV4>();

  auto* transport = &session->transport_infos()[0].description;
  v4->ice_ufrag = transport->ice_ufrag;
  v4->ice_pwd = transport->ice_pwd;

  // Get video codecs
  auto* video = cricket::GetFirstVideoContentDescription(session);
  if (video) {
    // We only support 1 CBP and 1 CHP codec.
    // So only include the first of each.
    // This should be OK because Android and iOS and native only
    // add one level per profile.
    bool has_h264_cbp = false;
    bool has_h264_chp = false;
    for (const auto& codec : video->codecs()) {
      auto codec_type = webrtc::PayloadStringToCodecType(codec.name);
      if (codec_type == webrtc::kVideoCodecVP8) {
        RffiVideoCodec vp8;
        vp8.type = kRffiVideoCodecVp8;
        vp8.level = 0;
        v4->receive_video_codecs.push_back(vp8);
      } else if (codec_type == webrtc::kVideoCodecH264) {
        std::string level_asymmetry_allowed;
        if (codec.GetParam(cricket::kH264FmtpLevelAsymmetryAllowed, &level_asymmetry_allowed) && level_asymmetry_allowed != "1") {
          RTC_LOG(LS_WARNING) << "Ignoring H264 codec because level-asymmetry-allowed = " << level_asymmetry_allowed;  
          continue;
        }

        std::string packetization_mode;
        if (codec.GetParam(cricket::kH264FmtpPacketizationMode, &packetization_mode) && packetization_mode != "1") {
          // Not a warning because WebRTC software H264 encoders say they support mode 0 (even though it's useless).
          RTC_LOG(LS_INFO) << "Ignoring H264 codec because packetization_mode = " << packetization_mode;  
          continue;
        }

        auto profile_level_id = webrtc::H264::ParseSdpProfileLevelId(codec.params);
        if (!profile_level_id) {
          std::string profile_level_id_string;
          codec.GetParam("profile-level-id", &profile_level_id_string);
          RTC_LOG(LS_WARNING) << "Ignoring H264 codec because profile-level-id=" << profile_level_id_string;  
          continue;
        }

        if (profile_level_id->profile == webrtc::H264::kProfileConstrainedHigh && !has_h264_chp) {
          RffiVideoCodec h264_chp;
          h264_chp.type = kRffiVideoCodecH264ConstrainedHigh;
          h264_chp.level = profile_level_id->level;
          v4->receive_video_codecs.push_back(h264_chp);
          has_h264_chp = true;
        } else if (profile_level_id->profile != webrtc::H264::kProfileConstrainedBaseline) {
          // Not a warning because WebRTC software H264 encoders say they support baseline, even though it's useless.
          RTC_LOG(LS_INFO) << "Ignoring H264 codec profile = " << profile_level_id->profile;  
          continue;
        }

        if (!has_h264_cbp) {
          // Any time we support anything, we assume we also support CBP
          // (but don't add it more than once)
          RffiVideoCodec h264_cbp;
          h264_cbp.type = kRffiVideoCodecH264ConstrainedBaseline;
          h264_cbp.level = profile_level_id->level;
          v4->receive_video_codecs.push_back(h264_cbp);
          has_h264_cbp = true;
        }
      }
    }
  }

  auto* rffi_v4 = new RffiConnectionParametersV4();
  rffi_v4->ice_ufrag = v4->ice_ufrag.c_str();
  rffi_v4->ice_pwd = v4->ice_pwd.c_str();
  rffi_v4->receive_video_codecs = v4->receive_video_codecs.data();
  rffi_v4->receive_video_codecs_size = v4->receive_video_codecs.size();
  rffi_v4->backing = v4.release();
  return rffi_v4;
}

RUSTEXPORT void
Rust_releaseV4(RffiConnectionParametersV4* v4) {
  if (!v4) {
    return;
  }

  delete v4->backing;
  delete v4;
}

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_sessionDescriptionFromV4(bool offer, const RffiConnectionParametersV4* v4) {
  // Major changes from the default WebRTC behavior:
  // 1. We remove all codecs except Opus, VP8, and H264
  // 2. We remove all header extensions except for transport-cc, video orientation,
  //    abs send time, and timestamp offset.
  // 3. Opus CBR is enabled.

  // The constants we hardcode and must match between different clients.
  int TRANSPORT_CC1_EXT_ID = 1;
  int TRANSPORT_CC2_EXT_ID = 2;
  int VIDEO_ORIENTATION_EXT_ID = 4;
  int ABS_SEND_TIME_EXT_ID = 12;
  int TX_TIME_OFFSET_EXT_ID = 13;

  // Payload types must be over 96 and less than 128.
  int DATA_PT = 101;
  int OPUS_PT = 102;
  int VP8_PT = 108;
  int VP8_RTX_PT = 118;
  int H264_CHP_PT = 104;
  int H264_CHP_RTX_PT = 114;
  int H264_CBP_PT = 103;
  int H264_CBP_RTX_PT = 113;
  int RED_PT = 120;
  int RED_RTX_PT = 121;
  int ULPFEC_PT = 122;

  // For some reason, WebRTC insists that the video SSRCs for one side don't 
  // overlap with SSRCs from the other side.  To avoid potential problems, we'll give the
  // caller side 1XXX and the callee side 2XXX;
  uint32_t BASE_SSRC = offer ? 1000 : 2000;
  uint32_t DATA_SSRC = BASE_SSRC + 1;
  uint32_t AUDIO_SSRC = BASE_SSRC + 2;
  uint32_t VIDEO_SSRC = BASE_SSRC + 3;
  uint32_t VIDEO_RTX_SSRC = BASE_SSRC + 13;

  // This must stay in sync with PeerConnection.createDataChannel.
  std::string DATA_CHANNEL_LABEL = "signaling";
  // This should stay in sync with PeerConnectionFactory.createAudioTrack
  std::string AUDIO_TRACK_ID = "audio1";
  // This must stay in sync with PeerConnectionFactory.createVideoTrack
  std::string VIDEO_TRACK_ID = "video1";

  auto transport = cricket::TransportDescription();
  transport.ice_mode = cricket::ICEMODE_FULL;
  transport.ice_ufrag = std::string(v4->ice_ufrag);
  transport.ice_pwd = std::string(v4->ice_pwd);
  transport.AddOption(cricket::ICE_OPTION_TRICKLE);
  transport.AddOption(cricket::ICE_OPTION_RENOMINATION);

  // DTLS is disabled
  transport.connection_role = cricket::CONNECTIONROLE_NONE;
  transport.identity_fingerprint = nullptr;

  auto set_rtp_params = [] (cricket::MediaContentDescription* media) {
    media->set_protocol(cricket::kMediaProtocolSavpf);
    media->set_rtcp_mux(true);
    media->set_direction(webrtc::RtpTransceiverDirection::kSendRecv);
  };

  auto data = std::make_unique<cricket::RtpDataContentDescription>();
  set_rtp_params(data.get());
  auto audio = std::make_unique<cricket::AudioContentDescription>();
  set_rtp_params(audio.get());
  auto video = std::make_unique<cricket::VideoContentDescription>();
  set_rtp_params(video.get());

  auto google_data = cricket::DataCodec(DATA_PT, cricket::kGoogleRtpDataCodecName);
  data->AddCodec(google_data);

  auto opus = cricket::AudioCodec(OPUS_PT, cricket::kOpusCodecName, 48000, 0, 2);
  // These are the current defaults for WebRTC
  // We set them explicitly to avoid having the defaults change on us.
  opus.SetParam("stereo", "0");  // "1" would cause non-VOIP mode to be used
  opus.SetParam("ptime", "20");
  opus.SetParam("minptime", "10");
  opus.SetParam("maxptime", "120");
  opus.SetParam("useinbandfec", "1");
  opus.SetParam("usedtx", "0");
  // This is not a default. WebRTC has a "send" default of 32000 and a "receive" default
  // of 64000. We use 40000 because as per RFC-7587 (3.1.1), it is the top end of FB speech.
  opus.SetParam("maxaveragebitrate", "40000");
  // This is not a default. We enable this for privacy.
  opus.SetParam("cbr", "1");
  opus.AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamTransportCc, cricket::kParamValueEmpty));
  audio->AddCodec(opus);

  auto add_video_feedback_params = [] (cricket::VideoCodec* video_codec) {
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamTransportCc, cricket::kParamValueEmpty));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamCcm, cricket::kRtcpFbCcmParamFir));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamNack, cricket::kParamValueEmpty));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamNack, cricket::kRtcpFbNackParamPli));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamRemb, cricket::kParamValueEmpty));
  };

  auto add_h264_params = [] (cricket::VideoCodec* h264_codec, webrtc::H264::Profile profile, uint32_t level) {
    // All of the codec implementations (iOS hardware, Android hardware) are only used by WebRTC
    // with packetization mode 1.  Software codecs also support mode 0, but who cares.  It's useless.
    // They also all allow for level asymmetry.
    h264_codec->SetParam(cricket::kH264FmtpLevelAsymmetryAllowed, "1");
    h264_codec->SetParam(cricket::kH264FmtpPacketizationMode, "1");
    // On Android and with software, the level is always 31.  But it could be anything with iOS.
    auto profile_level_id_string = webrtc::H264::ProfileLevelIdToString(webrtc::H264::ProfileLevelId(profile, webrtc::H264::Level(level)));
    if (profile_level_id_string) {
      h264_codec->SetParam("profile-level-id", *profile_level_id_string);
    }
  };

  for (size_t i = 0; i < v4->receive_video_codecs_size; i++) {
    RffiVideoCodec rffi_codec = v4->receive_video_codecs[i];
    cricket::VideoCodec codec;
    if (rffi_codec.type == kRffiVideoCodecVp8) {
      auto vp8 = cricket::VideoCodec(VP8_PT, cricket::kVp8CodecName);
      auto vp8_rtx = cricket::VideoCodec::CreateRtxCodec(VP8_RTX_PT, VP8_PT);
      add_video_feedback_params(&vp8);

      video->AddCodec(vp8);
      video->AddCodec(vp8_rtx);
    } else if (rffi_codec.type == kRffiVideoCodecH264ConstrainedHigh) {
      auto h264_chp = cricket::VideoCodec(H264_CHP_PT, cricket::kH264CodecName);
      auto h264_chp_rtx = cricket::VideoCodec::CreateRtxCodec(H264_CHP_RTX_PT, H264_CHP_PT);
      add_h264_params(&h264_chp, webrtc::H264::kProfileConstrainedHigh, rffi_codec.level);
      add_video_feedback_params(&h264_chp);

      video->AddCodec(h264_chp);
      video->AddCodec(h264_chp_rtx);
    } else if (rffi_codec.type == kRffiVideoCodecH264ConstrainedBaseline) {
      auto h264_cbp = cricket::VideoCodec(H264_CBP_PT, cricket::kH264CodecName);
      auto h264_cbp_rtx = cricket::VideoCodec::CreateRtxCodec(H264_CBP_RTX_PT, H264_CBP_PT);
      add_h264_params(&h264_cbp, webrtc::H264::kProfileConstrainedBaseline, rffi_codec.level);
      add_video_feedback_params(&h264_cbp);

      video->AddCodec(h264_cbp);
      video->AddCodec(h264_cbp_rtx);
    }
  }

  // These are "meta codecs" for redundancy and FEC.
  // They are enabled by default currently with WebRTC.
  auto red = cricket::VideoCodec(RED_PT, cricket::kRedCodecName);
  auto red_rtx = cricket::VideoCodec::CreateRtxCodec(RED_RTX_PT, RED_PT);
  auto ulpfec = cricket::VideoCodec(ULPFEC_PT, cricket::kUlpfecCodecName);

  video->AddCodec(red);
  video->AddCodec(red_rtx);
  video->AddCodec(ulpfec);

  auto transport_cc1 = webrtc::RtpExtension(webrtc::TransportSequenceNumber::kUri, TRANSPORT_CC1_EXT_ID);
  auto transport_cc2 = webrtc::RtpExtension(webrtc::TransportSequenceNumberV2::kUri, TRANSPORT_CC2_EXT_ID);
  auto video_orientation = webrtc::RtpExtension(webrtc::VideoOrientation ::kUri, VIDEO_ORIENTATION_EXT_ID);
  auto abs_send_time = webrtc::RtpExtension(webrtc::AbsoluteSendTime::kUri, ABS_SEND_TIME_EXT_ID);
  auto tx_time_offset = webrtc::RtpExtension(webrtc::TransmissionOffset::kUri, TX_TIME_OFFSET_EXT_ID);

  audio->AddRtpHeaderExtension(transport_cc1);
  // TransportCC 2 isn't enabled by default yet, so maybe we shouldn't use it yet.
  // audio->AddRtpHeaderExtension(transport_cc2);
  audio->AddRtpHeaderExtension(abs_send_time);

  video->AddRtpHeaderExtension(transport_cc1);
  // TransportCC 2 isn't enabled by default yet, so maybe we shouldn't use it yet.
  // video->AddRtpHeaderExtension(transport_cc2);
  video->AddRtpHeaderExtension(video_orientation);
  video->AddRtpHeaderExtension(abs_send_time);
  video->AddRtpHeaderExtension(tx_time_offset);

  auto data_stream = cricket::StreamParams();
  data_stream.add_ssrc(DATA_SSRC);
  data_stream.id = DATA_CHANNEL_LABEL;
  // RTP data channels are a little funny.
  // They use the following instead of the above
  // for communicating the data channel label.
  std::vector<std::string> data_stream_ids;
  data_stream_ids.push_back(DATA_CHANNEL_LABEL);
  data_stream.set_stream_ids(data_stream_ids);

  auto audio_stream = cricket::StreamParams();
  audio_stream.id = AUDIO_TRACK_ID;
  audio_stream.add_ssrc(AUDIO_SSRC);

  auto video_stream = cricket::StreamParams();
  video_stream.id = VIDEO_TRACK_ID;
  video_stream.add_ssrc(VIDEO_SSRC);
  video_stream.AddFidSsrc(VIDEO_SSRC, VIDEO_RTX_SSRC);  // AKA RTX

  // Things that are the same for all of them
  for (auto* stream : {&audio_stream, &video_stream, &data_stream}) {
    // WebRTC just generates a random 16-byte string for the entire PeerConnection.
    // It's used to send an SDES RTCP message.
    // The value doesn't seem to be used for anything else.
    // We'll set it around just in case.
    // But everything seems to work fine without it.
    stream->cname = "CNAMECNAMECNAME!";
  }

  data->AddStream(data_stream);
  audio->AddStream(audio_stream);
  video->AddStream(video_stream);

  // This is the default, and it doesn't really matter. Provided as a sanity check
  // in case there is a problem with the data channel.
  data->set_bandwidth(30720);

  // TODO: Why is this only for video by default in WebRTC? Should we enable it for all of them?
  video->set_rtcp_reduced_size(true);

  // Keep the order as the WebRTC default: (audio, video, data).
  auto audio_content_name = "audio";
  auto video_content_name = "video";
  auto data_content_name = "data";

  auto session = std::make_unique<cricket::SessionDescription>();
  session->AddTransportInfo(cricket::TransportInfo(audio_content_name, transport));
  session->AddTransportInfo(cricket::TransportInfo(video_content_name, transport));
  session->AddTransportInfo(cricket::TransportInfo(data_content_name, transport));

  bool stopped = false;
  session->AddContent(audio_content_name, cricket::MediaProtocolType::kRtp, stopped, std::move(audio));
  session->AddContent(video_content_name, cricket::MediaProtocolType::kRtp, stopped, std::move(video));
  session->AddContent(data_content_name, cricket::MediaProtocolType::kRtp, stopped, std::move(data));

  auto bundle = cricket::ContentGroup(cricket::GROUP_TYPE_BUNDLE);
  bundle.AddContentName(audio_content_name);
  bundle.AddContentName(video_content_name);
  bundle.AddContentName(data_content_name);
  session->AddGroup(bundle);

  // This is the default and used for "Plan B" SDP, which is what we use in V1, V2, and V3.
  session->set_msid_signaling(cricket::kMsidSignalingSsrcAttribute);

  auto typ = offer ? SdpType::kOffer : SdpType::kAnswer;
  return new webrtc::JsepSessionDescription(typ, std::move(session), "1", "1");
}

const uint32_t INVALID_DEMUX_ID = 0;

webrtc::JsepSessionDescription*
CreateSessionDescriptionForGroupCall(bool local, 
                                     const std::string& ice_ufrag,
                                     const std::string& ice_pwd,
                                     std::unique_ptr<rtc::SSLFingerprint> dtls_fingerprint,
                                     std::vector<uint32_t> rtp_demux_ids) {
  // Major changes from the default WebRTC behavior:
  // 1. We remove all codecs except Opus and VP8.
  // 2. We remove all header extensions except for transport-cc, video orientation,
  //    abs send time, timestamp offset, and audio level.
  // 3. Opus CBR is enabled.

  // We hardcode the header extension IDs and Payload types.
  int TRANSPORT_CC1_EXT_ID = 1;
  int TRANSPORT_CC2_EXT_ID = 2;
  int VIDEO_ORIENTATION_EXT_ID = 4;
  int AUDIO_LEVEL_EXT_ID = 5;
  int ABS_SEND_TIME_EXT_ID = 12;
  int TX_TIME_OFFSET_EXT_ID = 13;

  // Payload types must be over 96 and less than 128.
  // int DATA_PT = 101;  Used by group_call.rs
  int OPUS_PT = 102;
  int VP8_PT = 108;
  int VP8_RTX_PT = 118;
  int RED_PT = 120;
  int RED_RTX_PT = 121;

  // This must stay in sync with PeerConnectionFactory.createAudioTrack
  std::string LOCAL_AUDIO_TRACK_ID = "audio1";
  // This must stay in sync with PeerConnectionFactory.createVideoTrack
  std::string LOCAL_VIDEO_TRACK_ID = "video1";

  auto transport = cricket::TransportDescription();
  transport.ice_mode = cricket::ICEMODE_FULL;
  transport.ice_ufrag = ice_ufrag;
  transport.ice_pwd = ice_pwd;
  transport.AddOption(cricket::ICE_OPTION_TRICKLE);

  // WebRTC requires ACTPASS instead of ACTIVE for a local offer.
  transport.connection_role = local ? cricket::CONNECTIONROLE_ACTPASS : cricket::CONNECTIONROLE_PASSIVE;
  transport.identity_fingerprint = std::move(dtls_fingerprint);

  auto set_rtp_params = [] (cricket::MediaContentDescription* media) {
    media->set_protocol(cricket::kMediaProtocolDtlsSavpf);
    media->set_rtcp_mux(true);
    media->set_direction(webrtc::RtpTransceiverDirection::kSendRecv);
  };

  auto audio = std::make_unique<cricket::AudioContentDescription>();
  set_rtp_params(audio.get());
  auto video = std::make_unique<cricket::VideoContentDescription>();
  set_rtp_params(video.get());

  auto opus = cricket::AudioCodec(OPUS_PT, cricket::kOpusCodecName, 48000, 0, 2);
  // These are the current defaults for WebRTC
  // We set them explicitly to avoid having the defaults change on us.
  opus.SetParam("stereo", "0");  // "1" would cause non-VOIP mode to be used
  opus.SetParam("ptime", "20");
  opus.SetParam("minptime", "10");
  opus.SetParam("maxptime", "120");
  opus.SetParam("useinbandfec", "1");
  opus.SetParam("usedtx", "0");
  // This is not a default. WebRTC has a "send" default of 32000 and a "receive" default
  // of 64000. We use 40000 because as per RFC-7587 (3.1.1), it is the top end of FB speech.
  opus.SetParam("maxaveragebitrate", "40000");
  // This is not a default. We enable this for privacy.
  opus.SetParam("cbr", "1");
  opus.AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamTransportCc, cricket::kParamValueEmpty));
  audio->AddCodec(opus);

  auto add_video_feedback_params = [] (cricket::VideoCodec* video_codec) {
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamTransportCc, cricket::kParamValueEmpty));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamCcm, cricket::kRtcpFbCcmParamFir));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamNack, cricket::kParamValueEmpty));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamNack, cricket::kRtcpFbNackParamPli));
    video_codec->AddFeedbackParam(cricket::FeedbackParam(cricket::kRtcpFbParamRemb, cricket::kParamValueEmpty));
  };

  auto vp8 = cricket::VideoCodec(VP8_PT, cricket::kVp8CodecName);
  auto vp8_rtx = cricket::VideoCodec::CreateRtxCodec(VP8_RTX_PT, VP8_PT);
  add_video_feedback_params(&vp8);

  video->AddCodec(vp8);
  video->AddCodec(vp8_rtx);

  // These are "meta codecs" for redundancy and FEC.
  // They are enabled by default currently with WebRTC.
  auto red = cricket::VideoCodec(RED_PT, cricket::kRedCodecName);
  auto red_rtx = cricket::VideoCodec::CreateRtxCodec(RED_RTX_PT, RED_PT);

  video->AddCodec(red);
  video->AddCodec(red_rtx);

  auto transport_cc1 = webrtc::RtpExtension(webrtc::TransportSequenceNumber::kUri, TRANSPORT_CC1_EXT_ID);
  auto transport_cc2 = webrtc::RtpExtension(webrtc::TransportSequenceNumberV2::kUri, TRANSPORT_CC2_EXT_ID);
  auto video_orientation = webrtc::RtpExtension(webrtc::VideoOrientation::kUri, VIDEO_ORIENTATION_EXT_ID);
  auto audio_level = webrtc::RtpExtension(webrtc::AudioLevel::kUri, AUDIO_LEVEL_EXT_ID);
  auto abs_send_time = webrtc::RtpExtension(webrtc::AbsoluteSendTime::kUri, ABS_SEND_TIME_EXT_ID);
  auto tx_time_offset = webrtc::RtpExtension(webrtc::TransmissionOffset::kUri, TX_TIME_OFFSET_EXT_ID);

  audio->AddRtpHeaderExtension(transport_cc1);
  // TransportCC 2 isn't enabled by default yet, so maybe we shouldn't use it yet.
  // audio->AddRtpHeaderExtension(transport_cc2);
  audio->AddRtpHeaderExtension(audio_level);
  audio->AddRtpHeaderExtension(abs_send_time);

  video->AddRtpHeaderExtension(transport_cc1);
  // TransportCC 2 isn't enabled by default yet, so maybe we shouldn't use it yet.
  // video->AddRtpHeaderExtension(transport_cc2);
  video->AddRtpHeaderExtension(video_orientation);
  video->AddRtpHeaderExtension(abs_send_time);
  video->AddRtpHeaderExtension(tx_time_offset);

  for (uint32_t rtp_demux_id : rtp_demux_ids) {
    if (rtp_demux_id == INVALID_DEMUX_ID) {
      RTC_LOG(LS_WARNING) << "Ignoring demux ID of 0";
      continue;
    }

    uint32_t audio_ssrc = rtp_demux_id + 0;
    // Leave room for audio RTX
    uint32_t video1_ssrc = rtp_demux_id + 2;
    uint32_t video1_rtx_ssrc = rtp_demux_id + 3;
    uint32_t video2_ssrc = rtp_demux_id + 4;
    uint32_t video2_rtx_ssrc = rtp_demux_id + 5;
    uint32_t video3_ssrc = rtp_demux_id + 6;
    uint32_t video3_rtx_ssrc = rtp_demux_id + 7;
    // Leave room for some more video layers or FEC
    // uint32_t data_ssrc = rtp_demux_id + 0xD;  Used by group_call.rs

    auto audio_stream = cricket::StreamParams();

    // We will use the string version of the demux ID to know which
    // track is for which remote device.
    std::string rtp_demux_id_str = rtc::ToString(rtp_demux_id);

    // For local, this should stay in sync with PeerConnectionFactory.createAudioTrack
    // For remote, this will result in the remote audio track/receiver's ID,
    audio_stream.id = local ? LOCAL_AUDIO_TRACK_ID : rtp_demux_id_str;
    audio_stream.add_ssrc(audio_ssrc);

    auto video_stream = cricket::StreamParams();
    // For local, this should stay in sync with PeerConnectionFactory.createVideoSource
    // For remote, this will result in the remote video track/receiver's ID,
    video_stream.id = local ? LOCAL_VIDEO_TRACK_ID : rtp_demux_id_str;
    video_stream.add_ssrc(video1_ssrc);
    if (local) {
      // Don't add simulcast for remote descriptions
      video_stream.add_ssrc(video2_ssrc);
      video_stream.add_ssrc(video3_ssrc);
      video_stream.ssrc_groups.push_back(cricket::SsrcGroup(cricket::kSimSsrcGroupSemantics, video_stream.ssrcs));
    }
    video_stream.AddFidSsrc(video1_ssrc, video1_rtx_ssrc);  // AKA RTX
    if (local) {
      // Don't add simulcast for remote descriptions
      video_stream.AddFidSsrc(video2_ssrc, video2_rtx_ssrc);  // AKA RTX
      video_stream.AddFidSsrc(video3_ssrc, video3_rtx_ssrc);  // AKA RTX
    }
    // This makes screen share use 2 layers of the highest resolution
    // (but different quality/framerate) rather than 3 layers of
    // differing resolution.
    video->set_conference_mode(true);

    // Things that are the same for all of them
    for (auto* stream : {&audio_stream, &video_stream}) {
      // WebRTC just generates a random 16-byte string for the entire PeerConnection.
      // It's used to send an SDES RTCP message.
      // The value doesn't seem to be used for anything else.
      // We'll set it around just in case.
      // But everything seems to work fine without it.
      stream->cname = rtp_demux_id_str;
    }

    audio->AddStream(audio_stream);
    video->AddStream(video_stream);
  }

  // TODO: Why is this only for video by default in WebRTC? Should we enable it for all of them?
  video->set_rtcp_reduced_size(true);

  // We don't set the crypto keys here.
  // We expect that will be done later by Rust_disableDtlsAndSetSrtpKey.

  // Keep the order as the WebRTC default: (audio, video).
  auto audio_content_name = "audio";
  auto video_content_name = "video";

  auto session = std::make_unique<cricket::SessionDescription>();
  session->AddTransportInfo(cricket::TransportInfo(audio_content_name, transport));
  session->AddTransportInfo(cricket::TransportInfo(video_content_name, transport));

  bool stopped = false;
  session->AddContent(audio_content_name, cricket::MediaProtocolType::kRtp, stopped, std::move(audio));
  session->AddContent(video_content_name, cricket::MediaProtocolType::kRtp, stopped, std::move(video));

  auto bundle = cricket::ContentGroup(cricket::GROUP_TYPE_BUNDLE);
  bundle.AddContentName(audio_content_name);
  bundle.AddContentName(video_content_name);
  session->AddGroup(bundle);

  // This is the default and used for "Plan B" SDP, which is what we use in V1, V2, and V3.
  session->set_msid_signaling(cricket::kMsidSignalingSsrcAttribute);

  auto typ = local ? SdpType::kOffer : SdpType::kAnswer;
  // The session ID and session version (both "1" here) go into SDP, but are not used at all.
  return new webrtc::JsepSessionDescription(typ, std::move(session), "1", "1");
}

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_localDescriptionForGroupCall(const char* ice_ufrag,
                                  const char* ice_pwd,
                                  const uint8_t dtls_fingerprint_sha256[32],
                                  uint32_t rtp_demux_id) {
  std::unique_ptr<rtc::SSLFingerprint> dtls_fingerprint = std::make_unique<rtc::SSLFingerprint>(
    rtc::DIGEST_SHA_256, rtc::ArrayView<const uint8_t>(dtls_fingerprint_sha256, 32));
  std::vector<uint32_t> rtp_demux_ids;
  // A 0 demux_id means we don't know the demux ID yet and shouldn't include one.
  if (rtp_demux_id > 0) {
    rtp_demux_ids.push_back(rtp_demux_id);
  }
  return CreateSessionDescriptionForGroupCall(
    true /* local */, std::string(ice_ufrag), std::string(ice_pwd), std::move(dtls_fingerprint), rtp_demux_ids);
}

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_remoteDescriptionForGroupCall(const char* ice_ufrag,
                                   const char* ice_pwd,
                                   const uint8_t dtls_fingerprint_sha256[32],
                                   uint32_t* rtp_demux_ids_data,
                                   size_t rtp_demux_ids_len) {
  std::unique_ptr<rtc::SSLFingerprint> dtls_fingerprint = std::make_unique<rtc::SSLFingerprint>(
    rtc::DIGEST_SHA_256, rtc::ArrayView<const uint8_t>(dtls_fingerprint_sha256, 32));
  std::vector<uint32_t> rtp_demux_ids;
  rtp_demux_ids.assign(rtp_demux_ids_data, rtp_demux_ids_data + rtp_demux_ids_len);
  return CreateSessionDescriptionForGroupCall(
    false /* local */, std::string(ice_ufrag), std::string(ice_pwd), std::move(dtls_fingerprint), rtp_demux_ids);
}

RUSTEXPORT void
Rust_createAnswer(PeerConnectionInterface*              peer_connection,
                  CreateSessionDescriptionObserverRffi* csd_observer) {

  // No constraints are set
  MediaConstraints constraints = MediaConstraints();
  PeerConnectionInterface::RTCOfferAnswerOptions options;

  CopyConstraintsIntoOfferAnswerOptions(&constraints, &options);
  peer_connection->CreateAnswer(csd_observer, options);
}

// Warning!  This takes ownership of the remote description
RUSTEXPORT void
Rust_setRemoteDescription(PeerConnectionInterface*           peer_connection,
                          SetSessionDescriptionObserverRffi* ssd_observer,
                          SessionDescriptionInterface*       description) {
  peer_connection->SetRemoteDescription(ssd_observer, description);
}

RUSTEXPORT void
Rust_releaseSessionDescription(webrtc::SessionDescriptionInterface* description) {
  delete description;
}

RUSTEXPORT void
Rust_setOutgoingMediaEnabled(PeerConnectionInterface* peer_connection,
                             bool                     enabled) {
  // Note: calling SetAudioRecording(enabled) is deprecated and it's not clear
  // that it even does anything any more.
  int encodings_changed = 0;
  for (auto& sender : peer_connection->GetSenders()) {
    RtpParameters parameters = sender->GetParameters();
    for (auto& encoding: parameters.encodings) {
      encoding.active = enabled;
      encodings_changed++;
    }
    sender->SetParameters(parameters);
  }
  RTC_LOG(LS_INFO) << "Rust_setOutgoingMediaEnabled(" << enabled << ") for " << encodings_changed << " encodings.";
}

RUSTEXPORT bool
Rust_setIncomingMediaEnabled(PeerConnectionInterface* peer_connection,
                             bool                     enabled) {
  RTC_LOG(LS_INFO) << "Rust_setIncomingMediaEnabled(" << enabled << ")";
  return peer_connection->SetIncomingRtpEnabled(enabled);
}

RUSTEXPORT void
Rust_setAudioPlayoutEnabled(webrtc::PeerConnectionInterface* peer_connection,
                            bool                             enabled) {
  RTC_LOG(LS_INFO) << "Rust_setAudioPlayoutEnabled(" << enabled << ")";
  peer_connection->SetAudioPlayout(enabled);
}

RUSTEXPORT DataChannelInterface*
Rust_createSignalingDataChannel(PeerConnectionInterface* peer_connection,
                                PeerConnectionObserver* pc_observer) {
  struct DataChannelInit dc_config;
  rtc::scoped_refptr<DataChannelInterface> channel = peer_connection->CreateDataChannel("signaling", &dc_config);

  // Let the observer know a data channel was create so it can register itself to receive messages.
  pc_observer->OnDataChannel(channel);

  // Channel is now owned by caller.  Must call Rust_releaseRef() eventually.
  return channel.release();
}

RUSTEXPORT bool
Rust_addIceCandidateFromSdp(PeerConnectionInterface* peer_connection,
                            const char*              sdp) {
  // Since we always use bundle, we can always use index 0 and ignore the mid
  std::unique_ptr<IceCandidateInterface> ice_candidate(
      CreateIceCandidate("", 0, std::string(sdp), nullptr));

  return peer_connection->AddIceCandidate(ice_candidate.get());
}

RUSTEXPORT bool
Rust_removeIceCandidates(PeerConnectionInterface* pc, IpPort* removed_addresses_data, size_t removed_addresses_len) {
  std::vector<IpPort> removed_addresses;
  removed_addresses.assign(removed_addresses_data, removed_addresses_data + removed_addresses_len);

  std::vector<cricket::Candidate> candidates_removed;
  for (const auto& address_removed : removed_addresses) {
    // This only needs to contain the correct transport_name, component, protocol, and address.
    // SeeCandidate::MatchesForRemoval and JsepTransportController::RemoveRemoteCandidates
    // and JsepTransportController::RemoveRemoteCandidates.
    // But we know (because we bundle/rtcp-mux everything) that the transport name is "audio",
    // and the component is 1.  We also know (because we don't use TCP candidates) that the
    // protocol is UDP.  So we only need to know the address.
    cricket::Candidate candidate_removed;
    candidate_removed.set_transport_name("audio");
    candidate_removed.set_component(cricket::ICE_CANDIDATE_COMPONENT_RTP);
    candidate_removed.set_protocol(cricket::UDP_PROTOCOL_NAME);
    candidate_removed.set_address(IpPortToRtcSocketAddress(address_removed));

    candidates_removed.push_back(candidate_removed);
  }

  return pc->RemoveIceCandidates(candidates_removed);
}


RUSTEXPORT bool
Rust_addIceCandidateFromServer(PeerConnectionInterface* pc, Ip ip, uint16_t port, bool tcp) {
  cricket::Candidate candidate;
  // The default foundation is "", which is fine because we bundle.
  // The default generation is 0,  which is fine because we don't do ICE restarts.
  // The default username and password are "", which is fine because
  //   P2PTransportChannel::AddRemoteCandidate looks up the ICE ufrag and pwd
  //   from the remote description when the candidate's copy is empty.
  // Unset network ID, network cost, and network type are fine because they are for p2p use.
  // An unset relay protocol is fine because we aren't doing relay.
  // An unset related address is fine because we aren't doing relay or STUN.
  //
  // The critical values are component, type, protocol, and address, so we set those.
  //
  // The component doesn't really matter because we use RTCP-mux, so there is only one component.
  // However, WebRTC expects it to be set to ICE_CANDIDATE_COMPONENT_RTP(1), so we do that.
  //
  // The priority is also important for controlling whether we prefer IPv4 or IPv6 when both are available.
  // WebRTC generally prefers IPv6 over IPv4 for local candidates (see rtc_base::IPAddressPrecedence).
  // So we leave the priority unset to allow the local candidate preference to break the tie.
  candidate.set_component(cricket::ICE_CANDIDATE_COMPONENT_RTP);
  candidate.set_type(cricket::LOCAL_PORT_TYPE);  // AKA "host"
  candidate.set_address(rtc::SocketAddress(IpToRtcIp(ip), port));
  candidate.set_protocol(tcp ? cricket::TCP_PROTOCOL_NAME : cricket::UDP_PROTOCOL_NAME);

  // Since we always use bundle, we can always use index 0 and ignore the mid
  std::unique_ptr<IceCandidateInterface> ice_candidate(
      CreateIceCandidate("", 0, candidate));

  return pc->AddIceCandidate(ice_candidate.get());
}

RUSTEXPORT IceGathererInterface*
Rust_createSharedIceGatherer(PeerConnectionInterface* peer_connection) {
  rtc::scoped_refptr<IceGathererInterface> ice_gatherer = peer_connection->CreateSharedIceGatherer();

  // IceGatherer is now owned by caller.  Must call Rust_releaseRef() eventually.
  return ice_gatherer.release();
}

RUSTEXPORT bool
Rust_useSharedIceGatherer(PeerConnectionInterface* peer_connection,
                          IceGathererInterface* ice_gatherer) {
  return peer_connection->UseSharedIceGatherer(rtc::scoped_refptr<IceGathererInterface>(ice_gatherer));
}

RUSTEXPORT void
Rust_getStats(PeerConnectionInterface* peer_connection,
              StatsObserverRffi* stats_observer) {
  peer_connection->GetStats(stats_observer);
}

// This is fairly complex in WebRTC, but I think it's something like this:
// Must be that 0 <= min <= start <= max.
// But any value can be unset (-1).  If so, here is what happens:
// If min isn't set, either use 30kbps (from PeerConnectionFactory::CreateCall_w) or no min (0 from WebRtcVideoChannel::ApplyChangedParams)
// If start isn't set, use the previous start; initially 100kbps (from PeerConnectionFactory::CreateCall_w)
// If max isn't set, either use 2mbps (from PeerConnectionFactory::CreateCall_w) or no max (-1 from WebRtcVideoChannel::ApplyChangedParams
// If min and max are set but haven't changed since last the last unset value, nothing happens.
// There is only an action if either min or max has changed or start is set.
RUSTEXPORT void
Rust_setSendBitrates(PeerConnectionInterface* peer_connection,
                     int32_t                  min_bitrate_bps,
                     int32_t                  start_bitrate_bps,
                     int32_t                  max_bitrate_bps) {
    struct BitrateSettings bitrate_settings;
    if (min_bitrate_bps >= 0) {
      bitrate_settings.min_bitrate_bps = min_bitrate_bps;
    }
    if (start_bitrate_bps >= 0) {
      bitrate_settings.start_bitrate_bps = start_bitrate_bps;
    }
    if (max_bitrate_bps >= 0) {
      bitrate_settings.max_bitrate_bps = max_bitrate_bps;
    }
    peer_connection->SetBitrate(bitrate_settings);
}

RUSTEXPORT bool
Rust_sendRtp(webrtc::PeerConnectionInterface* peer_connection,
             uint8_t pt,
             uint16_t seqnum,
             uint32_t timestamp,
             uint32_t ssrc,
             const uint8_t* payload_data,
             size_t payload_size) {
  size_t packet_size = 12 /* RTP header */ + payload_size + 16 /* SRTP footer */;
  std::unique_ptr<RtpPacket> packet(
    new RtpPacket(nullptr /* header extension map */, packet_size));
  packet->SetPayloadType(pt);
  packet->SetSequenceNumber(seqnum);
  packet->SetTimestamp(timestamp);
  packet->SetSsrc(ssrc);
  memcpy(packet->AllocatePayload(payload_size), payload_data, payload_size);
  return peer_connection->SendRtp(std::move(packet));
}

RUSTEXPORT bool
Rust_receiveRtp(webrtc::PeerConnectionInterface* peer_connection, uint8_t pt) {
  return peer_connection->ReceiveRtp(pt);
}

RUSTEXPORT void
Rust_configureAudioEncoders(webrtc::PeerConnectionInterface* peer_connection, const webrtc::AudioEncoder::Config* config) {
  RTC_LOG(LS_INFO) << "Rust_configureAudioEncoders(...)";
  peer_connection->ConfigureAudioEncoders(*config);
}

RUSTEXPORT void
Rust_closePeerConnection(PeerConnectionInterface* peer_connection) {
    peer_connection->Close();
}

} // namespace rffi
} // namespace webrtc
