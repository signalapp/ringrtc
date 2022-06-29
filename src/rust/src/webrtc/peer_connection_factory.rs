//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Peer Connection

#[cfg(feature = "native")]
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_char;

use crate::common::Result;
use crate::error::RingRtcError;
use crate::webrtc;
#[cfg(feature = "simnet")]
use crate::webrtc::injectable_network::InjectableNetwork;
use crate::webrtc::media::{AudioTrack, VideoSource, VideoTrack};
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::peer_connection_observer::{
    PeerConnectionObserver, PeerConnectionObserverTrait,
};

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection_factory as pcf;

// TODO: sim::pcf
#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection_factory as pcf;

pub use pcf::{RffiPeerConnectionFactoryInterface, RffiPeerConnectionFactoryOwner};

#[cfg(target_os = "windows")] // For the default ADM.
const DEFAULT_COMMUNICATION_DEVICE_INDEX: u16 = 0xFFFF;

#[cfg(feature = "native")]
const ADM_MAX_DEVICE_NAME_SIZE: usize = 128;
#[cfg(feature = "native")]
const ADM_MAX_DEVICE_UUID_SIZE: usize = 128;

#[repr(C)]
pub struct RffiIceServer {
    pub username: webrtc::ptr::Borrowed<c_char>,
    pub password: webrtc::ptr::Borrowed<c_char>,
    pub urls: webrtc::ptr::Borrowed<webrtc::ptr::Borrowed<c_char>>,
    pub urls_size: usize,
}

#[derive(Clone, Debug)]
pub struct IceServer {
    username: CString,
    password: CString,
    // To own the strings
    _urls: Vec<CString>,
    // To hand the strings to C
    url_ptrs: Vec<webrtc::ptr::Borrowed<c_char>>,
}

unsafe impl Send for IceServer {}
unsafe impl Sync for IceServer {}

impl IceServer {
    pub fn new(username: String, password: String, urls_in: Vec<String>) -> Self {
        let mut urls = Vec::new();
        for url in urls_in {
            urls.push(CString::new(url).expect("CString of URL"));
        }
        let url_ptrs = urls
            .iter()
            .map(|s| webrtc::ptr::Borrowed::from_ptr(s.as_ptr()))
            .collect();
        Self {
            username: CString::new(username).expect("CString of username"),
            password: CString::new(password).expect("CString of password"),
            _urls: urls,
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
            username: webrtc::ptr::Borrowed::from_ptr(self.username.as_ptr()),
            password: webrtc::ptr::Borrowed::from_ptr(self.password.as_ptr()),
            urls: webrtc::ptr::Borrowed::from_ptr(self.url_ptrs.as_ptr()),
            urls_size: self.url_ptrs.len(),
        }
    }
}

/// Describes an audio input or output device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Name of the device
    pub name: String,
    /// Unique ID - truly unique on Windows, best effort on other platforms.
    pub unique_id: String,
    /// If the name requires translation, the translated string identifier.
    pub i18n_key: String,
}

#[cfg(target_os = "windows")] // For the default ADM.
impl AudioDevice {
    fn default() -> AudioDevice {
        AudioDevice {
            name: "Default".to_string(),
            unique_id: "Default".to_string(),
            i18n_key: "default_communication_device".to_string(),
        }
    }
}

/// Rust wrapper around WebRTC C++ PeerConnectionFactory object.
#[derive(Clone, Debug)]
#[allow(dead_code)] // use_new_audio_device_module is currently used only for Windows builds.
pub struct PeerConnectionFactory {
    rffi: webrtc::Arc<RffiPeerConnectionFactoryOwner>,
    use_new_audio_device_module: bool,
    playout_device_count: Option<u16>,
    recording_device_count: Option<u16>,
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
            let rffi = unsafe {
                webrtc::Arc::from_owned(pcf::Rust_createPeerConnectionFactory(
                    config.use_new_audio_device_module,
                    config.use_injectable_network,
                ))
            };

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
        Ok(Self {
            rffi,
            use_new_audio_device_module,
            playout_device_count: None,
            recording_device_count: None,
        })
    }

    pub fn rffi(&self) -> &webrtc::Arc<RffiPeerConnectionFactoryOwner> {
        &self.rffi
    }

    /// Wrap an existing C++ PeerConnectionFactory (not a PeerConnectionFactoryOwner).
    ///
    /// # Safety
    ///
    /// `native` must point to a C++ PeerConnectionFactory.
    pub unsafe fn from_native_factory(
        native: webrtc::Arc<RffiPeerConnectionFactoryInterface>,
    ) -> Self {
        let rffi = webrtc::Arc::from_owned(pcf::Rust_createPeerConnectionFactoryWrapper(
            native.as_borrowed(),
        ));
        Self {
            rffi,
            use_new_audio_device_module: false,
            playout_device_count: None,
            recording_device_count: None,
        }
    }

    #[cfg(feature = "simnet")]
    pub fn injectable_network(&self) -> Option<InjectableNetwork> {
        let rffi = unsafe { pcf::Rust_getInjectableNetwork(self.rffi.as_borrowed()) };
        if rffi.is_null() {
            return None;
        }
        Some(InjectableNetwork::new(rffi, self.rffi.clone()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_peer_connection<T: PeerConnectionObserverTrait>(
        &self,
        pc_observer: PeerConnectionObserver<T>,
        hide_ip: bool,
        ice_servers: &IceServer,
        outgoing_audio_track: AudioTrack,
        outgoing_video_track: Option<VideoTrack>,
    ) -> Result<PeerConnection> {
        debug!(
            "PeerConnectionFactory::create_peer_connection() {:?}",
            self.rffi
        );
        // Unlike on Android (see call_manager::create_peer_connection)
        // and iOS (see IosPlatform::create_connection),
        // the RffiPeerConnectionObserver is *not* passed as owned
        // by Rust_createPeerConnection, so we need to keep it alive
        // for as long as the native PeerConnection is alive.
        // we do this by passing a webrtc::ptr::Unique<RffiPeerConnectionObserver> to
        // the Rust-level PeerConnection and let it own it.
        let pc_observer_rffi = pc_observer.into_rffi();

        let rffi = webrtc::Arc::from_owned(unsafe {
            pcf::Rust_createPeerConnection(
                self.rffi.as_borrowed(),
                pc_observer_rffi.borrow(),
                hide_ip,
                ice_servers.rffi(),
                outgoing_audio_track.rffi().as_borrowed(),
                outgoing_video_track
                    .map_or_else(webrtc::ptr::BorrowedRc::null, |outgoing_video_track| {
                        outgoing_video_track.rffi().as_borrowed()
                    }),
            )
        });
        debug!(
            "PeerConnectionFactory::create_peer_connection() finished: {:?}",
            rffi
        );
        if rffi.is_null() {
            return Err(RingRtcError::CreatePeerConnection.into());
        }
        Ok(PeerConnection::new(
            rffi,
            Some(pc_observer_rffi),
            Some(self.rffi.clone()),
        ))
    }

    pub fn create_outgoing_audio_track(&self) -> Result<AudioTrack> {
        debug!("PeerConnectionFactory::create_outgoing_audio_track()");
        let rffi =
            webrtc::Arc::from_owned(unsafe { pcf::Rust_createAudioTrack(self.rffi.as_borrowed()) });
        if rffi.is_null() {
            return Err(RingRtcError::CreateAudioTrack.into());
        }
        Ok(AudioTrack::new(rffi, Some(self.rffi.clone())))
    }

    pub fn create_outgoing_video_source(&self) -> Result<VideoSource> {
        debug!("PeerConnectionFactory::create_outgoing_video_source()");
        let rffi = webrtc::Arc::from_owned(unsafe { pcf::Rust_createVideoSource() });
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
        let rffi = webrtc::Arc::from_owned(unsafe {
            pcf::Rust_createVideoTrack(
                self.rffi.as_borrowed(),
                outgoing_video_source.rffi().as_borrowed(),
            )
        });
        if rffi.is_null() {
            return Err(RingRtcError::CreateVideoTrack.into());
        }
        Ok(VideoTrack::new(rffi, Some(self.rffi.clone())))
    }

    #[cfg(feature = "native")]
    fn get_audio_playout_device(&self, index: u16) -> Result<AudioDevice> {
        let mut name_buf = [0; ADM_MAX_DEVICE_NAME_SIZE];
        let mut unique_id_buf = [0; ADM_MAX_DEVICE_UUID_SIZE];
        let rc = unsafe {
            pcf::Rust_getAudioPlayoutDeviceName(
                self.rffi.as_borrowed(),
                index,
                name_buf.as_mut_ptr(),
                unique_id_buf.as_mut_ptr(),
            )
        };
        if rc != 0 {
            error!("getAudioPlayoutDeviceName({}) failed: {}", index, rc);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        // SAFETY: the buffer pointers will be valid until the end of the scope,
        // and they should contain valid C strings if the return code indicated success.
        let name = unsafe { CStr::from_ptr(name_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let unique_id = unsafe { CStr::from_ptr(unique_id_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Ok(AudioDevice {
            name,
            unique_id,
            i18n_key: "".to_string(),
        })
    }

    #[cfg(feature = "native")]
    pub fn get_audio_playout_devices(&mut self) -> Result<Vec<AudioDevice>> {
        let device_count = unsafe { pcf::Rust_getAudioPlayoutDevices(self.rffi.as_borrowed()) };
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

        if self.playout_device_count != Some(device_count) {
            info!(
                "PeerConnectionFactory::get_audio_playout_devices(): device_count: {}",
                device_count
            );
            self.playout_device_count = Some(device_count);
        }

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
    fn get_default_playout_device_index(&mut self) -> Result<u16> {
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
    pub fn set_audio_playout_device(&mut self, index: u16) -> Result<()> {
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

        let ok = unsafe { pcf::Rust_setAudioPlayoutDevice(self.rffi.as_borrowed(), index) };
        if ok {
            Ok(())
        } else {
            error!("setAudioPlayoutDevice({}) failed", index);
            Err(RingRtcError::SetAudioDevice.into())
        }
    }

    #[cfg(feature = "native")]
    fn get_audio_recording_device(&self, index: u16) -> Result<AudioDevice> {
        let mut name_buf = [0; ADM_MAX_DEVICE_NAME_SIZE];
        let mut unique_id_buf = [0; ADM_MAX_DEVICE_UUID_SIZE];
        let rc = unsafe {
            pcf::Rust_getAudioRecordingDeviceName(
                self.rffi.as_borrowed(),
                index,
                name_buf.as_mut_ptr(),
                unique_id_buf.as_mut_ptr(),
            )
        };
        if rc != 0 {
            error!("getAudioRecordingDeviceName({}) failed: {}", index, rc);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        // SAFETY: the buffer pointers will be valid until the end of the scope,
        // and they should contain valid C strings if the return code indicated success.
        let name = unsafe { CStr::from_ptr(name_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let unique_id = unsafe { CStr::from_ptr(unique_id_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Ok(AudioDevice {
            name,
            unique_id,
            i18n_key: "".to_string(),
        })
    }

    #[cfg(feature = "native")]
    pub fn get_audio_recording_devices(&mut self) -> Result<Vec<AudioDevice>> {
        let device_count = unsafe { pcf::Rust_getAudioRecordingDevices(self.rffi.as_borrowed()) };
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

        if self.recording_device_count != Some(device_count) {
            info!(
                "PeerConnectionFactory::get_audio_recording_devices(): device_count: {}",
                device_count
            );
            self.recording_device_count = Some(device_count);
        }

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
    fn get_default_recording_device_index(&mut self) -> Result<u16> {
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
    pub fn set_audio_recording_device(&mut self, index: u16) -> Result<()> {
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

        info!(
            "PeerConnectionFactory::set_audio_recording_device({})",
            index
        );

        let ok = unsafe { pcf::Rust_setAudioRecordingDevice(self.rffi.as_borrowed(), index) };
        if ok {
            Ok(())
        } else {
            error!("setAudioRecordingDevice({}) failed", index);
            Err(RingRtcError::SetAudioDevice.into())
        }
    }
}
