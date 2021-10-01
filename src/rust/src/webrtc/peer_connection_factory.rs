//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Peer Connection

use std::ffi::CString;
use std::fmt;
use std::os::raw::c_char;

use crate::common::Result;
use crate::core::util::CppObject;
use crate::error::RingRtcError;
use crate::webrtc;
#[cfg(feature = "simnet")]
use crate::webrtc::injectable_network::InjectableNetwork;
use crate::webrtc::media::{AudioTrack, VideoSource, VideoTrack};
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::peer_connection_observer::{
    PeerConnectionObserver,
    PeerConnectionObserverTrait,
};

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection_factory as pcf;
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count;

// TODO: sim::pcf
#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection_factory as pcf;
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count;

pub use pcf::{RffiPeerConnectionFactoryInterface, RffiPeerConnectionFactoryOwner};

#[cfg(target_os = "windows")] // For the default ADM.
const DEFAULT_COMMUNICATION_DEVICE_INDEX: u16 = 0xFFFF;

#[cfg(feature = "native")]
const ADM_MAX_DEVICE_NAME_SIZE: usize = 128;
#[cfg(feature = "native")]
const ADM_MAX_DEVICE_UUID_SIZE: usize = 128;

/// Rust wrapper around WebRTC C++ RTCCertificate object.
pub struct Certificate {
    rffi: *const pcf::RffiCertificate,
}

impl Certificate {
    pub fn generate() -> Result<Certificate> {
        let rffi = unsafe { pcf::Rust_generateCertificate() };
        if rffi.is_null() {
            return Err(RingRtcError::GenerateCertificate.into());
        }
        Ok(Self { rffi })
    }

    pub fn compute_fingerprint_sha256(&self) -> Result<[u8; 32]> {
        let mut fingerprint = [0u8; 32];
        let ok =
            unsafe { pcf::Rust_computeCertificateFingerprintSha256(self.rffi, &mut fingerprint) };
        if !ok {
            return Err(RingRtcError::ComputeCertificateFingerprint.into());
        }
        Ok(fingerprint)
    }

    pub fn rffi(&self) -> *const pcf::RffiCertificate {
        self.rffi
    }
}

impl fmt::Display for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Certificate: {:p}", self.rffi)
    }
}

impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for Certificate {
    fn drop(&mut self) {
        debug!("Certificate::drop()");
        if !self.rffi.is_null() {
            ref_count::release_ref(self.rffi as CppObject);
        }
    }
}

impl Clone for Certificate {
    fn clone(&self) -> Self {
        debug!("Certificate::clone() {}", self.rffi as u64);
        if !self.rffi.is_null() {
            ref_count::add_ref(self.rffi as CppObject);
        }
        Self { rffi: self.rffi }
    }
}

unsafe impl Send for Certificate {}
unsafe impl Sync for Certificate {}

#[repr(C)]
pub struct RffiIceServer {
    pub username:  *const c_char,
    pub password:  *const c_char,
    pub urls:      *const *const c_char,
    pub urls_size: usize,
}

#[derive(Clone, Debug)]
pub struct IceServer {
    username: CString,
    password: CString,
    // To own the strings
    urls:     Vec<CString>,
    // To hand the strings to C
    url_ptrs: Vec<*const c_char>,
}

unsafe impl Send for IceServer {}
unsafe impl Sync for IceServer {}

impl IceServer {
    pub fn new(username: String, password: String, urls_in: Vec<String>) -> Self {
        let mut urls = Vec::new();
        for url in urls_in {
            urls.push(CString::new(url).expect("CString of URL"));
        }
        let url_ptrs = urls.iter().map(|s| s.as_ptr()).collect();
        Self {
            username: CString::new(username).expect("CString of username"),
            password: CString::new(password).expect("CString of password"),
            urls,
            url_ptrs,
        }
    }

    pub fn none() -> Self {
        // In the FFI C++, no urls means no IceServer is added
        Self::new(
            "".to_string(), // username
            "".to_string(), // password
            vec![],         // urls
        )
    }

    pub fn rffi(&self) -> RffiIceServer {
        RffiIceServer {
            username:  self.username.as_ptr(),
            password:  self.password.as_ptr(),
            urls:      self.url_ptrs.as_ptr(),
            urls_size: self.url_ptrs.len(),
        }
    }
}

/// Describes an audio input or output device.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioDevice {
    /// Name of the device
    pub name:      String,
    /// Unique ID - truly unique on Windows, best effort on other platforms.
    pub unique_id: String,
    /// If the name requires translation, the translated string identifier.
    pub i18n_key:  String,
}

#[cfg(target_os = "windows")] // For the default ADM.
impl AudioDevice {
    fn default() -> AudioDevice {
        AudioDevice {
            name:      "Default".to_string(),
            unique_id: "Default".to_string(),
            i18n_key:  "default_communication_device".to_string(),
        }
    }
}

/// Rust wrapper around WebRTC C++ AudioDeviceModule object.
#[derive(Debug)]
pub struct AudioDeviceModule {
    rffi: webrtc::Arc<pcf::RffiAudioDeviceModule>,
}

impl AudioDeviceModule {
    pub fn new(rffi: webrtc::Arc<pcf::RffiAudioDeviceModule>) -> Self {
        Self { rffi, }
    }
}

/// Rust wrapper around WebRTC C++ PeerConnectionFactory object.
#[derive(Clone)]
pub struct PeerConnectionFactory {
    rffi: webrtc::Arc<RffiPeerConnectionFactoryOwner>,
    use_new_audio_device_module: bool,
}

impl fmt::Display for PeerConnectionFactory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PeerConnectionFactory: {:p}", self.rffi.as_borrowed_ptr())
    }
}

impl fmt::Debug for PeerConnectionFactory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}


#[derive(Default)]
pub struct Config {
    pub use_new_audio_device_module: bool,
    pub use_injectable_network: bool,
}

impl PeerConnectionFactory {
    /// Create a new Rust PeerConnectionFactory object from a WebRTC C++
    /// PeerConnectionFactory object.
    pub fn new(config: Config) -> Result<Self> {
        debug!("PeerConnectionFactory::new()");

        let (rffi, use_new_audio_device_module) = {
            let use_new_audio_device_module = config.use_new_audio_device_module;
            let rffi = webrtc::Arc::from_owned_ptr(unsafe {
                 pcf::Rust_createPeerConnectionFactory(
                     config.use_new_audio_device_module,
                     config.use_injectable_network) 
            });

            #[cfg(target_os = "windows")]
            if use_new_audio_device_module {
                info!("PeerConnectionFactory::new(): Using the new ADM for Windows");
            } else {
                info!("PeerConnectionFactory::new(): Using the default ADM for Windows");
            }

            (rffi, use_new_audio_device_module)
        };
        if rffi.is_null() {
            return Err(RingRtcError::CreatePeerConnectionFactory.into());
        }
        Ok(Self { rffi, use_new_audio_device_module })
    }

    /// Wrap an existing C++ PeerConnectionFactory (not a PeerConnectionFactoryOwner).
    ///
    /// # Safety
    ///
    /// `native` must point to a C++ PeerConnectionFactory.
    pub unsafe fn from_native_factory(
        native: *const RffiPeerConnectionFactoryInterface
    ) -> Self {
        let rffi = webrtc::Arc::from_owned_ptr(
            pcf::Rust_createPeerConnectionFactoryWrapper(native)
        );
        Self { rffi, use_new_audio_device_module: false }
    }

    #[cfg(feature = "simnet")]
    pub fn injectable_network(&self) -> Option<InjectableNetwork> {
        let rffi = unsafe { pcf::Rust_getInjectableNetwork(self.rffi.as_borrowed_ptr()) };
        if rffi.is_null() {
            return None;
        }
        Some(InjectableNetwork::new(rffi, self))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_peer_connection<T: PeerConnectionObserverTrait>(
        &self,
        observer: PeerConnectionObserver<T>,
        certificate: Certificate,
        hide_ip: bool,
        ice_servers: &IceServer,
        outgoing_audio_track: AudioTrack,
        outgoing_video_track: Option<VideoTrack>,
        enable_dtls: bool,
        enable_rtp_data_channel: bool,
    ) -> Result<PeerConnection> {
        debug!(
            "PeerConnectionFactory::create_peer_connection() {:p}",
            self.rffi.as_borrowed_ptr()
        );
        let rffi = webrtc::Arc::from_owned_ptr(unsafe {
            pcf::Rust_createPeerConnection(
                self.rffi.as_borrowed_ptr(),
                observer.rffi(),
                certificate.rffi(),
                hide_ip,
                ice_servers.rffi(),
                outgoing_audio_track.rffi(),
                outgoing_video_track.map_or_else(std::ptr::null, |outgoing_video_track| {
                    outgoing_video_track.rffi()
                }),
                enable_dtls,
                enable_rtp_data_channel,
            )
        });
        debug!(
            "PeerConnectionFactory::create_peer_connection() finished: {:p}",
            rffi.as_borrowed_ptr()
        );
        if rffi.is_null() {
            return Err(RingRtcError::CreatePeerConnection.into());
        }
        Ok(PeerConnection::new(rffi, observer.rffi(), Some(self.rffi.clone())))
    }

    pub fn create_outgoing_audio_track(&self) -> Result<AudioTrack> {
        debug!("PeerConnectionFactory::create_outgoing_audio_track()");
        let rffi = unsafe { pcf::Rust_createAudioTrack(self.rffi.as_borrowed_ptr()) };
        if rffi.is_null() {
            return Err(RingRtcError::CreateAudioTrack.into());
        }
        Ok(AudioTrack::owned(rffi))
    }

    pub fn create_outgoing_video_source(&self) -> Result<VideoSource> {
        debug!("PeerConnectionFactory::create_outgoing_video_source()");
        let rffi = unsafe { pcf::Rust_createVideoSource(self.rffi.as_borrowed_ptr()) };
        if rffi.is_null() {
            return Err(RingRtcError::CreateVideoSource.into());
        }
        Ok(VideoSource::new(rffi))
    }

    // We take ownership of the VideoSource because Rust_createVideoTrack takes ownership
    // of one takes ownership of one ref count to the source.
    pub fn create_outgoing_video_track(
        &self,
        outgoing_video_source: &VideoSource,
    ) -> Result<VideoTrack> {
        debug!("PeerConnectionFactory::create_outgoing_video_track()");
        let rffi = unsafe { pcf::Rust_createVideoTrack(self.rffi.as_borrowed_ptr(), outgoing_video_source.rffi()) };
        if rffi.is_null() {
            return Err(RingRtcError::CreateVideoTrack.into());
        }
        Ok(VideoTrack::owned(rffi))
    }

    #[cfg(feature = "native")]
    fn get_audio_playout_device(&self, index: u16) -> Result<AudioDevice> {
        let (name, unique_id, rc) = unsafe {
            let name = CString::from_vec_unchecked(vec![0u8; ADM_MAX_DEVICE_NAME_SIZE]).into_raw();
            let unique_id =
                CString::from_vec_unchecked(vec![0u8; ADM_MAX_DEVICE_UUID_SIZE]).into_raw();
            let rc = pcf::Rust_getAudioPlayoutDeviceName(self.rffi.as_borrowed_ptr(), index, name, unique_id);
            // Take back ownership of the raw pointers before checking for errors.
            let name = CString::from_raw(name);
            let unique_id = CString::from_raw(unique_id);
            (name, unique_id, rc)
        };
        if rc != 0 {
            error!("getAudioPlayoutDeviceName({}) failed: {}", index, rc);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        let name = name.into_string()?;
        let unique_id = unique_id.into_string()?;
        Ok(AudioDevice {
            name,
            unique_id,
            i18n_key: "".to_string(),
        })
    }

    #[cfg(feature = "native")]
    pub fn get_audio_playout_devices(&self) -> Result<Vec<AudioDevice>> {
        let device_count = unsafe { pcf::Rust_getAudioPlayoutDevices(self.rffi.as_borrowed_ptr()) };
        if device_count < 0 {
            error!("getAudioPlayoutDevices() returned {}", device_count);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        let device_count = device_count as u16;
        let mut devices = Vec::<AudioDevice>::new();

        #[cfg(target_os = "windows")]
        let device_count = if self.use_new_audio_device_module {
            // For the new ADM, if there is at least one real device, add slots
            // for the "default" and "default communications" device. When setting,
            // the new ADM already has them, but doesn't include them in the count.
            if device_count > 0 {
                device_count + 2
            } else {
                0
            }
        } else {
            // For the default ADM, add a slot for the "default" device.
            devices.push(AudioDevice::default());
            device_count
        };

        info!("PeerConnectionFactory::get_audio_playout_devices(): device_count: {}", device_count);

        for i in 0..device_count {
            match self.get_audio_playout_device(i) {
                Ok(dev) => devices.push(dev),
                Err(fail) => {
                    error!("getAudioPlayoutDevice({}) failed: {}", i, fail);
                    return Err(fail);
                }
            }
        }
        // For devices missing unique_id, populate them with name + index
        for i in 0..devices.len() {
            if devices[i].unique_id.is_empty() {
                let same_name_count = devices[..i]
                    .iter()
                    .filter(|d| d.name == devices[i].name)
                    .count() as u16;
                devices[i].unique_id = format!("{}-{}", devices[i].name, same_name_count);
            }
        }

        #[cfg(target_os = "windows")]
        if self.use_new_audio_device_module && devices.len() > 1 {
            // For the new ADM, swap the first two devices, so that the
            // "default communications" device is first and the "default"
            // device is second. The UI treats the first index as the
            // default, which for VoIP we prefer communications devices.
            devices.swap(0, 1);

            // Also, give both of those artificial slots unique ids so that
            // the UI can manage them correctly.
            devices[0].unique_id.push_str("-0");
            devices[1].unique_id.push_str("-1");
        }

        Ok(devices)
    }

    #[cfg(all(feature = "native", target_os = "windows"))] // For the default ADM.
    fn get_default_playout_device_index(&self) -> Result<u16> {
        let default_device = self.get_audio_playout_device(DEFAULT_COMMUNICATION_DEVICE_INDEX)?;
        let all_devices = self.get_audio_playout_devices()?;
        if let Some(index) = all_devices.iter().position(|d| d == &default_device) {
            Ok((index - 1) as u16)
        } else {
            error!("get_default_playout_device_index: Default communication device is not present in the list of all devices");
            Err(RingRtcError::QueryAudioDevices.into())
        }
    }

    #[cfg(feature = "native")]
    pub fn set_audio_playout_device(&self, index: u16) -> Result<()> {
        #[cfg(target_os = "windows")]
        let index = if self.use_new_audio_device_module {
            // For the new ADM, swap the first two devices back to ordinal if
            // either are selected.
            match index {
                0 => 1,
                1 => 0,
                _ => index,
            }
        } else {
            // For the default ADM, if the default device is selected, find the
            // actual device index it represents.
            if index == 0 {
                if let Ok(default_device) = self.get_default_playout_device_index() {
                    default_device
                } else {
                    0
                }
            } else {
                // Account for device 0 being the synthetic "Default"
                index - 1
            }
        };

        info!("PeerConnectionFactory::set_audio_playout_device({})", index);

        let ok = unsafe { pcf::Rust_setAudioPlayoutDevice(self.rffi.as_borrowed_ptr(), index) };
        if ok {
            Ok(())
        } else {
            error!("setAudioPlayoutDevice({}) failed", index);
            Err(RingRtcError::SetAudioDevice.into())
        }
    }

    #[cfg(feature = "native")]
    fn get_audio_recording_device(&self, index: u16) -> Result<AudioDevice> {
        let (name, unique_id, rc) = unsafe {
            let name = CString::from_vec_unchecked(vec![0u8; ADM_MAX_DEVICE_NAME_SIZE]).into_raw();
            let unique_id =
                CString::from_vec_unchecked(vec![0u8; ADM_MAX_DEVICE_UUID_SIZE]).into_raw();
            let rc = pcf::Rust_getAudioRecordingDeviceName(self.rffi.as_borrowed_ptr(), index, name, unique_id);
            // Take back ownership of the raw pointers before checking for errors.
            let name = CString::from_raw(name);
            let unique_id = CString::from_raw(unique_id);
            (name, unique_id, rc)
        };
        if rc != 0 {
            error!("getAudioRecordingDeviceName({}) failed: {}", index, rc);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        let name = name.into_string()?;
        let unique_id = unique_id.into_string()?;
        Ok(AudioDevice {
            name,
            unique_id,
            i18n_key: "".to_string(),
        })
    }

    #[cfg(feature = "native")]
    pub fn get_audio_recording_devices(&self) -> Result<Vec<AudioDevice>> {
        let device_count = unsafe { pcf::Rust_getAudioRecordingDevices(self.rffi.as_borrowed_ptr()) };
        if device_count < 0 {
            error!("getAudioRecordingDevices() returned {}", device_count);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        let device_count = device_count as u16;
        let mut devices = Vec::<AudioDevice>::new();

        #[cfg(target_os = "windows")]
        let device_count = if self.use_new_audio_device_module {
            // For the new ADM, if there is at least one real device, add slots
            // for the "default" and "default communications" device. When setting,
            // the new ADM already has them, but doesn't include them in the count.
            if device_count > 0 {
                device_count + 2
            } else {
                0
            }
        } else {
            // For the default ADM, add a slot for the "default" device.
            devices.push(AudioDevice::default());
            device_count
        };

        info!("PeerConnectionFactory::get_audio_recording_devices(): device_count: {}", device_count);

        for i in 0..device_count {
            match self.get_audio_recording_device(i) {
                Ok(dev) => devices.push(dev),
                Err(fail) => {
                    error!("getAudioRecordingDevice({}) failed: {}", i, fail);
                    return Err(fail);
                }
            }
        }
        // For devices missing unique_id, populate them with name + index
        for i in 0..devices.len() {
            if devices[i].unique_id.is_empty() {
                let same_name_count = devices[..i]
                    .iter()
                    .filter(|d| d.name == devices[i].name)
                    .count() as u16;
                devices[i].unique_id = format!("{}-{}", devices[i].name, same_name_count);
            }
        }

        #[cfg(target_os = "windows")]
        if self.use_new_audio_device_module && devices.len() > 1 {
            // For the new ADM, swap the first two devices, so that the
            // "default communications" device is first and the "default"
            // device is second. The UI treats the first index as the
            // default, which for VoIP we prefer communications devices.
            devices.swap(0, 1);

            // Also, give both of those artificial slots unique ids so that
            // the UI can manage them correctly.
            devices[0].unique_id.push_str("-0");
            devices[1].unique_id.push_str("-1");
        }

        Ok(devices)
    }

    #[cfg(all(feature = "native", target_os = "windows"))] // For the default ADM.
    fn get_default_recording_device_index(&self) -> Result<u16> {
        let default_device = self.get_audio_recording_device(DEFAULT_COMMUNICATION_DEVICE_INDEX)?;
        let all_devices = self.get_audio_recording_devices()?;
        if let Some(index) = all_devices.iter().position(|d| d == &default_device) {
            Ok((index - 1) as u16)
        } else {
            error!("get_default_recording_device_index: Default communication device is not present in the list of all devices");
            Err(RingRtcError::QueryAudioDevices.into())
        }
    }

    #[cfg(feature = "native")]
    pub fn set_audio_recording_device(&self, index: u16) -> Result<()> {
        #[cfg(target_os = "windows")]
        let index = if self.use_new_audio_device_module {
            // For the new ADM, swap the first two devices back to ordinal if
            // either are selected.
            match index {
                0 => 1,
                1 => 0,
                _ => index,
            }
        } else {
            // For the default ADM, if the default device is selected, find the
            // actual device index it represents.
            if index == 0 {
                if let Ok(default_device) = self.get_default_recording_device_index() {
                    default_device
                } else {
                    0
                }
            } else {
                // Account for device 0 being the synthetic "Default"
                index - 1
            }
        };

        info!("PeerConnectionFactory::set_audio_recording_device({})", index);

        let ok = unsafe { pcf::Rust_setAudioRecordingDevice(self.rffi.as_borrowed_ptr(), index) };
        if ok {
            Ok(())
        } else {
            error!("setAudioRecordingDevice({}) failed", index);
            Err(RingRtcError::SetAudioDevice.into())
        }
    }
}
