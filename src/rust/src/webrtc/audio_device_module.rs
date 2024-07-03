//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;
use crate::webrtc::ffi::audio_device_module::RffiAudioTransport;
use std::ffi::c_void;
use std::os::raw::c_char;
use std::time::Duration;

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

pub struct AudioDeviceModule {
    audio_transport: webrtc::ptr::Borrowed<RffiAudioTransport>,
}

impl Default for AudioDeviceModule {
    fn default() -> Self {
        Self {
            audio_transport: webrtc::ptr::Borrowed::null(),
        }
    }
}

impl AudioDeviceModule {
    pub fn new() -> Self {
        Self {
            audio_transport: webrtc::ptr::Borrowed::null(),
        }
    }

    pub fn active_audio_layer(&self, _audio_layer: webrtc::ptr::Borrowed<AudioLayer>) -> i32 {
        -1
    }

    pub fn register_audio_callback(
        &mut self,
        audio_transport: webrtc::ptr::Borrowed<RffiAudioTransport>,
    ) -> i32 {
        // It is unsafe to change this callback while playing or recording, as
        // the change might then race with invocations of the callback, which
        // need not be serialized.
        if self.playing() || self.recording() {
            return -1;
        }
        self.audio_transport = audio_transport;
        0
    }

    // Main initialization and termination
    pub fn init(&self) -> i32 {
        -1
    }
    pub fn terminate(&self) -> i32 {
        -1
    }
    pub fn initialized(&self) -> bool {
        false
    }

    // Device enumeration
    pub fn playout_devices(&self) -> i16 {
        -1
    }
    pub fn recording_devices(&self) -> i16 {
        -1
    }
    pub fn playout_device_name(
        &self,
        _index: u16,
        _name: webrtc::ptr::Borrowed<c_char>,
        _guid: webrtc::ptr::Borrowed<c_char>,
    ) -> i32 {
        -1
    }
    pub fn recording_device_name(
        &self,
        _index: u16,
        _name: webrtc::ptr::Borrowed<c_char>,
        _guid: webrtc::ptr::Borrowed<c_char>,
    ) -> i32 {
        -1
    }

    // Device selection
    pub fn set_playout_device(&self, _index: u16) -> i32 {
        -1
    }
    pub fn set_playout_device_win(&self, _device: WindowsDeviceType) -> i32 {
        -1
    }

    pub fn set_recording_device(&self, _index: u16) -> i32 {
        -1
    }
    pub fn set_recording_device_win(&self, _device: WindowsDeviceType) -> i32 {
        -1
    }

    // Audio transport initialization
    pub fn playout_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
    pub fn init_playout(&self) -> i32 {
        -1
    }
    pub fn playout_is_initialized(&self) -> bool {
        false
    }
    pub fn recording_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
    pub fn init_recording(&self) -> i32 {
        -1
    }
    pub fn recording_is_initialized(&self) -> bool {
        false
    }

    // Audio transport control
    pub fn start_playout(&self) -> i32 {
        -1
    }
    pub fn stop_playout(&self) -> i32 {
        -1
    }
    pub fn playing(&self) -> bool {
        false
    }
    pub fn start_recording(&self) -> i32 {
        -1
    }
    pub fn stop_recording(&self) -> i32 {
        -1
    }
    pub fn recording(&self) -> bool {
        false
    }

    // Audio mixer initialization
    pub fn init_speaker(&self) -> i32 {
        -1
    }
    pub fn speaker_is_initialized(&self) -> bool {
        false
    }
    pub fn init_microphone(&self) -> i32 {
        -1
    }
    pub fn microphone_is_initialized(&self) -> bool {
        false
    }

    // Speaker volume controls
    pub fn speaker_volume_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
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
    pub fn microphone_volume_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
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
    pub fn speaker_mute_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
    pub fn set_speaker_mute(&self, _enable: bool) -> i32 {
        -1
    }
    pub fn speaker_mute(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }

    // Microphone mute control
    pub fn microphone_mute_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
    pub fn set_microphone_mute(&self, _enable: bool) -> i32 {
        -1
    }
    pub fn microphone_mute(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }

    // Stereo support
    pub fn stereo_playout_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
    pub fn set_stereo_playout(&self, _enable: bool) -> i32 {
        -1
    }
    pub fn stereo_playout(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
    pub fn stereo_recording_is_available(&self, _available: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }
    pub fn set_stereo_recording(&self, _enable: bool) -> i32 {
        -1
    }
    pub fn stereo_recording(&self, _enabled: webrtc::ptr::Borrowed<bool>) -> i32 {
        -1
    }

    // Playout delay
    pub fn playout_delay(&self, _delay_ms: webrtc::ptr::Borrowed<u16>) -> i32 {
        -1
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    fn recorded_data_is_available(
        &self,
        samples: Vec<i16>,
        channels: usize,
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
        // * self.audio_transport is within self, and will remain valid while this function is running
        //   because we enforce that the callback cannot change while playing or recording.
        // * The vector has sizeof(i16) * samples bytes allocated, and we pass both of these
        //   to the C layer, which should not read beyond that bound.
        // * The local new_mic_level pointer is valid and this function is synchronous, so it'll
        //   remain valid while it runs.
        let ret = unsafe {
            crate::webrtc::ffi::audio_device_module::Rust_recordedDataIsAvailable(
                self.audio_transport,
                samples.as_ptr() as *const c_void,
                samples.len(),
                std::mem::size_of::<i16>(),
                channels,
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

    #[allow(dead_code)]
    fn need_more_play_data(
        &self,
        samples: usize,
        channels: usize,
        samples_per_sec: u32,
    ) -> PlayData {
        let mut data = vec![0i16; samples];
        let mut samples_out = 0usize;
        let mut elapsed_time_ms = 0i64;
        let mut ntp_time_ms = 0i64;

        // Safety:
        // * self.audio_transport is within self, and will remain valid while this function is running
        //   because we enforce that the callback cannot change while playing or recording.
        // * The vector has sizeof(i16) * samples bytes allocated, and we pass both of these
        //   to the C layer, which should not write beyond that bound.
        // * The local variable pointers are all valid and this function is synchronous, so they'll
        //   remain valid while it runs.
        let ret = unsafe {
            crate::webrtc::ffi::audio_device_module::Rust_needMorePlayData(
                self.audio_transport,
                samples,
                std::mem::size_of::<i16>(),
                channels,
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
