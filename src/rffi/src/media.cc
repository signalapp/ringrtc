/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "api/video/i420_buffer.h"
#include "rtc_base/ref_counted_object.h"
#include "rffi/api/media.h"
#include "rtc_base/logging.h"
#include "rtc_base/time_utils.h"
#include "third_party/libyuv/include/libyuv/convert.h"
#include "third_party/libyuv/include/libyuv/convert_argb.h"
#include "third_party/libyuv/include/libyuv/convert_from.h"
namespace webrtc {
namespace rffi {

VideoSink::VideoSink(const rust_object obj, VideoSinkCallbacks* cbs)
  : obj_(obj), cbs_(*cbs) {
}

VideoSink::~VideoSink() {
}

void VideoSink::OnFrame(const webrtc::VideoFrame& frame) {
  VideoFrameMetadata metadata = {};
  metadata.width = frame.width();
  metadata.height = frame.height();
  metadata.rotation = frame.rotation();
  // We can't keep a reference to the buffer around or it will slow down the video decoder.
  // This introduces a copy, but only in the case where we aren't rotated,
  // and it's a copy of i420 and not RGBA (i420 is smaller than RGBA).
  // TODO: Figure out if we can make the decoder have a larger frame output pool
  // so that we don't need to do this.
  auto* buffer = Rust_copyAndRotateVideoFrameBuffer(frame.video_frame_buffer().get(), frame.rotation());
  // If we rotated the frame, we need to update metadata as well
  if ((metadata.rotation == kVideoRotation_90) || (metadata.rotation == kVideoRotation_270)) {
    metadata.width = frame.height();
    metadata.height = frame.width();
  }
  metadata.rotation = kVideoRotation_0;
  cbs_.onVideoFrame(obj_, metadata, buffer);
}

VideoSource::VideoSource() : VideoTrackSource(false /* remote */) {
   SetState(kLive);
}

VideoSource::~VideoSource() {
}

void VideoSource::PushVideoFrame(const webrtc::VideoFrame& frame) {
  broadcaster_.OnFrame(frame);
}

RUSTEXPORT void Rust_setAudioTrackEnabled(
    webrtc::AudioTrackInterface* track, bool enabled) {
  track->set_enabled(enabled);
}

RUSTEXPORT VideoTrackInterface* Rust_getFirstVideoTrack(MediaStreamInterface* stream) {
  auto tracks = stream->GetVideoTracks();
  if (tracks.empty()) {
    return nullptr;
  }
  return tracks[0].release();
}

RUSTEXPORT void Rust_addVideoSink(
    webrtc::VideoTrackInterface* track,
    const rust_object obj,
    VideoSinkCallbacks* cbs) {
  auto sink = new rtc::RefCountedObject<VideoSink>(obj, cbs);
  sink->AddRef();

  rtc::VideoSinkWants wants;
  // Note: this causes frames to be dropped, not rotated.
  // So don't set it to true, even if it seems to make sense!
  wants.rotation_applied = false;

  track->AddOrUpdateSink(sink, wants);
}

RUSTEXPORT void Rust_pushVideoFrame(webrtc::rffi::VideoSource* source, VideoFrameBuffer* buffer) {
  // At some point we might care about capture timestamps, but for now
  // using the current time is sufficient.
  auto timestamp_us = rtc::TimeMicros();
  auto frame = webrtc::VideoFrame::Builder()
      .set_video_frame_buffer(std::move(buffer))
      .set_timestamp_us(timestamp_us)
      .build();
  source->PushVideoFrame(std::move(frame));
}

RUSTEXPORT VideoFrameBuffer* Rust_createVideoFrameBufferFromRgba(
    uint32_t width, uint32_t height, uint8_t* rgba_buffer) {
  auto i420 = I420Buffer::Create(width, height).release();
  int rgba_stride = 4 * width;
  libyuv::ABGRToI420(
      rgba_buffer, rgba_stride,
      i420->MutableDataY(), i420->StrideY(),
      i420->MutableDataU(), i420->StrideU(),
      i420->MutableDataV(), i420->StrideV(),
      width, height);
  return i420;
}

RUSTEXPORT void Rust_convertVideoFrameBufferToRgba(const VideoFrameBuffer* buffer, uint8_t* rgba_buffer) {
  const I420BufferInterface* i420 = buffer->GetI420();
  uint32_t rgba_stride = 4 * i420->width();
  libyuv::I420ToABGR(
      i420->DataY(), i420->StrideY(),
      i420->DataU(), i420->StrideU(),
      i420->DataV(), i420->StrideV(),
      rgba_buffer, rgba_stride,
      i420->width(), i420->height());
}

RUSTEXPORT VideoFrameBuffer* Rust_copyAndRotateVideoFrameBuffer(
    const VideoFrameBuffer* buffer, VideoRotation rotation) {
  return webrtc::I420Buffer::Rotate(*buffer->GetI420(), rotation).release();
}

} // namespace rffi
} // namespace webrtc
