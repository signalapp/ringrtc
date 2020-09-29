/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "rffi/api/peer_connection_observer_intf.h"
#include "rffi/src/peer_connection_observer.h"

namespace webrtc {
namespace rffi {

PeerConnectionObserverRffi::PeerConnectionObserverRffi(const rust_object observer,
                                                       const PeerConnectionObserverCallbacks* callbacks)
  : observer_(observer), callbacks_(*callbacks)
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

  // Ownership of |stream| is transfered to the rust call back
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
    // Ownership of |channel| is transfered to the rust call back
    // handler.  Must call Rust_releaseRef() eventually.
    callbacks_.onSignalingDataChannel(observer_, channel.release());
  }
}

void PeerConnectionObserverRffi::OnRenegotiationNeeded() {
  RTC_LOG(LS_INFO) << "OnRenegotiationNeeded()";
}

void PeerConnectionObserverRffi::OnAddTrack(
    rtc::scoped_refptr<RtpReceiverInterface> receiver,
    const std::vector<rtc::scoped_refptr<MediaStreamInterface>>& streams) {
  RTC_LOG(LS_INFO) << "OnAddTrack()";
}

void PeerConnectionObserverRffi::OnTrack(
    rtc::scoped_refptr<RtpTransceiverInterface> transceiver) {
  RTC_LOG(LS_INFO) << "OnTrack()";
}

void PeerConnectionObserverRffi::OnMessage(const DataBuffer& buffer) {
  RTC_LOG(LS_INFO) << "OnMessage() size: " << buffer.size();
  callbacks_.onSignalingDataChannelMessage(observer_, buffer.data.cdata(), buffer.size());
}

RUSTEXPORT PeerConnectionObserverRffi*
Rust_createPeerConnectionObserver(const rust_object observer,
                                  const PeerConnectionObserverCallbacks* callbacks) {
  // This observer will be freed automatically by the the
  // PeerConnection object during its .close() method.
  return new PeerConnectionObserverRffi(observer, callbacks);
}

} // namespace rffi
} // namespace webrtc
