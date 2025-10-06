//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Utility functions for audio_device_module.rs
//! Nothing in here should depend on webrtc directly.

use std::ffi::{CString, c_uchar};

use anyhow::anyhow;
use cubeb::{DeviceCollection, DeviceState};
#[cfg(target_os = "linux")]
use cubeb_core::DeviceType;
use cubeb_core::{DeviceId, DevicePref};
use regex::Regex;

use crate::{webrtc, webrtc::peer_connection_factory::AudioDevice};

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct MinimalDeviceInfo {
    pub devid: DeviceId,
    pub device_id: Option<String>,
    pub friendly_name: String,
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
#[derive(PartialEq, Eq, Debug, Clone, Default)]
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
            if let Some(friendly) = device.friendly_name() {
                out.push(MinimalDeviceInfo {
                    devid: device.devid(),
                    device_id: device.device_id().as_ref().map(|s| s.to_string()),
                    friendly_name: friendly.to_string(),
                    #[cfg(target_os = "linux")]
                    device_type: device.device_type(),
                    preferred: device.preferred(),
                    state: device.state(),
                })
            } else {
                error!("Device {:?} has no friendly name", device.devid());
            }
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
        #[cfg(target_os = "linux")]
        let count = self.iter_non_monitor().count();
        if count == 0 {
            #[cfg(target_os = "macos")]
            return 0;
            #[cfg(target_os = "linux")]
            return
                // edge case: if there are only monitor devices, and one is the default,
                // allow it.
                if self.iter()
                    .any(|device| device.preferred.contains(DevicePref::VOICE)) {
                        1
                    } else {
                        0
                };
        } else {
            count + 1
        }
    }

    /// Extract all names and IDs, **including repetitions** for the default device(s)!
    pub fn extract_names(&self) -> Vec<Option<AudioDevice>> {
        // On mac and windows, this is relatively simple -- we get the count and then get each reported
        // device.
        #[cfg(not(target_os = "windows"))]
        let count = self.count();

        // On Windows, it's different: count does not include the defaults.
        #[cfg(target_os = "windows")]
        let count = self.count() + 2;

        let mut names = Vec::new();
        for i in 0..count {
            let info = if let Some(info) = self.get(i) {
                info
            } else {
                warn!("Internal error enumerating devices {} vs {}", i, count);
                names.push(None);
                continue;
            };
            let mut name_copy = info.friendly_name.clone();
            #[cfg(not(target_os = "windows"))]
            if i == 0 {
                name_copy = format!("default ({})", info.friendly_name);
            }
            #[cfg(target_os = "windows")]
            {
                if i == 0 {
                    name_copy = format!("Default - {}", info.friendly_name);
                } else if i == 1 {
                    name_copy = format!("Communication - {}", info.friendly_name);
                }
            }
            names.push(Some(AudioDevice {
                // For devices missing unique_id, populate them with name + index
                unique_id: info
                    .device_id
                    .clone()
                    .unwrap_or_else(|| format!("{}-{}", info.friendly_name, i)),
                name: name_copy,
                i18n_key: "".to_string(),
            }));
        }
        names
    }
}

/// Copy from |src| into |dest| at most |dest_size| - 1 bytes and write a nul terminator either after |src| or at the end of |dest_size|
pub fn copy_and_truncate_string(
    src: &str,
    mut dest: webrtc::ptr::Borrowed<c_uchar>,
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

/// Redact the given string |s| by retaining only a prefix, which is 4 characters
/// if the string is all ASCII and 1 otherwise.
pub fn redact_for_logging(s: &str) -> String {
    if cfg!(debug_assertions) && !cfg!(test) {
        // For debug testing/local builds only, allow the full string.
        s.to_string()
    } else {
        // Take a small number of characters, but fewer if they are non-ascii unicode, as
        // unicode provides a substantially higher amount of information per char.
        // (e.g. four mandarin characters could be a full name)
        let mut out: String = if s.is_ascii() {
            s.chars().take(4).collect()
        } else {
            s.chars().take(1).collect()
        };
        if out != s {
            out.push_str("...");
        }
        out
    }
}

/// Redact all capturing groups (except group 0) with |redact_for_logging|
/// if the regex matches. Otherwise, return None.
pub fn redact_by_regex(re: &Regex, s: &str) -> Option<String> {
    if re.is_match(s) {
        Some(
            re.replace(s, |caps: &regex::Captures| {
                let mut out = s.to_string();
                // Skip group 0 (the entire match)
                for group in caps.iter().skip(1).flatten() {
                    out = out.replace(group.as_str(), &redact_for_logging(group.as_str()));
                }
                out
            })
            .to_string(),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod audio_device_module_tests {
    #[cfg(target_os = "linux")]
    use cubeb_core::DeviceType;
    use lazy_static::lazy_static;

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

    #[test]
    fn redaction_tests() {
        assert_eq!(redact_for_logging("0123456789"), "0123...");
        assert_eq!(redact_for_logging("0123"), "0123");
        assert_eq!(redact_for_logging("0"), "0");
        assert_eq!(redact_for_logging("你好"), "你..."); // ni hao (hello)
        assert_eq!(redact_for_logging("你"), "你");
    }

    #[test]
    fn redaction_regex_tests() {
        lazy_static! {
            static ref ONE_RE: Regex =
                Regex::new(r"Device \d+ \((.*)\) has \d+.*channels").unwrap();
            static ref TWO_RE: Regex = Regex::new(r"Found matching device for (.*): (.*)").unwrap();
        }
        assert_eq!(
            redact_by_regex(
                &ONE_RE,
                "Device 12345 (My Super Sensitive Name) has 2 INPUT-channels"
            ),
            Some("Device 12345 (My S...) has 2 INPUT-channels".to_string())
        );
        // Should only redact matching strings
        assert_eq!(
            redact_by_regex(&ONE_RE, "Some other string with My Super Sensitive Name"),
            None
        );
        assert_eq!(
            redact_by_regex(
                &TWO_RE,
                "Found matching device for My Super Sensitive Name: My Super Sensitive Name"
            ),
            Some("Found matching device for My S...: My S...".to_string())
        );
        assert_eq!(
            redact_by_regex(
                &TWO_RE,
                "Found matching device for My Super Sensitive Name: My Other Sensitive Name"
            ),
            Some("Found matching device for My S...: My O...".to_string())
        );
    }

    #[test]
    fn extract_names_handles_normal() {
        let devs = DeviceCollectionWrapper {
            device_collection: vec![
                MinimalDeviceInfo {
                    devid: std::ptr::null(),
                    device_id: Some("devid1".to_string()),
                    friendly_name: "Device 1".to_string(),
                    #[cfg(target_os = "linux")]
                    device_type: DeviceType::INPUT,
                    preferred: DevicePref::all(),
                    state: DeviceState::Enabled,
                },
                MinimalDeviceInfo {
                    devid: std::ptr::null(),
                    device_id: Some("devid2".to_string()),
                    friendly_name: "Device 2".to_string(),
                    #[cfg(target_os = "linux")]
                    device_type: DeviceType::INPUT,
                    preferred: DevicePref::empty(),
                    state: DeviceState::Enabled,
                },
            ],
        };
        let names = DeviceCollectionWrapper::extract_names(&devs);

        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            names,
            vec![
                Some(AudioDevice {
                    name: "default (Device 1)".to_string(),
                    unique_id: "devid1".to_string(),
                    i18n_key: "".to_string(),
                }),
                Some(AudioDevice {
                    name: "Device 1".to_string(),
                    unique_id: "devid1".to_string(),
                    i18n_key: "".to_string(),
                }),
                Some(AudioDevice {
                    name: "Device 2".to_string(),
                    unique_id: "devid2".to_string(),
                    i18n_key: "".to_string(),
                })
            ]
        );

        #[cfg(target_os = "windows")]
        assert_eq!(
            names,
            vec![
                Some(AudioDevice {
                    name: "Default - Device 1".to_string(),
                    unique_id: "devid1".to_string(),
                    i18n_key: "".to_string(),
                }),
                Some(AudioDevice {
                    name: "Communication - Device 1".to_string(),
                    unique_id: "devid1".to_string(),
                    i18n_key: "".to_string(),
                }),
                Some(AudioDevice {
                    name: "Device 1".to_string(),
                    unique_id: "devid1".to_string(),
                    i18n_key: "".to_string(),
                }),
                Some(AudioDevice {
                    name: "Device 2".to_string(),
                    unique_id: "devid2".to_string(),
                    i18n_key: "".to_string(),
                })
            ]
        );
    }

    #[test]
    fn extract_names_handles_no_preferred_device() {
        let devs = DeviceCollectionWrapper {
            device_collection: vec![
                MinimalDeviceInfo {
                    devid: std::ptr::null(),
                    device_id: Some("devid1".to_string()),
                    friendly_name: "Device 1".to_string(),
                    #[cfg(target_os = "linux")]
                    device_type: DeviceType::INPUT,
                    preferred: DevicePref::empty(),
                    state: DeviceState::Enabled,
                },
                MinimalDeviceInfo {
                    devid: std::ptr::null(),
                    device_id: Some("devid2".to_string()),
                    friendly_name: "Device 2".to_string(),
                    #[cfg(target_os = "linux")]
                    device_type: DeviceType::INPUT,
                    preferred: DevicePref::empty(),
                    state: DeviceState::Enabled,
                },
            ],
        };
        let names = DeviceCollectionWrapper::extract_names(&devs);

        assert_eq!(
            names,
            vec![
                None,
                // Windows expects an extra communication device.
                #[cfg(target_os = "windows")]
                None,
                Some(AudioDevice {
                    name: "Device 1".to_string(),
                    unique_id: "devid1".to_string(),
                    i18n_key: "".to_string(),
                }),
                Some(AudioDevice {
                    name: "Device 2".to_string(),
                    unique_id: "devid2".to_string(),
                    i18n_key: "".to_string(),
                })
            ]
        )
    }
}
