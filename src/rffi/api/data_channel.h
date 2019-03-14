/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_DATA_CHANNEL_H__
#define RFFI_API_DATA_CHANNEL_H__


#include "rffi/api/rffi_defs.h"

// C version of: https://www.w3.org/TR/webrtc/#idl-def-rtcdatachannelinit
//
// Below taken from "struct DataChannelInit" in
// src/api/data_channel_interface.h
//
// Note: the std::string |protocol| field is the only reason we can't
// use the native DataChannelInit directly.

typedef struct {

  // Deprecated. Reliability is assumed, and channel will be unreliable if
  // maxRetransmitTime or MaxRetransmits is set.
  bool reliable;

  // True if ordered delivery is required.
  bool ordered;

  // The max period of time in milliseconds in which retransmissions will be
  // sent. After this time, no more retransmissions will be sent. -1 if unset.
  //
  // Cannot be set along with |maxRetransmits|.
  int maxRetransmitTime;

  // The max number of retransmissions. -1 if unset.
  //
  // Cannot be set along with |maxRetransmitTime|.
  int maxRetransmits;

  // This is set by the application and opaque to the WebRTC implementation.
  const char* protocol;

  // True if the channel has been externally negotiated and we do not send an
  // in-band signalling in the form of an "open" message. If this is true, |id|
  // below must be set; otherwise it should be unset and will be negotiated
  // in-band.
  bool negotiated;

  // The stream id, or SID, for SCTP data channels. -1 if unset (see above).
  int id;
} RffiDataChannelInit;


#endif /* RFFI_API_DATA_CHANNEL_H__ */
