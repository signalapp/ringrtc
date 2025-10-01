//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// TODO(mutexlox): Remove these after 2024 upgrade
#![warn(unsafe_attr_outside_unsafe)]
#![warn(unsafe_op_in_unsafe_fn)]
#![warn(missing_unsafe_on_extern)]
#![warn(rust_2024_incompatible_pat)]
#![warn(keyword_idents_2024)]

//! This crate is a wrapper around prost and tonic, removing the need for copies of protobuf files
//! and protobuf builds in build.rs. Note that dependent crates still need to add prost and tonic
//! to their dependencies.
//!
//! Just like prost/tonic, we expose a include_xyz_proto macros so protobuf types are local to a
//! crate. This allows crates to define traits on the types.
//!
//! We "curry" the protobuf code here. Since macros don't have eager evaluation, nested macros would
//! be evaluated at the wrong point in compilation,
//! i.e. `include!(concat!(env!("OUT_DIR"), "/group_call.rs"))` would look in the wrong OUT_DIR. So
//! we save the proto code here during this package's compilation and emit it using a proc macro.

use proc_macro::TokenStream;

#[cfg(feature = "signaling")]
const GROUP_PROTO: &str = include_str!(concat!(env!("OUT_DIR"), "/group_call.rs"));

#[cfg(feature = "signaling")]
const RTP_DATA_PROTO: &str = include_str!(concat!(env!("OUT_DIR"), "/rtp_data.rs"));

#[cfg(feature = "signaling")]
const SIGNALING_PROTO: &str = include_str!(concat!(env!("OUT_DIR"), "/signaling.rs"));

#[cfg(feature = "call_sim")]
const CALL_SIM_PROTO: &str = include_str!(concat!(env!("OUT_DIR"), "/calling.rs"));

#[cfg(feature = "signaling")]
#[proc_macro]
pub fn include_groupcall_proto(_input: TokenStream) -> TokenStream {
    GROUP_PROTO.parse().unwrap()
}

#[cfg(feature = "signaling")]
#[proc_macro]
pub fn include_rtp_proto(_input: TokenStream) -> TokenStream {
    RTP_DATA_PROTO.parse().unwrap()
}

#[cfg(feature = "signaling")]
#[proc_macro]
pub fn include_signaling_proto(_input: TokenStream) -> TokenStream {
    SIGNALING_PROTO.parse().unwrap()
}

#[cfg(feature = "call_sim")]
#[proc_macro]
pub fn include_call_sim_proto(_input: TokenStream) -> TokenStream {
    CALL_SIM_PROTO.parse().unwrap()
}
