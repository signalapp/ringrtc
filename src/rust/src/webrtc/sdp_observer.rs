//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Create Session Description Interface.

use std::ffi::{c_void, CStr, CString};
use std::fmt;
use std::os::raw::c_char;
use std::ptr;
use std::sync::{Arc, Condvar, Mutex};

use crate::common::Result;
use crate::core::util::{ptr_as_ref, FutureResult, RustObject};
use crate::error::RingRtcError;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::sdp_observer as sdp;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::sdp_observer::RffiSessionDescriptionInterface;

#[cfg(feature = "sim")]
use crate::webrtc::sim::sdp_observer as sdp;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::sdp_observer::RffiSessionDescriptionInterface;

/// Rust wrapper around WebRTC C++ SessionDescriptionInterface.
pub struct SessionDescriptionInterface {
    /// Pointer to C++ SessionDescriptionInterface object.
    sd_interface: *const RffiSessionDescriptionInterface,
}

impl fmt::Display for SessionDescriptionInterface {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "sd_interface: {:p}", self.sd_interface)
    }
}

impl fmt::Debug for SessionDescriptionInterface {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl SessionDescriptionInterface {
    /// Create a new SessionDescriptionInterface from a C++ SessionDescriptionInterface object.
    pub fn new(sd_interface: *const RffiSessionDescriptionInterface) -> Self {
        Self { sd_interface }
    }

    /// Return the internal WebRTC C++ SessionDescriptionInterface pointer.
    pub fn rffi_interface(&self) -> *const RffiSessionDescriptionInterface {
        self.sd_interface
    }

    /// Return SDP representation of this SessionDescriptionInterface.
    pub fn to_sdp(&self) -> Result<String> {
        let sdp_ptr = unsafe { sdp::Rust_toSdp(self.sd_interface) };
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
        Ok(SessionDescriptionInterface::new(answer))
    }

    /// Create a SDP offer from the session description string.
    pub fn offer_from_sdp(sdp: String) -> Result<Self> {
        let sdp = CString::new(sdp)?;
        let offer = unsafe { sdp::Rust_offerFromSdp(sdp.as_ptr()) };
        if offer.is_null() {
            return Err(RingRtcError::ConvertSdpOffer.into());
        }
        Ok(SessionDescriptionInterface::new(offer))
    }

    pub fn replace_rtp_data_channels_with_sctp(&mut self) -> Result<()> {
        let success = unsafe { sdp::Rust_replaceRtpDataChannelsWithSctp(self.sd_interface) };
        if success {
            Ok(())
        } else {
            Err(RingRtcError::MungeSdp.into())
        }
    }
}

#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::sdp_observer::RffiCreateSessionDescriptionObserver;

#[cfg(feature = "sim")]
pub use crate::webrtc::sim::sdp_observer::RffiCreateSessionDescriptionObserver;

/// Observer object for creating a session description.
#[derive(Debug)]
pub struct CreateSessionDescriptionObserver {
    /// condition varialbe used to signal the completion of the create
    /// session description operation.
    condition:         FutureResult<Result<*const RffiSessionDescriptionInterface>>,
    /// Pointer to C++ webrtc::rffi::CreateSessionDescriptionObserverRffi object
    rffi_csd_observer: *const RffiCreateSessionDescriptionObserver,
}

impl CreateSessionDescriptionObserver {
    /// Create a new CreateSessionDescriptionObserver.
    fn new() -> Self {
        Self {
            condition:         Arc::new((Mutex::new((false, Ok(ptr::null()))), Condvar::new())),
            rffi_csd_observer: ptr::null(),
        }
    }

    /// Called back when the create session description operation is a
    /// success.
    ///
    /// This call signals the condition variable.
    fn on_create_success(&self, desc: *const RffiSessionDescriptionInterface) {
        info!("on_create_success()");
        let &(ref mtx, ref cvar) = &*self.condition;
        if let Ok(mut guard) = mtx.lock() {
            guard.1 = Ok(desc);
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
    pub fn get_result(&self) -> Result<SessionDescriptionInterface> {
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
                Ok(v) => Ok(SessionDescriptionInterface::new(*v)),
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
    pub fn set_rffi_observer(&mut self, observer: *const RffiCreateSessionDescriptionObserver) {
        self.rffi_csd_observer = observer
    }

    /// Return the RFFI observer object.
    pub fn rffi_observer(&self) -> *const RffiCreateSessionDescriptionObserver {
        self.rffi_csd_observer
    }
}

/// CreateSessionDescription observer OnSuccess() callback.
#[no_mangle]
#[allow(non_snake_case)]
extern "C" fn csd_observer_OnSuccess(
    csd_observer: *mut CreateSessionDescriptionObserver,
    desc: *const RffiSessionDescriptionInterface,
) {
    info!("csd_observer_OnSuccess()");
    match unsafe { ptr_as_ref(csd_observer) } {
        Ok(v) => v.on_create_success(desc),
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
        desc: *const RffiSessionDescriptionInterface,
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
    let rffi_csd_observer = unsafe {
        sdp::Rust_createCreateSessionDescriptionObserver(
            csd_observer_ptr as RustObject,
            CSD_OBSERVER_CBS_PTR as *const c_void,
        )
    };
    let mut csd_observer = unsafe { Box::from_raw(csd_observer_ptr) };

    csd_observer.set_rffi_observer(rffi_csd_observer);
    csd_observer
}

#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::sdp_observer::RffiSetSessionDescriptionObserver;

#[cfg(feature = "sim")]
pub use crate::webrtc::sim::sdp_observer::RffiSetSessionDescriptionObserver;

/// Observer object for setting a session description.
#[derive(Debug)]
pub struct SetSessionDescriptionObserver {
    /// condition varialbe used to signal the completion of the set
    /// session description operation.
    condition:         FutureResult<Result<()>>,
    /// Pointer to C++ CreateSessionDescriptionObserver object
    rffi_ssd_observer: *const RffiSetSessionDescriptionObserver,
}

impl SetSessionDescriptionObserver {
    /// Create a new SetSessionDescriptionObserver.
    fn new() -> Self {
        Self {
            condition:         Arc::new((Mutex::new((false, Ok(()))), Condvar::new())),
            rffi_ssd_observer: ptr::null(),
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
    pub fn set_rffi_observer(&mut self, observer: *const RffiSetSessionDescriptionObserver) {
        self.rffi_ssd_observer = observer
    }

    /// Return the RFFI observer object.
    pub fn rffi_observer(&self) -> *const RffiSetSessionDescriptionObserver {
        self.rffi_ssd_observer
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

    ssd_observer.set_rffi_observer(rffi_ssd_observer as *const RffiSetSessionDescriptionObserver);
    ssd_observer
}
