//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS CallPlatform Interface.

use std::fmt;
use std::ffi::c_void;

use libc::size_t;

use crate::ios::call_connection_observer::{
    IOSCallConnectionObserver,
    IOSObserver,
};
use crate::ios::ios_util::*;
use crate::ios::webrtc_ios_media_stream::IOSMediaStream;
use crate::common::{
    Result,
    CallId,
    CallState,
};
use crate::core::call_connection::{
    AppMediaStreamTrait,
    CallConnection,
    CallPlatform,
};
use crate::core::call_connection_observer::{
    CallConnectionObserver,
    ClientEvent,
};
use crate::core::util::{
    ptr_as_box,
    ptr_as_mut,
};
use crate::error::RingRtcError;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media_stream::MediaStream;
use crate::webrtc::sdp_observer::SessionDescriptionInterface;

/// Concrete type for iOS AppMediaStream objects.
impl AppMediaStreamTrait for IOSMediaStream {}

/// Public type for iOS CallConnection.
pub type IOSCallConnection = CallConnection<IOSPlatform>;

/// iOS implementation of a core::CallPlatform object.
pub struct IOSPlatform {
    /// Application (Swift) CallConnection object. Set when originating
    /// (sending offer) or receiving a call (receiving offer).
    app_call_connection: Option<*mut AppCallConnection>,
    /// Application (Swift) Recipient object.
    recipient: IOSRecipient,
    /// CallConnectionObserver object.
    cc_observer: Option<Box<IOSCallConnectionObserver>>,
}

// needed to share raw *const pointer types
unsafe impl Sync for IOSPlatform {}
unsafe impl Send for IOSPlatform {}

impl fmt::Display for IOSPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let app_call_connection = match self.app_call_connection {
            Some(v) => format!("{:p}", v),
            None    => "None".to_string(),
        };
        write!(f, "app_call_connection: {}", app_call_connection)
    }
}

impl fmt::Debug for IOSPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for IOSPlatform {
    fn drop(&mut self) {
        info!("Dropping IOSPlatform");

        // Not currently dropping anything explicitly.
    }
}

impl CallPlatform for IOSPlatform {

    type AppMediaStream = IOSMediaStream;

    // Send an offer as the caller.
    fn app_send_offer(&self,
                      call_id:   CallId,
                      offer:     SessionDescriptionInterface) -> Result<()> {

        let description = offer.get_description()?;
        info!("app_send_offer():");

        let string_slice = IOSByteSlice{
            bytes: description.as_ptr(),
            len: description.len() as size_t,
        };

        (self.recipient.onSendOffer)(self.recipient.object, call_id, string_slice);

        info!("app_send_offer(): complete");

        Ok(())
    }

    fn app_send_answer(&self,
                       call_id:   CallId,
                       answer:    SessionDescriptionInterface) -> Result<()> {

        let description = answer.get_description()?;
        info!("app_send_answer():");

        let string_slice = IOSByteSlice{
            bytes: description.as_ptr(),
            len: description.len() as size_t,
        };

        (self.recipient.onSendAnswer)(self.recipient.object, call_id, string_slice);

        info!("app_send_answer(): complete");

        Ok(())
    }

    fn app_send_ice_updates(&self,
                            call_id:    CallId,
                            candidates: &[IceCandidate]) -> Result<()> {

        if candidates.is_empty() {
            return Ok(());
        }

        // The format of the IceCandidate structure is not enough for iOS,
        // so we will convert to a more appropriate structure.
        let mut v: Vec<IOSIceCandidate> = Vec::new();

        for candidate in candidates {

            let sdp_mid_slice = IOSByteSlice {
                bytes: candidate.sdp_mid.as_ptr(),
                len: candidate.sdp_mid.len() as size_t,
            };

            let sdp_slice = IOSByteSlice {
                bytes: candidate.sdp.as_ptr(),
                len: candidate.sdp.len() as size_t,
            };

            let ice_candidate = IOSIceCandidate {
                sdpMid: sdp_mid_slice,
                sdpMLineIndex: candidate.sdp_mline_index,
                sdp: sdp_slice,
            };

            v.push(ice_candidate);
        }

        let v_len = v.len();

        let ice_candidates = Box::new(IOSIceCandidateArray {
            candidates: Box::into_raw(v.into_boxed_slice()) as *const IOSIceCandidate,
            count: v_len
        });

        // We pass pointers to the strings up and the handler is expected
        // to consume (i.e. copy) the data immediately.
        let ptr = Box::into_raw(ice_candidates);

        info!("app_send_ice_updates():");

        (self.recipient.onSendIceCandidates)(self.recipient.object, call_id, ptr);

        info!("app_send_ice_updates(): complete");

        let result = Ok(());

        // Get the Boxes back so they are automatically cleaned up.
        let ice_candidates = unsafe { Box::from_raw(ptr) };
        let _ = unsafe { Box::from_raw(ice_candidates.candidates as *mut IOSIceCandidate) };

        result
    }

    fn app_send_hangup(&self, call_id: CallId) -> Result<()> {

        info!("app_send_hangup():");

        (self.recipient.onSendHangup)(self.recipient.object, call_id);

        info!("app_send_hangup(): complete");

        Ok(())
    }

    fn create_media_stream(&self, stream: MediaStream) -> Result<Self::AppMediaStream> {
        IOSMediaStream::new(self.get_app_call_connection()?, stream)
    }

    fn notify_client(&self, event: ClientEvent) -> Result<()> {
        if let Some(observer) = &self.cc_observer {
            info!("ios:notify_client(): event: {}", event);
            observer.notify_event(event);
        }

        Ok(())
    }

    fn notify_error(&self, error: failure::Error) -> Result<()> {
        if let Some(observer) = &self.cc_observer {
            info!("ios:notify_error(): {}", error);
            observer.notify_error(error);
        }

        Ok(())
    }

    /// Notify the client application about an avilable MediaStream.
    fn notify_on_add_stream(&self, stream: &Self::AppMediaStream) -> Result<()> {
        debug!("ios:notify_on_add_stream():");

        let ios_media_stream = stream as &IOSMediaStream;
        let ios_media_stream_ref = ios_media_stream.get_ref()?;
        if let Some(observer) = &self.cc_observer {
            observer.notify_on_add_stream(ios_media_stream_ref as *mut c_void);
        }

        Ok(())
    }

}

impl IOSPlatform {

    /// Create a new IOSPlatform object.
    pub fn new(recipient: IOSRecipient) -> Self {

        Self {
            app_call_connection: None,
            recipient,
            cc_observer: None,
        }
    }

    /// Update the CallConnection observer object.
    pub fn set_cc_observer(&mut self, cc_observer: Box<IOSCallConnectionObserver>) {
        self.cc_observer = Some(cc_observer);
    }

    /// Update the application's Swift CallConnection object.
    fn set_app_call_connection(&mut self, app_call_connection: *mut AppCallConnection) {
        self.app_call_connection = Some(app_call_connection);
    }

    /// Return the application's Swift CallConnection object.
    fn get_app_call_connection(&self) -> Result<*mut AppCallConnection> {
        match self.app_call_connection.as_ref() {
            Some(v) => Ok(*v),
            None => Err(RingRtcError::OptionValueNotSet("get_app_call_connection()".to_string(),
                                                        "app_call_connection".to_string()).into()),
        }
    }

}

/// Close the CallConnection and quiesce related threads.
pub fn native_close_call_connection(call_connection: *mut IOSCallConnection) -> Result<()> {


    let cc = unsafe { ptr_as_mut(call_connection)? };
    cc.close()

//    // We want to drop the handle when it goes out of scope here, as this
//    // is the destructor, so convert the pointer back into a box.
//    let mut cc_box = unsafe { ptr_as_box(call_connection)? };
//    cc_box.close()
}

/// Dispose of the CallConnection allocated on the heap.
pub fn native_dispose_call_connection(call_connection: *mut IOSCallConnection) -> Result<()> {

    // Convert the pointer back into a box, allowing it to go out of
    // scope.
    let cc_box = unsafe { ptr_as_box(call_connection)? };

    debug_assert_eq!(CallState::Closed, cc_box.state()?,
                     "Must call close() before calling dispose()!");

    Ok(())
}

/// Inject a SendOffer event to the FSM.
pub fn native_send_offer(call_connection: *mut IOSCallConnection,
                     app_call_connection: *mut AppCallConnection) -> Result<()> {

    let cc = unsafe { ptr_as_mut(call_connection)? };

    if let Ok(mut platform) = cc.platform() {
        platform.set_app_call_connection(app_call_connection);
    }

    cc.inject_send_offer()
}

/// Create a Rust CallConnectionObserver.
pub fn native_create_call_connection_observer(app_observer: IOSObserver,
                                                   call_id: CallId) -> Result<*mut c_void> {
    let cc_observer = IOSCallConnectionObserver::new(app_observer, call_id);

    let cc_observer_box = Box::new(cc_observer);
    Ok(Box::into_raw(cc_observer_box) as *mut c_void)
}

/// Inject an HandleAnswer event into the FSM.
pub fn native_handle_answer(call_connection: *mut IOSCallConnection,
                            answer: &str) -> Result<()> {
    let cc = unsafe { ptr_as_mut(call_connection)? };

    cc.inject_handle_answer(answer.to_string())
}

/// Inject a HandleOffer event into the FSM.
pub fn native_handle_offer(call_connection: *mut IOSCallConnection,
                       app_call_connection: *mut AppCallConnection,
                                     offer: &str) -> Result<()> {
    let cc = unsafe { ptr_as_mut(call_connection)? };

    if let Ok(mut platform) = cc.platform() {
        platform.set_app_call_connection(app_call_connection);
    }

    cc.inject_handle_offer(offer.to_string())
}

/// Inject a HangUp event into the FSM.
pub fn native_hang_up(call_connection: *mut IOSCallConnection) -> Result<()> {
    let cc = unsafe { ptr_as_mut(call_connection)? };
    cc.inject_hang_up()
}

/// Inject an AcceptCall event into the FSM.
pub fn native_accept_call(call_connection: *mut IOSCallConnection) -> Result<()> {
    info!("native_accept_call():");

    let cc = unsafe { ptr_as_mut(call_connection)? };
    cc.inject_accept_call()
}

/// Inject a LocalVideoStatus event into the FSM.
pub fn native_send_video_status(call_connection: *mut IOSCallConnection,
                                        enabled: bool) -> Result<()> {
    let cc = unsafe { ptr_as_mut(call_connection)? };
    cc.inject_local_video_status(enabled)
}

/// Inject a RemoteIceCandidate event into the FSM.
pub fn native_add_ice_candidate(call_connection:  *mut IOSCallConnection,
                                        sdp_mid:  &str,
                                sdp_mline_index:  i32,
                                            sdp:  &str) -> Result<()> {
    let cc = unsafe { ptr_as_mut(call_connection)? };

    let ice_candidate = IceCandidate::new(
        sdp_mid.to_string(),
        sdp_mline_index,
        sdp.to_string(),
    );

    cc.inject_remote_ice_candidate(ice_candidate)
}
