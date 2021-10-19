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

VideoSink::VideoSink(void* obj, VideoSinkCallbacks* cbs)
  : obj_(obj), cbs_(*cbs) {
}

VideoSink::~VideoSink() {
}

void VideoSink::OnFrame(const webrtc::VideoFrame& frame) {
  RffiVideoFrameMetadata metadata = {};
  metadata.width = frame.width();
  metadata.height = frame.height();
  metadata.rotation = frame.rotation();
  // We can't keep a reference to the buffer around or it will slow down the video decoder.
  // This introduces a copy, but only in the case where we aren't rotated,
  // and it's a copy of i420 and not RGBA (i420 is smaller than RGBA).
  // TODO: Figure out if we can make the decoder have a larger frame output pool
  // so that we don't need to do this.
  auto* buffer_owned_rc = Rust_copyAndRotateVideoFrameBuffer(frame.video_frame_buffer().get(), frame.rotation());
  // If we rotated the frame, we need to update metadata as well
  if ((metadata.rotation == kVideoRotation_90) || (metadata.rotation == kVideoRotation_270)) {
    metadata.width = frame.height();
    metadata.height = frame.width();
  }
  metadata.rotation = kVideoRotation_0;
  cbs_.onVideoFrame(obj_, metadata, buffer_owned_rc);
}

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

RUSTEXPORT VideoTrackInterface* Rust_getFirstVideoTrack(MediaStreamInterface* stream) {
  auto tracks = stream->GetVideoTracks();
  if (tracks.empty()) {
    return nullptr;
  }
  rtc::scoped_refptr<VideoTrackInterface> first = tracks[0];
  return take_rc(first);
}

// Passed-in "obj" must live at least as long as the VideoSink,
// which likely means as long as the VideoTrack it's attached to,
// which likely means as long as the PeerConnection.
RUSTEXPORT void Rust_addVideoSink(
    webrtc::VideoTrackInterface* track_borrowed_rc,
    void* obj_borrowed,
    VideoSinkCallbacks* cbs_borrowed) {
  auto sink_owned_rc = take_rc(make_ref_counted<VideoSink>(obj_borrowed, cbs_borrowed));
  // LEAK: This is never decremeted.  We should fix that.
  auto sink_borrowed = sink_owned_rc;

  rtc::VideoSinkWants wants;
  // Note: this causes frames to be dropped, not rotated.
  // So don't set it to true, even if it seems to make sense!
  wants.rotation_applied = false;

  // The sink gets stored in the track, but never destroys it.
  // The sink must live as long as the track.
  track_borrowed_rc->AddOrUpdateSink(sink_borrowed, wants);
}

RUSTEXPORT void Rust_pushVideoFrame(
    webrtc::rffi::VideoSource* source_borrowed_rc,
    VideoFrameBuffer* buffer_borrowed_rc) {
  // At some point we might care about capture timestamps, but for now
  // using the current time is sufficient.
  auto timestamp_us = rtc::TimeMicros();
  auto frame = webrtc::VideoFrame::Builder()
      .set_video_frame_buffer(inc_rc(buffer_borrowed_rc))
      .set_timestamp_us(timestamp_us)
      .build();
  source_borrowed_rc->PushVideoFrame(std::move(frame));
}

// Returns an owned RC.
RUSTEXPORT VideoFrameBuffer* Rust_createVideoFrameBufferFromRgba(
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
