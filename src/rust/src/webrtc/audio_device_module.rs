//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;
use crate::webrtc::audio_device_module_utils::{copy_and_truncate_string, DeviceCollectionWrapper};
use crate::webrtc::ffi::audio_device_module::RffiAudioTransport;
use anyhow::anyhow;
use cubeb::{Context, DeviceId, DeviceType, MonoFrame, Stream, StreamPrefs};
use std::collections::VecDeque;
use std::ffi::{c_uchar, c_void, CString};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com;

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

pub struct AudioDeviceModule {
    audio_transport: Arc<Mutex<RffiAudioTransport>>,
    cubeb_ctx: Option<Context>,
    initialized: bool,
    playout_device: Option<DeviceId>,
    recording_device: Option<DeviceId>,
    output_stream: Option<Stream<Frame>>,
    input_stream: Option<Stream<Frame>>,
    playing: bool,
    recording: bool,
}

impl Default for AudioDeviceModule {
    fn default() -> Self {
        Self {
            audio_transport: Arc::new(Mutex::new(RffiAudioTransport {
                callback: std::ptr::null(),
            })),
            cubeb_ctx: None,
            initialized: false,
            playout_device: None,
            recording_device: None,
            output_stream: None,
            input_stream: None,
            playing: false,
            recording: false,
        }
    }
}

impl Drop for AudioDeviceModule {
    // Clean up in case the application exits without properly calling terminate().
    fn drop(&mut self) {
        if self.initialized {
            let out = self.terminate();
            if out != 0 {
                error!("Failed to terminate: {}", out);
            }
        }
    }
}

// Maximum lengths (and allocated amount of memory) for device names and GUIDs.
const ADM_MAX_DEVICE_NAME_SIZE: usize = 128;
const ADM_MAX_GUID_SIZE: usize = 128;

/// Arbitrary string to uniquely identify ringrtc for creating the cubeb object.
const ADM_CONTEXT: &str = "ringrtc";

const SAMPLE_FREQUENCY: u32 = 48_000;
// Target sample latency. The actual sample latency will
// not always match this. (it's limited by cubeb's Context::min_latency)
const SAMPLE_LATENCY: u32 = SAMPLE_FREQUENCY / 100;

// WebRTC always expects to provide 10ms of samples at a time.
const WEBRTC_WINDOW: usize = SAMPLE_FREQUENCY as usize / 100;

const STREAM_FORMAT: cubeb::SampleFormat = cubeb::SampleFormat::S16NE;
const NUM_CHANNELS: u32 = 1;

fn write_to_null_or_valid_pointer<T>(ptr: webrtc::ptr::Borrowed<T>, v: T) -> anyhow::Result<()> {
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

impl AudioDeviceModule {
    pub fn new() -> Self {
        Self::default()
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
        self.audio_transport = std::sync::Arc::new(Mutex::new(RffiAudioTransport {
            callback: audio_transport,
        }));
        0
    }

    // Main initialization and termination
    pub fn init(&mut self) -> i32 {
        // Don't bother re-initializing.
        if self.initialized {
            return 0;
        }
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
                return -1;
            }
        }
        let ctx_name = CString::new(ADM_CONTEXT).unwrap();
        match Context::init(Some(ctx_name.as_c_str()), None) {
            Ok(ctx) => {
                self.cubeb_ctx = Some(ctx);
                self.initialized = true;
                0
            }
            Err(e) => {
                error!("Failed to initialize: {}", e);
                -1
            }
        }
    }

    pub fn terminate(&mut self) -> i32 {
        if self.recording {
            self.stop_recording();
        }
        if self.playing {
            self.stop_playout();
        }
        // Cause these to Drop
        self.input_stream = None;
        self.output_stream = None;
        self.cubeb_ctx = None;
        self.initialized = false;
        #[cfg(target_os = "windows")]
        {
            // Safety: No parameters, was already initialized.
            unsafe {
                Com::CoUninitialize();
            };
        }
        0
    }

    pub fn initialized(&self) -> bool {
        self.initialized
    }

    fn enumerate_devices(
        &self,
        device_type: DeviceType,
    ) -> anyhow::Result<DeviceCollectionWrapper> {
        match &self.cubeb_ctx {
            Some(ctx) => Ok(DeviceCollectionWrapper::new(
                ctx.enumerate_devices(device_type)?,
            )),
            None => Err(anyhow!("Cannot enumerate devices without a cubeb ctx"))?,
        }
    }

    fn device_str(device: &cubeb::DeviceInfo) -> String {
        // Only print friendly name in debug builds.
        #[cfg(debug_assertions)]
        let friendly_name = device.friendly_name();
        #[cfg(not(debug_assertions))]
        let friendly_name: Option<&str> = None;
        format!(
            concat!("dev id: {:?}, device_id: {:?}, friendly_name: {:?}, group_id: {:?}, ",
            "vendor_name: {:?}, device_type: {:?}, state: {:?}, preferred: {:?}, format: {:?}, ",
            "default_format: {:?}, max channels: {:?}, default_rate: {:?}, max_rate: {:?}, ",
            "min_rate: {:?}, latency_lo: {:?}, latency_hi: {:?})"),
            device.devid(),
            device.device_id(),
            friendly_name,
            device.group_id(),
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
    pub fn playout_devices(&self) -> i16 {
        match self.enumerate_devices(DeviceType::OUTPUT) {
            Ok(device_collection) => device_collection.count().try_into().unwrap_or(-1),
            Err(e) => {
                error!("Failed to get playout devices: {}", e);
                -1
            }
        }
    }

    pub fn recording_devices(&self) -> i16 {
        match self.enumerate_devices(DeviceType::INPUT) {
            Ok(device_collection) => device_collection.count().try_into().unwrap_or(-1),
            Err(e) => {
                error!("Failed to get recording devices: {}", e);
                -1
            }
        }
    }

    fn copy_name_and_id(
        index: u16,
        devices: DeviceCollectionWrapper,
        name_out: webrtc::ptr::Borrowed<c_uchar>,
        guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> anyhow::Result<()> {
        if let Some(d) = devices.get(index.into()) {
            if let Some(name) = d.friendly_name() {
                let mut name_copy = name.to_string();
                // TODO(mutexlox): Localize these strings.
                #[cfg(not(target_os = "windows"))]
                if index == 0 {
                    name_copy = format!("default ({})", name);
                }
                #[cfg(target_os = "windows")]
                {
                    if index == 0 {
                        name_copy = format!("Default - {}", name);
                    } else if index == 1 {
                        name_copy = format!("Communication - {}", name);
                    }
                }
                copy_and_truncate_string(&name_copy, name_out, ADM_MAX_DEVICE_NAME_SIZE)?;
            } else {
                return Err(anyhow!("Could not get device name"));
            }
            if let Some(id) = d.device_id() {
                copy_and_truncate_string(id, guid_out, ADM_MAX_GUID_SIZE)?;
            } else {
                return Err(anyhow!("Could not get device ID"));
            }
            Ok(())
        } else {
            Err(anyhow!(
                "Could not get device at index {} (len {})",
                index,
                devices.count()
            ))
        }
    }

    pub fn playout_device_name(
        &self,
        index: u16,
        name_out: webrtc::ptr::Borrowed<c_uchar>,
        guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> i32 {
        match self.enumerate_devices(DeviceType::OUTPUT) {
            Ok(devices) => {
                match AudioDeviceModule::copy_name_and_id(index, devices, name_out, guid_out) {
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
        &self,
        index: u16,
        name_out: webrtc::ptr::Borrowed<c_uchar>,
        guid_out: webrtc::ptr::Borrowed<c_uchar>,
    ) -> i32 {
        match self.enumerate_devices(DeviceType::INPUT) {
            Ok(devices) => {
                match AudioDeviceModule::copy_name_and_id(index, devices, name_out, guid_out) {
                    Ok(_) => 0,
                    Err(e) => {
                        error!("Failed to copy name and ID for recording device: {}", e);
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
        let device = match self.enumerate_devices(DeviceType::OUTPUT) {
            Ok(devices) => {
                for device in devices.iter() {
                    info!(
                        "Playout device: ({})",
                        AudioDeviceModule::device_str(device)
                    );
                }

                match devices.get(index as usize) {
                    Some(device) => device.devid(),
                    None => {
                        error!(
                            "Invalid device index {} requested (len {})",
                            index,
                            devices.count()
                        );
                        return -1;
                    }
                }
            }
            Err(e) => {
                error!("failed to enumerate devices for playout device: {}", e);
                return -1;
            }
        };
        self.playout_device = Some(device);
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
        let device = match self.enumerate_devices(DeviceType::INPUT) {
            Ok(devices) => {
                for device in devices.iter() {
                    info!(
                        "Recording device: ({})",
                        AudioDeviceModule::device_str(device)
                    );
                }
                match devices.get(index as usize) {
                    Some(device) => device.devid(),
                    None => {
                        error!(
                            "Invalid device index {} requested (len {})",
                            index,
                            devices.count()
                        );
                        return -1;
                    }
                }
            }
            Err(e) => {
                error!("failed to enumerate devices for playout device: {}", e);
                return -1;
            }
        };
        self.recording_device = Some(device);
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
        let available = self.initialized && self.playout_device.is_some();
        match write_to_null_or_valid_pointer(available_out, available) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing playout available state: {:?}", e);
                -1
            }
        }
    }

    pub fn init_playout(&mut self) -> i32 {
        if !self.initialized {
            error!("Tried to init playout without initializing ADM");
            return -1;
        }
        let out_device = if let Some(device) = self.playout_device {
            device
        } else {
            error!("Tried to init playout without a playout device");
            return -1;
        };
        let ctx = if let Some(c) = &self.cubeb_ctx {
            c
        } else {
            error!("Tried to init playout without a ctx");
            return -1;
        };
        let params = cubeb::StreamParamsBuilder::new()
            .format(STREAM_FORMAT)
            .rate(SAMPLE_FREQUENCY)
            .channels(NUM_CHANNELS)
            .layout(cubeb::ChannelLayout::MONO)
            .prefs(StreamPrefs::VOICE)
            .take();
        let mut builder = cubeb::StreamBuilder::<Frame>::new();
        let transport = Arc::clone(&self.audio_transport);
        let min_latency = ctx.min_latency(&params).unwrap_or_else(|e| {
            error!(
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
                    output[written] = Frame { m: data };
                    written += 1;
                    if written >= output.len() {
                        // Short-circuit; we already have enough data.
                        return output.len() as isize;
                    }
                }

                // Then, request more data from WebRTC.
                while written < output.len() {
                    let play_data = AudioDeviceModule::need_more_play_data(
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
                            output[written] = Frame { m: *data };
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
        match builder.init(ctx) {
            Ok(stream) => {
                self.output_stream = Some(stream);
                0
            }
            Err(e) => {
                error!("Couldn't initialize output stream: {}", e);
                -1
            }
        }
    }

    pub fn playout_is_initialized(&self) -> bool {
        self.output_stream.is_some()
    }

    pub fn recording_is_available(&self, available_out: webrtc::ptr::Borrowed<bool>) -> i32 {
        let available = self.initialized && self.recording_device.is_some();
        match write_to_null_or_valid_pointer(available_out, available) {
            Ok(_) => 0,
            Err(e) => {
                error!("writing recording available state: {:?}", e);
                -1
            }
        }
    }

    pub fn init_recording(&mut self) -> i32 {
        if !self.initialized {
            error!("Tried to init recording without initializing ADM");
            return -1;
        }
        let recording_device = if let Some(device) = self.recording_device {
            device
        } else {
            error!("Tried to init recording without a recording device");
            return -1;
        };
        let ctx = if let Some(c) = &self.cubeb_ctx {
            c
        } else {
            error!("Tried to init recording without a ctx");
            return -1;
        };
        let params = cubeb::StreamParamsBuilder::new()
            .format(STREAM_FORMAT)
            .rate(SAMPLE_FREQUENCY)
            .channels(NUM_CHANNELS)
            .layout(cubeb::ChannelLayout::MONO)
            .prefs(StreamPrefs::VOICE)
            .take();
        let mut builder = cubeb::StreamBuilder::<Frame>::new();
        let transport = Arc::clone(&self.audio_transport);
        let min_latency = ctx.min_latency(&params).unwrap_or_else(|e| {
            error!(
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
                    let (ret, _new_mic_level) = AudioDeviceModule::recorded_data_is_available(
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
        match builder.init(ctx) {
            Ok(stream) => {
                match ctx.supported_input_processing_params() {
                    Ok(params) => {
                        info!("Available input processing params: {:?}", params);
                    }
                    Err(e) => warn!("Failed to get supported input processing parameters; proceeding without: {}", e)
                }
                self.input_stream = Some(stream);
                0
            }
            Err(e) => {
                error!("Couldn't initialize input stream: {}", e);
                -1
            }
        }
    }

    pub fn recording_is_initialized(&self) -> bool {
        self.input_stream.is_some()
    }

    // Audio transport control
    pub fn start_playout(&mut self) -> i32 {
        if let Some(output_stream) = &self.output_stream {
            if let Err(e) = output_stream.start() {
                error!("Failed to start playout: {}", e);
                return -1;
            }
            self.playing = true;
            0
        } else {
            error!("Cannot start playout without an output stream -- did you forget init_playout?");
            -1
        }
    }

    pub fn stop_playout(&mut self) -> i32 {
        if let Some(output_stream) = &self.output_stream {
            if let Err(e) = output_stream.stop() {
                error!("Failed to stop playout: {}", e);
                return -1;
            }
            // Drop the stream so that it isn't reused on future calls.
            self.output_stream = None;
            self.playing = false;
        }
        0
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn start_recording(&mut self) -> i32 {
        if let Some(input_stream) = &self.input_stream {
            if let Err(e) = input_stream.start() {
                error!("Failed to start recording: {}", e);
                return -1;
            }
            self.recording = true;
            0
        } else {
            error!(
                "Cannot start recording without an input stream -- did you forget init_recording?"
            );
            -1
        }
    }

    pub fn stop_recording(&mut self) -> i32 {
        if let Some(input_stream) = &self.input_stream {
            if let Err(e) = input_stream.stop() {
                error!("Failed to stop recording: {}", e);
                return -1;
            }
            // Drop the stream so that it isn't reused on future calls.
            self.input_stream = None;
            self.recording = false;
        }
        0
    }

    pub fn recording(&self) -> bool {
        self.recording
    }

    // Audio mixer initialization
    pub fn init_speaker(&self) -> i32 {
        if self.initialized {
            0
        } else {
            -1
        }
    }

    pub fn speaker_is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn init_microphone(&self) -> i32 {
        if self.initialized {
            0
        } else {
            -1
        }
    }

    pub fn microphone_is_initialized(&self) -> bool {
        self.initialized
    }

    // Speaker volume controls
    pub fn speaker_volume_is_available(&self, available: webrtc::ptr::Borrowed<bool>) -> i32 {
        if !self.initialized {
            return -1;
        }
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
        if !self.initialized {
            return -1;
        }
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
        if !self.initialized {
            return -1;
        }
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
        if !self.initialized {
            return -1;
        }
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
        if !self.initialized {
            return -1;
        }
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
        if !self.initialized {
            return -1;
        }
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
        match &self.output_stream {
            Some(output_stream) => {
                let latency_samples = output_stream.latency();
                match latency_samples {
                    Ok(latency_samples) => {
                        let latency_ms = latency_samples / (SAMPLE_FREQUENCY / 1000);
                        match write_to_null_or_valid_pointer(delay_ms, latency_ms as u16) {
                            Ok(_) => 0,
                            Err(e) => {
                                error!("writing delay: {:?}", e);
                                -1
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to get latency: {}", e);
                        -1
                    }
                }
            }
            None => {
                error!("Cannot get playout delay with no stream");
                -1
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
}
