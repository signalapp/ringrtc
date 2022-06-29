//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! # RingRTC -- A Rust WebRTC Interface
//!
//! This crate provides a [WebRTC](https://webrtc.org/) peer
//! connection calling interface using the [Signal
//! Protocol](https://en.wikipedia.org/wiki/Signal_Protocol) for the
//! call signaling transport.
//!

// This lint is under review, check in a future nightly update.
#![allow(clippy::significant_drop_in_scrutinee)]

#[macro_use]
extern crate futures;

#[macro_use]
extern crate log;

#[macro_use]
extern crate static_assertions;

#[macro_use]
pub mod common;

mod error;

// Doesn't depend on WebRTC
pub mod lite {
    pub mod ffi;
    pub mod http;
    pub mod logging;
    pub mod sfu;
}

/// Core, platform independent functionality.
pub mod core {
    pub mod bandwidth_mode;
    pub mod call;
    pub mod call_fsm;
    pub mod call_manager;
    pub mod call_mutex;
    pub mod connection;
    pub mod connection_fsm;
    pub mod crypto;
    pub mod group_call;
    pub mod platform;
    pub mod signaling;
    pub mod util;
}

/// Protobuf Definitions.
pub mod protobuf;

#[cfg(any(target_os = "android", feature = "check-all"))]
/// Android specific implementation.
mod android {
    #[macro_use]
    mod jni_util;

    #[allow(clippy::missing_safety_doc)]
    mod api {
        mod jni_call_manager;
    }
    mod android_platform;
    mod call_manager;
    mod error;
    mod logging;
    mod webrtc_java_media_stream;
    mod webrtc_peer_connection_factory;
}

#[cfg(any(target_os = "ios", feature = "check-all"))]
/// iOS specific implementation.
mod ios {
    mod api {
        pub mod call_manager_interface;
    }
    mod call_manager;
    mod error;
    mod ios_media_stream;
    mod ios_platform;
}

#[cfg(feature = "electron")]
pub mod electron;

#[cfg(feature = "native")]
pub mod native;

/// Foreign Function Interface (FFI) to WebRTC C++ library.
pub mod webrtc {
    pub mod arc;
    pub use arc::Arc;
    pub mod ice_gatherer;
    #[cfg(feature = "simnet")]
    pub mod injectable_network;
    #[cfg(feature = "native")]
    pub mod logging;
    pub mod media;
    pub mod network;
    pub mod peer_connection;
    pub mod peer_connection_factory;
    pub mod peer_connection_observer;
    pub mod ptr;
    pub use ptr::RefCounted;
    pub mod rtp;
    pub mod sdp_observer;
    pub mod stats_observer;
    #[cfg(not(feature = "sim"))]
    mod ffi {
        pub mod ice_gatherer;
        pub mod logging;
        pub mod media;
        pub mod peer_connection;
        pub mod peer_connection_factory;
        pub mod peer_connection_observer;
        pub mod ref_count;
        pub mod sdp_observer;
        pub mod stats_observer;
    }
    #[cfg(feature = "sim")]
    pub mod sim {
        pub mod ice_gatherer;
        pub mod media;
        pub mod peer_connection;
        pub mod peer_connection_factory;
        pub mod peer_connection_observer;
        pub mod ref_count;
        pub mod sdp_observer;
        pub mod stats_observer;
    }
}

#[cfg(feature = "sim")]
pub mod sim {
    pub mod error;
    pub mod sim_platform;
}

#[cfg(feature = "simnet")]
pub mod simnet {
    pub mod router;
}
