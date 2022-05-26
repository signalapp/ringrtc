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

// TODO: Consider removing all these duplicative declarations.
// It compiles without it.

/**
 * Rust friendly wrapper around some webrtc::PeerConnectionInterface
 * methods
 */

// Borrows the observer until the result is given to the observer,
// so the observer must stay alive until it's given a result.
RUSTEXPORT void
Rust_createOffer(webrtc::PeerConnectionInterface*                    peer_connection_borrowed_rc,
                 webrtc::rffi::CreateSessionDescriptionObserverRffi* csd_observer_borrowed_rc);


// Borrows the observer until the result is given to the observer,
// so the observer must stay alive until it's given a result.
RUSTEXPORT void
Rust_setLocalDescription(webrtc::PeerConnectionInterface*                 peer_connection_borrowed_rc,
                         webrtc::rffi::SetSessionDescriptionObserverRffi* ssd_observer_borrowed_rc,
                         webrtc::SessionDescriptionInterface*             local_description_owned);

// Returns an owned pointer.
RUSTEXPORT const char*
Rust_toSdp(webrtc::SessionDescriptionInterface* session_description_borrowed);

// Returns an owned pointer.
RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_answerFromSdp(const char* sdp_borrowed);

// Returns an owned pointer.
RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_offerFromSdp(const char* sdp_borrowed);

RUSTEXPORT bool
Rust_disableDtlsAndSetSrtpKey(webrtc::SessionDescriptionInterface* session_description_borrowed,
                              int                                  crypto_suite,
                              const char*                          key_borrowed,
                              size_t                               key_len,
                              const char*                          salt_borrowed,
                              size_t                               salt_len);

enum RffiVideoCodecType {
    kRffiVideoCodecVp8 = 8,
    kRffiVideoCodecVp9 = 9,
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
  const char* ice_ufrag_borrowed;
  const char* ice_pwd_borrowed;
  RffiVideoCodec* receive_video_codecs_borrowed;
  size_t receive_video_codecs_size;

  // When this is released, we must release the storage
  ConnectionParametersV4* backing_owned;
} RffiConnectionParametersV4;

typedef struct {
  int suite;
  const char* key_borrowed;
  size_t key_len;
  const char* salt_borrowed;
  size_t salt_len;
} RffiSrtpKey;

// Returns an owned pointer.
RUSTEXPORT RffiConnectionParametersV4*
Rust_sessionDescriptionToV4(const webrtc::SessionDescriptionInterface* session_description_borrowed);

RUSTEXPORT void
Rust_deleteV4(RffiConnectionParametersV4* v4_owned);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_sessionDescriptionFromV4(bool offer, const RffiConnectionParametersV4* v4_borrowed);

RUSTEXPORT void
Rust_createAnswer(webrtc::PeerConnectionInterface*                    peer_connection_borrowed_rc,
                  webrtc::rffi::CreateSessionDescriptionObserverRffi* csd_observer_borrowed_rc);

RUSTEXPORT void
Rust_setRemoteDescription(webrtc::PeerConnectionInterface*                 peer_connection_borrowed_rc,
                          webrtc::rffi::SetSessionDescriptionObserverRffi* ssd_observer_borrowed_rc,
                          webrtc::SessionDescriptionInterface*             remote_description_owned);

RUSTEXPORT void
Rust_setOutgoingMediaEnabled(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                             bool                             enabled);

RUSTEXPORT bool
Rust_setIncomingMediaEnabled(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                             bool                             enabled);

RUSTEXPORT void
Rust_setAudioPlayoutEnabled(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                            bool                             enabled);

RUSTEXPORT void
Rust_setAudioRecordingEnabled(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                              bool                             enabled);

RUSTEXPORT bool
Rust_addIceCandidateFromSdp(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                            const char*                      sdp);

RUSTEXPORT bool
Rust_addIceCandidateFromServer(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                               webrtc::rffi::Ip,
                               uint16_t port,
                               bool tcp);

RUSTEXPORT bool
Rust_removeIceCandidates(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                         webrtc::rffi::IpPort* removed_addresses_borrowed,
                         size_t length);

RUSTEXPORT webrtc::IceGathererInterface*
Rust_createSharedIceGatherer(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc);

RUSTEXPORT bool
Rust_useSharedIceGatherer(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                          webrtc::IceGathererInterface* ice_gatherer_borrowed_rc);

RUSTEXPORT void
Rust_getStats(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
              webrtc::rffi::StatsObserverRffi* stats_observer_borrowed_rc);

RUSTEXPORT void
Rust_setSendBitrates(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                     int32_t                          min_bitrate_bps,
                     int32_t                          start_bitrate_bps,
                     int32_t                          max_bitrate_bps);

RUSTEXPORT bool
Rust_sendRtp(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
             uint8_t pt,
             uint16_t seqnum,
             uint32_t timestamp,
             uint32_t ssrc,
             const uint8_t* payload_data_borrowed,
             size_t payload_size);

RUSTEXPORT bool
Rust_receiveRtp(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc, uint8_t pt);

RUSTEXPORT void
Rust_configureAudioEncoders(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc, const webrtc::AudioEncoder::Config* config_borrowed);

RUSTEXPORT void
Rust_getAudioLevels(webrtc::PeerConnectionInterface* peer_connection_borrowed_rc,
                    cricket::AudioLevel* captured_out,
                    cricket::ReceivedAudioLevel* received_out,
                    size_t received_out_size,
                    size_t* received_size_out);

#endif /* RFFI_API_PEER_CONNECTION_INTF_H__ */
