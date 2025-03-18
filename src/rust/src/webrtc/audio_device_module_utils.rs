//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Utility functions for audio_device_module.rs
//! Nothing in here should depend on webrtc directly.

use std::ffi::{c_uchar, c_void, CString};

use anyhow::anyhow;
use cubeb::{DeviceCollection, DeviceState};
use cubeb_core::DevicePref;
#[cfg(target_os = "linux")]
use cubeb_core::DeviceType;

use crate::webrtc;

#[derive(PartialEq, Eq, Debug)]
pub struct MinimalDeviceInfo {
    pub devid: *const c_void,
    pub device_id: Option<String>,
    pub friendly_name: Option<String>,
    #[cfg(target_os = "linux")]
    device_type: DeviceType,
    preferred: DevicePref,
    state: DeviceState,
}

/// Wrapper struct for DeviceCollection that handles default devices.
///
/// Rather than storing the DeviceCollection directly, which raises complex
/// lifetime issues, store just the fields we need.
///
/// Note that, in some cases, `devid` may be a pointer to state in the cubeb ctx,
/// so in no event should this outlive the associated ctx.
#[derive(PartialEq, Eq, Debug)]
pub struct DeviceCollectionWrapper {
    device_collection: Vec<MinimalDeviceInfo>,
}

#[cfg(target_os = "linux")]
fn device_is_monitor(device: &MinimalDeviceInfo) -> bool {
    device.device_type == DeviceType::INPUT
        && device
            .device_id
            .as_ref()
            .is_some_and(|s| s.ends_with(".monitor"))
}

impl DeviceCollectionWrapper {
    pub fn new(device_collection: &DeviceCollection<'_>) -> DeviceCollectionWrapper {
        let mut out = Vec::new();
        for device in device_collection.iter() {
            out.push(MinimalDeviceInfo {
                devid: device.devid(),
                device_id: device.device_id().as_ref().map(|s| s.to_string()),
                friendly_name: device.friendly_name().as_ref().map(|s| s.to_string()),
                #[cfg(target_os = "linux")]
                device_type: device.device_type(),
                preferred: device.preferred(),
                state: device.state(),
            })
        }
        DeviceCollectionWrapper {
            device_collection: out,
        }
    }

    /// Iterate over all Enabled devices (those that are plugged in and not disabled by the OS)
    pub fn iter(
        &self,
    ) -> std::iter::Filter<std::slice::Iter<'_, MinimalDeviceInfo>, fn(&&MinimalDeviceInfo) -> bool>
    {
        self.device_collection
            .iter()
            .filter(|d| d.state == DeviceState::Enabled)
    }

    // For linux only, a method that will ignore "monitor" devices.
    #[cfg(target_os = "linux")]
    pub fn iter_non_monitor(
        &self,
    ) -> std::iter::Filter<std::slice::Iter<'_, MinimalDeviceInfo>, fn(&&MinimalDeviceInfo) -> bool>
    {
        self.device_collection
            .iter()
            .filter(|&d| d.state == DeviceState::Enabled && !device_is_monitor(d))
    }

    #[cfg(target_os = "windows")]
    /// Get a specified device index, accounting for the two default devices.
    pub fn get(&self, idx: usize) -> Option<&MinimalDeviceInfo> {
        // 0 should be "default device" and 1 should be "default communications device".
        // Note: On windows, CUBEB_DEVICE_PREF_VOICE will be set for default communications device,
        // and CUBEB_DEVICE_PREF_MULTIMEDIA | CUBEB_DEVICE_PREF_NOTIFICATION for default device.
        // https://github.com/mozilla/cubeb/blob/bbbe5bb0b29ed64cc7dd191d7a72fe24bba0d284/src/cubeb_wasapi.cpp#L3322
        if self.count() == 0 {
            None
        } else if idx > 1 {
            self.iter().nth(idx - 2)
        } else if idx == 1 {
            // Find a device that's preferred for VOICE -- device 1 is the "default communications"
            self.iter()
                .find(|&device| device.preferred.contains(DevicePref::VOICE))
        } else {
            // Find a device that's preferred for MULTIMEDIA -- device 0 is the "default"
            self.iter()
                .find(|&device| device.preferred.contains(DevicePref::MULTIMEDIA))
        }
    }

    #[cfg(not(target_os = "windows"))]
    /// Get a specified device index, accounting for the default device.
    pub fn get(&self, idx: usize) -> Option<&MinimalDeviceInfo> {
        if self.count() == 0 {
            None
        } else if idx > 0 {
            #[cfg(target_os = "macos")]
            {
                self.iter().nth(idx - 1)
            }
            #[cfg(target_os = "linux")]
            {
                // filter out "monitor" devices.
                self.iter_non_monitor().nth(idx - 1)
            }
        } else {
            // Find a device that's preferred for VOICE -- device 0 is the "default"
            // Even on linux, we do *NOT* filter monitor devices -- if the user specified that as
            // default, we respect it.
            self.iter()
                .find(|&device| device.preferred.contains(DevicePref::VOICE))
        }
    }

    #[cfg(target_os = "windows")]
    /// Returns the number of devices.
    /// Note: On Windows, this is 2 smaller than the number of addressable
    /// devices, because the default device and default communications device
    /// are not counted.
    pub fn count(&self) -> usize {
        self.iter().count()
    }

    #[cfg(not(target_os = "windows"))]
    /// Returns the number of devices, counting the default device.
    pub fn count(&self) -> usize {
        #[cfg(target_os = "macos")]
        let count = self.iter().count();
        // Whether a monitor device is default or not, there will be an additional default,
        // so no need to do anything different.
        #[cfg(target_os = "linux")]
        let count = self.iter_non_monitor().count();
        if count == 0 {
            0
        } else {
            count + 1
        }
    }
}

/// Copy from |src| into |dest| at most |dest_size| - 1 bytes and write a nul terminator either after |src| or at the end of |dest_size|
pub fn copy_and_truncate_string(
    src: &str,
    dest: webrtc::ptr::Borrowed<c_uchar>,
    dest_size: usize,
) -> anyhow::Result<()> {
    // Leave room for the nul terminator.
    let size = std::cmp::min(src.len(), dest_size - 1);
    let c_str = CString::new(src.get(0..size).ok_or(anyhow!("couldn't get substring"))?)?;
    let c_str_bytes = c_str.as_bytes_with_nul();
    // Safety: dest has at least |dest_size| bytes allocated, and we won't
    // write any more than that. In addition, we are copying from a slice that
    // includes the nul-terminator, and we are not copying beyond the end of that
    // slice.
    unsafe {
        std::ptr::copy(
            c_str_bytes.as_ptr(),
            std::ptr::from_mut(
                dest.as_mut()
                    .ok_or(anyhow!("couldn't get mutable pointer"))?,
            ),
            c_str_bytes.len(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod audio_device_module_tests {
    use super::*;
    #[test]
    // Verify that extremely long strings are properly truncated and
    // nul-terminated
    fn copy_and_truncate_long_string() {
        let data = vec![0xaau8; 10];
        let src = String::from_iter(['A'; 20]); // longer than data
        let out = webrtc::ptr::Borrowed::from_ptr(data.as_ptr());
        copy_and_truncate_string(&src, out, data.len()).unwrap();
        let mut expected = vec![0x41u8; 9]; // 'A'
        expected.push(0);
        assert_eq!(data, expected);
    }

    #[test]
    // Ensure that we do not read past the end of `src`
    fn copy_and_truncate_short_string() {
        let data = vec![0xaau8; 10];
        let src = String::from_iter(['A'; 4]); // shorter than data
        let out = webrtc::ptr::Borrowed::from_ptr(data.as_ptr());
        copy_and_truncate_string(&src, out, data.len()).unwrap();
        let expected = vec![0x41u8, 0x41, 0x41, 0x41, 0x0, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa];
        assert_eq!(data, expected);
    }

    #[test]
    // Check for off-by-one errors
    fn copy_and_truncate_max_len_string() {
        let data = vec![0xaau8; 10];
        let src = String::from_iter(['A'; 10]); // equal length to data
        let out = webrtc::ptr::Borrowed::from_ptr(data.as_ptr());
        copy_and_truncate_string(&src, out, data.len()).unwrap();
        let mut expected = vec![0x41u8; 9]; // 'A'
        expected.push(0);
        assert_eq!(data, expected);
    }

    #[test]
    // Check for off-by-one errors
    fn copy_and_truncate_barely_short_string() {
        let data = vec![0xaau8; 10];
        let src = String::from_iter(['A'; 9]); // one shorter than data
        let out = webrtc::ptr::Borrowed::from_ptr(data.as_ptr());
        copy_and_truncate_string(&src, out, data.len()).unwrap();
        let mut expected = vec![0x41u8; 9]; // 'A'
        expected.push(0);
        assert_eq!(data, expected);
    }

    #[test]
    // Check for overwrite errors
    fn copy_no_overwrite() {
        let data = vec![0xaau8; 10];
        let src = String::from_iter(['A'; 20]); // longer than data
        let out = webrtc::ptr::Borrowed::from_ptr(data.as_ptr());
        // State that data has one fewer byte than it actually does to make sure
        // the function doesn't write past the end.
        copy_and_truncate_string(&src, out, data.len() - 1).unwrap();
        let mut expected = vec![0x41u8; 8]; // 'A'
        expected.push(0);
        expected.push(0xaa);
        assert_eq!(data, expected);
    }

    #[test]
    // Verify that a string with internal nul characters is handled gracefully.
    fn string_with_nuls() {
        let data = vec![0xaau8; 10];
        let src = "a\0b";
        let out = webrtc::ptr::Borrowed::from_ptr(data.as_ptr());
        assert!(copy_and_truncate_string(src, out, data.len() - 1).is_err());
        // data should be untouched
        assert_eq!(data, vec![0xaau8; 10]);
    }

    #[test]
    // Verify that a null dest pointer is handled gracefully
    fn null_ptr() {
        let src = "AA";
        let out = webrtc::ptr::Borrowed::null();
        assert!(copy_and_truncate_string(src, out, 5).is_err());
    }
}
