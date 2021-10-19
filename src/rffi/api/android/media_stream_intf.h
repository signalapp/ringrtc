/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

/**
 * Rust friendly wrapper around webrtc::jni::JavaMediaStream object
 */

#ifndef ANDROID_MEDIA_STREAM_INTF_H__
#define ANDROID_MEDIA_STREAM_INTF_H__

#include "rffi/api/rffi_defs.h"
#include "sdk/android/src/jni/pc/media_stream.h"

// Create a JavaMediaStream C++ object from a
// webrtc::MediaStreamInterface* object.
// Returns an owned pointer.
RUSTEXPORT webrtc::jni::JavaMediaStream*
Rust_createJavaMediaStream(webrtc::MediaStreamInterface* media_stream_borrowed_rc);

// Delete a JavaMediaStream C++ object.
RUSTEXPORT void
Rust_deleteJavaMediaStream(webrtc::jni::JavaMediaStream* java_media_stream_owned);

// Return the Java JNI object contained within the JavaMediaStream C++
// object.
RUSTEXPORT jobject
Rust_getJavaMediaStreamObject(webrtc::jni::JavaMediaStream* java_media_stream_borrowed);

#endif /* ANDROID_MEDIA_STREAM_INTF_H__ */
