/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_API_PEER_CONNECTION_INTF_H__
#define RFFI_API_PEER_CONNECTION_INTF_H__

#include "rffi/api/network.h"
#include "rffi/api/rtp_observer_intf.h"
#include "rffi/api/sdp_observer_intf.h"
#include "rffi/api/stats_observer_intf.h"

// These should stay in sync with android/iOS-specific ringrtc code
constexpr char kAudioTrackId[] = "audio1";
constexpr char kVideoTrackId[] = "video1";

namespace webrtc {
class IceGathererInterface;
class PeerConnectionInterface;

namespace rffi {
class ConnectionParametersV4;

RUSTEXPORT bool Rust_setScalabilityMode(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<const char> scalability_mode,
    int max_bitrate_bps);

RUSTEXPORT bool Rust_updateTransceivers(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<const uint32_t> remote_demux_ids_data_borrowed,
    size_t length);

/**
 * Rust friendly wrapper around some webrtc::PeerConnectionInterface
 * methods
 */

// Borrows the observer until the result is given to the observer,
// so the observer must stay alive until it's given a result.
RUSTEXPORT void Rust_createOffer(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::BorrowedRc<webrtc::rffi::CreateSessionDescriptionObserverRffi>
        csd_observer_borrowed_rc);

// Add a send-only transceiver for the send stream.
RUSTEXPORT bool Rust_createSendOnlyTransceiver(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc);

// Borrows the observer until the result is given to the observer,
// so the observer must stay alive until it's given a result.
RUSTEXPORT void Rust_setLocalDescription(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::BorrowedRc<webrtc::rffi::SetSessionDescriptionObserverRffi>
        ssd_observer_borrowed_rc,
    ptr::Owned<webrtc::SessionDescriptionInterface> local_description_owned);

RUSTEXPORT ptr::Owned<const char> Rust_toSdp(
    ptr::Borrowed<webrtc::SessionDescriptionInterface>
        session_description_borrowed);

RUSTEXPORT bool Rust_disableDtlsAndSetSrtpKey(
    ptr::Borrowed<webrtc::SessionDescriptionInterface>
        session_description_borrowed,
    int crypto_suite,
    ptr::Borrowed<const char> key_borrowed,
    size_t key_len,
    ptr::Borrowed<const char> salt_borrowed,
    size_t salt_len);

enum RffiVideoCodecType {
  kRffiVideoCodecVp8 = 8,
  kRffiVideoCodecVp9 = 9,
};

typedef struct {
  RffiVideoCodecType type;
} RffiVideoCodec;

typedef struct {
  // These all just refer to the storage
  ptr::Borrowed<const char> ice_ufrag_borrowed;
  ptr::Borrowed<const char> ice_pwd_borrowed;
  ptr::Borrowed<RffiVideoCodec> encode_only_video_codecs_borrowed;
  size_t encode_only_video_codecs_size;
  ptr::Borrowed<RffiVideoCodec> decode_only_video_codecs_borrowed;
  size_t decode_only_video_codecs_size;

  // When this is released, we must release the storage
  ptr::Owned<webrtc::rffi::ConnectionParametersV4> backing_owned;
} RffiConnectionParametersV4;

typedef struct {
  int suite;
  ptr::Borrowed<const char> key_borrowed;
  size_t key_len;
  ptr::Borrowed<const char> salt_borrowed;
  size_t salt_len;
} RffiSrtpKey;

RUSTEXPORT ptr::Owned<RffiConnectionParametersV4> Rust_sessionDescriptionToV4(
    ptr::Borrowed<const webrtc::SessionDescriptionInterface>
        session_description_borrowed,
    bool enable_vp9_encode,
    bool enable_vp9_decode);

RUSTEXPORT void Rust_deleteV4(ptr::Owned<RffiConnectionParametersV4> v4_owned);

RUSTEXPORT ptr::Owned<webrtc::SessionDescriptionInterface>
Rust_sessionDescriptionFromV4(
    bool offer,
    ptr::Borrowed<const RffiConnectionParametersV4> v4_borrowed,
    bool enable_tcc_audio,
    bool enable_vp9_encode,
    bool enable_vp9_decode,
    bool v4_is_local);

RUSTEXPORT ptr::Owned<webrtc::SessionDescriptionInterface>
Rust_localDescriptionForGroupCall(
    ptr::Borrowed<const char> ice_ufrag_borrowed,
    ptr::Borrowed<const char> ice_pwd_borrowed,
    RffiSrtpKey server_srtp_key,
    uint32_t local_demux_id,
    ptr::Borrowed<const uint32_t> remote_demux_ids_borrowed,
    size_t remote_demux_ids_len,
    ptr::Borrowed<const uint32_t> remote_demux_ids_require_svc_borrowed,
    size_t remote_demux_ids_needs_svc_len,
    bool enable_vp9);

RUSTEXPORT ptr::Owned<webrtc::SessionDescriptionInterface>
Rust_remoteDescriptionForGroupCall(
    ptr::Borrowed<const char> ice_ufrag_borrowed,
    ptr::Borrowed<const char> ice_pwd_borrowed,
    RffiSrtpKey server_srtp_key,
    uint32_t local_demux_id,
    ptr::Borrowed<const uint32_t> remote_demux_ids_borrowed,
    size_t remote_demux_ids_len,
    ptr::Borrowed<const uint32_t> remote_demux_ids_require_svc_borrowed,
    size_t remote_demux_ids_needs_svc_len,
    bool enable_vp9);

RUSTEXPORT void Rust_createAnswer(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::BorrowedRc<webrtc::rffi::CreateSessionDescriptionObserverRffi>
        csd_observer_borrowed_rc);

RUSTEXPORT void Rust_setRemoteDescription(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::BorrowedRc<webrtc::rffi::SetSessionDescriptionObserverRffi>
        ssd_observer_borrowed_rc,
    ptr::Owned<webrtc::SessionDescriptionInterface> remote_description_owned);

RUSTEXPORT void Rust_deleteSessionDescription(
    ptr::Owned<webrtc::SessionDescriptionInterface> description_owned);

RUSTEXPORT void Rust_setOutgoingMediaEnabled(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    bool enabled);

RUSTEXPORT bool Rust_setIncomingMediaEnabled(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    bool enabled);

RUSTEXPORT void Rust_setAudioPlayoutEnabled(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    bool enabled);

RUSTEXPORT void Rust_setAudioRecordingEnabled(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    bool enabled);

RUSTEXPORT bool Rust_addIceCandidateFromSdp(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<const char> sdp);

RUSTEXPORT bool Rust_addIceCandidateFromServer(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    webrtc::rffi::Ip,
    uint16_t port,
    bool tcp,
    ptr::Borrowed<const char> hostname);

RUSTEXPORT bool Rust_removeIceCandidates(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<webrtc::rffi::IpPort> removed_addresses_borrowed,
    size_t length,
    bool group,
    bool tcp,
    ptr::Borrowed<const char> hostname);

RUSTEXPORT ptr::OwnedRc<webrtc::IceGathererInterface>
Rust_createSharedIceGatherer(ptr::BorrowedRc<webrtc::PeerConnectionInterface>
                                 peer_connection_borrowed_rc);

RUSTEXPORT bool Rust_useSharedIceGatherer(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::BorrowedRc<webrtc::IceGathererInterface> ice_gatherer_borrowed_rc);

RUSTEXPORT void Rust_getStats(ptr::BorrowedRc<webrtc::PeerConnectionInterface>
                                  peer_connection_borrowed_rc,
                              ptr::BorrowedRc<webrtc::rffi::StatsObserverRffi>
                                  stats_observer_borrowed_rc);

RUSTEXPORT void Rust_setSendBitrates(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    int32_t min_bitrate_bps,
    int32_t start_bitrate_bps,
    int32_t max_bitrate_bps);

RUSTEXPORT bool Rust_sendRtp(ptr::BorrowedRc<webrtc::PeerConnectionInterface>
                                 peer_connection_borrowed_rc,
                             uint8_t pt,
                             uint16_t seqnum,
                             uint32_t timestamp,
                             uint32_t ssrc,
                             ptr::Borrowed<const uint8_t> payload_data_borrowed,
                             size_t payload_size);

RUSTEXPORT bool Rust_receiveRtp(ptr::BorrowedRc<webrtc::PeerConnectionInterface>
                                    peer_connection_borrowed_rc,
                                uint8_t pt,
                                bool enable_incoming);

RUSTEXPORT void Rust_configureAudioEncoders(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<const webrtc::AudioEncoderConfig> config_borrowed);

RUSTEXPORT void Rust_configureAudioDecoders(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<const webrtc::AudioDecoderConfig> config_borrowed);

RUSTEXPORT void Rust_getAudioLevels(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<uint16_t> captured_out,
    ptr::Borrowed<webrtc::ReceivedAudioLevel> received_out,
    size_t received_out_size,
    ptr::Borrowed<size_t> received_size_out);

RUSTEXPORT uint32_t Rust_getLastBandwidthEstimateBps(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc);

RUSTEXPORT void Rust_setRtpPacketObserver(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc,
    ptr::Borrowed<webrtc::rffi::RtpObserverRffi> rtp_observer_borrowed);

RUSTEXPORT void Rust_closePeerConnection(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc);

RUSTEXPORT void Rust_regatherOnAllNetworks(
    ptr::BorrowedRc<webrtc::PeerConnectionInterface>
        peer_connection_borrowed_rc);

}  // namespace rffi

}  // namespace webrtc

#endif /* RFFI_API_PEER_CONNECTION_INTF_H__ */
