//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS CallConnection Interface.

use std::collections::HashMap;
use std::fmt;
use std::thread;
use std::ffi::c_void;

use libc::size_t;

use crate::ios::call_connection_observer::{
    iOSCallConnectionObserver,
    IOSObserver,
};
use crate::ios::ios_util::*;
use crate::ios::webrtc_app_media_stream::AppMediaStream;
use crate::common::{
    Result,
    CallId,
    CallState,
    CallDirection,
};
use crate::core::call_connection::{
    CallConnectionInterface,
    CallConnectionHandle,
    ClientStreamTrait,
    ClientRecipientTrait,
};
use crate::core::call_connection_observer::{
    CallConnectionObserver,
    ClientEvent,
};
use crate::error::RingRtcError;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::media_stream::{
    MediaStream,
    RffiMediaStreamInterface,
};
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::sdp_observer::SessionDescriptionInterface;

/// Concrete type for iOS ClientStream objects.
#[allow(non_camel_case_types)]
pub type iOSClientStream = jlong;
impl ClientStreamTrait for iOSClientStream {}

/// Concrete type for iOS ClientRecipient objects.
#[allow(non_camel_case_types)]
pub type iOSClientRecipient = String;
impl ClientRecipientTrait for iOSClientRecipient {}

#[allow(non_camel_case_types)]
type iOSCallConnectionHandle = CallConnectionHandle<iOSCallConnection>;

/// iOS implementation of a core::CallConnectionInterface object.
#[allow(non_camel_case_types)]
pub struct iOSCallConnection {
    /// Raw pointer to C++ webrtc::PeerConnectionInterface object.
    pc_interface: Option<PeerConnection>,
    /// Rust DataChannel object.
    data_channel: Option<DataChannel>,
    /// Rust DataChannelObserver object.
    data_channel_observer: Option<DataChannelObserver<Self>>,
    /// Application (Swift) CallConnection object. Set when originating
    /// (sending offer) or receiving a call (receiving offer).
    app_call_connection: Option<jlong>,
    /// Call state variable.
    state: CallState,
    /// Unique call identifier.
    call_id: CallId,
    /// Call direction.
    direction: CallDirection,
    /// Application (Swift) Recipient object.
    recipient: IOSRecipient,
    /// CallConnectionObserver object.
    cc_observer: Option<Box<iOSCallConnectionObserver>>,
    /// For outgoing calls, buffer local ICE candidates until an SDP
    /// answer is received in response to the outbound SDP offer.
    pending_outbound_ice_candidates: Vec<IceCandidate>,
    /// For incoming calls, buffer remote ICE candidates until the SDP
    /// answer is sent in response to the remote SDP offer.
    pending_inbound_ice_candidates: Vec<IceCandidate>,
    stream_map: HashMap<*const RffiMediaStreamInterface, AppMediaStream>,
}

// needed to share raw *const pointer types
unsafe impl Sync for iOSCallConnection {}
unsafe impl Send for iOSCallConnection {}

impl fmt::Display for iOSCallConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(thread: {:?}, direction: {:?}, pc_interface: ({:?}), call_id: 0x{:x}, state: {:?})",
               thread::current().id(), self.direction, self.pc_interface, self.call_id, self.state)
    }
}

impl fmt::Debug for iOSCallConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for iOSCallConnection {
    fn drop(&mut self) {
        info!("Dropping iOSCallConnection");

        // Not currently dropping anything explicitly.
    }
}

impl CallConnectionInterface for iOSCallConnection {

    type ClientStream = iOSClientStream;
    type ClientRecipient = iOSClientRecipient;

    fn get_call_id(&self) -> CallId {
        self.call_id
    }

    fn get_state(&self) -> CallState {
        self.state
    }

    fn set_state(&mut self, state: CallState) {
        self.state = state;
    }

    fn get_direction(&self) -> CallDirection {
        self.direction
    }

    fn set_direction(&mut self, direction: CallDirection) {
        self.direction = direction;
    }

    fn get_pc_interface(&self) -> Result<&PeerConnection> {
        if let Some(pc_interface) = self.pc_interface.as_ref() {
            Ok(pc_interface)
        } else {
            Err(RingRtcError::OptionValueNotSet("get_pc_interface".to_string(),
                                                "pc_interface".to_string()).into())
        }
    }

    fn get_data_channel(&self) -> Result<&DataChannel> {
        if let Some(data_channel) = self.data_channel.as_ref() {
            Ok(data_channel)
        } else {
            Err(RingRtcError::OptionValueNotSet("get_data_channel".to_string(),
                                                "data_channel".to_string()).into())
        }
    }

    // Send an offer as the caller.
    fn send_offer(&self) -> Result<()> {
        let offer = self.create_offer()?;
        self.set_local_description(&offer)?;

        // send offer via signal
        app_send_offer(&self.recipient, self.call_id, offer)
     }

    // Received answer to a sent offer as caller.
    fn accept_answer(&mut self, answer: String) -> Result<()> {
        self.send_pending_ice_updates()?;

        let desc = SessionDescriptionInterface::create_sdp_answer(answer)?;
        self.set_remote_description(&desc)?;

        Ok(())
    }

    // Received offer as the callee.
    fn accept_offer(&self, offer: String) -> Result<()> {
        let desc = SessionDescriptionInterface::create_sdp_offer(offer)?;
        self.set_remote_description(&desc)?;

        let answer = self.create_answer()?;
        self.set_local_description(&answer)?;

        // send answer via signal
        app_send_answer(&self.recipient, self.call_id, answer)
    }

    fn add_local_candidate(&mut self, candidate: IceCandidate) {
        self.pending_outbound_ice_candidates.push(candidate);
        debug!("add_local_candidate(): outbound_ice_candidates: {}", self.pending_outbound_ice_candidates.len());
    }

    fn add_remote_candidate(&mut self, candidate: IceCandidate) {
        self.pending_inbound_ice_candidates.push(candidate);
        debug!("add_remote_candidate(): inbound_ice_candidates: {}", self.pending_inbound_ice_candidates.len());
    }

    fn send_pending_ice_updates(&mut self) -> Result<()> {
        if self.pending_outbound_ice_candidates.is_empty() {
            return Ok(());
        }

        debug!("send_pending_ice_updates(): Pending ICE candidates length: {}", self.pending_outbound_ice_candidates.len());

        // The format of the IceCandidate structure is not enough for iOS,
        // so we will convert to a more appropriate structure.
        let mut v: Vec<IOSIceCandidate> = Vec::new();

        for candidate in &self.pending_outbound_ice_candidates {

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
        let result = app_send_ice_updates(&self.recipient, self.call_id, ptr);

        // Get the Boxes back so they are automatically cleaned up.
        let ice_candidates = unsafe { Box::from_raw(ptr) };
        let _ = unsafe { Box::from_raw(ice_candidates.candidates as *mut IOSIceCandidate) };

        // @note We are currently clearing regardless of success of sending.
        self.pending_outbound_ice_candidates.clear();

        result
    }

    fn process_remote_ice_updates(&mut self) -> Result<()> {
        if self.pending_inbound_ice_candidates.is_empty() {
            return Ok(());
        }

        debug!("process_remote_ice_updates(): Remote ICE candidates length: {}", self.pending_inbound_ice_candidates.len());

        for candidate in &self.pending_inbound_ice_candidates {
            self.get_pc_interface()?.add_ice_candidate(candidate)?;
        }

        self.pending_inbound_ice_candidates.clear();

        Ok(())
    }

    fn send_signal_message_hang_up(&self) -> Result<()> {
        // send hangup via signal
        app_send_hangup(&self.recipient, self.call_id)
    }

    // @note send_busy is not supported on iOS.
    fn send_busy(&self, _recipient: Self::ClientRecipient, _call_id: CallId) -> Result<()> {
        // send busy via signal
        app_send_busy(&self.recipient, self.call_id)
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

    #[allow(clippy::map_entry)]
    fn notify_on_add_stream(&mut self, stream: MediaStream) -> Result<()> {
        debug!("ios:notify_on_add_stream():");

        let app_call_connection = self.get_app_call_connection()?;

        let media_stream_interface = stream.get_rffi_interface();
        
        if !self.stream_map.contains_key(&media_stream_interface) {
            let app_media_stream = AppMediaStream::new(app_call_connection as *const c_void, stream)?;
            self.stream_map.insert(media_stream_interface, app_media_stream);
        }
        
        let app_media_stream = &self.stream_map[&media_stream_interface];
        let app_media_stream_ref = app_media_stream.get_ref()?;
        
        if let Some(observer) = &self.cc_observer {
            observer.notify_on_add_stream(app_media_stream_ref as jlong);
        }

        Ok(())
    }

    fn on_data_channel(&mut self,
                       data_channel: DataChannel,
                       cc_handle:    CallConnectionHandle<Self>) -> Result<()>
    {
        debug!("on_data_channel()");
        let dc_observer = DataChannelObserver::new(cc_handle)?;
        data_channel.register_observer(dc_observer.get_rffi_interface())?;
        self.set_data_channel(data_channel);
        self.set_data_channel_observer(dc_observer);
        Ok(())
    }

}

impl iOSCallConnection {

    /// Create a new iOSCallConnection object.
    pub fn new(call_id: CallId, direction: CallDirection, recipient: IOSRecipient) -> Self {

        Self {
            pc_interface: None,
            data_channel: None,
            data_channel_observer: None,
            app_call_connection: None,
            state: CallState::Idle,
            call_id,
            direction,
            recipient,
            cc_observer: None,
            pending_outbound_ice_candidates: Vec::new(),
            pending_inbound_ice_candidates: Vec::new(),
            stream_map: Default::default(),
        }
    }

    /// Update a number of iOSCallConnection fields.
    ///
    /// Initializing an iOSCallConnection object is a multi-step
    /// process.  This step initializes the object using the input
    /// parameters.
    pub fn update_pc(&mut self,
                     pc_interface: PeerConnection,
                      cc_observer: Box<iOSCallConnectionObserver>) -> Result<()> {
        self.pc_interface = Some(pc_interface);
        self.cc_observer = Some(cc_observer);
        self.state = CallState::Idle;

        Ok(())
    }

    fn set_app_call_connection(&mut self, app_call_connection: jlong) {
        self.app_call_connection = Some(app_call_connection);
    }

    fn get_app_call_connection(&self) -> Result<jlong> {
        match self.app_call_connection.as_ref() {
            Some(v) => Ok(*v),
            None => Err(RingRtcError::OptionValueNotSet("get_app_call_connection()".to_string(),
                                                        "app_call_connection".to_string()).into()),
        }
    }

    /// Store the DataChannel
    pub fn set_data_channel(&mut self, data_channel: DataChannel) {
        self.data_channel = Some(data_channel);
    }

    /// Store the DataChannelObserver
    pub fn set_data_channel_observer(&mut self, data_channel_observer: DataChannelObserver<Self>) {
        self.data_channel_observer = Some(data_channel_observer);
    }

    /// Shutdown the CallConnection object.
    fn close(&mut self) {
        // dispose of all the media stream objects
        self.stream_map.clear();

        if let Some(data_channel) = self.data_channel.take().as_mut() {
            if let Some(dc_observer) = self.data_channel_observer.take().as_mut() {
                data_channel.unregister_observer(dc_observer.get_rffi_interface());
            }
            data_channel.dispose();
        }
    }

}

/// Close the CallConnection.
pub fn native_close_call_connection(call_connection: jlong) -> Result<()> {
    // We want to drop the handle when it goes out of scope here, as this
    // is the destructor.
    let mut cc_handle: Box<iOSCallConnectionHandle> = get_object_from_jlong(call_connection)?;
    cc_handle.terminate()?;
    if let Ok(mut cc) = cc_handle.lock() {
        cc.close();
    }
    Ok(())
}

/// Send the SDP offer via the Application (Swift).
fn app_send_offer(recipient: &IOSRecipient,
                    call_id: CallId,
                      offer: SessionDescriptionInterface) -> Result<()> {
    let description = offer.get_description()?;
    info!("app_send_offer(): {}", description);

    let string_slice = IOSByteSlice{
        bytes: description.as_ptr(),
        len: description.len() as size_t,
    };

    (recipient.onSendOffer)(recipient.object, call_id, string_slice);

    info!("app_send_offer(): complete");

    Ok(())
}

/// Inject a SendOffer event to the FSM.
pub fn native_send_offer(call_connection: jlong,
                     app_call_connection: jlong) -> Result<()> {

    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    if let Ok(mut cc) = cc_handle.lock() {
        cc.set_app_call_connection(app_call_connection);
    }

    cc_handle.inject_send_offer()
}

/// Create a Rust CallConnectionObserver.
pub fn native_create_call_connection_observer(app_observer: IOSObserver,
                                                   call_id: jlong) -> Result<jlong> {
    let cc_observer = iOSCallConnectionObserver::new(app_observer, call_id);

    let cc_observer_box = Box::new(cc_observer);
    Ok(Box::into_raw(cc_observer_box) as jlong)
}

/// Inject an AcceptAnswer event into the FSM.
pub fn native_handle_offer_answer(call_connection: jlong,
                                           answer: &str) -> Result<()> {
    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    cc_handle.inject_accept_answer(answer.to_string())
}

/// Send an SDP answer via Application (Swift).
fn app_send_answer(recipient: &IOSRecipient,
                     call_id: CallId,
                      answer: SessionDescriptionInterface) -> Result<()> {
    let description = answer.get_description()?;
    info!("app_send_answer(): {}", description);

    let string_slice = IOSByteSlice{
        bytes: description.as_ptr(),
        len: description.len() as size_t,
    };

    (recipient.onSendAnswer)(recipient.object, call_id, string_slice);

    info!("app_send_answer(): complete");

    Ok(())
}

/// Inject an AcceptOffer event into the FSM.
pub fn native_accept_offer(call_connection: jlong,
                       app_call_connection: jlong,
                                     offer: &str) -> Result<()> {
    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    if let Ok(mut cc) = cc_handle.lock() {
        cc.set_app_call_connection(app_call_connection);
    }

    cc_handle.inject_accept_offer(offer.to_string())
}

/// Send a ICE update to the remote peer via Application (Swift).
fn app_send_ice_updates(recipient: &IOSRecipient,
                          call_id: CallId,
                       candidates: *const IOSIceCandidateArray) -> Result<()> {
    info!("app_send_ice_updates():");

    (recipient.onSendIceCandidates)(recipient.object, call_id, candidates);

    info!("app_send_ice_updates(): complete");

    Ok(())
}

/// Send a HangUp message to the remote peer via Application (Swift).
fn app_send_hangup(recipient: &IOSRecipient,
                     call_id: CallId) -> Result<()> {
    info!("app_send_hangup():");

    (recipient.onSendHangup)(recipient.object, call_id);

    info!("app_send_hangup(): complete");

    Ok(())
}

/// Inject a HangUp event into the FSM.
pub fn native_hang_up(call_connection: jlong) -> Result<()> {
    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    cc_handle.inject_hang_up()
}

/// Inject a AnswerCall event into the FSM.
pub fn native_answer_call(call_connection: jlong) -> Result<()> {
    info!("native_answer_call():");

    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    cc_handle.inject_answer_call()
}

/// Inject a LocalVideoStatus event into the FSM.
pub fn native_send_video_status(call_connection: jlong,
                                        enabled: bool) -> Result<()> {
    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;
    cc_handle.inject_local_video_status(enabled)
}

/// Inject a RemoteIceCandidate event into the FSM.
pub fn native_add_ice_candidate(call_connection:  jlong,
                                        sdp_mid:  &str,
                                sdp_mline_index:  i32,
                                            sdp:  &str) -> Result<()> {
    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    let ice_candidate = IceCandidate::new(
        sdp_mid.to_string(),
        sdp_mline_index,
        sdp.to_string(),
    );

    cc_handle.inject_remote_ice_candidate(ice_candidate)
}

/// Send a Busy message to the remote peer via Application (Swift).
fn app_send_busy(recipient: &IOSRecipient,
                   call_id: CallId) -> Result<()> {
    info!("app_send_busy():");

    (recipient.onSendBusy)(recipient.object, call_id);

    info!("app_send_busy(): complete");

    Ok(())
}

/// Inject a SendBusy event into the FSM.
/// @note This function isn't supported on iOS at this time.
pub fn native_send_busy(call_connection: jlong,
                                call_id: CallId) -> Result<()> {
    let cc_handle: &mut iOSCallConnectionHandle = get_object_ref_from_jlong(call_connection)?;

    cc_handle.inject_send_busy("unknown".to_string(), call_id)
}
