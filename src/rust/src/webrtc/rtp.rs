//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

pub type PayloadType = u8;
pub type Ssrc = u32;
pub type SequenceNumber = u16;
pub type Timestamp = u32;

#[derive(Clone, Debug)]
pub struct Header {
    pub pt:        PayloadType,
    pub seqnum:    SequenceNumber,
    pub timestamp: Timestamp,
    pub ssrc:      Ssrc,
}
