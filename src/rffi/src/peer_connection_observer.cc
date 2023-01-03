/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "rffi/api/peer_connection_observer_intf.h"

#include "rffi/api/media.h"
#include "rffi/src/peer_connection_observer.h"
#include "rffi/src/ptr.h"

namespace webrtc {
namespace rffi {

PeerConnectionObserverRffi::PeerConnectionObserverRffi(void* observer,
                                                       const PeerConnectionObserverCallbacks* callbacks,
                                                       bool enable_frame_encryption,
                                                       bool enable_video_frame_event,
                                                       bool enable_video_frame_content)
  : observer_(observer), callbacks_(*callbacks), enable_frame_encryption_(enable_frame_encryption), enable_video_frame_event_(enable_video_frame_event), enable_video_frame_content_(enable_video_frame_content)
{
  RTC_LOG(LS_INFO) << "PeerConnectionObserverRffi:ctor(): " << this->observer_;
}

PeerConnectionObserverRffi::~PeerConnectionObserverRffi() {
  RTC_LOG(LS_INFO) << "PeerConnectionObserverRffi:dtor(): " << this->observer_;
}

void PeerConnectionObserverRffi::OnIceCandidate(const IceCandidateInterface* candidate) {
  RustIceCandidate rust_candidate;

  std::string sdp;
  candidate->ToString(&sdp);
  rust_candidate.sdp_borrowed = sdp.c_str();

  rust_candidate.is_relayed = (candidate->candidate().type() == cricket::RELAY_PORT_TYPE);
  rust_candidate.relay_protocol = TransportProtocol::kUnknown;
  if (candidate->candidate().relay_protocol() == cricket::UDP_PROTOCOL_NAME) {
    rust_candidate.relay_protocol = TransportProtocol::kUdp;
  } else if (candidate->candidate().relay_protocol() == cricket::TCP_PROTOCOL_NAME) {
    rust_candidate.relay_protocol = TransportProtocol::kTcp;
  } else if (candidate->candidate().relay_protocol() == cricket::TLS_PROTOCOL_NAME) {
    rust_candidate.relay_protocol = TransportProtocol::kTls;
  }

  callbacks_.onIceCandidate(observer_, &rust_candidate);
}

void PeerConnectionObserverRffi::OnIceCandidatesRemoved(
    const std::vector<cricket::Candidate>& candidates) {

  std::vector<IpPort> removed_addresses;
  for (const auto& candidate: candidates) {
    removed_addresses.push_back(RtcSocketAddressToIpPort(candidate.address()));
  }

  callbacks_.onIceCandidatesRemoved(observer_, removed_addresses.data(), removed_addresses.size());
}

void PeerConnectionObserverRffi::OnIceCandidateError(
    const std::string& address,
    int port,
    const std::string& url,
    int error_code,
    const std::string& error_text) {
  // Error code 701 is when we have an IPv4 local port trying to reach an IPv6 server or vice versa.
  // That's expected to not work, so we don't want to log that all the time.
  if (error_code != 701) {
    RTC_LOG(LS_WARNING) << "Failed to gather local ICE candidate from " << address << ":"  << port <<  " to " << url << "; error " << error_code << ": " << error_text;
  }
}

void PeerConnectionObserverRffi::OnSignalingChange(
    PeerConnectionInterface::SignalingState new_state) {
}

void PeerConnectionObserverRffi::OnIceConnectionChange(
    PeerConnectionInterface::IceConnectionState new_state) {
  callbacks_.onIceConnectionChange(observer_, new_state);
}

void PeerConnectionObserverRffi::OnConnectionChange(
    PeerConnectionInterface::PeerConnectionState new_state) {
}

void PeerConnectionObserverRffi::OnIceConnectionReceivingChange(bool receiving) {
  RTC_LOG(LS_INFO) << "OnIceConnectionReceivingChange()";
}

void PeerConnectionObserverRffi::OnIceSelectedCandidatePairChanged(
    const cricket::CandidatePairChangeEvent& event) {
  auto& local = event.selected_candidate_pair.local_candidate();
  auto& remote = event.selected_candidate_pair.remote_candidate();
  auto local_adapter_type = local.network_type();
  auto local_adapter_type_under_vpn = local.underlying_type_for_vpn();
  bool local_relayed = (local.type() == cricket::RELAY_PORT_TYPE) || !local.relay_protocol().empty();
  TransportProtocol local_relay_protocol = TransportProtocol::kUnknown;
  if (local.relay_protocol() == cricket::UDP_PROTOCOL_NAME) {
    local_relay_protocol = TransportProtocol::kUdp;
  } else if (local.relay_protocol() == cricket::TCP_PROTOCOL_NAME) {
    local_relay_protocol = TransportProtocol::kTcp;
  } else if (local.relay_protocol() == cricket::TLS_PROTOCOL_NAME) {
    local_relay_protocol = TransportProtocol::kTls;
  }
  bool remote_relayed = (remote.type() == cricket::RELAY_PORT_TYPE);
  auto network_route = webrtc::rffi::NetworkRoute{ local_adapter_type, local_adapter_type_under_vpn, local_relayed, local_relay_protocol, remote_relayed};
  callbacks_.onIceNetworkRouteChange(observer_, network_route);
}

void PeerConnectionObserverRffi::OnIceGatheringChange(
    PeerConnectionInterface::IceGatheringState new_state) {
  RTC_LOG(LS_INFO) << "OnIceGatheringChange()";
}

void PeerConnectionObserverRffi::OnAddStream(
    rtc::scoped_refptr<MediaStreamInterface> stream) {
  RTC_LOG(LS_INFO) << "OnAddStream()";

  auto video_tracks = stream->GetVideoTracks();
  if (!video_tracks.empty()) {
    AddVideoSink(video_tracks[0].get());
  }
  callbacks_.onAddStream(observer_, take_rc(stream));
}

void PeerConnectionObserverRffi::OnRemoveStream(
    rtc::scoped_refptr<MediaStreamInterface> stream) {
  RTC_LOG(LS_INFO) << "OnRemoveStream()";
}

void PeerConnectionObserverRffi::OnRtpPacket(const RtpPacketReceived& rtp_packet) {
  uint8_t pt = rtp_packet.PayloadType();
  uint16_t seqnum = rtp_packet.SequenceNumber();
  uint32_t timestamp = rtp_packet.Timestamp();
  uint32_t ssrc = rtp_packet.Ssrc();
  const uint8_t* payload_data = rtp_packet.payload().data();
  size_t payload_size = rtp_packet.payload().size();
  RTC_LOG(LS_VERBOSE) << "OnRtpReceived() << pt: " << pt  << " seqnum: " << seqnum << " timestamp: " << timestamp << " ssrc: " << ssrc << " size: " << payload_size;
  callbacks_.onRtpReceived(observer_, pt, seqnum, timestamp, ssrc, payload_data, payload_size);
}

void PeerConnectionObserverRffi::OnRenegotiationNeeded() {
  RTC_LOG(LS_INFO) << "OnRenegotiationNeeded()";
}

void PeerConnectionObserverRffi::OnAddTrack(
    rtc::scoped_refptr<RtpReceiverInterface> receiver,
    const std::vector<rtc::scoped_refptr<MediaStreamInterface>>& streams) {
  // TODO: Define FFI for an RtpReceiver and pass that here instead.
  // Ownership is transferred to the rust call back
  // handler.  Someone must call RefCountInterface::Release()
  // eventually.
  if (receiver->media_type() == cricket::MEDIA_TYPE_AUDIO) {
    if (enable_frame_encryption_) {
      uint32_t id = Rust_getTrackIdAsUint32(receiver->track().get());
      if (id != 0) {
        receiver->SetFrameDecryptor(CreateDecryptor(id));
        callbacks_.onAddAudioRtpReceiver(observer_, take_rc(receiver->track()));
      } else {
        RTC_LOG(LS_WARNING) << "Not sending decryptor for RtpReceiver with strange ID: " << receiver->track()->id();
      }
    } else {
      callbacks_.onAddAudioRtpReceiver(observer_, take_rc(receiver->track()));
    }
  } else if (receiver->media_type() == cricket::MEDIA_TYPE_VIDEO) {
    if (enable_frame_encryption_) {
      uint32_t id = Rust_getTrackIdAsUint32(receiver->track().get());
      if (id != 0) {
        receiver->SetFrameDecryptor(CreateDecryptor(id));
        AddVideoSink(static_cast<webrtc::VideoTrackInterface*>(receiver->track().get()));
        callbacks_.onAddVideoRtpReceiver(observer_, take_rc(receiver->track()));
      } else {
        RTC_LOG(LS_WARNING) << "Not sending decryptor for RtpReceiver with strange ID: " << receiver->track()->id();
      }
    } else {
      AddVideoSink(static_cast<webrtc::VideoTrackInterface*>(receiver->track().get()));
      callbacks_.onAddVideoRtpReceiver(observer_, take_rc(receiver->track()));
    }
  }
}

void PeerConnectionObserverRffi::OnTrack(
    rtc::scoped_refptr<RtpTransceiverInterface> transceiver) {
  RTC_LOG(LS_INFO) << "OnTrack()";
}

class Encryptor : public webrtc::FrameEncryptorInterface {
 public:
  // Passed-in observer must live at least as long as the Encryptor,
  // which likely means as long as the PeerConnection.
  Encryptor(void* observer, PeerConnectionObserverCallbacks* callbacks) : observer_(observer), callbacks_(callbacks) {}

  // This is called just before Encrypt to get the size of the ciphertext
  // buffer that will be given to Encrypt.
  size_t GetMaxCiphertextByteSize(cricket::MediaType media_type,
                                  size_t plaintext_size) override {
    bool is_audio = (media_type == cricket::MEDIA_TYPE_AUDIO);
    bool is_video = (media_type == cricket::MEDIA_TYPE_VIDEO);
    if (!is_audio && !is_video) {
      RTC_LOG(LS_WARNING) << "GetMaxCiphertextByteSize called with weird media type: " << media_type;
      return 0;
    }
    return callbacks_->getMediaCiphertextBufferSize(observer_, is_audio, plaintext_size);
  }
                                          
  int Encrypt(cricket::MediaType media_type,
              // Our encryption mechanism is the same regardless of SSRC
              uint32_t _ssrc,
              // This is not supported by our SFU currently, so don't bother trying to use it.
              rtc::ArrayView<const uint8_t> _generic_video_header,
              rtc::ArrayView<const uint8_t> plaintext,
              rtc::ArrayView<uint8_t> ciphertext_buffer,
              size_t* ciphertext_size) override {
    bool is_audio = (media_type == cricket::MEDIA_TYPE_AUDIO);
    bool is_video = (media_type == cricket::MEDIA_TYPE_VIDEO);
    if (!is_audio && !is_video) {
      RTC_LOG(LS_WARNING) << "Encrypt called with weird media type: " << media_type;
      return -1;  // Error
    }
    if (!callbacks_->encryptMedia(observer_, is_audio, plaintext.data(), plaintext.size(), ciphertext_buffer.data(), ciphertext_buffer.size(), ciphertext_size)) {
      return -2;  // Error
    }
    return 0;  // No error
  }

 private:
  void* observer_;
  PeerConnectionObserverCallbacks* callbacks_;
};

rtc::scoped_refptr<FrameEncryptorInterface> PeerConnectionObserverRffi::CreateEncryptor() {
  // The PeerConnectionObserverRffi outlives the Encryptor because it outlives the PeerConnection,
  // which outlives the RtpSender, which owns the Encryptor.
  // So we know the PeerConnectionObserverRffi outlives the Encryptor.
  return rtc::make_ref_counted<Encryptor>(observer_, &callbacks_);
}

void PeerConnectionObserverRffi::AddVideoSink(VideoTrackInterface* track) {
  if (!enable_video_frame_event_ || !track) {
    return;
  }

  uint32_t track_id = Rust_getTrackIdAsUint32(track);
  auto sink = std::make_unique<VideoSink>(track_id, this);

  rtc::VideoSinkWants wants;
  // Note: this causes frames to be dropped, not rotated.
  // So don't set it to true, even if it seems to make sense!
  wants.rotation_applied = false;

  // The sink gets stored in the track, but never destroys it.
  // The sink must live as long as the track, which is why we
  // stored it in the PeerConnectionObserverRffi.
  track->AddOrUpdateSink(sink.get(), wants);
  video_sinks_.push_back(std::move(sink));
}

VideoSink::VideoSink(uint32_t track_id, PeerConnectionObserverRffi* pc_observer)
  : track_id_(track_id), pc_observer_(pc_observer) {
}

void VideoSink::OnFrame(const webrtc::VideoFrame& frame) {
  pc_observer_->OnVideoFrame(track_id_, frame);
}

void PeerConnectionObserverRffi::OnVideoFrame(uint32_t track_id, const webrtc::VideoFrame& frame) {
  RffiVideoFrameMetadata metadata = {};
  metadata.width = frame.width();
  metadata.height = frame.height();
  metadata.rotation = frame.rotation();
  // We can't keep a reference to the buffer around or it will slow down the video decoder.
  // This introduces a copy, but only in the case where we aren't rotated,
  // and it's a copy of i420 and not RGBA (i420 is smaller than RGBA).
  // TODO: Figure out if we can make the decoder have a larger frame output pool
  // so that we don't need to do this.
  auto* buffer_owned_rc = enable_video_frame_content_ ? Rust_copyAndRotateVideoFrameBuffer(frame.video_frame_buffer().get(), frame.rotation()) : nullptr;
  // If we rotated the frame, we need to update metadata as well
  if ((metadata.rotation == kVideoRotation_90) || (metadata.rotation == kVideoRotation_270)) {
    metadata.width = frame.height();
    metadata.height = frame.width();
  }
  metadata.rotation = kVideoRotation_0;

  callbacks_.onVideoFrame(observer_, track_id, metadata, buffer_owned_rc);
}

class Decryptor : public webrtc::FrameDecryptorInterface {
 public:
  // Passed-in observer must live at least as long as the Decryptor,
  // which likely means as long as the PeerConnection.
  Decryptor(uint32_t track_id, void* observer, PeerConnectionObserverCallbacks* callbacks) : track_id_(track_id), observer_(observer), callbacks_(callbacks) {}

  // This is called just before Decrypt to get the size of the plaintext
  // buffer that will be given to Decrypt.
  size_t GetMaxPlaintextByteSize(cricket::MediaType media_type,
                                 size_t ciphertext_size) override {
    bool is_audio = (media_type == cricket::MEDIA_TYPE_AUDIO);
    bool is_video = (media_type == cricket::MEDIA_TYPE_VIDEO);
    if (!is_audio && !is_video) {
      RTC_LOG(LS_WARNING) << "GetMaxPlaintextByteSize called with weird media type: " << media_type;
      return 0;
    }
    return callbacks_->getMediaPlaintextBufferSize(observer_, track_id_, is_audio, ciphertext_size);
  }

  FrameDecryptorInterface::Result Decrypt(cricket::MediaType media_type,
                                          // Our encryption mechanism is the same regardless of CSRCs
                                          const std::vector<uint32_t>& _csrcs,
                                          // This is not supported by our SFU currently, so don't bother trying to use it.
                                          rtc::ArrayView<const uint8_t> _generic_video_header,
                                          rtc::ArrayView<const uint8_t> ciphertext,
                                          rtc::ArrayView<uint8_t> plaintext_buffer) override {
    bool is_audio = (media_type == cricket::MEDIA_TYPE_AUDIO);
    bool is_video = (media_type == cricket::MEDIA_TYPE_VIDEO);
    if (!is_audio && !is_video) {
      RTC_LOG(LS_WARNING) << "Decrypt called with weird media type: " << media_type;
      return FrameDecryptorInterface::Result(FrameDecryptorInterface::Status::kUnknown, 0);
    }
    size_t plaintext_size = 0;
    if (!callbacks_->decryptMedia(observer_, track_id_, is_audio, ciphertext.data(), ciphertext.size(), plaintext_buffer.data(), plaintext_buffer.size(), &plaintext_size)) {
      return FrameDecryptorInterface::Result(FrameDecryptorInterface::Status::kFailedToDecrypt, 0);
    }
    return FrameDecryptorInterface::Result(FrameDecryptorInterface::Status::kOk, plaintext_size);
  }

 private:
  uint32_t track_id_;
  void* observer_;
  PeerConnectionObserverCallbacks* callbacks_;
};

rtc::scoped_refptr<FrameDecryptorInterface> PeerConnectionObserverRffi::CreateDecryptor(uint32_t track_id) {
  // The PeerConnectionObserverRffi outlives the Decryptor because it outlives the PeerConnection,
  // which outlives the RtpReceiver, which owns the Decryptor.
  // So we know the PeerConnectionObserverRffi outlives the Decryptor.
  return rtc::make_ref_counted<Decryptor>(track_id, observer_, &callbacks_);
}

// Returns an owned pointer.
// Passed-in observer must live at least as long as the returned value,
// which in turn must live at least as long as the PeerConnection.
RUSTEXPORT PeerConnectionObserverRffi*
Rust_createPeerConnectionObserver(void* observer_borrowed,
                                  const PeerConnectionObserverCallbacks* callbacks_borrowed,
                                  bool enable_frame_encryption,
                                  bool enable_video_frame_event,
                                  bool enable_video_frame_content) {
  return new PeerConnectionObserverRffi(observer_borrowed, callbacks_borrowed, enable_frame_encryption, enable_video_frame_event, enable_video_frame_content);
}

RUSTEXPORT void
Rust_deletePeerConnectionObserver(PeerConnectionObserverRffi* observer_owned) {
  delete observer_owned;
}

} // namespace rffi
} // namespace webrtc
