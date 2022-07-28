/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_PEER_CONNECTION_OBSERVER_H__
#define RFFI_PEER_CONNECTION_OBSERVER_H__

#include "api/crypto/frame_encryptor_interface.h"
#include "api/media_stream_interface.h"
#include "api/peer_connection_interface.h"

/**
 * Adapter between the C++ PeerConnectionObserver interface and the
 * Rust PeerConnection.Observer interface.  Wraps an instance of the
 * Rust interface and dispatches C++ callbacks to Rust.
 */

namespace webrtc {
namespace rffi {

class VideoSink;

class PeerConnectionObserverRffi : public PeerConnectionObserver {
 public:
  // Passed-in observer must live at least as long as the PeerConnectionObserverRffi.
  PeerConnectionObserverRffi(void* observer,
                             const PeerConnectionObserverCallbacks* callbacks,
                             bool enable_frame_encryption,
                             bool enable_video_frame_event,
                             bool enable_video_frame_content);
  ~PeerConnectionObserverRffi() override;

  // If enabled, the PeerConnection will be configured to encrypt and decrypt
  // media frames using PeerConnectionObserverCallbacks.
  bool enable_frame_encryption() { return enable_frame_encryption_; }
  // These will be a passed into RtpSenders and will be implemented
  // with callbacks to PeerConnectionObserverCallbacks.
  rtc::scoped_refptr<FrameEncryptorInterface> CreateEncryptor();
  // These will be a passed into RtpReceivers and will be implemented
  // with callbacks to PeerConnectionObserverCallbacks.
  rtc::scoped_refptr<FrameDecryptorInterface> CreateDecryptor(uint32_t track_id);

  // Implementation of PeerConnectionObserver interface, which propagates
  // the callbacks to the Rust observer.
  void OnIceCandidate(const IceCandidateInterface* candidate) override;
  void OnIceCandidatesRemoved(
      const std::vector<cricket::Candidate>& candidates) override;
  void OnIceCandidateError(
      const std::string& address,
      int port,
      const std::string& url,
      int error_code,
      const std::string& error_text) override;
  void OnSignalingChange(
      PeerConnectionInterface::SignalingState new_state) override;
  void OnIceConnectionChange(
      PeerConnectionInterface::IceConnectionState new_state) override;
  void OnConnectionChange(
      PeerConnectionInterface::PeerConnectionState new_state) override;
  void OnIceConnectionReceivingChange(bool receiving) override;
  void OnIceGatheringChange(
      PeerConnectionInterface::IceGatheringState new_state) override;
  void OnIceSelectedCandidatePairChanged(
      const cricket::CandidatePairChangeEvent& event) override;
  void OnAddStream(rtc::scoped_refptr<MediaStreamInterface> stream) override;
  void OnRemoveStream(rtc::scoped_refptr<MediaStreamInterface> stream) override;
  void OnDataChannel(rtc::scoped_refptr<DataChannelInterface> channel) override {}
  void OnRtpPacket(const RtpPacketReceived& rtp_packet) override;
  void OnRenegotiationNeeded() override;
  void OnAddTrack(rtc::scoped_refptr<RtpReceiverInterface> receiver,
                  const std::vector<rtc::scoped_refptr<MediaStreamInterface>>&
                      streams) override;
  void OnTrack(
      rtc::scoped_refptr<RtpTransceiverInterface> transceiver) override;

  // Called by the VideoSinks in video_sinks_.
  void OnVideoFrame(uint32_t track_id, const webrtc::VideoFrame& frame);

 private:
  // Add a VideoSink to the video_sinks_ for ownership and pass
  // a borrowed pointer to the track.
  void AddVideoSink(VideoTrackInterface* track);

  void* observer_;
  PeerConnectionObserverCallbacks callbacks_;
  bool enable_frame_encryption_ = false;
  bool enable_video_frame_event_ = false;
  bool enable_video_frame_content_ = false;
  std::vector<std::unique_ptr<VideoSink>> video_sinks_;
};

// A simple implementation of a VideoSinkInterface which passes video frames
// back to the PeerConnectionObserver with a track_id.
class VideoSink : public rtc::VideoSinkInterface<webrtc::VideoFrame> {
 public:
  VideoSink(uint32_t track_id, PeerConnectionObserverRffi*);
  ~VideoSink() override = default;

  void OnFrame(const webrtc::VideoFrame& frame) override;

 private:
  uint32_t track_id_;
  PeerConnectionObserverRffi* pc_observer_;
};



} // namespace rffi
} // namespace webrtc

#endif /* RFFI_PEER_CONNECTION_OBSERVER_H__ */
