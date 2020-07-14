/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_PEER_CONNECTION_INTERFACE_INTF_H__
#define RFFI_API_PEER_CONNECTION_INTERFACE_INTF_H__

#include "api/peer_connection_interface.h"
#include "rffi/api/data_channel.h"
#include "rffi/api/sdp_observer_intf.h"
#include "rffi/api/stats_observer_intf.h"

/**
 * Rust friendly wrapper around some webrtc::PeerConnectionInterface
 * methods
 *
 */

RUSTEXPORT void
Rust_createOffer(webrtc::PeerConnectionInterface*                    pc_interface,
                 webrtc::rffi::CreateSessionDescriptionObserverRffi* csd_observer);

RUSTEXPORT void
Rust_setLocalDescription(webrtc::PeerConnectionInterface*                 pc_interface,
                         webrtc::rffi::SetSessionDescriptionObserverRffi* ssd_observer,
                         webrtc::SessionDescriptionInterface*             description);

RUSTEXPORT const char*
Rust_toSdp(webrtc::SessionDescriptionInterface* sdi);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_answerFromSdp(const char* sdp);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_offerFromSdp(const char* sdp);

RUSTEXPORT bool
Rust_replaceRtpDataChannelsWithSctp(webrtc::SessionDescriptionInterface* sdi);

RUSTEXPORT void
Rust_createAnswer(webrtc::PeerConnectionInterface*                    pc_interface,
                  webrtc::rffi::CreateSessionDescriptionObserverRffi* csd_observer);

RUSTEXPORT void
Rust_setRemoteDescription(webrtc::PeerConnectionInterface*                 pc_interface,
                          webrtc::rffi::SetSessionDescriptionObserverRffi* ssd_observer,
                          webrtc::SessionDescriptionInterface*             description);

RUSTEXPORT void
Rust_setOutgoingAudioEnabled(webrtc::PeerConnectionInterface* pc_interface,
                             bool                             enabled);

RUSTEXPORT bool
Rust_setIncomingRtpEnabled(webrtc::PeerConnectionInterface* pc_interface,
                           bool                             enabled);

/*
 * NOTE: The object created with Rust_createDataChannel() must be
 * freed using Rust_releaseRef().
 */
RUSTEXPORT webrtc::DataChannelInterface*
Rust_createDataChannel(webrtc::PeerConnectionInterface* pc_interface,
                       const char*                      label,
                       const RffiDataChannelInit*       config);

RUSTEXPORT void
Rust_releaseRef(rtc::RefCountInterface* ref_counted_ptr);

RUSTEXPORT void
Rust_addRef(rtc::RefCountInterface* ref_counted_ptr);

RUSTEXPORT bool
Rust_addIceCandidate(webrtc::PeerConnectionInterface* pc_interface,
                     const char*                      sdp);

RUSTEXPORT webrtc::IceGathererInterface*
Rust_createSharedIceGatherer(webrtc::PeerConnectionInterface* pc_interface);

RUSTEXPORT bool
Rust_useSharedIceGatherer(webrtc::PeerConnectionInterface* pc_interface,
                          webrtc::IceGathererInterface* ice_gatherer);

RUSTEXPORT void
Rust_getStats(webrtc::PeerConnectionInterface* pc_interface,
              webrtc::rffi::StatsObserverRffi* stats_observer);

RUSTEXPORT void
Rust_setMaxSendBitrate(webrtc::PeerConnectionInterface* pc_interface,
                       int32_t                          max_bitrate_bps);

#endif /* RFFI_API_PEER_CONNECTION_INTERFACE_INTF_H__ */
