//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Peer Connection

#[cfg(feature = "native")]
use std::ffi::c_void;
#[cfg(feature = "native")]
use std::sync::{Arc, Mutex};
use std::{ffi::CString, fmt::Formatter, os::raw::c_char};

#[cfg(feature = "native")]
use anyhow::anyhow;
pub use pcf::{RffiPeerConnectionFactoryInterface, RffiPeerConnectionFactoryOwner};

#[cfg(feature = "native")]
use crate::webrtc::audio_device_module::AudioDeviceModule;
#[cfg(feature = "native")]
use crate::webrtc::audio_device_module_callbacks::{AUDIO_DEVICE_CBS_PTR, decrement_adm_ref_count};
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection_factory as pcf;
#[cfg(feature = "injectable_network")]
use crate::webrtc::injectable_network::InjectableNetwork;
#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection_factory as pcf;
use crate::{
    common::Result,
    core::util::truncate_for_logging,
    error::RingRtcError,
    webrtc,
    webrtc::{
        media::{AudioTrack, VideoSource, VideoTrack},
        peer_connection::PeerConnection,
        peer_connection_observer::{PeerConnectionObserver, PeerConnectionObserverTrait},
    },
};

#[repr(C)]
pub struct RffiIceServer {
    pub username: webrtc::ptr::Borrowed<c_char>,
    pub password: webrtc::ptr::Borrowed<c_char>,
    pub hostname: webrtc::ptr::Borrowed<c_char>,
    pub urls: webrtc::ptr::Borrowed<webrtc::ptr::Borrowed<c_char>>,
    pub urls_size: usize,
}

#[repr(u8)]
pub enum RffiPeerConnectionKind {
    Direct,
    Relayed,
    GroupCall,
}

#[derive(Clone, Debug, Default)]
pub struct IceServer {
    username: CString,
    password: CString,
    hostname: CString,
    // To own the strings
    _urls: Vec<CString>,
    // To hand the strings to C
    url_ptrs: Vec<webrtc::ptr::Borrowed<c_char>>,
}

unsafe impl Send for IceServer {}
unsafe impl Sync for IceServer {}

impl IceServer {
    pub fn new(username: String, password: String, hostname: String, urls_in: Vec<String>) -> Self {
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
            hostname: CString::new(hostname).expect("CString of hostname"),
            _urls: urls,
            url_ptrs,
        }
    }

    pub fn none() -> Self {
        // In the FFI C++, no urls means no IceServer is added
        Self::new(
            "".to_string(), // username
            "".to_string(), // password
            "".to_string(), // hostname
            vec![],         // urls
        )
    }

    pub fn rffi(&self) -> RffiIceServer {
        RffiIceServer {
            username: webrtc::ptr::Borrowed::from_ptr(self.username.as_ptr()),
            password: webrtc::ptr::Borrowed::from_ptr(self.password.as_ptr()),
            hostname: webrtc::ptr::Borrowed::from_ptr(self.hostname.as_ptr()),
            urls: webrtc::ptr::Borrowed::from_ptr(self.url_ptrs.as_ptr()),
            urls_size: self.url_ptrs.len(),
        }
    }
}

#[repr(C)]
pub struct RffiIceServers {
    servers: webrtc::ptr::Borrowed<RffiIceServer>,
    servers_size: usize,
}

/// Describes an audio input or output device.
#[derive(Clone, PartialEq, Eq)]
pub struct AudioDevice {
    /// Name of the device
    pub name: String,
    /// Unique ID
    pub unique_id: String,
}
impl std::fmt::Debug for AudioDevice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AudioDevice{{ name: {}, unique_id: {} }}",
            truncate_for_logging(&self.name),
            truncate_for_logging(&self.unique_id)
        )
    }
}

// See https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-roles
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsDefaultAudioDevice {
    // E.g. for VoIP. historically labeled "communications"
    pub communications_device: Option<AudioDevice>,
    // E.g. for games, system notifications. historically labeled "default"
    pub console_device: Option<AudioDevice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefaultAudioDevice {
    Windows(WindowsDefaultAudioDevice),
    NonWindows(Option<AudioDevice>),
}

impl DefaultAudioDevice {
    pub fn empty_windows() -> Self {
        DefaultAudioDevice::Windows(WindowsDefaultAudioDevice {
            communications_device: None,
            console_device: None,
        })
    }

    pub fn empty_non_windows() -> Self {
        DefaultAudioDevice::NonWindows(None)
    }

    pub fn default_for_target() -> Self {
        if cfg!(target_os = "windows") {
            Self::empty_windows()
        } else {
            Self::empty_non_windows()
        }
    }

    // get the default device for VoIP, if any exists.
    pub fn default_for_voip(&self) -> Option<AudioDevice> {
        match self {
            Self::Windows(WindowsDefaultAudioDevice {
                communications_device,
                ..
            }) => communications_device.clone(),
            Self::NonWindows(device) => device.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceList {
    pub list: Vec<AudioDevice>,
    pub default: DefaultAudioDevice,
}

impl AudioDeviceList {
    pub fn new_for_target_os() -> Self {
        Self::new_with_default(DefaultAudioDevice::default_for_target())
    }
    pub fn new_with_default(default: DefaultAudioDevice) -> Self {
        Self {
            list: Vec::new(),
            default,
        }
    }
}

/// Stays in sync with RffiAudioConfig in peer_connection_factory.h.
#[repr(C)]
pub struct RffiAudioConfig {
    pub high_pass_filter_enabled: bool,
    pub aec_enabled: bool,
    pub ns_enabled: bool,
    pub agc_enabled: bool,
    #[cfg(feature = "native")]
    pub adm_borrowed: webrtc::ptr::Borrowed<c_void>,
    #[cfg(feature = "native")]
    pub rust_audio_device_callbacks: webrtc::ptr::Borrowed<c_void>,
    #[cfg(feature = "native")]
    pub free_adm_cb: unsafe extern "C" fn(webrtc::ptr::Borrowed<c_void>),
}
pub struct RffiAudioConfigWrapper {
    rffi: RffiAudioConfig,
    #[cfg(feature = "native")]
    adm: Option<Arc<Mutex<AudioDeviceModule>>>,
}

#[derive(Clone, Debug)]
pub struct AudioConfig {
    pub high_pass_filter_enabled: bool,
    pub aec_enabled: bool,
    pub ns_enabled: bool,
    pub agc_enabled: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            high_pass_filter_enabled: true,
            aec_enabled: true,
            ns_enabled: true,
            agc_enabled: true,
        }
    }
}

// An observer trait that receives notifications whenever the input or output
// devices change.
// These callbacks should run "quickly", as they'll be run from the cubeb worker
// thread, and any delays will block playout_delay calls, which happen
// constantly during calls (short blocking is OK, as is passing the
// event to another thread; calling a client directly is not).
// Currently only applicable on Desktop
pub trait AudioDeviceObserver: Send + std::fmt::Debug {
    fn output_changed(&self, devices: AudioDeviceList);
    fn input_changed(&self, devices: AudioDeviceList);
    fn settle_promise(&self, success: bool, audio_device: Option<AudioDevice>, request_id: u32);
    fn settle_start_recording_via_callback(&self, success: bool, audio_device: Option<AudioDevice>);
    fn settle_start_playback_via_callback(&self, success: bool, audio_device: Option<AudioDevice>);
}

/// The parameter type to set_audio_warmup, which should invoke a callback on start of audio warmup,
/// but **not** on stop, because there is no action clients can take on failure to stop.
#[derive(Debug, Copy, Clone)]
pub enum SetAudioWarmup {
    Enable(u32), // the request_id to notify on success or failure
    Disable,
}

impl AudioConfig {
    // Return both the RffiAudioConfig as well as the name of the cubeb backend
    // in use, if any.
    // Fallible only with `native`, where registering the ADM callback can fail.
    #[cfg_attr(not(feature = "native"), allow(clippy::unnecessary_wraps))]
    fn rffi(
        &self,
        // iOS and Android won't use this; that's fine.
        #[cfg_attr(not(feature = "native"), allow(unused_mut, unused_variables))]
        mut audio_device_observer: Option<Box<dyn AudioDeviceObserver>>,
    ) -> Result<RffiAudioConfigWrapper> {
        #[cfg(feature = "native")]
        let (adm_borrowed, adm_arc) = match AudioDeviceModule::new() {
            Ok(mut adm) => {
                if let Some(observer) = audio_device_observer.take() {
                    adm.register_audio_device_callback(observer)?;
                }
                let adm_arc = Arc::new(Mutex::new(adm));
                (
                    // This will need to be explicitly destroyed by the
                    // C++ layer by calling decrement_adm_ref_count to
                    // turn it back into an Arc.
                    // We use into_raw(...clone()) here to ensure that
                    // the ADM stays alive until the C++ layer is done
                    // using it.
                    webrtc::ptr::Borrowed::from_ptr(Arc::<Mutex<AudioDeviceModule>>::into_raw(
                        adm_arc.clone(),
                    ))
                    .to_void(),
                    Some(adm_arc),
                )
            }
            Err(e) => {
                error!("Failed to initialize adm: {}", e);
                (webrtc::ptr::Borrowed::null(), None)
            }
        };

        Ok(RffiAudioConfigWrapper {
            rffi: RffiAudioConfig {
                high_pass_filter_enabled: self.high_pass_filter_enabled,
                aec_enabled: self.aec_enabled,
                ns_enabled: self.ns_enabled,
                agc_enabled: self.agc_enabled,
                #[cfg(feature = "native")]
                adm_borrowed,
                #[cfg(feature = "native")]
                rust_audio_device_callbacks: webrtc::ptr::Borrowed::from_ptr(AUDIO_DEVICE_CBS_PTR)
                    .to_void(),
                #[cfg(feature = "native")]
                free_adm_cb: decrement_adm_ref_count,
            },
            #[cfg(feature = "native")]
            adm: adm_arc,
        })
    }
}

/// Stays in sync with RffiAudioJitterBufferConfig in peer_connection_factory.h.
#[repr(C)]
pub struct RffiAudioJitterBufferConfig {
    pub max_packets: i32,
    pub min_delay_ms: i32,
    pub max_target_delay_ms: i32,
    pub fast_accelerate: bool,
}

#[derive(Clone, Debug)]
pub struct AudioJitterBufferConfig {
    pub max_packets: i32,
    pub min_delay_ms: i32,
    pub max_target_delay_ms: i32,
    pub fast_accelerate: bool,
}

impl Default for AudioJitterBufferConfig {
    fn default() -> Self {
        Self {
            max_packets: 50,
            min_delay_ms: 0,
            max_target_delay_ms: 500,
            fast_accelerate: false,
        }
    }
}

impl AudioJitterBufferConfig {
    fn rffi(&self) -> RffiAudioJitterBufferConfig {
        RffiAudioJitterBufferConfig {
            max_packets: self.max_packets,
            min_delay_ms: self.min_delay_ms,
            max_target_delay_ms: self.max_target_delay_ms,
            fast_accelerate: self.fast_accelerate,
        }
    }
}

/// Rust wrapper around WebRTC C++ PeerConnectionFactory object.
#[derive(Clone, Debug)]
pub struct PeerConnectionFactory {
    rffi: webrtc::Arc<RffiPeerConnectionFactoryOwner>,
    // Hold this so we run `drop` on it on shutdown
    #[cfg(feature = "native")]
    adm: Option<Arc<Mutex<AudioDeviceModule>>>,
}

impl PeerConnectionFactory {
    /// Create a new Rust PeerConnectionFactory object from a WebRTC C++
    /// PeerConnectionFactory object.
    pub fn new(
        audio_config: &AudioConfig,
        use_injectable_network: bool,
        field_trials_string: &str,
        audio_device_observer: Option<Box<dyn AudioDeviceObserver>>,
    ) -> Result<Self> {
        debug!("PeerConnectionFactory::new()");

        let audio_config_rffi = audio_config.rffi(audio_device_observer)?;

        let field_trials_cstr = CString::new(field_trials_string)?;

        let rffi = unsafe {
            webrtc::Arc::from_owned(pcf::Rust_createPeerConnectionFactory(
                webrtc::ptr::Borrowed::from_ptr(&audio_config_rffi.rffi),
                use_injectable_network,
                field_trials_cstr.as_ptr(),
            ))
        };
        if rffi.is_null() {
            return Err(RingRtcError::CreatePeerConnectionFactory.into());
        }
        Ok(Self {
            rffi,
            #[cfg(feature = "native")]
            adm: audio_config_rffi.adm,
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
        let rffi = unsafe {
            webrtc::Arc::from_owned(pcf::Rust_createPeerConnectionFactoryWrapper(
                native.as_borrowed(),
            ))
        };
        Self {
            rffi,
            #[cfg(feature = "native")]
            adm: None,
        }
    }

    #[cfg(feature = "injectable_network")]
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
        kind: RffiPeerConnectionKind,
        audio_jitter_buffer_config: &AudioJitterBufferConfig,
        audio_rtcp_report_interval_ms: i32,
        ice_servers: &[IceServer],
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
        // We do this by passing a webrtc::ptr::Unique<RffiPeerConnectionObserver> to
        // the Rust-level PeerConnection and let it own it.
        let pc_observer_rffi = pc_observer.into_rffi();
        let servers: Vec<RffiIceServer> = ice_servers.iter().map(|s| s.rffi()).collect();
        let rffi_ice_servers = RffiIceServers {
            servers: webrtc::ptr::Borrowed::from_ptr(servers.as_ptr()),
            servers_size: servers.len(),
        };

        let rffi = webrtc::Arc::from_owned(unsafe {
            pcf::Rust_createPeerConnection(
                self.rffi.as_borrowed(),
                pc_observer_rffi.borrow(),
                kind,
                webrtc::ptr::Borrowed::from_ptr(&audio_jitter_buffer_config.rffi()),
                audio_rtcp_report_interval_ms,
                webrtc::ptr::Borrowed::from_ptr(&rffi_ice_servers),
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
    pub fn get_audio_playout_devices(&mut self) -> Result<AudioDeviceList> {
        self.adm
            .as_ref()
            .and_then(|adm| adm.lock().ok())
            .map_or(Err(anyhow!("couldn't access ADM")), |mut adm| {
                adm.get_audio_playout_devices()
            })
    }

    #[cfg(feature = "native")]
    pub fn set_audio_playout_device_by_id(
        &mut self,
        device_id: &str,
        request_id: u32,
    ) -> Result<()> {
        self.adm.as_ref().and_then(|adm| adm.lock().ok()).map_or(
            Err(anyhow!("couldn't access ADM")),
            |mut adm| {
                if let Err(e) = adm.set_playout_device_by_id(device_id, request_id) {
                    error!("Failed to set playout device: {}", e);
                    return Err(RingRtcError::SetAudioDevice.into());
                }
                Ok(())
            },
        )
    }

    #[cfg(feature = "native")]
    pub fn set_input_voice_processing_enabled(
        &mut self,
        enabled: bool,
        request_id: u32,
    ) -> Result<()> {
        self.adm
            .as_ref()
            .and_then(|adm| adm.lock().ok())
            .map_or(Err(anyhow!("couldn't access ADM")), |mut adm| {
                adm.set_input_processing_enabled(enabled, request_id)
            })
    }

    #[cfg(feature = "native")]
    pub fn get_audio_recording_devices(&mut self) -> Result<AudioDeviceList> {
        self.adm
            .as_ref()
            .and_then(|adm| adm.lock().ok())
            .map_or(Err(anyhow!("couldn't access ADM")), |mut adm| {
                adm.get_audio_recording_devices()
            })
    }

    #[cfg(feature = "native")]
    pub fn set_audio_recording_device_by_id(
        &mut self,
        device_id: &str,
        request_id: u32,
    ) -> Result<()> {
        self.adm.as_ref().and_then(|adm| adm.lock().ok()).map_or(
            Err(anyhow!("couldn't access ADM")),
            |mut adm| {
                if let Err(e) = adm.set_recording_device_by_id(device_id, request_id) {
                    error!("Failed to set recording device: {}", e);
                    return Err(RingRtcError::SetAudioDevice.into());
                }
                Ok(())
            },
        )
    }

    #[cfg(feature = "native")]
    pub fn audio_backend(&self) -> Option<String> {
        self.adm
            .as_ref()
            .and_then(|adm| adm.lock().ok())
            .map(|adm| adm.backend_name())
    }

    #[cfg(feature = "native")]
    pub fn set_audio_warmup(&mut self, warmup_param: SetAudioWarmup) -> Result<()> {
        if self
            .adm
            .as_ref()
            .and_then(|adm| adm.lock().ok())
            .and_then(|mut adm| match warmup_param {
                SetAudioWarmup::Enable(request_id) => adm.warmup_recording(request_id).ok(),
                SetAudioWarmup::Disable => {
                    if adm.stop_recording() != 0 {
                        warn!("failed to stop recording");
                        return None;
                    }
                    Some(())
                }
            })
            .is_none()
        {
            if matches!(warmup_param, SetAudioWarmup::Enable(_)) {
                return Err(anyhow!("Failed to warm up mic"));
            } else {
                return Err(anyhow!("Failed to stop warming up mic"));
            }
        };
        Ok(())
    }
}
