//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    convert::TryInto,
    io::{Read, Seek, SeekFrom, Write},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use ringrtc::{
    lite::sfu::DemuxId,
    webrtc::media::{VideoFrame, VideoPixelFormat, VideoSink},
};

pub const FRAME_INTERVAL_30FPS: Duration = Duration::from_millis(33);

pub trait VideoInput {
    fn next_frame(&mut self) -> VideoFrame;
}

/// Yields frames from a source of I420 YUV data.
///
/// Loops if it reaches the end.
pub struct I420Source<T> {
    width: u32,
    height: u32,
    buffer: Vec<u8>,
    input: Option<T>,
}

impl<T: Seek> I420Source<T> {
    pub fn new(width: u32, height: u32, mut input: T) -> Self {
        let frame_size = (width as usize) * (height as usize) * 3 / 2;

        let stream_len = input.seek(SeekFrom::End(0)).expect("invalid input stream");
        input.rewind().expect("invalid input stream");
        assert!(
            stream_len % (frame_size as u64) == 0,
            "input length ({}) is not a multiple of the frame size in bytes ({})",
            stream_len,
            frame_size,
        );

        Self {
            width,
            height,
            buffer: vec![0; frame_size],
            input: Some(input),
        }
    }
}

impl<T: Read + Seek + Send> VideoInput for I420Source<T> {
    fn next_frame(&mut self) -> VideoFrame {
        if let Some(input) = &mut self.input {
            let is_at_start = input.stream_position().ok() == Some(0);
            match input.read_exact(&mut self.buffer) {
                Ok(()) => {}
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof
                        && !is_at_start
                        && input.rewind().is_ok()
                    {
                        return self.next_frame();
                    }
                    // If we fail to rewind (or had an error other than EOF),
                    // produce a black frame from now on.
                    self.input = None;
                    self.buffer.fill(0);
                }
            }
        }
        VideoFrame::copy_from_slice(
            self.width,
            self.height,
            VideoPixelFormat::I420,
            &self.buffer,
        )
    }
}

#[derive(Clone)]
pub struct DefaultVideoSink;

impl VideoSink for DefaultVideoSink {
    fn on_video_frame(&self, _demux_id: DemuxId, _frame: VideoFrame) {
        // Do nothing.
    }

    fn box_clone(&self) -> Box<dyn VideoSink> {
        Box::new(self.clone())
    }
}

pub struct WriterVideoSink<T> {
    shared_state: Arc<(T, AtomicU64)>,
    epoch: Instant,
    width: u32,
    height: u32,
}

impl<T> WriterVideoSink<T> {
    pub fn new(output: T, width: u32, height: u32) -> Self {
        Self {
            shared_state: Arc::new((output, AtomicU64::new(0))),
            epoch: Instant::now(),
            width,
            height,
        }
    }
}

impl<T> Clone for WriterVideoSink<T> {
    fn clone(&self) -> Self {
        Self {
            shared_state: self.shared_state.clone(),
            epoch: self.epoch,
            width: self.width,
            height: self.height,
        }
    }
}

impl<T: Send + Sync + 'static> VideoSink for WriterVideoSink<T>
where
    for<'a> &'a T: Write,
{
    fn on_video_frame(&self, _demux_id: DemuxId, frame: VideoFrame) {
        let write_frame_data = |frame_data| {
            (&self.shared_state.0)
                .write_all(frame_data)
                .expect("failed to write to file")
        };
        let save_next_frame_time = |new_elapsed: Duration| {
            self.shared_state.1.store(
                new_elapsed
                    .as_millis()
                    .try_into()
                    .expect("unreasonably long test"),
                Ordering::Relaxed,
            );
        };

        // `frame` can vary in size based on network conditions, so scale it to a known size to
        // simplify further processing.
        let input_frame = if self.width > 0 && self.height > 0 {
            frame.scale(self.width, self.height)
        } else {
            frame
        };

        let frame_data = input_frame.as_i420().expect("I420 data not available");
        let elapsed = self.epoch.elapsed();

        let mut next_frame_elapsed =
            Duration::from_millis(self.shared_state.1.load(Ordering::Relaxed));
        if next_frame_elapsed.is_zero() {
            // First frame!
            save_next_frame_time(elapsed + FRAME_INTERVAL_30FPS);
            write_frame_data(frame_data);
            return;
        }

        // Write several copies of the current frame so that our output is 30fps too.
        // This isn't quite right; it's the *previous* frame that would be stuck on the screen.
        // But it's probably close enough for testing.
        // Allow frames to arrive a little early, but not too early.
        if next_frame_elapsed >= elapsed + Duration::from_millis(3) {
            // Not enough time has passed. Skip this frame, don't update anything.
            return;
        }
        while next_frame_elapsed < elapsed + Duration::from_millis(3) {
            write_frame_data(frame_data);
            next_frame_elapsed += FRAME_INTERVAL_30FPS;
        }
        // Save the time we expect the next frame to arrive.
        save_next_frame_time(next_frame_elapsed);
    }

    fn box_clone(&self) -> Box<dyn VideoSink> {
        Box::new(self.clone())
    }
}
