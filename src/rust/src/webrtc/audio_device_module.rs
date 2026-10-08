//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use core::convert::AsRef;
use std::{
    collections::VecDeque,
    ffi::{CStr, c_uchar, c_void},
    fmt::Display,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    thread::JoinHandle,
    time::Duration,
};

use anyhow::{Context as AnyhowContext, anyhow, bail};
use cubeb::{Context, DeviceId, MonoFrame, StereoFrame, Stream, StreamPrefs};
use cubeb_core::{InputProcessingParams, LogLevel, log_enabled, set_logging};
use thiserror::Error;
#[cfg(not(feature = "sim"))]
use webrtc::ffi::audio_device_module::{Rust_needMorePlayData, Rust_recordedDataIsAvailable};
#[cfg(feature = "sim")]
use webrtc::sim::audio_device_module::{Rust_needMorePlayData, Rust_recordedDataIsAvailable};
#[cfg(target_os = "windows")]
use windows::Win32::System::Com;

use crate::{
    core::util::truncate_for_logging,
    webrtc,
    webrtc::{
        audio_device_module_utils::{DeviceCollectionWrapper, do_cubeb_redactions},
        peer_connection_factory::{AudioDevice, AudioDeviceList, AudioDeviceObserver},
    },
};

#[derive(Error, Debug)]
pub enum AudioStartError {
    #[error("Start without successful Init call")]
    StartWithoutInit,
    #[error("Cubeb failure: {0}")]
    CubebError(cubeb::Error),
}

// Stays in sync with AudioLayer in webrtc
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AudioLayer {
    PlatformDefaultAudio,
    WindowsCoreAudio,
    WindowsCoreAudio2,
    LinuxAlsaAudio,
    LinuxPulseAudio,
    AndroidJavaAudio,
    AndroidOpenSLESAudio,
    AndroidJavaInputAndOpenSLESOutputAudio,
    AndroidAAudioAudio,
    AndroidJavaInputAndAAudioOutputAudio,
    DummyAudio,
}

/// Return type for need_more_play_data
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct PlayData {
    /// Actual return value of the underlying C function
    success: i32,
    /// Data generated
    data: Vec<i16>,
    /// Elapsed time, if one could be read
    elapsed_time: Option<Duration>,
    /// NTP time, if one could be read
    ntp_time: Option<Duration>,
}

/// Restricted version of cubeb's DeviceType enum that only has input and output.
/// Basically a convenience type to avoid having to add fallback cases for matches, since we
/// only use Input or Output, never a combination (or neither)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeviceType {
    Input,
    Output,
}

impl DeviceType {
    fn to_cubeb(self) -> cubeb::DeviceType {
        match self {
            Self::Input => cubeb::DeviceType::INPUT,
            Self::Output => cubeb::DeviceType::OUTPUT,
        }
    }
}

type Frame = MonoFrame<i16>;
type OutFrame = StereoFrame<i16>;

#[derive(Debug)]
enum Event {
    RefreshCache(DeviceType),
    SetPlayoutDevice { device: String, request_id: u32 },
    SetRecordingDevice { device: String, request_id: u32 },
    InitPlayout,
    StartPlayout,
    StopPlayout,
    InitRecording,
    StartRecording,
    WarmupRecording(u32),
    SetInputProcessing(bool, u32),
    StopRecording,
    PlayoutDelay,
    Terminate,
    RegisterAudioObserver(Box<dyn AudioDeviceObserver>),
    SetCallbackIndirect(usize),
}

// Print the enum branch without the associated data for logging.
impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Event::RefreshCache(_) => "RefreshCache",
                Event::SetPlayoutDevice { .. } => "SetPlayoutDevice",
                Event::SetRecordingDevice { .. } => "SetRecordingDevice",
                Event::InitPlayout => "InitPlayout",
                Event::StartPlayout => "StartPlayout",
                Event::StopPlayout => "StopPlayout",
                Event::InitRecording => "InitRecording",
                Event::StartRecording => "StartRecording",
                Event::WarmupRecording(_) => "WarmupRecording",
                Event::SetInputProcessing(_, _) => "SetInputProcessing",
                Event::StopRecording => "StopRecording",
                Event::PlayoutDelay => "PlayoutDelay",
                Event::Terminate => "Terminate",
                Event::RegisterAudioObserver(_) => "RegisterAudioObserver",
                Event::SetCallbackIndirect(_) => "SetCallbackIndirect",
            }
        )
    }
}

#[derive(Debug, Clone)]
struct UpdateCallbackData {
    device_type: DeviceType,
    sender: mpsc::Sender<Event>,
}

struct Worker {
    ctx: Context,
    // We will pass raw pointers to these to the cubeb API.
    // These must be destroyed **after** we unregister the callbacks with cubeb.
    input_data: UpdateCallbackData,
    output_data: UpdateCallbackData,
    preferred_recording_device: Option<AudioDevice>,
    preferred_playout_device: Option<AudioDevice>,
    // Live playout/recording devices. They will only be set between
    // init_{recording,playout} and stop_{recording,playout}.
    // Note that the DeviceIds must not outlive the ctx.
    playout_device: Option<DeviceId>,
    recording_device: Option<DeviceId>,
    // Note that the streams must not outlive the ctx.
    output_stream: Option<Stream<OutFrame>>,
    input_stream: Option<Stream<Frame>>,
    voice_processing_enabled: bool,
    // Note that the caches must not outlive the ctx.
    input_device_cache: DeviceCollectionWrapper,
    output_device_cache: DeviceCollectionWrapper,
    // These may outlive the ctx
    input_device_names: Arc<Mutex<AudioDeviceList>>,
    output_device_names: Arc<Mutex<AudioDeviceList>>,
    audio_device_observer: Option<Box<dyn AudioDeviceObserver>>,
    send_to_webrtc: Arc<AtomicBool>,
    // Has an attempt been made to init_{playing,recording} without a corresponding stop?
    // Used to determine if we should try to re-initialize **and start** on a device swap
    // (we always expect a Start to follow an Init)
    should_reinit_play: bool,
    should_reinit_record: bool,
    opaque_callback_data: Option<usize>,
}

impl Worker {
    fn refresh_device_cache(&mut self, device_type: DeviceType) -> anyhow::Result<()> {
        info!("Refresh {:?} devices", device_type);
        let devices = {
            // Pause logging because enumeration is noisy
            let _guard = LogDisableGuard::new();
            self.ctx.enumerate_devices(device_type.to_cubeb())?
        };

        let last = match device_type {
            DeviceType::Input => &self.input_device_cache,
            DeviceType::Output => &self.output_device_cache,
        };
        let collection = DeviceCollectionWrapper::new(&devices);
        if &collection == last {
            // Nothing to do; spurious wakeup
            return Ok(());
        }

        for device in devices.iter() {
            info!(
                "{:?} device: ({})",
                device_type,
                AudioDeviceModule::device_str(device)
            );
        }

        let names = collection.extract_names();

        match device_type {
            DeviceType::Input => {
                if let Some(ado) = &self.audio_device_observer {
                    ado.input_changed(names.clone());
                }
                self.input_device_cache = collection;
                *self.input_device_names.lock().unwrap() = names;
            }
            DeviceType::Output => {
                if let Some(ado) = &self.audio_device_observer {
                    ado.output_changed(names.clone());
                }
                self.output_device_cache = collection;
                *self.output_device_names.lock().unwrap() = names;
            }
        }
        Ok(())
    }

    /// Safety: Must be called with a valid |data| pointer. (NULL is okay.)
    unsafe extern "C" fn device_changed(_ctx: *mut cubeb::ffi::cubeb, data: *mut c_void) {
        // Flag that an update is needed; this will be processed in the worker thread.
        if let Some(d) = unsafe { (data as *mut UpdateCallbackData).as_ref() }
            && let Err(e) = d.sender.send(Event::RefreshCache(d.device_type))
        {
            error!("Failed to request {:?} cache refresh: {}", d.device_type, e);
        }
    }

    fn register_device_collection_changed(
        &mut self,
        device_type: DeviceType,
    ) -> anyhow::Result<()> {
        unsafe {
            // Safety: |device_changed| will remain a valid pointer for the lifetime of the program.
            // |input_data| and |output_data| will live until after the callback is unregistered.
            Ok(self.ctx.register_device_collection_changed(
                device_type.to_cubeb(),
                Some(Worker::device_changed),
                match device_type {
                    DeviceType::Input => &mut self.input_data,
                    DeviceType::Output => &mut self.output_data,
                } as *mut UpdateCallbackData as *mut c_void,
            )?)
        }
    }

    // After calling this, data may be deallocated (but does not need to be immediately)
    fn deregister_device_collection_changed(
        &mut self,
        device_type: DeviceType,
    ) -> anyhow::Result<()> {
        unsafe {
            // Safety: We are calling this with None, which will unset the callback,
            // so passing null is safe.
            Ok(self.ctx.register_device_collection_changed(
                device_type.to_cubeb(),
                None,
                std::ptr::null_mut(),
            )?)
        }
    }

    fn terminate(&mut self) {
        if let Some(input) = &self.input_stream
            && let Err(e) = input.stop()
        {
            error!("Failed to stop input: {}", e);
        }
        if let Some(output) = &self.output_stream
            && let Err(e) = output.stop()
        {
            error!("Failed to stop output: {}", e);
        }

        // Cause these to Drop.
        self.input_stream = None;
        self.output_stream = None;

        self.preferred_recording_device = None;
        self.preferred_playout_device = None;

        // Ensure these are not reused.
        self.playout_device = None;
        self.recording_device = None;

        self.input_device_cache = Default::default();
        self.output_device_cache = Default::default();
        self.input_device_names = Arc::new(Mutex::new(AudioDeviceList::new_for_target_os()));
        self.output_device_names = Arc::new(Mutex::new(AudioDeviceList::new_for_target_os()));

        if let Err(e) = self.deregister_device_collection_changed(DeviceType::Input) {
            warn!("failed to clear input callback: {}", e);
        }
        if let Err(e) = self.deregister_device_collection_changed(DeviceType::Output) {
            warn!("failed to clear output callback: {}", e);
        }
        // Now safe to invalidate the ctx (note that any references to it, like `DeviceId`s,
        // must have already been dropped).
        #[cfg(target_os = "windows")]
        {
            // Safety: No parameters, was already initialized.
            unsafe {
                Com::CoUninitialize();
            };
        }
    }

    /// Find the device ID for the preferred |device_type| device, if it is present.
    ///
    /// Refreshes the cache and retries once if the first attempt fails.
    fn preferred_devid_for(&mut self, device_type: DeviceType) -> Option<DeviceId> {
        fn query_device(worker: &Worker, device_type: DeviceType) -> Option<DeviceId> {
            let (cache, device_pref) = match device_type {
                DeviceType::Input => (
                    &worker.input_device_cache,
                    &worker.preferred_recording_device,
                ),
                DeviceType::Output => (
                    &worker.output_device_cache,
                    &worker.preferred_playout_device,
                ),
            };
            if let Some(device) = device_pref {
                if let Some(device_info) = cache.iter().find(|d| d.device_id == device.unique_id) {
                    return Some(device_info.devid);
                } else {
                    warn!(
                        "Saved user preference for {device_type:?} {device:?} does not correspond to a currently-known device",
                    );
                }
            } else {
                warn!(
                    "Attempting to initialize an {device_type:?} stream with no saved device preference"
                );
            }
            None
        }

        if let Some(d) = query_device(self, device_type) {
            return Some(d);
        }

        // try to refresh the cache in case it is stale
        if let Err(e) = self.refresh_device_cache(device_type) {
            warn!(
                "Error {e:?} while refreshing cache while getting preferred {device_type:?} device"
            );
            return None;
        }

        // Try once more now that the cache is refreshed.
        query_device(self, device_type)
    }

    /// Find an AudioDevice object for |device_id|, if any is connected.
    ///
    /// Refreshes the cache and retries once if the first attempt fails.
    fn find_audio_device_for(
        &mut self,
        device_type: DeviceType,
        device_id: &str,
    ) -> Option<AudioDevice> {
        fn query_device(
            worker: &Worker,
            device_type: DeviceType,
            device_id: &str,
        ) -> Option<AudioDevice> {
            let cache = match device_type {
                DeviceType::Input => &worker.input_device_cache,
                DeviceType::Output => &worker.output_device_cache,
            };
            if let Some(info) = cache.iter().find(|d| d.device_id == device_id).cloned() {
                return Some(AudioDevice {
                    unique_id: info.device_id.clone(),
                    name: info.friendly_name.clone(),
                });
            } else {
                warn!(
                    "Requested {device_type:?} device {:?} does not correspond to a currently-known device",
                    truncate_for_logging(device_id)
                );
            }
            None
        }

        if let Some(d) = query_device(self, device_type, device_id) {
            return Some(d);
        }

        // try to refresh the cache in case it is stale
        if let Err(e) = self.refresh_device_cache(device_type) {
            warn!(
                "Error {e:?} while refreshing cache while getting preferred {device_type:?} device"
            );
            return None;
        }

        // Try once more now that the cache is refreshed.
        query_device(self, device_type, device_id)
    }

    fn init_playout(&mut self) -> anyhow::Result<()> {
        self.should_reinit_play = true;
        let out_device = self
            .preferred_devid_for(DeviceType::Output)
            .ok_or(anyhow!("Failed to init playout - could not find device"))?;
        self.playout_device = Some(out_device);

        let params = cubeb::StreamParamsBuilder::new()
            .format(STREAM_FORMAT)
            .rate(SAMPLE_FREQUENCY)
            .channels(2)
            .layout(cubeb::ChannelLayout::STEREO)
            .prefs(StreamPrefs::VOICE)
            .take();
        let mut builder = cubeb::StreamBuilder::<OutFrame>::new();
        let min_latency = self.ctx.min_latency(&params).unwrap_or_else(|e| {
            warn!(
                "Could not get min latency for playout; using default: {:?}",
                e
            );
            SAMPLE_LATENCY
        });
        info!("min playout latency: {}", min_latency);
        // WebRTC can only report data in WEBRTC_WINDOW-sized chunks.
        // This buffer tracks any extra data that would not fit in `output`,
        // if `output.len()` is not an exact multiple of WEBRTC_WINDOW.
        let mut buffer = VecDeque::<i16>::new();
        let Some(opaque_callback_data) = self.opaque_callback_data else {
            bail!("Opaque callback data wasn't initialized");
        };
        buffer.reserve(WEBRTC_WINDOW);
        builder
            .name("ringrtc output")
            .output(out_device, &params)
            .latency(std::cmp::max(SAMPLE_LATENCY, min_latency))
            .data_callback(move |_, output| {
                if output.is_empty() {
                    return 0;
                }

                // WebRTC cannot give data in anything other than 10ms chunks, so request
                // these.
                // If the data callback is invoked with an `output` length that is
                // not a multiple of WEBRTC_WINDOW, make one "extra" call to webrtc and
                // store "extra" data in `buffer`.

                // First, copy any leftover data from prior invocations.
                let mut written = 0;
                while let Some(data) = buffer.pop_front() {
                    output[written] = OutFrame { l: data, r: data };
                    written += 1;
                    if written >= output.len() {
                        // Short-circuit; we already have enough data.
                        return output.len() as isize;
                    }
                }

                // Then, request more data from WebRTC.
                while written < output.len() {
                    let play_data = Worker::need_more_play_data(
                        opaque_callback_data,
                        WEBRTC_WINDOW,
                        NUM_CHANNELS,
                        SAMPLE_FREQUENCY,
                    );
                    if play_data.success < 0 {
                        // C function failed; propagate error and don't continue.
                        return play_data.success as isize;
                    } else if play_data.data.len() > WEBRTC_WINDOW {
                        error!("need_more_play_data returned too much data");
                        return -1;
                    } else if play_data.data.is_empty() {
                        warn!("need_more_play_data returned no data; short-circuiting");
                        break;
                    }
                    // Put data into the right format and add it to the output
                    // array for cubeb to play.
                    // If there's more data than was requested, add it to the
                    // buffer for the next invocation of the callback.
                    for data in play_data.data.iter() {
                        if written < output.len() {
                            output[written] = OutFrame { l: *data, r: *data };
                            written += 1;
                        } else {
                            buffer.push_back(*data);
                        }
                    }
                }

                if written != output.len() {
                    error!(
                        "Got wrong amount of output data (want {} got {}), may drain.",
                        output.len(),
                        written
                    );
                }
                written as isize
            })
            .state_callback(|state| {
                warn!("Playout state: {:?}", state);
            });
        match builder.init(&self.ctx) {
            Ok(stream) => {
                self.output_stream = Some(stream);
                Ok(())
            }
            Err(e) => {
                bail!("Couldn't initialize output stream: {}", e);
            }
        }
    }

    fn start_playout(&mut self) -> Result<(), AudioStartError> {
        if let Some(output_stream) = &self.output_stream {
            output_stream.start().map_err(AudioStartError::CubebError)
        } else {
            Err(AudioStartError::StartWithoutInit)
        }
    }

    fn stop_playout(&mut self) -> anyhow::Result<()> {
        self.should_reinit_play = false;
        if let Some(output_stream) = &self.output_stream {
            if let Err(e) = output_stream.stop() {
                bail!("Failed to stop playout: {}", e);
            }
            // Drop the stream so that it isn't reused on future calls.
            self.output_stream = None;
        }
        // Also drop the DeviceID - only save the AudioDevice
        self.playout_device = None;
        Ok(())
    }

    // Update the playout device, stopping and resuming playout if needed.
    // Can only fail if it fails to restart a stream.
    fn update_playout_device(&mut self, device: AudioDevice) -> anyhow::Result<()> {
        let should_reinit = self.should_reinit_play;
        if should_reinit && let Err(e) = self.stop_playout() {
            info!(
                "update_playout_device: failed to stop_playout: {e}. Likely the stream was not running."
            );
        }
        self.preferred_playout_device = Some(device);
        if should_reinit {
            // this will always set should_reinit_play, so if it fails and the
            // user selects a new device, we will still re-initialize
            self.init_playout()?;
            self.start_playout()?;
        }
        Ok(())
    }

    fn init_recording(&mut self) -> anyhow::Result<()> {
        self.should_reinit_record = true;
        if !self.send_to_webrtc.load(Ordering::SeqCst) && self.input_stream.is_some() {
            // Assume that this is the flow to "upgrade" from warmup to recording and do nothing
            info!("Skipping init_recording because we seem to be in warmup mode");
            return Ok(());
        }

        let recording_device = self
            .preferred_devid_for(DeviceType::Input)
            .ok_or(anyhow!("Failed to init recording - could not find device"))?;
        self.recording_device = Some(recording_device);

        let builder = cubeb::StreamParamsBuilder::new()
            .format(STREAM_FORMAT)
            .rate(SAMPLE_FREQUENCY)
            .channels(NUM_CHANNELS)
            .layout(cubeb::ChannelLayout::MONO);
        let params = if cfg!(not(target_os = "macos")) || self.voice_processing_enabled {
            builder.prefs(StreamPrefs::VOICE)
        } else {
            builder
        }
        .take();
        let mut builder = cubeb::StreamBuilder::<Frame>::new();
        let min_latency = self.ctx.min_latency(&params).unwrap_or_else(|e| {
            warn!(
                "Could not get min latency for recording; using default: {:?}",
                e
            );
            SAMPLE_LATENCY
        });
        info!("min recording latency: {}", min_latency);

        // Default this to sending to WebRTC
        self.send_to_webrtc.store(true, Ordering::SeqCst);
        // WebRTC can only accept data in WEBRTC_WINDOW-sized chunks.
        // This buffer tracks any extra data that would not fit in a call to WebRTC,
        // if `input.len()` is not an exact multiple of WEBRTC_WINDOW.
        let mut buffer = VecDeque::<i16>::new();
        let send_to_webrtc = self.send_to_webrtc.clone();
        let Some(opaque_callback_data) = self.opaque_callback_data else {
            bail!("Opaque callback data wasn't initialized");
        };
        buffer.reserve(WEBRTC_WINDOW);
        builder
            .name("ringrtc input")
            .input(recording_device, &params)
            .latency(std::cmp::max(SAMPLE_LATENCY, min_latency))
            .data_callback(move |input, _| {
                if !send_to_webrtc.load(Ordering::SeqCst) {
                    // Just drop this; we're warming the mic
                    return input.len() as isize;
                }
                // First add data from prior call(s).
                let data = buffer
                    .drain(0..)
                    .chain(input.iter().map(|f| f.m))
                    .collect::<Vec<_>>();
                // WebRTC cannot accept data in anything other than 10ms chunks, so report in these.
                // Buffer any excess data beyond a multiple of WEBRTC_WINDOW for a subsequent
                // callback invocation.
                let input_chunks = data.chunks(WEBRTC_WINDOW);
                for chunk in input_chunks {
                    if chunk.len() < WEBRTC_WINDOW {
                        // Do not try to invoke WebRTC with a too-short chunk.
                        buffer.extend(chunk);
                        break;
                    }
                    let (ret, _new_mic_level) = Worker::recorded_data_is_available(
                        opaque_callback_data,
                        chunk.to_vec(),
                        NUM_CHANNELS,
                        SAMPLE_FREQUENCY,
                        // TODO(mutexlox): do we need different values here?
                        Duration::new(0, 0),
                        0,
                        0,
                        false,
                        None,
                    );
                    if ret < 0 {
                        error!("Failed to report recorded data: {}", ret);
                        return ret as isize;
                    }
                }
                input.len() as isize
            })
            .state_callback(|state| {
                warn!("recording state: {:?}", state);
            });
        match builder.init(&self.ctx) {
            Ok(stream) => {
                if cfg!(target_os = "macos") && self.voice_processing_enabled {
                    // Note: On Mac, the AEC pipeline runs at 24kHz (FB15839727 tracks this).
                    // This results in a slightly thin sound because of the Nyquist theorem.
                    // See https://en.wikipedia.org/wiki/Nyquist_frequency
                    match self.ctx.supported_input_processing_params() {
                        Ok(params) => {
                            // With cubeb-coreaudio-rs, the VPIO input is inaudible without these settings.
                            // See https://github.com/mozilla/cubeb-coreaudio-rs/issues/239#issuecomment-2430361990
                            info!("Available input processing params: {:?}", params);
                            let mut desired_params = InputProcessingParams::empty();
                            if params.contains(InputProcessingParams::AUTOMATIC_GAIN_CONTROL)
                                && self.voice_processing_enabled
                            {
                                desired_params |= InputProcessingParams::AUTOMATIC_GAIN_CONTROL;
                            }
                            // With the coreaudio-rust backend, these settings must be set together.
                            if params.contains(
                                InputProcessingParams::ECHO_CANCELLATION
                                    | InputProcessingParams::NOISE_SUPPRESSION,
                            ) && self.voice_processing_enabled
                            {
                                desired_params |= InputProcessingParams::ECHO_CANCELLATION
                                    | InputProcessingParams::NOISE_SUPPRESSION;
                            }
                            if let Err(e) = stream.set_input_processing_params(desired_params) {
                                error!("couldn't set input params: {:?}", e);
                            }
                        }
                        Err(e) => warn!(
                            "Failed to get supported input processing parameters; proceeding without: {}",
                            e
                        ),
                    }
                }

                self.input_stream = Some(stream);
                Ok(())
            }
            Err(e) => {
                bail!("Couldn't initialize input stream: {}", e);
            }
        }
    }

    fn start_recording(&mut self, send_to_webrtc: bool) -> Result<(), AudioStartError> {
        let was_sending = self.send_to_webrtc.swap(send_to_webrtc, Ordering::SeqCst);
        if !was_sending {
            // We were warming up; all we need to do is start sending to webrtc (or continue warmup).
            return Ok(());
        }
        if let Some(input_stream) = &self.input_stream {
            input_stream.start().map_err(AudioStartError::CubebError)
        } else {
            Err(AudioStartError::StartWithoutInit)
        }
    }

    fn stop_recording(&mut self) -> anyhow::Result<()> {
        self.should_reinit_record = false;
        if let Some(input_stream) = &self.input_stream {
            let res = input_stream.stop();
            // Reset **after** we stop recording to avoid sending data to webrtc incorrectly
            self.send_to_webrtc.store(true, Ordering::SeqCst);
            if let Err(e) = res {
                bail!("Failed to stop recording: {}", e);
            }
            // Drop the stream so that it isn't reused on future calls.
            self.input_stream = None;
        }
        // Also drop the DeviceID - only save the AudioDevice
        self.recording_device = None;
        Ok(())
    }

    // Update the recording device, stopping and resuming recording if needed.
    // Can only fail if it fails to restart a stream.
    fn update_recording_device(&mut self, device: AudioDevice) -> anyhow::Result<()> {
        let should_reinit = self.should_reinit_record;
        let was_sending_webrtc = self.send_to_webrtc.load(Ordering::SeqCst);
        if should_reinit && let Err(e) = self.stop_recording() {
            info!(
                "update_recording_device: failed to stop_recording: {e}. Likely the stream was not running."
            );
        }
        self.preferred_recording_device = Some(device);
        if should_reinit {
            // this will always set should_reinit_record, so if it fails and the
            // user selects a new device, we will still re-initialize
            self.init_recording()?;
            self.start_recording(was_sending_webrtc)?;
        }
        Ok(())
    }

    fn set_input_processing_enabled(&mut self, enabled: bool) -> anyhow::Result<()> {
        if enabled == self.voice_processing_enabled {
            return Ok(());
        }
        self.voice_processing_enabled = enabled;

        if let Some(device) = self.preferred_recording_device.clone() {
            self.update_recording_device(device)
        } else {
            Ok(())
        }
    }

    // Get the playout delay, in ms.
    fn playout_delay(&mut self) -> anyhow::Result<u16> {
        match &self.output_stream {
            Some(output_stream) => {
                // Pause logging because this is noisy
                let _guard = LogDisableGuard::new();
                let latency_samples = output_stream.latency();
                match latency_samples {
                    Ok(latency_samples) => Ok((latency_samples / (SAMPLE_FREQUENCY / 1000)) as u16),
                    Err(e) => bail!("Failed to get latency: {}", e),
                }
            }
            None => bail!("No stream, cannot get playout delay"),
        }
    }

    /// Utility function to notify of a completed "start recording" attempt
    fn notify_start_recording_result_via_callback(
        &self,
        success: bool,
        device: Option<&AudioDevice>,
    ) {
        if let Some(ado) = self.audio_device_observer.as_ref() {
            ado.settle_start_recording_via_callback(success, device.cloned());
        }
    }

    /// Utility function to notify of a completed "start playout" attempt
    fn notify_start_playout_result_via_callback(
        &self,
        success: bool,
        device: Option<&AudioDevice>,
    ) {
        if let Some(ado) = self.audio_device_observer.as_ref() {
            ado.settle_start_playback_via_callback(success, device.cloned());
        }
    }

    /// Utility function to resolve a promise for an ADM call
    fn notify_promise_result(&self, success: bool, device: Option<&AudioDevice>, request_id: u32) {
        if let Some(ado) = self.audio_device_observer.as_ref() {
            ado.settle_promise(success, device.cloned(), request_id);
        }
    }

    fn work(
        &mut self,
        receiver: mpsc::Receiver<Event>,
        playout_delay_sender: mpsc::Sender<anyhow::Result<u16>>,
    ) {
        for received in receiver {
            let log_str = received.to_string();
            if let Err(e) = match received {
                Event::RefreshCache(d) => self.refresh_device_cache(d),
                Event::SetPlayoutDevice { device, request_id } => {
                    match self.find_audio_device_for(DeviceType::Output, &device) {
                        None => {
                            let e =
                                anyhow!("No such output device {}", truncate_for_logging(&device));
                            self.notify_promise_result(
                                false,
                                Some(&AudioDevice {
                                    name: String::new(),
                                    unique_id: device,
                                }),
                                request_id,
                            );
                            Err(e)
                        }
                        Some(audio_device) => self
                            .update_playout_device(audio_device.clone())
                            .inspect_err(|_| {
                                self.notify_promise_result(false, Some(&audio_device), request_id)
                            })
                            .inspect(|_| {
                                self.notify_promise_result(true, Some(&audio_device), request_id)
                            }),
                    }
                }
                Event::SetRecordingDevice { device, request_id } => {
                    match self.find_audio_device_for(DeviceType::Input, &device) {
                        None => {
                            let e =
                                anyhow!("No such input device {}", truncate_for_logging(&device));
                            self.notify_promise_result(
                                false,
                                Some(&AudioDevice {
                                    name: String::new(),
                                    unique_id: device,
                                }),
                                request_id,
                            );
                            Err(e)
                        }
                        Some(audio_device) => self
                            .update_recording_device(audio_device.clone())
                            .inspect_err(|_| {
                                self.notify_promise_result(false, Some(&audio_device), request_id)
                            })
                            .inspect(|_| {
                                self.notify_promise_result(true, Some(&audio_device), request_id)
                            }),
                    }
                }
                Event::InitPlayout => self.init_playout().inspect_err(|_| {
                    // InitPlayout is not directly called by desktop but rather by WebRTC,
                    // so there is not a request ID.
                    // Note that we should **not** notify on success, as "successful init" is not
                    // meaningful to the user -- we wait for "successful start".
                    self.notify_start_playout_result_via_callback(
                        false,
                        self.preferred_playout_device.as_ref(),
                    )
                }),
                Event::StartPlayout => self
                    .start_playout()
                    .inspect_err(|e| {
                        // Skip the callback in this case; it's quite likely that we just sent a callback
                        // about the init failure
                        if !matches!(e, AudioStartError::StartWithoutInit) {
                            // StartPlayout is not directly called by desktop but rather by WebRTC,
                            // so there is not a request ID
                            self.notify_start_playout_result_via_callback(
                                false,
                                self.preferred_playout_device.as_ref(),
                            )
                        }
                    })
                    .inspect(|_| {
                        self.notify_start_playout_result_via_callback(
                            true,
                            self.preferred_playout_device.as_ref(),
                        )
                    })
                    .context("StartPlayout"),
                Event::StopPlayout => self.stop_playout(), // don't propagate to client; nothing they can do
                Event::InitRecording => self.init_recording().inspect_err(|_| {
                    // InitRecording is not directly called by desktop but rather by WebRTC,
                    // so there is not a request ID
                    // Note that we should **not** notify on success, as "successful init" is not
                    // meaningful to the user -- we wait for "successful start".
                    self.notify_start_recording_result_via_callback(
                        false,
                        self.preferred_recording_device.as_ref(),
                    )
                }),
                Event::StartRecording => self
                    .start_recording(true)
                    .inspect_err(|e| {
                        if !matches!(e, AudioStartError::StartWithoutInit) {
                            // StartRecording is not directly called by desktop but rather by WebRTC,
                            // so there is not a request ID
                            self.notify_start_recording_result_via_callback(
                                false,
                                self.preferred_recording_device.as_ref(),
                            )
                        }
                    })
                    .inspect(|_| {
                        self.notify_start_recording_result_via_callback(
                            true,
                            self.preferred_recording_device.as_ref(),
                        )
                    })
                    .context("StartRecording"),
                Event::WarmupRecording(request_id) => {
                    if let Err(e) = self.init_recording() {
                        self.notify_promise_result(
                            false,
                            self.preferred_recording_device.as_ref(),
                            request_id,
                        );
                        Err(e)
                    } else {
                        self.start_recording(false)
                            .inspect_err(|_| {
                                self.notify_promise_result(
                                    false,
                                    self.preferred_recording_device.as_ref(),
                                    request_id,
                                );
                            })
                            .inspect(|_| {
                                self.notify_promise_result(
                                    true,
                                    self.preferred_recording_device.as_ref(),
                                    request_id,
                                )
                            })
                            .context("WarmupRecording")
                    }
                }
                Event::SetInputProcessing(voice_processing_enabled, request_id) => {
                    // Note that (as of mid-2026) this can only fail if recording init/start fails,
                    // which might mean that the requested device is no longer available (or cannot
                    // be initialized with this mode).
                    self.set_input_processing_enabled(voice_processing_enabled)
                        .inspect_err(|_| {
                            self.notify_promise_result(
                                false,
                                self.preferred_recording_device.as_ref(),
                                request_id,
                            )
                        })
                        .inspect(|_| {
                            self.notify_promise_result(
                                true,
                                self.preferred_recording_device.as_ref(),
                                request_id,
                            )
                        })
                }
                Event::StopRecording => self.stop_recording(), // don't propagate to client; nothing they can do
                Event::PlayoutDelay => {
                    if let Err(e) = playout_delay_sender.send(self.playout_delay()) {
                        Err(anyhow!("Failed to send playout delay: {}", e))
                    } else {
                        Ok(())
                    }
                }
                Event::Terminate => {
                    self.terminate();
                    return;
                }
                Event::RegisterAudioObserver(audio_device_observer) => {
                    self.audio_device_observer = Some(audio_device_observer);
                    continue;
                }
                Event::SetCallbackIndirect(opaque_callback_data) => {
                    self.opaque_callback_data = Some(opaque_callback_data);
                    Ok(())
                }
            } {
                warn!("{} failed: {:?}", log_str, e);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn recorded_data_is_available(
        opauque_callback_data: usize,
        samples: Vec<i16>,
        channels: u32,
        samples_per_sec: u32,
        total_delay: Duration,
        clock_drift: i32,
        current_mic_level: u32,
        key_pressed: bool,
        estimated_capture_time: Option<Duration>,
    ) -> (i32, u32) {
        let mut new_mic_level = 0u32;
        let estimated_capture_time_ns = estimated_capture_time.map_or(-1, |d| d.as_nanos() as i64);

        // Safety:
        // * The vector has sizeof(i16) * samples bytes allocated, and we pass both of these
        //   to the C layer, which should not read beyond that bound.
        // * The local new_mic_level pointer is valid and this function is synchronous, so it'll
        //   remain valid while it runs.
        let ret = unsafe {
            Rust_recordedDataIsAvailable(
                opauque_callback_data,
                samples.as_ptr() as *const c_void,
                samples.len(),
                std::mem::size_of::<i16>(),
                channels.try_into().unwrap(), // constant, so unwrap is safe
                samples_per_sec,
                total_delay.as_millis() as u32,
                clock_drift,
                current_mic_level,
                key_pressed,
                &mut new_mic_level,
                estimated_capture_time_ns,
            )
        };
        (ret, new_mic_level)
    }

    fn need_more_play_data(
        opaque_callback_data: usize,
        samples: usize,
        channels: u32,
        samples_per_sec: u32,
    ) -> PlayData {
        let mut data = vec![0i16; samples];
        let mut samples_out = 0usize;
        let mut elapsed_time_ms = 0i64;
        let mut ntp_time_ms = 0i64;

        // Safety:
        // * The vector has sizeof(i16) * samples bytes allocated, and we pass both of these
        //   to the C layer, which should not write beyond that bound.
        // * The local variable pointers are all valid and this function is synchronous, so they'll
        //   remain valid while it runs.
        let ret = unsafe {
            Rust_needMorePlayData(
                opaque_callback_data,
                samples,
                std::mem::size_of::<i16>(),
                channels.try_into().unwrap(), // constant, so unwrap is safe
                samples_per_sec,
                data.as_mut_ptr() as *mut c_void,
                &mut samples_out,
                &mut elapsed_time_ms,
                &mut ntp_time_ms,
            )
        };

        if ret != 0 {
            // For safety, prevent reading any potentially invalid data if the call failed
            // (note the truncate below).
            error!("failed to get output data");
            samples_out = 0;
        }

        data.truncate(samples_out);

        PlayData {
            success: ret,
            data,
            elapsed_time: elapsed_time_ms.try_into().ok().map(Duration::from_millis),
            ntp_time: ntp_time_ms.try_into().ok().map(Duration::from_millis),
        }
    }

    pub fn spawn(
        started_signal: mpsc::Sender<Option<String>>,
        receiver: mpsc::Receiver<Event>,
        sender: mpsc::Sender<Event>,
        playout_delay_sender: mpsc::Sender<anyhow::Result<u16>>,
        input_device_names: Arc<Mutex<AudioDeviceList>>,
        output_device_names: Arc<Mutex<AudioDeviceList>>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            #[cfg(target_os = "windows")]
            {
                // Safety: calling with valid parameters.
                let res = unsafe {
                    Com::CoInitializeEx(
                        None,
                        Com::COINIT_MULTITHREADED | Com::COINIT_DISABLE_OLE1DDE,
                    )
                };
                if res.is_err() {
                    error!("Failed to initialize COM: {}", res);
                    if let Err(e) = started_signal.send(None) {
                        error!("Further, failed to notify about start failure: {}", e);
                    }
                    return;
                }
            }

            // We must initialize this here because cubeb's Context is not Send
            let ctx = match Context::init(Some(ADM_CONTEXT), None) {
                Ok(ctx) => ctx,
                Err(e) => {
                    error!("Failed to initialize cubeb: {:?}", e);
                    if let Err(e) = started_signal.send(None) {
                        error!("Further, failed to notify about start failure: {}", e);
                    }
                    return;
                }
            };
            let name = ctx.backend_id().to_string();
            info!("Successfully initialized cubeb backend {}", name);
            let mut worker = Worker {
                ctx,
                input_data: UpdateCallbackData {
                    device_type: DeviceType::Input,
                    sender: sender.clone(),
                },
                output_data: UpdateCallbackData {
                    device_type: DeviceType::Output,
                    sender: sender.clone(),
                },
                preferred_playout_device: None,
                preferred_recording_device: None,
                playout_device: None,
                recording_device: None,
                output_stream: None,
                input_stream: None,
                voice_processing_enabled: false,
                input_device_cache: Default::default(),
                output_device_cache: Default::default(),
                input_device_names,
                output_device_names,
                audio_device_observer: None,
                send_to_webrtc: Arc::new(AtomicBool::new(true)),
                should_reinit_play: false,
                should_reinit_record: false,
                opaque_callback_data: None,
            };
            if let Err(e) = worker.register_device_collection_changed(DeviceType::Input) {
                error!("Failed to register input device callback: {}", e);
                if let Err(e) = started_signal.send(None) {
                    error!("Further, failed to notify about start failure: {}", e);
                }
                return;
            }
            if let Err(e) = worker.register_device_collection_changed(DeviceType::Output) {
                error!("Failed to register output device callback: {}", e);
                if let Err(e) = started_signal.send(None) {
                    error!("Further, failed to notify about start failure: {}", e);
                }
                return;
            }

            // Refresh both caches before we signal a successful start so that callers can
            // immediately enumerate devices.
            if let Err(e) = sender.send(Event::RefreshCache(DeviceType::Input)) {
                error!("Failed to request initial input device refresh: {}", e);
                if let Err(e) = started_signal.send(None) {
                    error!("Further, failed to notify about start failure: {}", e);
                }
            }
            if let Err(e) = sender.send(Event::RefreshCache(DeviceType::Output)) {
                error!("Failed to request initial output device refresh: {}", e);
                if let Err(e) = started_signal.send(None) {
                    error!("Further, failed to notify about start failure: {}", e);
                }
            }

            if let Err(e) = started_signal.send(Some(name)) {
                error!(
                    "Failed to notify of successful start; app will hang!!! Error: {}",
                    e
                );
            }

            worker.work(receiver, playout_delay_sender);
        })
    }
}

#[derive(Debug)]
pub struct AudioDeviceModule {
    backend_name: String,
    input_device_names: Arc<Mutex<AudioDeviceList>>,
    output_device_names: Arc<Mutex<AudioDeviceList>>,
    // Tracker flags to indicate whether we have **attempted** init_playout and
    // friends. Cleared on stop_playout and stop_recording.
    // We use these because some code, e.g. SetAudioRecordingDevice in
    // ringrtc/rffi/src/peer_connection_factory.cc, assumes that we should
    // only restart playout if playout was initialized **and** started.
    // We thus use these flags to answer questions like, "recording_is_initialized".
    attempted_playout_init: bool,
    attempted_recording_init: bool,
    attempted_playout_start: bool,
    attempted_recording_start: bool,
    // Worker thread for all direct cubeb interaction
    // This is only an Option so that we can `take` out of it.
    cubeb_worker: Option<JoinHandle<()>>,
    mpsc_sender: mpsc::Sender<Event>,
    playout_delay_receiver: mpsc::Receiver<anyhow::Result<u16>>,
}

impl Drop for AudioDeviceModule {
    // Clean up in case the application exits without properly calling terminate().
    fn drop(&mut self) {
        self.input_device_names = Arc::new(Mutex::new(AudioDeviceList::new_for_target_os()));
        self.output_device_names = Arc::new(Mutex::new(AudioDeviceList::new_for_target_os()));

        if let Err(e) = self.mpsc_sender.send(Event::Terminate) {
            error!("Failed to request cubeb termination: {}", e);
        }
        if let Some(cw) = self.cubeb_worker.take()
            && let Err(e) = cw.join()
        {
            error!("Failed to terminate cubeb worker: {:?}", e);
        }
    }
}

/// Arbitrary string to uniquely identify ringrtc for creating the cubeb object.
const ADM_CONTEXT: &CStr = c"ringrtc";

const SAMPLE_FREQUENCY: u32 = 48_000;
// Target sample latency. The actual sample latency will
// not always match this. (it's limited by cubeb's Context::min_latency)
const SAMPLE_LATENCY: u32 = SAMPLE_FREQUENCY / 100;

// WebRTC always expects to provide 10ms of samples at a time.
const WEBRTC_WINDOW: usize = SAMPLE_FREQUENCY as usize / 100;

const STREAM_FORMAT: cubeb::SampleFormat = cubeb::SampleFormat::S16NE;
const NUM_CHANNELS: u32 = 1;

fn write_to_null_or_valid_pointer<T>(
    mut ptr: webrtc::ptr::Borrowed<T>,
    v: T,
) -> anyhow::Result<()> {
    // Safety: As long as the C code passes a valid or null pointer, this is safe.
    unsafe {
        match ptr.as_mut() {
            Some(p) => {
                *p = v;
                Ok(())
            }
            None => Err(anyhow!("null pointer")),
        }
    }
}

static PAUSE_LOGGING: AtomicBool = AtomicBool::new(false);

struct LogDisableGuard;
impl LogDisableGuard {
    fn new() -> Self {
        PAUSE_LOGGING.store(true, Ordering::SeqCst);
        LogDisableGuard
    }
}

impl Drop for LogDisableGuard {
    fn drop(&mut self) {
        PAUSE_LOGGING.store(false, Ordering::SeqCst);
    }
}

fn log_c_str(s: &CStr) {
    if PAUSE_LOGGING.load(Ordering::SeqCst) {
        return;
    }

    match s.to_str() {
        Ok(msg) => {
            if let Some(s) = do_cubeb_redactions(msg) {
                info!("cubeb: {s}");
            }
        }
        Err(e) => {
            warn!("cubeb log message not UTF-8: {:?}", e);
        }
    }
}

impl AudioDeviceModule {
    pub fn new() -> anyhow::Result<Self> {
        if !log_enabled()
            && let Err(e) = set_logging(LogLevel::Normal, Some(log_c_str))
        {
            warn!("failed to set cubeb logging: {:?}", e);
        }

        let input_device_names = Arc::new(Mutex::new(AudioDeviceList::new_for_target_os()));
        let output_device_names = Arc::new(Mutex::new(AudioDeviceList::new_for_target_os()));
        let (sender, receiver) = mpsc::channel();
        let (started_sender, started_receiver) = mpsc::channel();
        let (playout_delay_sender, playout_delay_receiver) = mpsc::channel();
        let cubeb_worker = Worker::spawn(
            started_sender,
            receiver,
            sender.clone(),
            playout_delay_sender,
            input_device_names.clone(),
            output_device_names.clone(),
        );

        // Ensure the thread started correctly

        let backend_name = match started_receiver.recv().ok().flatten() {
            Some(s) => s,
            None => {
                bail!("Failed to initialize");
            }
        };

        Ok(AudioDeviceModule {
            backend_name,
            input_device_names,
            output_device_names,
            attempted_playout_init: false,
            attempted_recording_init: false,
            attempted_playout_start: false,
            attempted_recording_start: false,
            cubeb_worker: Some(cubeb_worker),
            mpsc_sender: sender,
            playout_delay_receiver,
        })
    }

    pub fn active_audio_layer(&self, _audio_layer: webrtc::ptr::Borrowed<AudioLayer>) -> i32 {
        -1
    }

    // Main initialization and termination
    pub fn init(&mut self, opaque_callback_data: usize) -> i32 {
        // Don't need to fully initialize (new did most of it), but *do* pass
        // this data from the C++ layer
        if let Err(e) = self
            .mpsc_sender
            .send(Event::SetCallbackIndirect(opaque_callback_data))
        {
            error!("Failed to request SetCallbackIndirect: {}", e);
            return -1;
        }
        0
    }

    pub fn backend_name(&self) -> String {
        self.backend_name.clone()
    }

    pub fn terminate(&mut self) -> i32 {
        0
    }

    pub fn initialized(&self) -> bool {
        true
    }

    fn enumerate_devices(&mut self, device_type: DeviceType) -> anyhow::Result<AudioDeviceList> {
        let collection = match device_type {
            DeviceType::Input => self
                .input_device_names
                .as_ref()
                .lock()
                .map_err(|_| anyhow!("failed to lock"))?
                .clone(),
            DeviceType::Output => self
                .output_device_names
                .as_ref()
                .lock()
                .map_err(|_| anyhow!("failed to lock"))?
                .clone(),
        };
        Ok(collection)
    }

    fn device_str(device: &cubeb::DeviceInfo) -> String {
        format!(
            concat!(
                "dev id: {:?}, device_id: {:?}, friendly_name: {:?}, group_id: {:?}, ",
                "vendor_name: {:?}, device_type: {:?}, state: {:?}, preferred: {:?}, format: {:?}, ",
                "default_format: {:?}, max channels: {:?}, default_rate: {:?}, max_rate: {:?}, ",
                "min_rate: {:?}, latency_lo: {:?}, latency_hi: {:?})"
            ),
            device.devid(),
            // Truncate these fields, as they can contain e.g. mac addresses or user-specified names.
            device.device_id().map(truncate_for_logging),
            device.friendly_name().map(truncate_for_logging),
            device.group_id().map(truncate_for_logging),
            device.vendor_name(),
            device.device_type(),
            device.state(),
            device.preferred(),
            device.format(),
            device.default_format(),
            device.max_channels(),
            device.default_rate(),
            device.max_rate(),
            device.min_rate(),
            device.latency_lo(),
            device.latency_hi()
        )
    }

    // Device enumeration
    pub fn playout_devices(&mut self) -> i16 {
        // NOTE: We expect that this is no longer used, since we have moved peer_connection_factory away from it.
        error!("Unexpectedly called playout_devices");
        -1
    }

    pub fn recording_devices(&mut self) -> i16 {
        // NOTE: We expect that this is no longer used, since we have moved peer_connection_factory away from it.
        error!("Unexpectedly called recording_devices");
        -1
    }

    pub fn playout_device_name(
        &mut self,
        _index: u16,
        _name_out: webrtc::ptr::Borrowed<c_uchar>,
        _guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> i32 {
        error!("Unexpectedly called playout_device_name");
        -1
    }

    pub fn recording_device_name(
        &mut self,
        _index: u16,
        _name_out: webrtc::ptr::Borrowed<c_uchar>,
        _guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> i32 {
        error!("Unexpectedly called recording_device_name");
        -1
    }

    // Audio transport initialization
    pub fn playout_is_available(&self, available_out: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available_out, true) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing playout available state: {:?}", e);
                -1
            }
        }
    }

    pub fn init_playout(&mut self) -> i32 {
        self.attempted_playout_init = true;
        if let Err(e) = self.mpsc_sender.send(Event::InitPlayout) {
            error!("Failed to request InitPlayout: {}", e);
            return -1;
        }
        0
    }

    pub fn playout_is_initialized(&self) -> bool {
        self.attempted_playout_init
    }

    pub fn recording_is_available(&self, available_out: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available_out, true) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing recording available state: {:?}", e);
                -1
            }
        }
    }

    pub fn init_recording(&mut self) -> i32 {
        self.attempted_recording_init = true;
        if let Err(e) = self.mpsc_sender.send(Event::InitRecording) {
            error!("Failed to request InitRecording: {}", e);
            return -1;
        }
        0
    }

    pub fn recording_is_initialized(&self) -> bool {
        self.attempted_recording_init
    }

    // Audio transport control
    pub fn start_playout(&mut self) -> i32 {
        self.attempted_playout_start = true;
        if let Err(e) = self.mpsc_sender.send(Event::StartPlayout) {
            error!("Failed to request StartPlayout: {}", e);
            return -1;
        }
        0
    }

    pub fn stop_playout(&mut self) -> i32 {
        self.attempted_playout_init = false;
        self.attempted_playout_start = false;
        if let Err(e) = self.mpsc_sender.send(Event::StopPlayout) {
            error!("Failed to request StopPlayout: {}", e);
            return -1;
        }
        0
    }

    pub fn playing(&self) -> bool {
        self.attempted_playout_start
    }

    pub fn start_recording(&mut self) -> i32 {
        self.attempted_recording_start = true;
        if let Err(e) = self.mpsc_sender.send(Event::StartRecording) {
            error!("Failed to request StartRecording: {}", e);
            return -1;
        }
        0
    }

    pub fn stop_recording(&mut self) -> i32 {
        self.attempted_recording_init = false;
        self.attempted_recording_start = false;
        if let Err(e) = self.mpsc_sender.send(Event::StopRecording) {
            error!("Failed to request StopRecording: {}", e);
            return -1;
        }
        0
    }

    pub fn recording(&self) -> bool {
        self.attempted_recording_start
    }

    // Audio mixer initialization
    pub fn init_speaker(&self) -> i32 {
        0
    }

    pub fn speaker_is_initialized(&self) -> bool {
        true
    }

    pub fn init_microphone(&self) -> i32 {
        0
    }

    pub fn microphone_is_initialized(&self) -> bool {
        true
    }

    // Speaker volume controls
    pub fn speaker_volume_is_available(&self, available: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available, false) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing speaker volume status: {:?}", e);
                -1
            }
        }
    }

    // This implementation doesn't support overriding speaker volume.
    pub fn set_speaker_volume(&self, _volume: u32) -> i32 {
        -1
    }

    pub fn speaker_volume(&self, _volume: webrtc::ptr::Borrowed<u32>) -> i32 {
        -1
    }

    pub fn max_speaker_volume(&self, _max_volume: webrtc::ptr::Borrowed<u32>) -> i32 {
        -1
    }

    pub fn min_speaker_volume(&self, _min_volume: webrtc::ptr::Borrowed<u32>) -> i32 {
        -1
    }

    // Microphone volume controls
    pub fn microphone_volume_is_available(&self, available: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available, false) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing microphone volume status: {:?}", e);
                -1
            }
        }
    }

    // This implementation doesn't support setting microphone volume.
    pub fn set_microphone_volume(&self, _volume: u32) -> i32 {
        -1
    }

    pub fn microphone_volume(&self, _volume: webrtc::ptr::Borrowed<u32>) -> i32 {
        -1
    }

    pub fn max_microphone_volume(&self, _max_volume: webrtc::ptr::Borrowed<u32>) -> i32 {
        -1
    }

    pub fn min_microphone_volume(&self, _min_volume: webrtc::ptr::Borrowed<u32>) -> i32 {
        -1
    }

    // Speaker mute control
    pub fn speaker_mute_is_available(&self, available: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available, false) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing speaker mute status: {:?}", e);
                -1
            }
        }
    }

    // This implementation doesn't support speaker mute in this way
    pub fn set_speaker_mute(&self, _enable: bool) -> i32 {
        -1
    }

    pub fn speaker_mute(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }

    // Microphone mute control
    pub fn microphone_mute_is_available(&self, available: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available, false) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing microphone mute status: {:?}", e);
                -1
            }
        }
    }

    pub fn set_microphone_mute(&self, _enable: bool) -> i32 {
        -1
    }

    pub fn microphone_mute(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }

    // Stereo support
    pub fn stereo_playout_is_available(&self, available: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available, false) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing stereo playout status: {:?}", e);
                -1
            }
        }
    }

    // This implementation only supports mono playout
    pub fn set_stereo_playout(&self, _enable: bool) -> i32 {
        -1
    }

    pub fn stereo_playout(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }

    pub fn stereo_recording_is_available(&self, available: webrtc::ptr::Borrowed<bool>) -> i32 {
        match write_to_null_or_valid_pointer(available, false) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing stereo recording status: {:?}", e);
                -1
            }
        }
    }

    // This implementation only supports mono recording.
    pub fn set_stereo_recording(&self, _enable: bool) -> i32 {
        -1
    }

    pub fn stereo_recording(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }

    pub fn playout_delay(&self, delay_ms: webrtc::ptr::Borrowed<u16>) -> i32 {
        if let Err(e) = self.mpsc_sender.send(Event::PlayoutDelay) {
            error!("Failed to request PlayoutDelay: {}", e);
            return -1;
        }
        // If we wait more than 50ms the delay probably won't be helpful anymore anyway.
        match self
            .playout_delay_receiver
            .recv_timeout(Duration::from_millis(50))
            .context("timed out trying to recv")
            .flatten()
        {
            Ok(latency_ms) => match write_to_null_or_valid_pointer(delay_ms, latency_ms) {
                Ok(_) => 0,
                Err(e) => {
                    error!("writing delay: {:?}", e);
                    -1
                }
            },
            Err(e) => {
                error!("Failed to get latency: {}", e);
                -1
            }
        }
    }

    // The below are inaccessible to C++, so they can use more complex rust types

    // Register a new callback to be notified of audio device changes, **replacing any existing one**
    pub fn register_audio_device_callback(
        &mut self,
        audio_device_observer: Box<dyn AudioDeviceObserver>,
    ) -> anyhow::Result<()> {
        if let Err(e) = self
            .mpsc_sender
            .send(Event::RegisterAudioObserver(audio_device_observer))
        {
            bail!("Failed to send RegisterAudioObserver request: {}", e);
        }
        Ok(())
    }

    pub fn get_audio_playout_devices(&mut self) -> anyhow::Result<AudioDeviceList> {
        self.enumerate_devices(DeviceType::Output)
    }

    pub fn get_audio_recording_devices(&mut self) -> anyhow::Result<AudioDeviceList> {
        self.enumerate_devices(DeviceType::Input)
    }

    pub fn warmup_recording(&mut self, request_id: u32) -> anyhow::Result<()> {
        if let Err(e) = self.mpsc_sender.send(Event::WarmupRecording(request_id)) {
            return Err(anyhow!("Failed to request WarmupRecording: {}", e));
        }
        Ok(())
    }

    fn set_device_by_id(
        &mut self,
        id: &str,
        device_type: DeviceType,
        request_id: u32,
    ) -> anyhow::Result<()> {
        let event = match device_type {
            DeviceType::Output => Event::SetPlayoutDevice {
                device: id.to_string(),
                request_id,
            },
            DeviceType::Input => Event::SetRecordingDevice {
                device: id.to_string(),
                request_id,
            },
        };
        let event_str = event.to_string();
        if let Err(e) = self.mpsc_sender.send(event) {
            bail!("Failed to request {event_str}: {e}");
        }
        Ok(())
    }

    pub fn set_playout_device_by_id(&mut self, id: &str, request_id: u32) -> anyhow::Result<()> {
        self.set_device_by_id(id, DeviceType::Output, request_id)
    }

    pub fn set_recording_device_by_id(&mut self, id: &str, request_id: u32) -> anyhow::Result<()> {
        self.set_device_by_id(id, DeviceType::Input, request_id)
    }

    pub fn set_input_processing_enabled(
        &mut self,
        enabled: bool,
        request_id: u32,
    ) -> anyhow::Result<()> {
        if let Err(e) = self
            .mpsc_sender
            .send(Event::SetInputProcessing(enabled, request_id))
        {
            return Err(anyhow!("Failed to request SetInputProcessing: {}", e));
        }
        Ok(())
    }
}

#[cfg(test)]
mod audio_device_module_tests {
    use crate::webrtc::audio_device_module::AudioDeviceModule;

    #[test]
    fn init_backend_id() {
        #[cfg(target_os = "windows")]
        let expected_backend = "wasapi";
        #[cfg(target_os = "macos")]
        let expected_backend = "audiounit-rust";
        #[cfg(target_os = "linux")]
        let expected_backend = "pulse-rust";

        cubeb_core::set_logging(
            cubeb_core::LogLevel::Normal,
            Some(|cstr| println!("{:?}", cstr)),
        )
        .expect("failed to set logging");
        let mut adm = AudioDeviceModule::new().unwrap();
        assert_eq!(adm.init(0), 0);

        assert_eq!(adm.backend_name(), expected_backend.to_string());
    }
}
