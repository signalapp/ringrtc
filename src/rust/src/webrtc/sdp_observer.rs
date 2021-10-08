//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Create Session Description Interface.

use std::ffi::{c_void, CStr, CString};
use std::fmt;
use std::os::raw::c_char;
use std::ptr;
use std::sync::{Arc, Condvar, Mutex};

use crate::common::Result;
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::util::{ptr_as_ref, FutureResult, RustObject};
use crate::error::RingRtcError;
use crate::protobuf;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::sdp_observer as sdp;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::sdp_observer::RffiSessionDescription;

#[cfg(feature = "sim")]
use crate::webrtc::sim::sdp_observer as sdp;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::sdp_observer::RffiSessionDescription;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum SrtpCryptoSuite {
    // Matches webrtc/rtc_base/ssl_stream_adapter.h
    Aes128CmSha1 = 1,  // 16-byte key; 14-byte salt
    AeadAes128Gcm = 7, // 16-byte key; 12-byte salt
    AeadAes256Gcm = 8, // 32-byte key; 12-byte salt
}

pub struct SrtpKey {
    pub suite: SrtpCryptoSuite,
    pub key: Vec<u8>,
    pub salt: Vec<u8>,
}
/// Rust wrapper around WebRTC C++ SessionDescription.
pub struct SessionDescription {
    /// Pointer to C++ SessionDescription object.
    rffi: *mut RffiSessionDescription,
}

impl fmt::Display for SessionDescription {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rffi_session_description: {:p}", self.rffi)
    }
}

impl fmt::Debug for SessionDescription {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum RffiVideoCodecType {
    Vp8 = 8,
    H264ConstrainedHigh = 46,
    H264ConstrainedBaseline = 40,
}

/// cbindgen:field-names=[type, level]
#[repr(C)]
pub struct RffiVideoCodec {
    r#type: RffiVideoCodecType,
    level: u32,
}

#[repr(C)]
pub struct RffiConnectionParametersV4 {
    pub ice_ufrag: *const c_char,
    pub ice_pwd: *const c_char,
    pub receive_video_codecs: *const RffiVideoCodec,
    pub receive_video_codecs_size: usize,
}

impl Drop for SessionDescription {
    fn drop(&mut self) {
        if !self.rffi.is_null() {
            unsafe { sdp::Rust_releaseSessionDescription(self.rffi) };
        }
    }
}

impl SessionDescription {
    /// Create a new SessionDescription from a C++ SessionDescription object.
    pub fn new(rffi: *mut RffiSessionDescription) -> Self {
        Self { rffi }
    }

    pub fn take_rffi(mut self) -> *mut RffiSessionDescription {
        let rffi = self.rffi;
        // This makes it so drop() will not release it a second time.
        self.rffi = std::ptr::null_mut();
        rffi
    }

    /// Return SDP representation of this SessionDescription.
    pub fn to_sdp(&self) -> Result<String> {
        let sdp_ptr = unsafe { sdp::Rust_toSdp(self.rffi) };
        if sdp_ptr.is_null() {
            return Err(RingRtcError::ToSdp.into());
        }
        let sdp = unsafe { CStr::from_ptr(sdp_ptr).to_string_lossy().into_owned() };
        unsafe { libc::free(sdp_ptr as *mut libc::c_void) };
        Ok(sdp)
    }

    /// Create a SDP answer from the session description string.
    pub fn answer_from_sdp(sdp: String) -> Result<Self> {
        let sdp = CString::new(sdp)?;
        let answer = unsafe { sdp::Rust_answerFromSdp(sdp.as_ptr()) };
        if answer.is_null() {
            return Err(RingRtcError::ConvertSdpAnswer.into());
        }
        Ok(SessionDescription::new(answer))
    }

    /// Create a SDP offer from the session description string.
    pub fn offer_from_sdp(sdp: String) -> Result<Self> {
        let sdp = CString::new(sdp)?;
        let offer = unsafe { sdp::Rust_offerFromSdp(sdp.as_ptr()) };
        if offer.is_null() {
            return Err(RingRtcError::ConvertSdpOffer.into());
        }
        Ok(SessionDescription::new(offer))
    }

    pub fn disable_dtls_and_set_srtp_key(&mut self, key: &SrtpKey) -> Result<()> {
        let success = unsafe {
            sdp::Rust_disableDtlsAndSetSrtpKey(
                self.rffi,
                key.suite,
                key.key.as_ptr(),
                key.key.len(),
                key.salt.as_ptr(),
                key.salt.len(),
            )
        };
        if success {
            Ok(())
        } else {
            Err(RingRtcError::MungeSdp.into())
        }
    }

    pub fn to_v4(
        &self,
        public_key: Vec<u8>,
        bandwidth_mode: BandwidthMode,
    ) -> Result<protobuf::signaling::ConnectionParametersV4> {
        let rffi_v4_ptr = unsafe { sdp::Rust_sessionDescriptionToV4(self.rffi) };

        if rffi_v4_ptr.is_null() {
            return Err(RingRtcError::MungeSdp.into());
        }

        let rffi_v4 = unsafe { &*rffi_v4_ptr };
        let ice_ufrag = from_cstr(rffi_v4.ice_ufrag);
        let ice_pwd = from_cstr(rffi_v4.ice_pwd);
        let receive_video_codecs: Vec<protobuf::signaling::VideoCodec> = unsafe {
            std::slice::from_raw_parts(
                rffi_v4.receive_video_codecs,
                rffi_v4.receive_video_codecs_size,
            )
        }
        .iter()
        .map(|rffi_codec| {
            let r#type = match rffi_codec.r#type {
                RffiVideoCodecType::Vp8 => protobuf::signaling::VideoCodecType::Vp8,
                RffiVideoCodecType::H264ConstrainedHigh => {
                    protobuf::signaling::VideoCodecType::H264ConstrainedHigh
                }
                RffiVideoCodecType::H264ConstrainedBaseline => {
                    protobuf::signaling::VideoCodecType::H264ConstrainedBaseline
                }
            };
            let level = if rffi_codec.level > 0 {
                Some(rffi_codec.level)
            } else {
                None
            };
            protobuf::signaling::VideoCodec {
                r#type: Some(r#type as i32),
                level,
            }
        })
        .collect();

        unsafe { sdp::Rust_releaseV4(rffi_v4_ptr) };

        Ok(protobuf::signaling::ConnectionParametersV4 {
            public_key: Some(public_key),
            ice_ufrag: Some(ice_ufrag),
            ice_pwd: Some(ice_pwd),
            receive_video_codecs,
            max_bitrate_bps: Some(bandwidth_mode.max_bitrate().as_bps()),
        })
    }

    pub fn offer_from_v4(v4: &protobuf::signaling::ConnectionParametersV4) -> Result<Self> {
        Self::from_v4(true, v4)
    }

    pub fn answer_from_v4(v4: &protobuf::signaling::ConnectionParametersV4) -> Result<Self> {
        Self::from_v4(false, v4)
    }

    fn from_v4(offer: bool, v4: &protobuf::signaling::ConnectionParametersV4) -> Result<Self> {
        let rffi_ice_ufrag = to_cstring(&v4.ice_ufrag)?;
        let rffi_ice_pwd = to_cstring(&v4.ice_pwd)?;
        let mut rffi_video_codecs: Vec<RffiVideoCodec> = Vec::new();
        for codec in &v4.receive_video_codecs {
            if let protobuf::signaling::VideoCodec {
                r#type: Some(r#type),
                level,
            } = codec
            {
                const VP8: i32 = protobuf::signaling::VideoCodecType::Vp8 as i32;
                const H264_CHP: i32 =
                    protobuf::signaling::VideoCodecType::H264ConstrainedHigh as i32;
                const H264_CBP: i32 =
                    protobuf::signaling::VideoCodecType::H264ConstrainedBaseline as i32;
                let rffi_type = match *r#type {
                    VP8 => Some(RffiVideoCodecType::Vp8),
                    H264_CHP => Some(RffiVideoCodecType::H264ConstrainedHigh),
                    H264_CBP => Some(RffiVideoCodecType::H264ConstrainedBaseline),
                    _ => None,
                };
                let rffi_level = level.unwrap_or(0);
                if let Some(rffi_type) = rffi_type {
                    rffi_video_codecs.push(RffiVideoCodec {
                        r#type: rffi_type,
                        level: rffi_level,
                    });
                }
            }
        }
        let rffi_v4 = RffiConnectionParametersV4 {
            ice_ufrag: rffi_ice_ufrag.as_ptr(),
            ice_pwd: rffi_ice_pwd.as_ptr(),
            receive_video_codecs: rffi_video_codecs.as_ptr(),
            receive_video_codecs_size: rffi_video_codecs.len(),
        };
        let rffi = unsafe { sdp::Rust_sessionDescriptionFromV4(offer, &rffi_v4) };
        if rffi.is_null() {
            return Err(RingRtcError::MungeSdp.into());
        }
        Ok(Self::new(rffi))
    }

    pub fn local_for_group_call(
        ice_ufrag: &str,
        ice_pwd: &str,
        dtls_fingerprint_sha256: &[u8; 32],
        rtp_demux_id: Option<u32>,
    ) -> Result<Self> {
        let rffi_ice_ufrag = CString::new(ice_ufrag.as_bytes())?;
        let rffi_ice_pwd = CString::new(ice_pwd.as_bytes())?;

        let sdi = unsafe {
            sdp::Rust_localDescriptionForGroupCall(
                rffi_ice_ufrag.as_ptr(),
                rffi_ice_pwd.as_ptr(),
                dtls_fingerprint_sha256,
                rtp_demux_id.unwrap_or(0),
            )
        };
        if sdi.is_null() {
            return Err(RingRtcError::MungeSdp.into());
        }
        Ok(Self::new(sdi))
    }

    pub fn remote_for_group_call(
        ice_ufrag: &str,
        ice_pwd: &str,
        dtls_fingerprint_sha256: &[u8; 32],
        rtp_demux_ids: &[u32],
    ) -> Result<Self> {
        let rffi_ice_ufrag = CString::new(ice_ufrag.as_bytes())?;
        let rffi_ice_pwd = CString::new(ice_pwd.as_bytes())?;

        let sdi = unsafe {
            sdp::Rust_remoteDescriptionForGroupCall(
                rffi_ice_ufrag.as_ptr(),
                rffi_ice_pwd.as_ptr(),
                dtls_fingerprint_sha256,
                rtp_demux_ids.as_ptr(),
                rtp_demux_ids.len(),
            )
        };
        if sdi.is_null() {
            return Err(RingRtcError::MungeSdp.into());
        }
        Ok(Self::new(sdi))
    }
}

fn to_cstring(s: &Option<String>) -> Result<CString> {
    Ok(if let Some(s) = s.as_ref() {
        CString::new(s.as_bytes())?
    } else {
        CString::new("")?
    })
}

fn from_cstr(c: *const c_char) -> String {
    if c.is_null() {
        "".to_string()
    } else {
        unsafe { CStr::from_ptr(c) }.to_string_lossy().into_owned()
    }
}

#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::sdp_observer::RffiCreateSessionDescriptionObserver;

#[cfg(feature = "sim")]
pub use crate::webrtc::sim::sdp_observer::RffiCreateSessionDescriptionObserver;

/// Observer object for creating a session description.
#[derive(Debug)]
pub struct CreateSessionDescriptionObserver {
    /// condition variable used to signal the completion of the create
    /// session description operation.
    condition: FutureResult<Result<*mut RffiSessionDescription>>,
    /// Pointer to C++ webrtc::rffi::RffiCreateSessionDescriptionObserver object
    rffi: *const RffiCreateSessionDescriptionObserver,
}

impl CreateSessionDescriptionObserver {
    /// Create a new CreateSessionDescriptionObserver.
    fn new() -> Self {
        Self {
            condition: Arc::new((Mutex::new((false, Ok(ptr::null_mut()))), Condvar::new())),
            rffi: ptr::null(),
        }
    }

    /// Called back when the create session description operation is a
    /// success.
    ///
    /// This call signals the condition variable.
    fn on_create_success(&self, session_description: *mut RffiSessionDescription) {
        info!("on_create_success()");
        let &(ref mtx, ref cvar) = &*self.condition;
        if let Ok(mut guard) = mtx.lock() {
            guard.1 = Ok(session_description);
            guard.0 = true;
            // We notify the condvar that the value has changed.
            cvar.notify_one();
        }
    }

    /// Called back when the create session description operation is a
    /// failure.
    ///
    /// This call signals the condition variable.
    fn on_create_failure(&self, err_message: String, err_type: i32) {
        warn!(
            "on_create_failure(). error msg: {}, type: {}",
            err_message, err_type
        );
        let &(ref mtx, ref cvar) = &*self.condition;
        if let Ok(mut guard) = mtx.lock() {
            guard.1 =
                Err(RingRtcError::CreateSessionDescriptionObserver(err_message, err_type).into());
            guard.0 = true;
            // We notify the condvar that the value has changed.
            cvar.notify_one();
        }
    }

    /// Retrieve the result of the create session description operation.
    ///
    /// This call blocks on the condition variable.
    pub fn get_result(&self) -> Result<SessionDescription> {
        let &(ref mtx, ref cvar) = &*self.condition;
        if let Ok(mut guard) = mtx.lock() {
            while !guard.0 {
                guard = cvar.wait(guard).map_err(|_| {
                    RingRtcError::MutexPoisoned(
                        "CreateSessionDescription condvar mutex".to_string(),
                    )
                })?;
            }
            // TODO: implement guard.1.clone() here ....
            match &guard.1 {
                Ok(v) => Ok(SessionDescription::new(*v)),
                Err(e) => Err(
                    RingRtcError::CreateSessionDescriptionObserverResult(format!("{}", e)).into(),
                ),
            }
        } else {
            Err(
                RingRtcError::MutexPoisoned("CreateSessionDescription condvar mutex".to_string())
                    .into(),
            )
        }
    }

    /// Set the RFFI observer object.
    pub fn set_rffi(&mut self, observer: *const RffiCreateSessionDescriptionObserver) {
        self.rffi = observer
    }

    /// Return the RFFI observer object.
    pub fn rffi(&self) -> *const RffiCreateSessionDescriptionObserver {
        self.rffi
    }
}

/// CreateSessionDescription observer OnSuccess() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn csd_observer_OnSuccess(
    csd_observer: *mut CreateSessionDescriptionObserver,
    session_description: *mut RffiSessionDescription,
) {
    info!("csd_observer_OnSuccess()");
    match unsafe { ptr_as_ref(csd_observer) } {
        Ok(csd_observer) => csd_observer.on_create_success(session_description),
        Err(e) => error!("csd_observer_OnSuccess(): {}", e),
    };
}

/// CreateSessionDescription observer OnFailure() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn csd_observer_OnFailure(
    csd_observer: *mut CreateSessionDescriptionObserver,
    err_message: *const c_char,
    err_type: i32,
) {
    let err_string: String = unsafe { CStr::from_ptr(err_message).to_string_lossy().into_owned() };
    error!(
        "csd_observer_OnFailure(): {}, type: {}",
        err_string, err_type
    );

    match unsafe { ptr_as_ref(csd_observer) } {
        Ok(v) => v.on_create_failure(err_string, err_type),
        Err(e) => error!("csd_observer_OnFailure(): {}", e),
    };
}

/// CreateSessionDescription observer callback function pointers.
#[repr(C)]
#[allow(non_snake_case)]
pub struct CreateSessionDescriptionObserverCallbacks {
    pub onSuccess: extern "C" fn(
        csd_observer: *mut CreateSessionDescriptionObserver,
        session_description: *mut RffiSessionDescription,
    ),
    pub onFailure: extern "C" fn(
        csd_observer: *mut CreateSessionDescriptionObserver,
        error_message: *const c_char,
        error_type: i32,
    ),
}

const CSD_OBSERVER_CBS: CreateSessionDescriptionObserverCallbacks =
    CreateSessionDescriptionObserverCallbacks {
        onSuccess: csd_observer_OnSuccess,
        onFailure: csd_observer_OnFailure,
    };
const CSD_OBSERVER_CBS_PTR: *const CreateSessionDescriptionObserverCallbacks = &CSD_OBSERVER_CBS;

/// Create a new Rust CreateSessionDescriptionObserver object.
///
/// Creates a new WebRTC C++ CreateSessionDescriptionObserver object,
/// registering the observer callbacks to this module, and wraps the
/// result in a Rust CreateSessionDescriptionObserver object.
pub fn create_csd_observer() -> Box<CreateSessionDescriptionObserver> {
    let csd_observer = Box::new(CreateSessionDescriptionObserver::new());
    let csd_observer_ptr = Box::into_raw(csd_observer);
    let rffi = unsafe {
        sdp::Rust_createCreateSessionDescriptionObserver(
            csd_observer_ptr as RustObject,
            CSD_OBSERVER_CBS_PTR as *const c_void,
        )
    };
    let mut csd_observer = unsafe { Box::from_raw(csd_observer_ptr) };

    csd_observer.set_rffi(rffi);
    csd_observer
}

#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::sdp_observer::RffiSetSessionDescriptionObserver;

#[cfg(feature = "sim")]
pub use crate::webrtc::sim::sdp_observer::RffiSetSessionDescriptionObserver;

/// Observer object for setting a session description.
#[derive(Debug)]
pub struct SetSessionDescriptionObserver {
    /// condition variable used to signal the completion of the set
    /// session description operation.
    condition: FutureResult<Result<()>>,
    /// Pointer to C++ CreateSessionDescriptionObserver object
    rffi: *const RffiSetSessionDescriptionObserver,
}

impl SetSessionDescriptionObserver {
    /// Create a new SetSessionDescriptionObserver.
    fn new() -> Self {
        Self {
            condition: Arc::new((Mutex::new((false, Ok(()))), Condvar::new())),
            rffi: ptr::null(),
        }
    }

    /// Called back when the set session description operation is a
    /// success.
    ///
    /// This call signals the condition variable.
    fn on_set_success(&self) {
        info!("on_set_success()");
        let &(ref mtx, ref cvar) = &*self.condition;
        if let Ok(mut guard) = mtx.lock() {
            guard.1 = Ok(());
            guard.0 = true;
            // We notify the condvar that the value has changed.
            cvar.notify_one();
        }
    }

    /// Called back when the set session description operation is a
    /// failure.
    ///
    /// This call signals the condition variable.
    fn on_set_failure(&self, err_message: String, err_type: i32) {
        warn!(
            "on_set_failure(). error msg: {}, type: {}",
            err_message, err_type
        );
        let &(ref mtx, ref cvar) = &*self.condition;
        if let Ok(mut guard) = mtx.lock() {
            guard.1 =
                Err(RingRtcError::SetSessionDescriptionObserver(err_message, err_type).into());
            guard.0 = true;
            // We notify the condvar that the value has changed.
            cvar.notify_one();
        }
    }

    /// Retrieve the result of the create session description operation.
    ///
    /// This call blocks on the condition variable.
    pub fn get_result(&self) -> Result<()> {
        let &(ref mtx, ref cvar) = &*self.condition;
        if let Ok(mut guard) = mtx.lock() {
            while !guard.0 {
                guard = cvar.wait(guard).map_err(|_| {
                    RingRtcError::MutexPoisoned("SetSessionDescription condvar mutex".to_string())
                })?;
            }
            // TODO: implement guard.1.clone() here ....
            match &guard.1 {
                Ok(_) => Ok(()),
                Err(e) => {
                    Err(RingRtcError::SetSessionDescriptionObserverResult(format!("{}", e)).into())
                }
            }
        } else {
            Err(
                RingRtcError::MutexPoisoned("SetSessionDescription condvar mutex".to_string())
                    .into(),
            )
        }
    }

    /// Set the RFFI observer object.
    pub fn set_rffi(&mut self, rffi: *const RffiSetSessionDescriptionObserver) {
        self.rffi = rffi
    }

    /// Return the RFFI observer object.
    pub fn rffi(&self) -> *const RffiSetSessionDescriptionObserver {
        self.rffi
    }
}

/// SetSessionDescription observer OnSuccess() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn ssd_observer_OnSuccess(ssd_observer: *mut SetSessionDescriptionObserver) {
    info!("ssd_observer_OnSuccess()");
    match unsafe { ptr_as_ref(ssd_observer) } {
        Ok(v) => v.on_set_success(),
        Err(e) => error!("ssd_observer_OnSuccess(): {}", e),
    };
}

/// SetSessionDescription observer OnFailure() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn ssd_observer_OnFailure(
    ssd_observer: *mut SetSessionDescriptionObserver,
    err_message: *const c_char,
    err_type: i32,
) {
    let err_string: String = unsafe { CStr::from_ptr(err_message).to_string_lossy().into_owned() };
    error!(
        "ssd_observer_OnFailure(): {}, type: {}",
        err_string, err_type
    );

    match unsafe { ptr_as_ref(ssd_observer) } {
        Ok(v) => v.on_set_failure(err_string, err_type),
        Err(e) => error!("ssd_observer_OnFailure(): {}", e),
    };
}

/// SetSessionDescription observer callback function pointers.
#[repr(C)]
#[allow(non_snake_case)]
pub struct SetSessionDescriptionObserverCallbacks {
    pub onSuccess: extern "C" fn(ssd_observer: *mut SetSessionDescriptionObserver),
    pub onFailure: extern "C" fn(
        ssd_observer: *mut SetSessionDescriptionObserver,
        error_message: *const c_char,
        error_type: i32,
    ),
}

const SSD_OBSERVER_CBS: SetSessionDescriptionObserverCallbacks =
    SetSessionDescriptionObserverCallbacks {
        onSuccess: ssd_observer_OnSuccess,
        onFailure: ssd_observer_OnFailure,
    };
const SSD_OBSERVER_CBS_PTR: *const SetSessionDescriptionObserverCallbacks = &SSD_OBSERVER_CBS;

/// Create a new Rust SetSessionDescriptionObserver object.
///
/// Creates a new WebRTC C++ SetSessionDescriptionObserver object,
/// registering the observer callbacks to this module, and wraps the
/// result in a Rust SetSessionDescriptionObserver object.
pub fn create_ssd_observer() -> Box<SetSessionDescriptionObserver> {
    let ssd_observer = Box::new(SetSessionDescriptionObserver::new());
    let ssd_observer_ptr = Box::into_raw(ssd_observer);
    let rffi_ssd_observer = unsafe {
        sdp::Rust_createSetSessionDescriptionObserver(
            ssd_observer_ptr as RustObject,
            SSD_OBSERVER_CBS_PTR as *const c_void,
        )
    };
    let mut ssd_observer = unsafe { Box::from_raw(ssd_observer_ptr) };

    ssd_observer.set_rffi(rffi_ssd_observer as *const RffiSetSessionDescriptionObserver);
    ssd_observer
}
