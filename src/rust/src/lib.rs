//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! # RingRTC -- A Rust WebRTC Interface
//!
//! This crate provides a [WebRTC](https://webrtc.org/) peer
//! connection calling interface using the [Signal
//! Protocol](https://en.wikipedia.org/wiki/Signal_Protocol) for the
//! call signaling transport.
//!

#[macro_use]
extern crate failure;

#[macro_use]
extern crate futures;

#[macro_use]
extern crate log;

#[cfg(feature = "sim")]
extern crate simplelog;

#[macro_use]
pub mod common;

mod error;

/// Core, platform independent functionality.
pub mod core {
    pub mod call;
    pub mod call_fsm;
    pub mod call_manager;
    pub mod call_mutex;
    pub mod connection;
    pub mod connection_fsm;
    pub mod platform;
    pub mod signaling;
    pub mod util;
}

/// Protobuf Definitions.
mod protobuf {
    pub mod data_channel;
    pub mod signaling;
}

#[cfg(target_os = "android")]
/// Android specific implementation.
mod android {
    extern crate jni;
    #[allow(clippy::missing_safety_doc)]
    mod api {
        mod jni_call_manager;
    }
    mod android_platform;
    mod call_manager;
    mod error;
    mod jni_util;
    mod logging;
    mod webrtc_java_media_stream;
    mod webrtc_peer_connection_factory;
}

#[cfg(target_os = "ios")]
/// iOS specific implementation.
mod ios {
    mod api {
        pub mod call_manager_interface;
    }
    mod call_manager;
    mod error;
    mod ios_media_stream;
    mod ios_platform;
    mod ios_util;
    mod logging;
}

#[cfg(feature = "electron")]
pub mod electron;

#[cfg(feature = "native")]
pub mod native;

/// Foreign Function Interface (FFI) to WebRTC C++ library.
pub mod webrtc {
    pub mod data_channel;
    pub mod data_channel_observer;
    pub mod ice_gatherer;
    #[cfg(feature = "simnet")]
    pub mod injectable_network;
    #[cfg(feature = "native")]
    pub mod logging;
    pub mod media;
    pub mod peer_connection;
    #[cfg(any(feature = "native", feature = "sim"))]
    pub mod peer_connection_factory;
    pub mod peer_connection_observer;
    pub mod sdp_observer;
    pub mod stats_observer;
    #[cfg(not(feature = "sim"))]
    mod ffi {
        pub mod data_channel;
        pub mod data_channel_observer;
        pub mod ice_gatherer;
        pub mod logging;
        pub mod media;
        pub mod peer_connection;
        #[cfg(feature = "native")]
        pub mod peer_connection_factory;
        pub mod peer_connection_observer;
        pub mod ref_count;
        pub mod sdp_observer;
        pub mod stats_observer;
    }
    #[cfg(feature = "sim")]
    pub mod sim {
        pub mod data_channel;
        pub mod data_channel_observer;
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
    pub mod actor;
    pub mod router;
}
