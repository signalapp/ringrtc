//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Simulation Create / Set Session Description Interface.

use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::ptr;

use libc::strdup;

use crate::core::util::RustObject;
use crate::webrtc::sdp_observer::{
    CreateSessionDescriptionObserver,
    CreateSessionDescriptionObserverCallbacks,
    SetSessionDescriptionObserver,
    SetSessionDescriptionObserverCallbacks,
};

/// Simulation type for SessionDescriptionInterface.
pub type RffiSessionDescriptionInterface = &'static str;

static FAKE_SDP: &str = "FAKE SDP";
static FAKE_SDP_OFFER: &str = "FAKE SDP OFFER";
static FAKE_SDP_ANSWER: &str = "FAKE SDP ANSWER";

/// Simulation type for webrtc::rffi::CreateSessionDescriptionObserverRffi
pub type RffiCreateSessionDescriptionObserver = u32;

static FAKE_CSD_OBSERVER: u32 = 13;

/// Simulation type for webrtc::rffi::SetSessionDescriptionObserverRffi
pub type RffiSetSessionDescriptionObserver = u32;

static FAKE_SSD_OBSERVER: u32 = 15;

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createSetSessionDescriptionObserver(
    ssd_observer: RustObject,
    ssd_observer_cb: *const c_void,
) -> *const RffiSetSessionDescriptionObserver {
    info!("Rust_createSetSessionDescriptionObserver():");

    // Hit the onSuccess() callback
    let call_backs = ssd_observer_cb as *const SetSessionDescriptionObserverCallbacks;
    ((*call_backs).onSuccess)(ssd_observer as *mut SetSessionDescriptionObserver);

    &FAKE_SSD_OBSERVER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_createCreateSessionDescriptionObserver(
    csd_observer: RustObject,
    csd_observer_cb: *const c_void,
) -> *const RffiCreateSessionDescriptionObserver {
    info!("Rust_createCreateSessionDescriptionObserver():");

    // Hit the onSuccess() callback
    let call_backs = csd_observer_cb as *const CreateSessionDescriptionObserverCallbacks;
    ((*call_backs).onSuccess)(
        csd_observer as *mut CreateSessionDescriptionObserver,
        &FAKE_SDP,
    );

    &FAKE_CSD_OBSERVER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_toSdp(sdi: *const RffiSessionDescriptionInterface) -> *const c_char {
    info!("Rust_toSdp(): ");
    match CString::new(*sdi) {
        Ok(cstr) => strdup(cstr.as_ptr()),
        Err(_) => ptr::null(),
    }
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_offerFromSdp(_sdp: *const c_char) -> *const RffiSessionDescriptionInterface {
    info!("Rust_offerFromSdp(): ");
    &FAKE_SDP_ANSWER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_answerFromSdp(_sdp: *const c_char) -> *const RffiSessionDescriptionInterface {
    info!("Rust_answerFromSdp(): ");
    &FAKE_SDP_OFFER
}

#[allow(non_snake_case, clippy::missing_safety_doc)]
pub unsafe fn Rust_replaceRtpDataChannelsWithSctp(
    _sdi: *const RffiSessionDescriptionInterface,
) -> bool {
    info!("Rust_replaceRtpDataChannelsWithSctp(): ");
    true
}
