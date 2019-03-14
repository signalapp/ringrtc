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
Rust_getOfferDescription(webrtc::SessionDescriptionInterface* offer);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_createSessionDescriptionAnswer(const char* description);

RUSTEXPORT webrtc::SessionDescriptionInterface*
Rust_createSessionDescriptionOffer(const char* description);

RUSTEXPORT void
Rust_createAnswer(webrtc::PeerConnectionInterface*                    pc_interface,
                  webrtc::rffi::CreateSessionDescriptionObserverRffi* csd_observer);

RUSTEXPORT void
Rust_setRemoteDescription(webrtc::PeerConnectionInterface*                 pc_interface,
                          webrtc::rffi::SetSessionDescriptionObserverRffi* ssd_observer,
                          webrtc::SessionDescriptionInterface*             description);

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
                     const char*                      sdp_mid,
                     int32_t                          sdp_mline_index,
                     const char*                      sdp);

#endif /* RFFI_API_PEER_CONNECTION_INTERFACE_INTF_H__ */
