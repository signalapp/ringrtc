/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

/**
 * Rust friendly wrapper around webrtc::jni::JavaMediaStream object
 *
 */

#ifndef ANDROID_MEDIA_STREAM_INTF_H__
#define ANDROID_MEDIA_STREAM_INTF_H__

#include "rffi/api/rffi_defs.h"
#include "sdk/android/src/jni/pc/media_stream.h"

// Create a JavaMediaStream C++ object from a
// webrtc::MediaStreamInterface* object.
RUSTEXPORT webrtc::jni::JavaMediaStream*
Rust_createJavaMediaStream(webrtc::MediaStreamInterface* media_stream);

// Free a JavaMediaStream C++ object.
RUSTEXPORT void
Rust_freeJavaMediaStream(webrtc::jni::JavaMediaStream* java_media_stream);

// Return the Java JNI object contained within the JavaMediaStream C++
// object.
RUSTEXPORT jobject
Rust_getObjectJavaMediaStream(webrtc::jni::JavaMediaStream* java_media_stream);

#endif /* ANDROID_MEDIA_STREAM_INTF_H__ */
