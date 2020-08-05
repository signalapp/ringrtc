/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_MEDIA_H__
#define RFFI_API_MEDIA_H__

#include "api/media_stream_interface.h"
#include "media/base/video_broadcaster.h"
#include "pc/video_track_source.h"
#include "rffi/api/rffi_defs.h"

typedef struct {
  uint32_t width;
  uint32_t height;
  webrtc::VideoRotation rotation;
} VideoFrameMetadata;

typedef struct {
  // Passes ownership of the buffer
  void (*onVideoFrame)(rust_object, VideoFrameMetadata, webrtc::VideoFrameBuffer*);
} VideoSinkCallbacks;

namespace webrtc {
namespace rffi {

// A simple implementation of a VideoSinkInterface which be used to attach to a incoming video
// track for rendering by calling Rust_addVideoSink.
class VideoSink : public rtc::VideoSinkInterface<webrtc::VideoFrame>, rtc::RefCountInterface {
 public:
  VideoSink(const rust_object obj, VideoSinkCallbacks* cbs);
  ~VideoSink() override;

  void OnFrame(const webrtc::VideoFrame& frame) override;

 private:
  const rust_object obj_;
  VideoSinkCallbacks cbs_;
};

// A simple implementation of a VideoTrackSource which can be used for pushing frames into
// an outgoing video track for encoding by calling Rust_pushVideoFrame.
class VideoSource : public VideoTrackSource {
 public:
  VideoSource();
  ~VideoSource() override;

  void PushVideoFrame(const webrtc::VideoFrame& frame);

 protected:
  rtc::VideoSourceInterface<webrtc::VideoFrame>* source() override {
    return &broadcaster_;
  }

 private:
  rtc::VideoBroadcaster broadcaster_;
};

} // namespace rffi
} // namespace webrtc

// Same as AudioTrackEnabled::set_enabled
RUSTEXPORT void Rust_setAudioTrackEnabled(webrtc::AudioTrackInterface*, bool);

// Gets the first video track from the stream, or nullptr if there is none.
RUSTEXPORT webrtc::VideoTrackInterface* Rust_getFistVideoTrack(
    webrtc::MediaStreamInterface*);

// Creates an VideoSink to the given track and attaches it to the track to
// get frames from C++ to Rust.
RUSTEXPORT void Rust_addVideoSink(
    webrtc::VideoTrackInterface*, const rust_object, VideoSinkCallbacks* cbs);

// Same as VideoSource::PushVideoFrame, to get frames from Rust to C++.
RUSTEXPORT void Rust_pushVideoFrame(webrtc::rffi::VideoSource*, webrtc::VideoFrameBuffer* buffer);

// RGBA => I420
RUSTEXPORT webrtc::VideoFrameBuffer* Rust_createVideoFrameBufferFromRgba(
  uint32_t width, uint32_t height, uint8_t* rgba_buffer);

// I420 => RGBA
RUSTEXPORT void Rust_convertVideoFrameBufferToRgba(
  const webrtc::VideoFrameBuffer* buffer, uint8_t* rgba_buffer);

// RGBA => I420
RUSTEXPORT webrtc::VideoFrameBuffer* Rust_copyAndRotateVideoFrameBuffer(
    const webrtc::VideoFrameBuffer* buffer, webrtc::VideoRotation rotation);


#endif /* RFFI_API_MEDIA_H__ */
