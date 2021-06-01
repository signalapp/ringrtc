/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_API_PEER_CONNECTION_INTF_H__
#define RFFI_API_PEER_CONNECTION_INTF_H__

#include "api/peer_connection_interface.h"
#include "rffi/api/network.h"
#include "rffi/api/sdp_observer_intf.h"
#include "rffi/api/stats_observer_intf.h"

/**
 * Rust friendly wrapper around some webrtc::PeerConnectionInterface
 * methods
 */

RUSTEXPORT void
Rust_createOffer(webrtc::PeerConnectionInterface*                    peer_connection,
                 webrtc::rffi::CreateSessionDescriptionObserverRffi* csd_observer);

RUSTEXPORT void
Rust_setLocalDescription(webrtc::PeerConnectionInterface*                 peer_connection,
                         webrtc::rffi::SetSessionDescriptionObserverRffi* ssd_observer,
                         webrtc::SessionDescriptionInterface*             local_description);

RUSTEXPORT const char*
Rust_toSdp(webrtc::SessionDescriptionInterface* session_description);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_answerFromSdp(const char* sdp);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_offerFromSdp(const char* sdp);

RUSTEXPORT bool
Rust_disableDtlsAndSetSrtpKey(webrtc::SessionDescriptionInterface* session_description,
                              int                                  crypto_suite,
                              const char*                          key_ptr,
                              size_t                               key_len,
                              const char*                          salt_ptr,
                              size_t                               salt_len);

enum RffiVideoCodecType {
    kRffiVideoCodecVp8 = 8,
    kRffiVideoCodecH264ConstrainedHigh = 46,
    kRffiVideoCodecH264ConstrainedBaseline = 40,
};

typedef struct {
  RffiVideoCodecType type;
  uint32_t level;
} RffiVideoCodec;

class ConnectionParametersV4 {
 public:
  std::string ice_ufrag;
  std::string ice_pwd;
  std::vector<RffiVideoCodec> receive_video_codecs;
};

typedef struct {
  // These all just refer to the storage
  const char* ice_ufrag;
  const char* ice_pwd;
  const RffiVideoCodec* receive_video_codecs;
  size_t receive_video_codecs_size;

  // When this is released, we must release the storage
  ConnectionParametersV4* backing;
} RffiConnectionParametersV4;

// Must call Rust_releaseV4 once finished with the result
RUSTEXPORT RffiConnectionParametersV4*
Rust_sessionDescriptionToV4(const webrtc::SessionDescriptionInterface* session_description);

RUSTEXPORT void
Rust_releaseV4(RffiConnectionParametersV4* v4);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_sessionDescriptionFromV4(bool offer, const RffiConnectionParametersV4* v4);

RUSTEXPORT void
Rust_createAnswer(webrtc::PeerConnectionInterface*                    peer_connection,
                  webrtc::rffi::CreateSessionDescriptionObserverRffi* csd_observer);

RUSTEXPORT void
Rust_setRemoteDescription(webrtc::PeerConnectionInterface*                 peer_connection,
                          webrtc::rffi::SetSessionDescriptionObserverRffi* ssd_observer,
                          webrtc::SessionDescriptionInterface*             remote_description);

RUSTEXPORT void
Rust_setOutgoingMediaEnabled(webrtc::PeerConnectionInterface* peer_connection,
                             bool                             enabled);

RUSTEXPORT bool
Rust_setIncomingMediaEnabled(webrtc::PeerConnectionInterface* peer_connection,
                             bool                             enabled);

/*
 * NOTE: The object created with Rust_createSignalingDataChannel() must be
 * freed using Rust_releaseRef().
 */
RUSTEXPORT webrtc::DataChannelInterface*
Rust_createSignalingDataChannel(webrtc::PeerConnectionInterface* peer_connection,
                                webrtc::PeerConnectionObserver* pc_observer);

RUSTEXPORT void
Rust_releaseRef(rtc::RefCountInterface* ref_counted_ptr);

RUSTEXPORT void
Rust_addRef(rtc::RefCountInterface* ref_counted_ptr);

RUSTEXPORT bool
Rust_addIceCandidateFromSdp(webrtc::PeerConnectionInterface* peer_connection,
                            const char*                      sdp);

RUSTEXPORT bool
Rust_addIceCandidateFromServer(webrtc::PeerConnectionInterface* peer_connection,
                               webrtc::rffi::Ip,
                               uint16_t port,
                               bool tcp);

RUSTEXPORT webrtc::IceGathererInterface*
Rust_createSharedIceGatherer(webrtc::PeerConnectionInterface* peer_connection);

RUSTEXPORT bool
Rust_useSharedIceGatherer(webrtc::PeerConnectionInterface* peer_connection,
                          webrtc::IceGathererInterface* ice_gatherer);

RUSTEXPORT void
Rust_getStats(webrtc::PeerConnectionInterface* peer_connection,
              webrtc::rffi::StatsObserverRffi* stats_observer);

RUSTEXPORT void
Rust_setSendBitrates(webrtc::PeerConnectionInterface* peer_connection,
                     int32_t                          min_bitrate_bps,
                     int32_t                          start_bitrate_bps,
                     int32_t                          max_bitrate_bps);

RUSTEXPORT bool
Rust_sendRtp(webrtc::PeerConnectionInterface* peer_connection,
             uint8_t pt,
             uint16_t seqnum,
             uint32_t timestamp,
             uint32_t ssrc,
             const uint8_t* payload_data,
             size_t payload_size);

RUSTEXPORT bool
Rust_receiveRtp(webrtc::PeerConnectionInterface* peer_connection, uint8_t pt);

RUSTEXPORT void
Rust_configureAudioEncoders(webrtc::PeerConnectionInterface* peer_connection, const webrtc::AudioEncoder::Config* config);

#endif /* RFFI_API_PEER_CONNECTION_INTF_H__ */
