//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

#![allow(clippy::derive_partial_eq_without_eq)]

pub mod group_call {
    call_protobuf::include_groupcall_proto!();
}

pub mod rtp_data {
    call_protobuf::include_rtp_proto!();
}

pub mod signaling {
    call_protobuf::include_signaling_proto!();
}
