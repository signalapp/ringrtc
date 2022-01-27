/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "api/video/i420_buffer.h"
#include "rtc_base/ref_counted_object.h"
#include "rffi/api/media.h"
#include "rffi/src/ptr.h"
#include "rtc_base/logging.h"
#include "rtc_base/time_utils.h"
#include "third_party/libyuv/include/libyuv/convert.h"
#include "third_party/libyuv/include/libyuv/convert_argb.h"
#include "third_party/libyuv/include/libyuv/convert_from.h"
namespace webrtc {
namespace rffi {

VideoSource::VideoSource() : VideoTrackSource(false /* remote */) {
   SetState(kLive);
}

VideoSource::~VideoSource() {
}

void VideoSource::PushVideoFrame(const webrtc::VideoFrame& frame) {
  broadcaster_.OnFrame(frame);
}

// Returns 0 upon failure
RUSTEXPORT uint32_t Rust_getTrackIdAsUint32(webrtc::MediaStreamTrackInterface* track_borrowed_rc) {
  uint32_t id = 0;
  rtc::FromString(track_borrowed_rc->id(), &id);
  return id;
}

RUSTEXPORT void Rust_setAudioTrackEnabled(
    webrtc::AudioTrackInterface* track_borrowed_rc, bool enabled) {
  track_borrowed_rc->set_enabled(enabled);
}

RUSTEXPORT void Rust_setVideoTrackEnabled(
    webrtc::VideoTrackInterface* track_borrowed_rc, bool enabled) {
  track_borrowed_rc->set_enabled(enabled);
}

RUSTEXPORT void Rust_setVideoTrackContentHint(
    webrtc::VideoTrackInterface* track_borrowed_rc, bool is_screenshare) {
  track_borrowed_rc->set_content_hint(is_screenshare ? VideoTrackInterface::ContentHint::kText : VideoTrackInterface::ContentHint::kNone);
}

RUSTEXPORT void Rust_pushVideoFrame(
    webrtc::rffi::VideoSource* source_borrowed_rc,
    VideoFrameBuffer* buffer_borrowed_rc) {
  auto timestamp_us = rtc::TimeMicros();
  auto frame = webrtc::VideoFrame::Builder()
      .set_video_frame_buffer(inc_rc(buffer_borrowed_rc))
      .set_timestamp_us(timestamp_us)
      .build();
  source_borrowed_rc->PushVideoFrame(std::move(frame));
}

// Returns an owned RC.
RUSTEXPORT VideoFrameBuffer* Rust_copyVideoFrameBufferFromI420(
    uint32_t width, uint32_t height, uint8_t* src_borrowed) {
  int width_y = static_cast<int>(width);
  int height_y = static_cast<int>(height);
  int width_u = (width_y + 1) / 2;
  int height_u = (height_y + 1) / 2;

  int stride_y = width_y;
  int stride_u = width_u;
  int stride_v = width_u;

  int size_y = width_y * height_y;
  int size_u = width_u * height_u;

  uint8_t* src_y = src_borrowed;
  uint8_t* src_u = src_y + size_y;
  uint8_t* src_v = src_u + size_u;
  
  return take_rc(I420Buffer::Copy(width, height, src_y, stride_y, src_u, stride_u, src_v, stride_v));
}

// Returns an owned RC.
RUSTEXPORT VideoFrameBuffer* Rust_copyVideoFrameBufferFromNv12(
    uint32_t width, uint32_t height, uint8_t* src_borrowed) {
  int width_y = static_cast<int>(width);
  int height_y = static_cast<int>(height);
  int width_u = (width_y + 1) / 2;
  int width_v = width_u;

  int stride_y = width_y;
  int stride_uv = width_u + width_v;

  int size_y = width_y * height_y;

  uint8_t* src_y = src_borrowed;
  uint8_t* src_uv = src_y + size_y;

  auto dest = I420Buffer::Create(width, height);
  libyuv::NV12ToI420(
      src_y, stride_y,
      src_uv, stride_uv,
      dest->MutableDataY(), dest->StrideY(),
      dest->MutableDataU(), dest->StrideU(),
      dest->MutableDataV(), dest->StrideV(),
      width_y, height_y);
  return take_rc(dest);
}

// Returns an owned RC.
RUSTEXPORT VideoFrameBuffer* Rust_copyVideoFrameBufferFromRgba(
    uint32_t width, uint32_t height, uint8_t* rgba_borrowed) {
  auto i420 = I420Buffer::Create(width, height);
  int rgba_stride = 4 * width;
  libyuv::ABGRToI420(
      rgba_borrowed, rgba_stride,
      i420->MutableDataY(), i420->StrideY(),
      i420->MutableDataU(), i420->StrideU(),
      i420->MutableDataV(), i420->StrideV(),
      width, height);
  return take_rc(i420);
}

RUSTEXPORT void Rust_convertVideoFrameBufferToRgba(const VideoFrameBuffer* buffer_borrowed_rc, uint8_t* rgba_out) {
  const I420BufferInterface* i420 = buffer_borrowed_rc->GetI420();
  uint32_t rgba_stride = 4 * i420->width();
  libyuv::I420ToABGR(
      i420->DataY(), i420->StrideY(),
      i420->DataU(), i420->StrideU(),
      i420->DataV(), i420->StrideV(),
      rgba_out, rgba_stride,
      i420->width(), i420->height());
}

// Returns an owned RC.
RUSTEXPORT VideoFrameBuffer* Rust_copyAndRotateVideoFrameBuffer(
    const VideoFrameBuffer* buffer_borrowed_rc, VideoRotation rotation) {
  return take_rc(webrtc::I420Buffer::Rotate(*buffer_borrowed_rc->GetI420(), rotation));
}

} // namespace rffi
} // namespace webrtc
