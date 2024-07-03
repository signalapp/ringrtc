//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC FFI ADM interface.

use crate::webrtc;
use crate::webrtc::audio_device_module::{AudioDeviceModule, AudioLayer, WindowsDeviceType};
use libc::size_t;
use std::ffi::c_void;
use std::os::raw::c_char;

/// Incomplete type for C++ AudioTransport.
#[repr(C)]
pub struct RffiAudioTransport {
    _private: [u8; 0],
}

/// all_adm_functions is a higher-level macro that enables "tt muncher" macros
/// The list of functions MUST be kept in sync with AudioDeviceCallbacks in webrtc C++, and
/// in particular the order must match.
macro_rules! all_adm_functions {
    ($macro:ident) => {
        $macro!(
            active_audio_layer(audio_layer: webrtc::ptr::Borrowed<AudioLayer>) -> i32;

            register_audio_callback(audio_callback: webrtc::ptr::Borrowed<RffiAudioTransport>) -> i32;

            // Main initialization and termination
            init() -> i32;
            terminate() -> i32;
            initialized() -> bool;

            // Device enumeration
            playout_devices() -> i16;
            recording_devices() -> i16;
            playout_device_name(index: u16, name: webrtc::ptr::Borrowed<c_char>, guid: webrtc::ptr::Borrowed<c_char>) -> i32;
            recording_device_name(index: u16, name: webrtc::ptr::Borrowed<c_char>, guid: webrtc::ptr::Borrowed<c_char>) -> i32;

            // Device selection
            set_playout_device(index: u16) -> i32;
            set_playout_device_win(device: WindowsDeviceType) -> i32;

            set_recording_device(index: u16) -> i32;
            set_recording_device_win(device: WindowsDeviceType) -> i32;

            // Audio transport initialization
            playout_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            init_playout() -> i32;
            playout_is_initialized() -> bool;

            recording_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            init_recording() -> i32;
            recording_is_initialized() -> bool;

            // Audio transport control
            start_playout() -> i32;
            stop_playout() -> i32;

            playing() -> bool;
            start_recording() -> i32;
            stop_recording() -> i32;
            recording() -> bool;

            // Audio mixer initialization
            init_speaker() -> i32;
            speaker_is_initialized() -> bool;
            init_microphone() -> i32;
            microphone_is_initialized() -> bool;

            // Speaker volume controls
            speaker_volume_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            set_speaker_volume(volume: u32) -> i32;
            speaker_volume(volume: webrtc::ptr::Borrowed<u32>) -> i32;
            max_speaker_volume(max_volume: webrtc::ptr::Borrowed<u32>) -> i32;
            min_speaker_volume(min_volume: webrtc::ptr::Borrowed<u32>) -> i32;

            // Microphone volume controls
            microphone_volume_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            set_microphone_volume(volume: u32) -> i32;
            microphone_volume(volume: webrtc::ptr::Borrowed<u32>) -> i32;
            max_microphone_volume(max_volume: webrtc::ptr::Borrowed<u32>) -> i32;
            min_microphone_volume(min_volume: webrtc::ptr::Borrowed<u32>) -> i32;

            // Speaker mute control
            speaker_mute_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            set_speaker_mute(enable: bool) -> i32;
            speaker_mute(enabled: webrtc::ptr::Borrowed<bool>) -> i32;

            // Microphone mute control
            microphone_mute_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            set_microphone_mute(enable: bool) -> i32;
            microphone_mute(enabled: webrtc::ptr::Borrowed<bool>) -> i32;

            // Stereo support
            stereo_playout_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            set_stereo_playout(enable: bool) -> i32;
            stereo_playout(enabled: webrtc::ptr::Borrowed<bool>) -> i32;
            stereo_recording_is_available(available: webrtc::ptr::Borrowed<bool>) -> i32;
            set_stereo_recording(enable: bool) -> i32;
            stereo_recording(enabled: webrtc::ptr::Borrowed<bool>) -> i32;

            // Playout delay
            playout_delay(delay_ms: webrtc::ptr::Borrowed<u16>) -> i32;
        );
    }
}

// Enum used to tag failures due to the adm pointer being null
enum InternalFailure {
    NullPtr,
}
// Methods to convert rust-style errors into return types matching C/++ types
impl From<InternalFailure> for i32 {
    fn from(_failure: InternalFailure) -> i32 {
        -1
    }
}
impl From<InternalFailure> for i16 {
    fn from(_failure: InternalFailure) -> i16 {
        -1
    }
}
impl From<InternalFailure> for bool {
    fn from(_failure: InternalFailure) -> bool {
        false
    }
}

/// Generator macro for the full list of functions to be called by C++ rffi.
/// These dispatch to AudioDeviceModule.
/// Note that these functions are dispatched via pointers, and *not* called directly, so they don't
/// need to worry about name mangling or matching case with C++.
macro_rules! adm_wrapper {
    () => {};
    ($f:ident($($param:ident: $arg_ty:ty),*) -> $ret:ty ; $($t:tt)*) => {
        extern "C" fn $f(ptr: webrtc::ptr::Borrowed<AudioDeviceModule>, $($param: $arg_ty),*) -> $ret {
            debug!("{} wrapper", stringify!($f));
            if let Some(adm) = unsafe { ptr.as_mut() } {
                adm.$f($($param),*)
            } else {
                error!("{} wrapper with null pointer", stringify!($f));
                InternalFailure::NullPtr.into()
            }
        }
        adm_wrapper!($($t)*);
    }
}

// Actual generation of C-interface functions.
all_adm_functions!(adm_wrapper);

/// Generator macro for the struct type of function pointers. A pointer to this
/// struct is passed to the C++ rffi, so it's vital that the generated struct match
/// the same order as the struct in the C++.
macro_rules! adm_struct_definition {
    (struct AudioDeviceCallbacks { $($inner:tt)* } => ) => {
        #[repr(C)]
        #[allow(non_snake_case)]
         pub struct AudioDeviceCallbacks {
            $($inner)*
         }
    };
    (struct AudioDeviceCallbacks { $($inner:tt)* } => $f:ident($($param:ident: $arg_ty:ty),*) -> $ret:ty ; $($t:tt)*) => {
        adm_struct_definition!(struct AudioDeviceCallbacks {
            $($inner)*
            pub $f: extern "C" fn(
              adm_borrowed: webrtc::ptr::Borrowed<AudioDeviceModule>, $($param: $arg_ty),*) -> $ret,
        } => $($t)*);
    };
    ($f:ident($($param:ident: $arg_ty:ty),*) -> $ret:ty ; $($t:tt)*) => {
        adm_struct_definition!(struct AudioDeviceCallbacks {
          pub $f: extern "C" fn(
              adm_borrowed: webrtc::ptr::Borrowed<AudioDeviceModule>, $($param: $arg_ty),*) -> $ret,
        } => $($t)*);
    }
}

all_adm_functions!(adm_struct_definition);

/// Generator macro for the instantiation of the function pointer struct.
macro_rules! adm_struct_instantiation {
    (AudioDeviceCallbacks { $($inner:tt)* } => ) => {
        const AUDIO_DEVICE_CBS: AudioDeviceCallbacks = AudioDeviceCallbacks {
            $($inner)*
        };
    };
    (AudioDeviceCallbacks { $($inner:tt)* } => $f:ident($($_args:tt)*) -> $_ret:ty ; $($t:tt)*) => {
        adm_struct_instantiation!(
            AudioDeviceCallbacks {
                $($inner)*
                $f: crate::webrtc::ffi::audio_device_module::$f,
            } => $($t)*
        );
    };
    ($f:ident($($_args:tt)*) -> $_ret:ty ; $($t:tt)*) => {
        adm_struct_instantiation!(
            AudioDeviceCallbacks {
                $f: crate::webrtc::ffi::audio_device_module::$f,
            } => $($t)*
        );
    }
}

all_adm_functions!(adm_struct_instantiation);
pub const AUDIO_DEVICE_CBS_PTR: *const AudioDeviceCallbacks = &AUDIO_DEVICE_CBS;

extern "C" {
    pub fn Rust_recordedDataIsAvailable(
        audio_transport: webrtc::ptr::Borrowed<RffiAudioTransport>,
        audio_samples: *const c_void,
        n_samples: size_t,
        n_bytes_per_sample: size_t,
        n_channels: size_t,
        samples_per_sec: u32,
        total_delay_ms: u32,
        clock_drift: i32,
        current_mic_level: u32,
        key_pressed: bool,
        new_mic_level: *mut u32,
        estimated_capture_time_ns: i64,
    ) -> i32;

    pub fn Rust_needMorePlayData(
        audio_transport: webrtc::ptr::Borrowed<RffiAudioTransport>,
        n_samples: size_t,
        n_bytes_per_sample: size_t,
        n_channels: size_t,
        samples_per_sec: u32,
        audio_samples: *mut c_void,
        n_samples_out: *mut size_t,
        elapsed_time_ms: *mut i64,
        ntp_time_ms: *mut i64,
    ) -> i32;
}
