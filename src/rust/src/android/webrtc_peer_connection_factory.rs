//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Re-exports WebRTC JNI interfaces

use jni::{
    objects::{JClass, JObject},
    sys::jlong,
    JNIEnv,
};

use crate::{webrtc, webrtc::peer_connection::RffiPeerConnection};

extern "C" {
    /// Export the nativeCreatePeerConnection() call from the
    /// org.webrtc.PeerConnectionFactory class.
    pub fn Java_org_webrtc_PeerConnectionFactory_nativeCreatePeerConnection(
        env: JNIEnv,
        class: JClass,
        factory: jlong,
        rtcConfig: JObject,
        constraints: JObject,
        nativeObserver: jlong,
        sslCertificateVerifier: JObject,
    ) -> jlong;
}

// Get the native PeerConnection inside of the Java wrapper.
extern "C" {
    pub fn Rust_borrowPeerConnectionFromJniOwnedPeerConnection(
        jni_owned_pc: i64,
    ) -> webrtc::ptr::BorrowedRc<RffiPeerConnection>;
}
