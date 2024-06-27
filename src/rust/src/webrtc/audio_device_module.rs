//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use crate::webrtc;
use std::os::raw::c_char;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioDeviceModule {
    // TODO(mutexlox): Implement.
}

unsafe impl Send for AudioDeviceModule {}
unsafe impl Sync for AudioDeviceModule {}

impl AudioDeviceModule {
    pub fn active_audio_layer(&self, _audio_layer: webrtc::ptr::Borrowed<AudioLayer>) -> i32 {
        -1
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
}
