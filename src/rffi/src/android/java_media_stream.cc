/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

/*
 * Rust friendly wrapper around JavaMediaStream object
 */

#include "sdk/android/src/jni/pc/peer_connection.h"
#include "rffi/api/android/media_stream_intf.h"

#include <string>

namespace webrtc {
namespace rffi {

RUSTEXPORT webrtc::jni::JavaMediaStream*
Rust_createJavaMediaStream(MediaStreamInterface *stream) {

  rtc::scoped_refptr<MediaStreamInterface> media_stream(stream);
  JNIEnv* env = AttachCurrentThreadIfNeeded();

  // NOTE: JavaMediaStream() takes ownership of the MediaStream* ref counted pointer.
  jni::JavaMediaStream *java_media_stream = new jni::JavaMediaStream(env, media_stream);
  return java_media_stream;
}

RUSTEXPORT void
Rust_freeJavaMediaStream(webrtc::jni::JavaMediaStream *java_media_stream) {
  delete java_media_stream;
}

RUSTEXPORT jobject
Rust_getJavaMediaStreamObject(webrtc::jni::JavaMediaStream *java_media_stream) {
  return java_media_stream->j_media_stream().obj();
}

} // namespace rffi
} // namespace webrtc
