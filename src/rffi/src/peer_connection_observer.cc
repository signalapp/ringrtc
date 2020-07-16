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

PeerConnectionObserverRffi::PeerConnectionObserverRffi(
                                                       const rust_object call_connection,
                                                       const PeerConnectionObserverCallbacks* pc_observer_cbs)
  : call_connection_(call_connection), pc_observer_cbs_(*pc_observer_cbs)
{
  RTC_LOG(LS_INFO) << "PeerConnectionObserverRffi:ctor(): " << this->call_connection_;
}

PeerConnectionObserverRffi::~PeerConnectionObserverRffi() {
  RTC_LOG(LS_INFO) << "PeerConnectionObserverRffi:dtor(): " << this->call_connection_;
}

void PeerConnectionObserverRffi::OnIceCandidate(const IceCandidateInterface* candidate) {
  RustIceCandidate rust_candidate;

  std::string sdp;
  candidate->ToString(&sdp);
  rust_candidate.sdp = sdp.c_str();

  pc_observer_cbs_.onIceCandidate(call_connection_, &rust_candidate);

}

void PeerConnectionObserverRffi::OnIceCandidatesRemoved(
    const std::vector<cricket::Candidate>& candidates) {
  RTC_LOG(LS_INFO) << "OnIceCandidatesRemoved()";

  /* This callback is ignored for now */
  // pc_observer_cbs_.onIceCandidatesRemoved(call_connection_);
}

void PeerConnectionObserverRffi::OnSignalingChange(
    PeerConnectionInterface::SignalingState new_state) {
  pc_observer_cbs_.onSignalingChange(call_connection_, new_state);
}

void PeerConnectionObserverRffi::OnIceConnectionChange(
    PeerConnectionInterface::IceConnectionState new_state) {
  pc_observer_cbs_.onIceConnectionChange(call_connection_, new_state);
}

void PeerConnectionObserverRffi::OnConnectionChange(
    PeerConnectionInterface::PeerConnectionState new_state) {
  pc_observer_cbs_.onConnectionChange(call_connection_, new_state);
}

void PeerConnectionObserverRffi::OnIceConnectionReceivingChange(bool receiving) {
  RTC_LOG(LS_INFO) << "OnIceConnectionReceivingChange()";

  /* This callback is ignored for now */
  // pc_observer_cbs_.onIceConnectionReceivingChange(call_connection_);
}

void PeerConnectionObserverRffi::OnIceGatheringChange(
    PeerConnectionInterface::IceGatheringState new_state) {
  pc_observer_cbs_.onIceGatheringChange(call_connection_, new_state);
}

void PeerConnectionObserverRffi::OnAddStream(
    rtc::scoped_refptr<MediaStreamInterface> stream) {
  RTC_LOG(LS_INFO) << "OnAddStream()";

  // Ownership of |stream| is transfered to the rust call back
  // handler.  Someone must call RefCountInterface::Release()
  // eventually.
  pc_observer_cbs_.onAddStream(call_connection_, stream.release());
}

void PeerConnectionObserverRffi::OnRemoveStream(
    rtc::scoped_refptr<MediaStreamInterface> stream) {
  RTC_LOG(LS_INFO) << "OnRemoveStream()";
  pc_observer_cbs_.onRemoveStream(call_connection_);
}

void PeerConnectionObserverRffi::OnDataChannel(rtc::scoped_refptr<DataChannelInterface> channel) {
  RTC_LOG(LS_INFO) << "OnDataChannel()";

  // Ownership of |channel| is transfered to the rust call back
  // handler.  Must call Rust_releaseRef() eventually.
  pc_observer_cbs_.onDataChannel(call_connection_, channel.release());
}

void PeerConnectionObserverRffi::OnRenegotiationNeeded() {
  RTC_LOG(LS_INFO) << "OnRenegotiationNeeded()";
  pc_observer_cbs_.onRenegotiationNeeded(call_connection_);
}

void PeerConnectionObserverRffi::OnAddTrack(
    rtc::scoped_refptr<RtpReceiverInterface> receiver,
    const std::vector<rtc::scoped_refptr<MediaStreamInterface>>& streams) {
  RTC_LOG(LS_INFO) << "OnAddTrack()";
  pc_observer_cbs_.onAddTrack(call_connection_);
}

void PeerConnectionObserverRffi::OnTrack(
    rtc::scoped_refptr<RtpTransceiverInterface> transceiver) {
  RTC_LOG(LS_INFO) << "OnTrack()";
  pc_observer_cbs_.onTrack(call_connection_);
}

RUSTEXPORT PeerConnectionObserverRffi*
Rust_createPeerConnectionObserver(const rust_object call_connection,
                                  const PeerConnectionObserverCallbacks* pc_observer_cbs) {
  // This observer will be freed automatically by the the
  // PeerConnection object during its .close() method.
  return new PeerConnectionObserverRffi(call_connection, pc_observer_cbs);
}

} // namespace rffi
} // namespace webrtc
