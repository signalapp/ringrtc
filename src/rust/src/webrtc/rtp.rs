//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

pub type PayloadType = u8;
pub type Ssrc = u32;
pub type SequenceNumber = u16;
pub type Timestamp = u32;

#[derive(Clone, Debug)]
pub struct Header {
    pub pt: PayloadType,
    pub seqnum: SequenceNumber,
    pub timestamp: Timestamp,
    pub ssrc: Ssrc,
}
