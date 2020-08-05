/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_PEER_CONNECTION_FACTORY_H__
#define RFFI_API_PEER_CONNECTION_FACTORY_H__

#include "rffi/api/peer_connection_interface_intf.h"
#include "rffi/api/injectable_network.h"
#include "rtc_base/ref_count.h"

namespace rtc {
  class RTCCertificite;
}

namespace webrtc {
  class PeerConnectionInterface;
  class PeerConnectionFactoryInterface;
  class AudioSourceInterface;
  class AudioTrackInterface;

  // This little indirection is needed so that we can have something
  // that owns the signaling thread (and other threads).
  // We could make our owner implement the PeerConnectionFactoryInterface,
  // but it's not worth the trouble.  This is easier.
  class PeerConnectionFactoryOwner : public rtc::RefCountInterface {
    public:
    virtual ~PeerConnectionFactoryOwner() {}
    virtual PeerConnectionFactoryInterface* peer_connection_factory() = 0;
    // If we are using an injectable network, this is it.
    virtual rffi::InjectableNetwork* injectable_network() = 0;
    virtual int16_t AudioPlayoutDevices() = 0;
    virtual int32_t AudioPlayoutDeviceName(uint16_t index, char *out_name, char *out_uuid) = 0;
    virtual bool SetAudioPlayoutDevice(uint16_t index) = 0;
    virtual int16_t AudioRecordingDevices() = 0;
    virtual int32_t AudioRecordingDeviceName(uint16_t index, char *out_name, char *out_uuid) = 0;
    virtual bool SetAudioRecordingDevice(uint16_t index) = 0;
  };
}

typedef struct {
  const char* username;
  const char* password;
  const char** urls;
  size_t urls_size;
} RffiIceServer;

// Technically creates a PeerConnectionFactoryOwner, but if you only use the
// functions below, that won't matter to you.
// You can create more than one, but you should probably only have one unless
// you want to test separate endpoints that are as independent as possible.
RUSTEXPORT webrtc::PeerConnectionFactoryOwner* Rust_createPeerConnectionFactory(bool use_injectable_network);
RUSTEXPORT webrtc::rffi::InjectableNetwork* Rust_getInjectableNetwork(
    webrtc::PeerConnectionFactoryOwner*);

// Creates a PeerConnection using a fairly small set of controls.  It assumes you
// want all the normal stuff like
RUSTEXPORT webrtc::PeerConnectionInterface* Rust_createPeerConnection(
  webrtc::PeerConnectionFactoryOwner*,
  webrtc::PeerConnectionObserver*,
  rtc::RTCCertificate* certificate,
  bool hide_ip,
  RffiIceServer ice_server,
  webrtc::AudioTrackInterface*,
  webrtc::VideoTrackSourceInterface*,
  bool enable_dtls,
  bool enable_rtp_data_channel);
RUSTEXPORT webrtc::AudioTrackInterface* Rust_createAudioTrack(webrtc::PeerConnectionFactoryOwner*);
RUSTEXPORT webrtc::VideoTrackSourceInterface* Rust_createVideoSource(webrtc::PeerConnectionFactoryOwner*);
RUSTEXPORT int16_t Rust_getAudioPlayoutDevices(webrtc::PeerConnectionFactoryOwner*);
RUSTEXPORT int32_t Rust_getAudioPlayoutDeviceName(webrtc::PeerConnectionFactoryOwner*, uint16_t index, char *out_name, char *out_uuid);
RUSTEXPORT bool Rust_setAudioPlayoutDevice(webrtc::PeerConnectionFactoryOwner*, uint16_t index);
RUSTEXPORT int16_t Rust_getAudioRecordingDevices(webrtc::PeerConnectionFactoryOwner*);
RUSTEXPORT int32_t Rust_getAudioRecordingDeviceName(webrtc::PeerConnectionFactoryOwner*, uint16_t index, char *out_name, char *out_uuid);
RUSTEXPORT bool Rust_setAudioRecordingDevice(webrtc::PeerConnectionFactoryOwner*, uint16_t index);
RUSTEXPORT rtc::RTCCertificate* Rust_generateCertificate();

#endif /* RFFI_API_PEER_CONNECTION_FACTORY_H__ */
