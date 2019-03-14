//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Re-exports WebRTC JNI interfaces

use jni::JNIEnv;
use jni::objects::{
    JObject,
    JClass,
};
use jni::sys::jlong;

extern {
    /// Export the nativeCreatepeerconnection() call from the
    /// org.webrtc.PeerConnectionFactory class.
    pub fn Java_org_webrtc_PeerConnectionFactory_nativeCreatePeerConnection(
        env:                    JNIEnv,
        class:                  JClass,
        factory:                jlong,
        rtcConfig:              JObject,
        constraints:            JObject,
        nativeObserver:         jlong,
        sslCertificateVerifier: JObject) -> jlong;
}
