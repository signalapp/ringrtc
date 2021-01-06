/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_PEER_CONNECTION_OBSERVER_H__
#define RFFI_PEER_CONNECTION_OBSERVER_H__

#include "api/data_channel_interface.h"
#include "api/crypto/frame_encryptor_interface.h"
#include "api/peer_connection_interface.h"

/**
 * Adapter between the C++ PeerConnectionObserver interface and the
 * Rust PeerConnection.Observer interface.  Wraps an instance of the
 * Rust interface and dispatches C++ callbacks to Rust.
 */

namespace webrtc {
namespace rffi {

class PeerConnectionObserverRffi : public PeerConnectionObserver, public DataChannelObserver {
 public:
  PeerConnectionObserverRffi(const rust_object observer,
                             const PeerConnectionObserverCallbacks* callbacks,
                             bool enable_frame_encryption);
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
  void OnSignalingChange(
      PeerConnectionInterface::SignalingState new_state) override;
  void OnIceConnectionChange(
      PeerConnectionInterface::IceConnectionState new_state) override;
  void OnConnectionChange(
      PeerConnectionInterface::PeerConnectionState new_state) override;
  void OnIceConnectionReceivingChange(bool receiving) override;
  void OnIceGatheringChange(
      PeerConnectionInterface::IceGatheringState new_state) override;
  void OnAddStream(rtc::scoped_refptr<MediaStreamInterface> stream) override;
  void OnRemoveStream(rtc::scoped_refptr<MediaStreamInterface> stream) override;
  void OnDataChannel(rtc::scoped_refptr<DataChannelInterface> channel) override;
  void OnRtpPacket(const RtpPacketReceived& rtp_packet) override;
  void OnRenegotiationNeeded() override;
  void OnAddTrack(rtc::scoped_refptr<RtpReceiverInterface> receiver,
                  const std::vector<rtc::scoped_refptr<MediaStreamInterface>>&
                      streams) override;
  void OnTrack(
      rtc::scoped_refptr<RtpTransceiverInterface> transceiver) override;

  // Implementation of DataChannelObserver interface, which propagates
  // the callbacks to the Rust observer.
  void OnMessage(const DataBuffer& buffer) override;
  void OnBufferedAmountChange(uint64_t previous_amount) override {}
  void OnStateChange() override {}

 private:
  const rust_object observer_;
  PeerConnectionObserverCallbacks callbacks_;
  bool enable_frame_encryption_ = false;
};

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_PEER_CONNECTION_OBSERVER_H__ */
