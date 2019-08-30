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

mod common;
mod error;

/// Core, platform independent functionality.
mod core {
    pub mod call_connection;
    pub mod call_connection_factory;
    pub mod call_connection_observer;
    pub mod call_fsm;
    pub mod util;
}

/// Protobuf Definitions.
mod protobuf {
    pub mod data_channel;
}

#[cfg(target_os="android")]
/// Android specific implementation.
mod android {
    extern crate jni;
    mod api {
        mod jni_call_connection;
        mod jni_call_connection_factory;
    }
    mod call_connection;
    mod call_connection_factory;
    mod call_connection_observer;
    mod error;
    mod jni_util;
    mod logging;
    mod webrtc_java_media_stream;
    mod webrtc_peer_connection_factory;
}

#[cfg(target_os="ios")]
/// iOS specific implementation.
mod ios {
    mod api {
        mod objc_call_connection;
        mod objc_call_connection_factory;
        mod objc_call_connection_global;
    }
    mod call_connection;
    mod call_connection_factory;
    mod call_connection_observer;
    mod error;
    mod ios_util;
    mod logging;
    mod webrtc_app_media_stream;
    mod webrtc_app_peer_connection;
}

/// Foreign Function Interface (FFI) to WebRTC C++ library.
mod webrtc {
    pub mod data_channel;
    pub mod data_channel_observer;
    pub mod ice_candidate;
    pub mod media_stream;
    pub mod peer_connection;
    pub mod peer_connection_observer;
    pub mod ref_count;
    pub mod sdp_observer;
}


#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
