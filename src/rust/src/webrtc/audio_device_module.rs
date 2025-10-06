//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use core::convert::AsRef;
use std::{
    collections::VecDeque,
    ffi::{CStr, c_uchar, c_void},
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
use cubeb::{Context, DeviceId, DeviceType, MonoFrame, StereoFrame, Stream, StreamPrefs};
use cubeb_core::{LogLevel, log_enabled, set_logging};
use lazy_static::lazy_static;
use regex::Regex;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com;

use crate::{
    webrtc,
    webrtc::{
        audio_device_module_utils::{
            DeviceCollectionWrapper, copy_and_truncate_string, redact_by_regex, redact_for_logging,
        },
        ffi::audio_device_module::RffiAudioTransport,
        peer_connection_factory::{AudioDevice, AudioDeviceObserver},
    },
};

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

// Stays in sync with WindowsDeviceType in webrtc
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WindowsDeviceType {
    DefaultCommunicationDevice = -1,
    DefaultDevice = -2,
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

type Frame = MonoFrame<i16>;
type OutFrame = StereoFrame<i16>;

#[derive(Debug)]
enum Event {
    RefreshCache(DeviceType),
    SetCallback(Arc<Mutex<RffiAudioTransport>>),
    SetPlayoutDevice(u16),
    SetRecordingDevice(u16),
    InitPlayout,
    StartPlayout,
    StopPlayout,
    InitRecording,
    StartRecording,
    StopRecording,
    PlayoutDelay,
    Terminate,
    RegisterAudioObserver(Box<dyn AudioDeviceObserver>),
}

#[derive(Debug, Clone)]
struct UpdateCallbackData {
    device_type: DeviceType,
    sender: mpsc::Sender<Event>,
}

struct Worker {
    ctx: Context,
    // We will  pass raw pointers to these to the cubeb API.
    // These must be destroyed **after** we unregister the callbacks with cubeb.
    input_data: UpdateCallbackData,
    output_data: UpdateCallbackData,
    // Note that the DeviceIds must not outlive the ctx.
    playout_device: Option<DeviceId>,
    recording_device: Option<DeviceId>,
    // Note that the streams must not outlive the ctx.
    output_stream: Option<Stream<OutFrame>>,
    input_stream: Option<Stream<Frame>>,
    // Note that the caches must not outlive the ctx.
    input_device_cache: DeviceCollectionWrapper,
    output_device_cache: DeviceCollectionWrapper,
    // These may outlive the ctx
    input_device_names: Arc<Mutex<Vec<Option<AudioDevice>>>>,
    output_device_names: Arc<Mutex<Vec<Option<AudioDevice>>>>,
    audio_transport: Arc<Mutex<RffiAudioTransport>>,
    audio_device_observer: Option<Box<dyn AudioDeviceObserver>>,
}

impl Worker {
    fn refresh_device_cache(&mut self, device_type: DeviceType) -> anyhow::Result<()> {
        info!("Refresh {:?} devices", device_type);
        let devices = {
            // Pause logging because enumeration is noisy
            let _guard = LogDisableGuard::new();
            self.ctx.enumerate_devices(device_type)?
        };

        let last = match device_type {
            DeviceType::INPUT => &self.input_device_cache,
            DeviceType::OUTPUT => &self.output_device_cache,
            _ => bail!("Bad device type {:?}", device_type),
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
            DeviceType::INPUT => {
                if let Some(ado) = &self.audio_device_observer {
                    ado.input_changed(names.clone());
                }
                self.input_device_cache = collection;
                *self.input_device_names.lock().unwrap() = names;
            }
            DeviceType::OUTPUT => {
                if let Some(ado) = &self.audio_device_observer {
                    ado.output_changed(names.clone());
                }
                self.output_device_cache = collection;
                *self.output_device_names.lock().unwrap() = names;
            }
            _ => bail!("Bad device type {:?}", device_type),
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
                device_type,
                Some(Worker::device_changed),
                match device_type {
                    DeviceType::INPUT => &mut self.input_data,
                    DeviceType::OUTPUT => &mut self.output_data,
                    _ => bail!("Bad device type {:?}", device_type),
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
                device_type,
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

        // Ensure these are not reused.
        self.playout_device = None;
        self.recording_device = None;

        self.input_device_cache = Default::default();
        self.output_device_cache = Default::default();
        self.input_device_names = Arc::new(Mutex::new(Vec::new()));
        self.output_device_names = Arc::new(Mutex::new(Vec::new()));

        if let Err(e) = self.deregister_device_collection_changed(DeviceType::INPUT) {
            warn!("failed to clear input callback: {}", e);
        }
        if let Err(e) = self.deregister_device_collection_changed(DeviceType::OUTPUT) {
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

    fn init_playout(&mut self) -> anyhow::Result<()> {
        let out_device = if let Some(device) = self.playout_device {
            device
        } else {
            bail!("Tried to init playout without a playout device");
        };
        let params = cubeb::StreamParamsBuilder::new()
            .format(STREAM_FORMAT)
            .rate(SAMPLE_FREQUENCY)
            .channels(2)
            .layout(cubeb::ChannelLayout::STEREO)
            .prefs(StreamPrefs::VOICE)
            .take();
        let mut builder = cubeb::StreamBuilder::<OutFrame>::new();
        let transport = Arc::clone(&self.audio_transport);
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
                        Arc::clone(&transport),
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

    fn start_playout(&mut self) -> anyhow::Result<()> {
        if let Some(output_stream) = &self.output_stream {
            if let Err(e) = output_stream.start() {
                bail!("Failed to start playout: {}", e);
            }
            Ok(())
        } else {
            bail!("Cannot start playout without an output stream -- did you forget init_playout?");
        }
    }

    fn stop_playout(&mut self) -> anyhow::Result<()> {
        if let Some(output_stream) = &self.output_stream {
            if let Err(e) = output_stream.stop() {
                bail!("Failed to stop playout: {}", e);
            }
            // Drop the stream so that it isn't reused on future calls.
            self.output_stream = None;
        }
        Ok(())
    }

    fn init_recording(&mut self) -> anyhow::Result<()> {
        let recording_device = if let Some(device) = self.recording_device {
            device
        } else {
            bail!("Tried to init recording without a recording device");
        };

        let builder = cubeb::StreamParamsBuilder::new()
            .format(STREAM_FORMAT)
            .rate(SAMPLE_FREQUENCY)
            .channels(NUM_CHANNELS)
            .layout(cubeb::ChannelLayout::MONO);
        // On Mac, the AEC pipeline runs at 24kHz (FB15839727 tracks this). For now,
        // disable it.
        let params = if cfg!(not(target_os = "macos")) {
            builder.prefs(StreamPrefs::VOICE)
        } else {
            builder
        }
        .take();
        let mut builder = cubeb::StreamBuilder::<Frame>::new();
        let transport = Arc::clone(&self.audio_transport);
        let min_latency = self.ctx.min_latency(&params).unwrap_or_else(|e| {
            warn!(
                "Could not get min latency for recording; using default: {:?}",
                e
            );
            SAMPLE_LATENCY
        });
        info!("min recording latency: {}", min_latency);
        // WebRTC can only accept data in WEBRTC_WINDOW-sized chunks.
        // This buffer tracks any extra data that would not fit in a call to WebRTC,
        // if `input.len()` is not an exact multiple of WEBRTC_WINDOW.
        let mut buffer = VecDeque::<i16>::new();
        buffer.reserve(WEBRTC_WINDOW);
        builder
            .name("ringrtc input")
            .input(recording_device, &params)
            .latency(std::cmp::max(SAMPLE_LATENCY, min_latency))
            .data_callback(move |input, _| {
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
                        Arc::clone(&transport),
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
                self.input_stream = Some(stream);
                Ok(())
            }
            Err(e) => {
                bail!("Couldn't initialize input stream: {}", e);
            }
        }
    }

    fn start_recording(&mut self) -> anyhow::Result<()> {
        if let Some(input_stream) = &self.input_stream {
            if let Err(e) = input_stream.start() {
                bail!("Failed to start recording: {}", e);
            }
        } else {
            bail!(
                "Cannot start recording without an input stream -- did you forget init_recording?"
            );
        }
        Ok(())
    }

    fn stop_recording(&mut self) -> anyhow::Result<()> {
        if let Some(input_stream) = &self.input_stream {
            if let Err(e) = input_stream.stop() {
                bail!("Failed to stop recording: {}", e);
            }
            // Drop the stream so that it isn't reused on future calls.
            self.input_stream = None;
        }
        Ok(())
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

    fn work(
        &mut self,
        receiver: mpsc::Receiver<Event>,
        playout_delay_sender: mpsc::Sender<anyhow::Result<u16>>,
    ) {
        for received in receiver {
            if let Err(e) = match received {
                Event::RefreshCache(d) => self.refresh_device_cache(d),
                Event::SetCallback(ref transport) => {
                    self.audio_transport = transport.clone();
                    Ok(())
                }
                Event::SetPlayoutDevice(index) => {
                    if let Some(d) = self.output_device_cache.get(index.into()) {
                        self.playout_device = Some(d.devid);
                        Ok(())
                    } else {
                        Err(anyhow!(
                            "Invalid playout device index {} requested (len {:?})",
                            index,
                            self.output_device_cache.count()
                        ))
                    }
                }
                Event::SetRecordingDevice(index) => {
                    if let Some(d) = self.input_device_cache.get(index.into()) {
                        self.recording_device = Some(d.devid);
                        Ok(())
                    } else {
                        Err(anyhow!(
                            "Invalid playout device index {} requested (len {:?})",
                            index,
                            self.input_device_cache.count()
                        ))
                    }
                }
                Event::InitPlayout => self.init_playout(),
                Event::StartPlayout => self.start_playout(),
                Event::StopPlayout => self.stop_playout(),
                Event::InitRecording => self.init_recording(),
                Event::StartRecording => self.start_recording(),
                Event::StopRecording => self.stop_recording(),
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
            } {
                warn!("{:?} failed: {:?}", received, e);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn recorded_data_is_available(
        rffi_audio_transport: Arc<Mutex<RffiAudioTransport>>,
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

        let guard = match rffi_audio_transport.lock() {
            Ok(g) => g,
            Err(e) => {
                error!("Failed to get mutex: {:?}", e);
                return (-1, 0);
            }
        };
        // Safety:
        // * self.audio_transport is within self, and will remain valid while this function is running
        //   because we enforce that the callback cannot change while playing or recording.
        // * The vector has sizeof(i16) * samples bytes allocated, and we pass both of these
        //   to the C layer, which should not read beyond that bound.
        // * The local new_mic_level pointer is valid and this function is synchronous, so it'll
        //   remain valid while it runs.
        let ret = unsafe {
            crate::webrtc::ffi::audio_device_module::Rust_recordedDataIsAvailable(
                guard.callback,
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
        rffi_audio_transport: Arc<Mutex<RffiAudioTransport>>,
        samples: usize,
        channels: u32,
        samples_per_sec: u32,
    ) -> PlayData {
        let mut data = vec![0i16; samples];
        let mut samples_out = 0usize;
        let mut elapsed_time_ms = 0i64;
        let mut ntp_time_ms = 0i64;

        let guard = match rffi_audio_transport.lock() {
            Ok(g) => g,
            Err(e) => {
                error!("Failed to get mutex: {:?}", e);
                return PlayData {
                    success: -1,
                    data: Vec::new(),
                    elapsed_time: None,
                    ntp_time: None,
                };
            }
        };
        // Safety:
        // * rffi_audio_transport will remain valid while this function is running
        //   because we enforce that the callback cannot change while playing or recording.
        // * The vector has sizeof(i16) * samples bytes allocated, and we pass both of these
        //   to the C layer, which should not write beyond that bound.
        // * The local variable pointers are all valid and this function is synchronous, so they'll
        //   remain valid while it runs.
        let ret = unsafe {
            crate::webrtc::ffi::audio_device_module::Rust_needMorePlayData(
                guard.callback,
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
        input_device_names: Arc<Mutex<Vec<Option<AudioDevice>>>>,
        output_device_names: Arc<Mutex<Vec<Option<AudioDevice>>>>,
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
                    device_type: DeviceType::INPUT,
                    sender: sender.clone(),
                },
                output_data: UpdateCallbackData {
                    device_type: DeviceType::OUTPUT,
                    sender: sender.clone(),
                },
                playout_device: None,
                recording_device: None,
                output_stream: None,
                input_stream: None,
                input_device_cache: Default::default(),
                output_device_cache: Default::default(),
                input_device_names,
                output_device_names,
                audio_transport: Arc::new(Mutex::new(RffiAudioTransport {
                    callback: std::ptr::null(),
                })),
                audio_device_observer: None,
            };
            if let Err(e) = worker.register_device_collection_changed(DeviceType::INPUT) {
                error!("Failed to register input device callback: {}", e);
                if let Err(e) = started_signal.send(None) {
                    error!("Further, failed to notify about start failure: {}", e);
                }
                return;
            }
            if let Err(e) = worker.register_device_collection_changed(DeviceType::OUTPUT) {
                error!("Failed to register output device callback: {}", e);
                if let Err(e) = started_signal.send(None) {
                    error!("Further, failed to notify about start failure: {}", e);
                }
                return;
            }

            // Refresh both caches before we signal a successful start so that callers can
            // immediately enumerate devices.
            if let Err(e) = sender.send(Event::RefreshCache(DeviceType::INPUT)) {
                error!("Failed to request initial input device refresh: {}", e);
                if let Err(e) = started_signal.send(None) {
                    error!("Further, failed to notify about start failure: {}", e);
                }
            }
            if let Err(e) = sender.send(Event::RefreshCache(DeviceType::OUTPUT)) {
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
    input_device_names: Arc<Mutex<Vec<Option<AudioDevice>>>>,
    output_device_names: Arc<Mutex<Vec<Option<AudioDevice>>>>,
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
    // The next two flags are for |{playout,recording}_is_available|
    has_playout_device: bool,
    has_recording_device: bool,
    // Worker thread for all direct cubeb interaction
    // This is only an option so that we can `take` out of it.
    cubeb_worker: Option<JoinHandle<()>>,
    mpsc_sender: mpsc::Sender<Event>,
    playout_delay_receiver: mpsc::Receiver<anyhow::Result<u16>>,
}

impl Drop for AudioDeviceModule {
    // Clean up in case the application exits without properly calling terminate().
    fn drop(&mut self) {
        self.input_device_names = Arc::new(Mutex::new(Vec::new()));
        self.output_device_names = Arc::new(Mutex::new(Vec::new()));
        self.has_playout_device = false;
        self.has_recording_device = false;

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

// Maximum lengths (and allocated amount of memory) for device names and GUIDs.
const ADM_MAX_DEVICE_NAME_SIZE: usize = 128;
const ADM_MAX_GUID_SIZE: usize = 128;

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
    lazy_static! {
        static ref LINE_RE: Regex = Regex::new(r"(?<ident>[^\s]+:\d+)(?<message>.*)").unwrap();
    }
    // These patterns describe the places friendly_name, device_id, and group_id are logged,
    // for the backends where those are potentially sensitive.
    //
    // The capture groups will be redacted.
    //
    // Note that there's another one that isn't here: cubeb.c logs something like:
    //   LOG("DeviceID: \"%s\"%s\n"
    //       "\tName:\t\"%s\"\n"
    //       ...
    // But we just ignore this line altogether.
    lazy_static! {
        static ref REDACTION_RES: Vec<Regex> = vec![
            // cubeb_wasapi.cpp: wasapi_find_bt_handsfree_output_device
            Regex::new(r"Found matching device for (.*): (.*)").unwrap(),
            // cubeb_wasapi.cpp: wasapi_collection_notification_client::OnDefaultDeviceChanged
            Regex::new(r"collection: Audio device default changed, id = (.*)\.").unwrap(),
            // cubeb_wasapi.cpp: wasapi_collection_notification_client::OnDeviceStateChanged
            Regex::new(r"collection: Audio device state changed, id = (.*), state = .*\.").unwrap(),
            // cubeb_wasapi.cpp: wasapi_endpoint_notification_client::OnDefaultDeviceChanged
            Regex::new(r"endpoint: Audio device default changed flow=.* role=.* new_device_id=(.*)\.").unwrap(),
            // cubeb-coreaudio-rs src/backend/mod.rs audiounit_get_devices_of_type
            Regex::new(r"Device \d+ \((.*)\) has \d+.*channels").unwrap(),
            // cubeb-coreaudio-rs src/backend/mod.rs should_block_vpio_for_device_pair
            Regex::new(r#".* uid="(.*)", model_uid="(.*)", transport_type=.*, source=.*, source_name="(.*)", name="(.*)", manufacturer=".*""#).unwrap(),
        ];
    }
    if PAUSE_LOGGING.load(Ordering::SeqCst) {
        return;
    }

    match s.to_str() {
        Ok(msg) => {
            if msg.contains("DeviceID") && msg.contains("Name") {
                // Shortcut:
                // Entirely suppress spammy lines that log all devices (we already do this)
                // See `log_device` in cubeb.c
                return;
            }

            // Assume valid lines are formatted "file:lineno" and ignore anything
            // not matching
            let Some(caps) = LINE_RE.captures(msg) else {
                return;
            };
            let ident = &caps["ident"];
            let contents = &caps["message"];

            let to_log = if cfg!(debug_assertions) {
                contents.to_string()
            } else {
                let mut out = contents.to_string();
                for re in REDACTION_RES.iter() {
                    if let Some(new) = redact_by_regex(re, contents) {
                        out = new;
                        break;
                    }
                }
                out
            };
            info!("cubeb: {}{}", ident, to_log);
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

        let input_device_names = Arc::new(Mutex::new(Vec::new()));
        let output_device_names = Arc::new(Mutex::new(Vec::new()));
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
            has_playout_device: false,
            has_recording_device: false,
            cubeb_worker: Some(cubeb_worker),
            mpsc_sender: sender,
            playout_delay_receiver,
        })
    }

    pub fn active_audio_layer(&self, _audio_layer: webrtc::ptr::Borrowed<AudioLayer>) -> i32 {
        -1
    }

    pub fn register_audio_callback(&mut self, audio_transport: *const c_void) -> i32 {
        // It is unsafe to change this callback while playing or recording, as
        // the change might then race with invocations of the callback, which
        // need not be serialized.
        if self.playing() || self.recording() {
            return -1;
        }
        if let Err(e) = self
            .mpsc_sender
            .send(Event::SetCallback(std::sync::Arc::new(Mutex::new(
                RffiAudioTransport {
                    callback: audio_transport,
                },
            ))))
        {
            error!("Failed to request SetCallback: {}", e);
            return -1;
        }
        0
    }

    // Main initialization and termination
    pub fn init(&mut self) -> i32 {
        // Don't bother re-initializing -- new handled it.
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

    fn enumerate_devices(
        &mut self,
        device_type: DeviceType,
    ) -> anyhow::Result<Vec<Option<AudioDevice>>> {
        let collection = match device_type {
            DeviceType::INPUT => self
                .input_device_names
                .as_ref()
                .lock()
                .map_err(|_| anyhow!("failed to lock"))?
                .clone(),
            DeviceType::OUTPUT => self
                .output_device_names
                .as_ref()
                .lock()
                .map_err(|_| anyhow!("failed to lock"))?
                .clone(),
            _ => bail!("Bad device type {:?}", device_type),
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
            device.device_id().map(redact_for_logging),
            device.friendly_name().map(redact_for_logging),
            device.group_id().map(redact_for_logging),
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
        let raw = match self.enumerate_devices(DeviceType::OUTPUT) {
            Ok(devices) => devices.len().try_into().unwrap_or(-1),
            Err(e) => {
                error!("Failed to get playout device count: {}", e);
                -1
            }
        };
        // Windows has special logic: it doesn't count the default devices
        #[cfg(not(target_os = "windows"))]
        return raw;
        #[cfg(target_os = "windows")]
        return raw - 2;
    }

    pub fn recording_devices(&mut self) -> i16 {
        let raw = match self.enumerate_devices(DeviceType::INPUT) {
            Ok(devices) => devices.len().try_into().unwrap_or(-1),
            Err(e) => {
                error!("Failed to get recording device count: {}", e);
                -1
            }
        };
        // Windows has special logic: it doesn't count the default devices
        #[cfg(not(target_os = "windows"))]
        return raw;
        #[cfg(target_os = "windows")]
        return raw - 2;
    }

    fn copy_name_and_id(
        index: u16,
        devices: &[Option<AudioDevice>],
        name_out: webrtc::ptr::Borrowed<c_uchar>,
        guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> anyhow::Result<()> {
        if let Some(Some(d)) = devices.get(index as usize) {
            copy_and_truncate_string(&d.name, name_out, ADM_MAX_DEVICE_NAME_SIZE)?;
            copy_and_truncate_string(&d.unique_id, guid_out, ADM_MAX_GUID_SIZE)?;
            Ok(())
        } else {
            Err(anyhow!(
                "Could not get device at index {} (len {})",
                index,
                devices.len()
            ))
        }
    }

    pub fn playout_device_name(
        &mut self,
        index: u16,
        name_out: webrtc::ptr::Borrowed<c_uchar>,
        guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> i32 {
        match self.enumerate_devices(DeviceType::OUTPUT) {
            Ok(devices) => {
                match AudioDeviceModule::copy_name_and_id(index, &devices, name_out, guid_out) {
                    Ok(_) => 0,
                    Err(e) => {
                        error!("Failed to copy name and ID for playout device: {}", e);
                        -1
                    }
                }
            }
            Err(e) => {
                error!("Failed to enumerate devices for playout device: {}", e);
                -1
            }
        }
    }

    pub fn recording_device_name(
        &mut self,
        index: u16,
        name_out: webrtc::ptr::Borrowed<c_uchar>,
        guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> i32 {
        match self.enumerate_devices(DeviceType::INPUT) {
            Ok(devices) => {
                match AudioDeviceModule::copy_name_and_id(index, &devices, name_out, guid_out) {
                    Ok(_) => 0,
                    Err(e) => {
                        warn!("Failed to copy name and ID for recording device: {}", e);
                        -1
                    }
                }
            }
            Err(e) => {
                error!("Failed to enumerate devices for recording device: {}", e);
                -1
            }
        }
    }

    // Device selection
    pub fn set_playout_device(&mut self, index: u16) -> i32 {
        match self.enumerate_devices(DeviceType::OUTPUT) {
            Ok(devices) => match devices.get(index as usize) {
                Some(_) => {
                    if let Err(e) = self.mpsc_sender.send(Event::SetPlayoutDevice(index)) {
                        error!("Failed to request SetPlayoutDevice: {}", e);
                        return -1;
                    }
                }
                None => {
                    warn!(
                        "Invalid playout device index {} requested (len {})",
                        index,
                        devices.len()
                    );
                    if devices.is_empty() {
                        info!("Likely failed due to benign startup race");
                        return 0;
                    }
                    return -1;
                }
            },
            Err(e) => {
                error!("failed to enumerate devices for playout device: {}", e);
                return -1;
            }
        };
        self.has_playout_device = true;
        0
    }

    pub fn set_playout_device_win(&mut self, device: WindowsDeviceType) -> i32 {
        // DefaultDevice is at index 0 and DefaultCommunicationDevice at index 1
        self.set_playout_device(if device == WindowsDeviceType::DefaultDevice {
            0
        } else {
            1
        })
    }

    pub fn set_recording_device(&mut self, index: u16) -> i32 {
        match self.enumerate_devices(DeviceType::INPUT) {
            Ok(devices) => match devices.get(index as usize) {
                Some(_) => {
                    if let Err(e) = self.mpsc_sender.send(Event::SetRecordingDevice(index)) {
                        error!("Failed to request SetRecordingDevice: {}", e);
                        return -1;
                    }
                }
                None => {
                    warn!(
                        "Invalid recording device index {} requested (len {})",
                        index,
                        devices.len()
                    );
                    if devices.is_empty() {
                        info!("Likely failed due to benign startup race");
                        return 0;
                    }
                    return -1;
                }
            },
            Err(e) => {
                error!("failed to enumerate devices for playout device: {}", e);
                return -1;
            }
        };
        self.has_recording_device = true;
        0
    }

    pub fn set_recording_device_win(&mut self, device: WindowsDeviceType) -> i32 {
        // DefaultDevice is at index 0 and DefaultCommunicationDevice at index 1
        self.set_recording_device(if device == WindowsDeviceType::DefaultDevice {
            0
        } else {
            1
        })
    }

    // Audio transport initialization
    pub fn playout_is_available(&self, available_out: webrtc::ptr::Borrowed<bool>) -> i32 {
        let available = self.has_playout_device;
        match write_to_null_or_valid_pointer(available_out, available) {
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
        let available = self.has_recording_device;
        match write_to_null_or_valid_pointer(available_out, available) {
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
        assert_eq!(adm.init(), 0);

        assert_eq!(adm.backend_name(), expected_backend.to_string());
    }
}
