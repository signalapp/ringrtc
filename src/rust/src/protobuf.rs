//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

#![allow(clippy::derive_partial_eq_without_eq)]

pub mod group_call {
    include!(concat!(env!("OUT_DIR"), "/group_call.rs"));
}

pub mod rtp_data {
    include!(concat!(env!("OUT_DIR"), "/rtp_data.rs"));
}

pub mod signaling {
    include!(concat!(env!("OUT_DIR"), "/signaling.rs"));
}
