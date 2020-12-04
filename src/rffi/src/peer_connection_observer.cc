/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "rffi/api/peer_connection_observer_intf.h"

#include "rffi/api/media.h"
#include "rffi/src/peer_connection_observer.h"

namespace webrtc {
namespace rffi {

PeerConnectionObserverRffi::PeerConnectionObserverRffi(const rust_object observer,
                                                       const PeerConnectionObserverCallbacks* callbacks,
                                                       bool enable_frame_encryption)
  : observer_(observer), callbacks_(*callbacks), enable_frame_encryption_(enable_frame_encryption)
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
  rust_candidate.sdp = sdp.c_str();

  callbacks_.onIceCandidate(observer_, &rust_candidate);

}

void PeerConnectionObserverRffi::OnIceCandidatesRemoved(
    const std::vector<cricket::Candidate>& candidates) {
  RTC_LOG(LS_INFO) << "OnIceCandidatesRemoved()";
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

void PeerConnectionObserverRffi::OnIceGatheringChange(
    PeerConnectionInterface::IceGatheringState new_state) {
  RTC_LOG(LS_INFO) << "OnIceGatheringChange()";
}

void PeerConnectionObserverRffi::OnAddStream(
    rtc::scoped_refptr<MediaStreamInterface> stream) {
  RTC_LOG(LS_INFO) << "OnAddStream()";

  // Ownership of |stream| is transferred to the rust call back
  // handler.  Someone must call RefCountInterface::Release()
  // eventually.
  callbacks_.onAddStream(observer_, stream.release());
}

void PeerConnectionObserverRffi::OnRemoveStream(
    rtc::scoped_refptr<MediaStreamInterface> stream) {
  RTC_LOG(LS_INFO) << "OnRemoveStream()";
}

void PeerConnectionObserverRffi::OnDataChannel(rtc::scoped_refptr<DataChannelInterface> channel) {
  RTC_LOG(LS_INFO) << "OnDataChannel() label: " << channel->label();

  if (channel->label() == "signaling") {
    channel->RegisterObserver(this);
    // Ownership of |channel| is transferred to the rust call back
    // handler.  Must call Rust_releaseRef() eventually.
    callbacks_.onSignalingDataChannel(observer_, channel.release());
  }
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
      uint32_t id = Rust_getTrackIdAsUint32(receiver->track());
      if (id != 0) {
        receiver->SetFrameDecryptor(CreateDecryptor(id));
        callbacks_.onAddAudioRtpReceiver(observer_, receiver->track().release());
      } else {
        RTC_LOG(LS_WARNING) << "Not sending decryptor for RtpReceiver with strange ID: " << receiver->track()->id();
      }
    } else {
      callbacks_.onAddAudioRtpReceiver(observer_, receiver->track().release());
    }
  } else if (receiver->media_type() == cricket::MEDIA_TYPE_VIDEO) {
    if (enable_frame_encryption_) {
      uint32_t id = Rust_getTrackIdAsUint32(receiver->track());
      if (id != 0) {
        receiver->SetFrameDecryptor(CreateDecryptor(id));
        callbacks_.onAddVideoRtpReceiver(observer_, receiver->track().release());
      } else {
        RTC_LOG(LS_WARNING) << "Not sending decryptor for RtpReceiver with strange ID: " << receiver->track()->id();
      }
    } else {
      callbacks_.onAddVideoRtpReceiver(observer_, receiver->track().release());
    }
  }
}

void PeerConnectionObserverRffi::OnTrack(
    rtc::scoped_refptr<RtpTransceiverInterface> transceiver) {
  RTC_LOG(LS_INFO) << "OnTrack()";
}

void PeerConnectionObserverRffi::OnMessage(const DataBuffer& buffer) {
  RTC_LOG(LS_INFO) << "OnMessage() size: " << buffer.size();
  callbacks_.onSignalingDataChannelMessage(observer_, buffer.data.cdata(), buffer.size());
}

class Encryptor : public webrtc::FrameEncryptorInterface {
 public:
  Encryptor(const rust_object observer, PeerConnectionObserverCallbacks* callbacks) : observer_(observer), callbacks_(callbacks) {}

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
  const rust_object observer_;
  PeerConnectionObserverCallbacks* callbacks_;
};

rtc::scoped_refptr<FrameEncryptorInterface> PeerConnectionObserverRffi::CreateEncryptor() {
  // The PeerConnectionObserverRffi outlives the Encryptor because it outlives the PeerConnection,
  // which outlives the RtpSender, which owns the Encryptor.
  // So we know the PeerConnectionObserverRffi outlives the Encryptor.
  return new rtc::RefCountedObject<Encryptor>(observer_, &callbacks_);
}

class Decryptor : public webrtc::FrameDecryptorInterface {
 public:
  Decryptor(uint32_t track_id, const rust_object observer, PeerConnectionObserverCallbacks* callbacks) : track_id_(track_id), observer_(observer), callbacks_(callbacks) {}

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
  const rust_object observer_;
  PeerConnectionObserverCallbacks* callbacks_;
};

rtc::scoped_refptr<FrameDecryptorInterface> PeerConnectionObserverRffi::CreateDecryptor(uint32_t track_id) {
  // The PeerConnectionObserverRffi outlives the Decryptor because it outlives the PeerConnection,
  // which outlives the RtpReceiver, which owns the Decryptor.
  // So we know the PeerConnectionObserverRffi outlives the Decryptor.
  return new rtc::RefCountedObject<Decryptor>(track_id, observer_, &callbacks_);
}

RUSTEXPORT PeerConnectionObserverRffi*
Rust_createPeerConnectionObserver(const rust_object observer,
                                  const PeerConnectionObserverCallbacks* callbacks,
                                  bool enable_frame_encryption) {
  // This observer will be freed automatically by the the
  // PeerConnection object during its .close() method.
  return new PeerConnectionObserverRffi(observer, callbacks, enable_frame_encryption);
}

} // namespace rffi
} // namespace webrtc
