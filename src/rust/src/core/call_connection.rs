//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! A peer-to-peer call connection interface.

extern crate tokio;

use std::collections::HashMap;
use std::fmt;
use std::sync::{
    Arc,
    Condvar,
    Mutex,
    MutexGuard,
};
use std::thread;

use crate::common::{
    CallDirection,
    CallId,
    CallState,
    Result,
};
use crate::core::call_connection_factory::EventPump;
use crate::core::call_connection_observer::ClientEvent;
use crate::core::call_fsm::CallEvent;
use crate::core::util::redact_string;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media_stream::{
    MediaStream,
    RffiMediaStreamInterface,
};
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::sdp_observer::{
    SessionDescriptionInterface,
    create_csd_observer,
    create_ssd_observer,
};

/// A simple wrapper around std::sync::Mutex
pub struct CallMutex<T: ?Sized> {
    /// Human readable label for the mutex
    label: String,
    /// The actual mutex
    mutex: Mutex<T>,
}

unsafe impl<T: ?Sized + Send> Send for CallMutex<T> { }
unsafe impl<T: ?Sized + Send> Sync for CallMutex<T> { }

impl<T> CallMutex<T> {
    /// Creates a new CallMutex
    pub fn new(t: T, label: &str) -> CallMutex<T> {
        CallMutex {
            mutex: Mutex::new(t),
            label: label.to_string(),
        }
    }

    /// Wrapper around std::mpsc::Mutex::lock() that on error consumes
    /// the poisoned mutex and returns a simple error code.
    pub fn lock(&self) -> Result<MutexGuard<'_, T>> {
        match self.mutex.lock() {
            Ok(v) => Ok(v),
            Err(_) => Err(RingRtcError::MutexPoisoned(self.label.clone()).into()),
        }
    }
}

/// A trait representing a platform independent media stream.
pub trait AppMediaStreamTrait : Sync + Send + 'static {}

/// A trait describing the interface an operating system platform must
/// implement for calling.
pub trait CallPlatform : fmt::Debug + fmt::Display + Sync + Send + 'static
where
    <Self as CallPlatform>::AppMediaStream: AppMediaStreamTrait,
{

    type AppMediaStream;

    /// Send an SDP offer to a remote peer using the signaling
    /// channel.
    fn app_send_offer(&self,
                      call_id:   CallId,
                      offer:     SessionDescriptionInterface) -> Result<()>;

    /// Send an SDP answer to a remote peer using the signaling
    /// channel.
    fn app_send_answer(&self,
                       call_id:   CallId,
                       answer:    SessionDescriptionInterface) -> Result<()>;

    /// Send ICE Candidates to a remote peer using the signaling
    /// channel.
    fn app_send_ice_updates(&self,
                            call_id:    CallId,
                            candidates: &[IceCandidate]) -> Result<()>;

    /// Send a call hangup message to a remote peer using the
    /// signaling channel.
    fn app_send_hangup(&self, call_id: CallId) -> Result<()>;

    /// Create a platform dependent media stream from the base WebRTC
    /// MediaStream.
    fn create_media_stream(&self, stream: MediaStream) -> Result<Self::AppMediaStream>;

    /// Notify the client application about an event.
    fn notify_client(&self, event: ClientEvent) -> Result<()>;

    /// Notify the client application about an error.
    fn notify_error(&self, error: failure::Error) -> Result<()>;

    /// Notify the client application about an avilable MediaStream.
    fn notify_on_add_stream(&self, stream: &Self::AppMediaStream) -> Result<()>;
}

/// Encapsulates several WebRTC objects associated with the
/// CallConnection object.
pub struct WebRtcData<T>
where
    T: CallPlatform,
{
    /// PeerConnection object
    pc_interface:          Option<PeerConnection>,
    /// DataChannel object
    data_channel:          Option<DataChannel>,
    /// DataChannelObserver object
    data_channel_observer: Option<DataChannelObserver<T>>,
    /// HaspMap, mapping WebRTC MediaStreams to platform specific
    /// AppMediaStreams.
    stream_map:            HashMap<*const RffiMediaStreamInterface,
                                   <T as CallPlatform>::AppMediaStream>,
}

// Send and Sync needed to share *const pointer types across threads.
unsafe impl<T> Send for WebRtcData<T>
where
    T: CallPlatform,
{}

unsafe impl<T> Sync for WebRtcData<T>
where
    T: CallPlatform,
{}

impl<T> fmt::Display for WebRtcData<T>
where
    T: CallPlatform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "pc_interface: {:?}, data_channel: {:?}, data_channel_observer: {:?}",
               self.pc_interface,
               self.data_channel,
               self.data_channel_observer)
    }
}

impl<T> fmt::Debug for WebRtcData<T>
where
    T: CallPlatform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}


impl<T> WebRtcData<T>
where
    T: CallPlatform,
{
    fn pc_interface(&self) -> Result<&PeerConnection> {
        match self.pc_interface.as_ref() {
            Some(v) => Ok(v),
            None    => Err(RingRtcError::OptionValueNotSet("pc_interface".to_string(),
                                                           "pc_interface".to_string()).into()),
        }
    }

    fn data_channel(&self) -> Result<&DataChannel> {
        match self.data_channel.as_ref() {
            Some(v) => Ok(v),
            None    => Err(RingRtcError::OptionValueNotSet("data_channel".to_string(),
                                                               "data_channel".to_string()).into()),
        }
    }
}

/// Represents the connection between a local client and one remote
/// peer.
///
/// This object is thread-safe.
pub struct CallConnection<T>
where
    T: CallPlatform,
{
    /// Injects events into the [CallStateMachine](../call_fsm/struct.CallStateMachine.html).
    event_pump:                      EventPump<T>,
    /// Unique 64-bit number identifying the call.
    call_id:                         CallId,
    /// The call direction, inbound or outbound.
    direction:                       CallDirection,
    /// The current state of the call connection
    state:                           Arc<CallMutex<CallState>>,
    /// Ancillary WebRTC data.
    webrtc:                          Arc<CallMutex<WebRtcData<T>>>,
    platform:                        Arc<CallMutex<T>>,
    pending_outbound_ice_candidates: Arc<CallMutex<Vec<IceCandidate>>>,
    pending_inbound_ice_candidates:  Arc<CallMutex<Vec<IceCandidate>>>,

    /// Condition variable used at termination to quiesce and synchronize the FSM.
    terminate_condvar:               Arc<(Mutex<bool>, Condvar)>,
}

impl<T> fmt::Display for CallConnection<T>
where
    T: CallPlatform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let webrtc = match self.webrtc.lock() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        let platform = match self.platform.lock() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        let state = match self.state() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        write!(f, "thread: {:?}, direction: {:?}, call_id: 0x{:x}, state: {:?}, webrtc: ({}), platform: ({})",
               thread::current().id(), self.direction, self.call_id, state, webrtc, platform)
    }
}

impl<T> fmt::Debug for CallConnection<T>
where
    T: CallPlatform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Drop for CallConnection<T>
where
    T: CallPlatform,
{
    fn drop(&mut self) {
        debug!("Dropping CallConnection: ref_count: {}", self.ref_count());
    }
}

impl<T> Clone for CallConnection<T>
where
    T: CallPlatform,
{
    fn clone(&self) -> Self {
        CallConnection {
            event_pump:                      self.event_pump.clone(),
            call_id:                         self.call_id,
            direction:                       self.direction,
            state:                           Arc::clone(&self.state),
            webrtc:                          Arc::clone(&self.webrtc),
            platform:                        Arc::clone(&self.platform),
            pending_outbound_ice_candidates: Arc::clone(&self.pending_outbound_ice_candidates),
            pending_inbound_ice_candidates:  Arc::clone(&self.pending_inbound_ice_candidates),
            terminate_condvar:               Arc::clone(&self.terminate_condvar),
        }
    }
}

impl<T> CallConnection<T>
where
    T: CallPlatform,
{

    /// Create a new CallConnection.
    #[allow(clippy::mutex_atomic)]
    pub fn new(event_pump: EventPump<T>,
               call_id:    CallId,
               direction:  CallDirection,
               platform:   T) -> Self {

        let webrtc = WebRtcData {
            pc_interface:          None,
            data_channel:          None,
            data_channel_observer: None,
            stream_map:            Default::default(),
        };

        Self {
            event_pump,
            call_id,
            direction,
            state:                           Arc::new(CallMutex::new(CallState::Idle, "state")),
            webrtc:                          Arc::new(CallMutex::new(webrtc, "webrtc")),
            platform:                        Arc::new(CallMutex::new(platform, "platform")),
            pending_outbound_ice_candidates: Arc::new(CallMutex::new(Vec::new(), "pending_outbound_ice_candidates")),
            pending_inbound_ice_candidates:  Arc::new(CallMutex::new(Vec::new(), "pending_inbound_ice_candidates")),
            terminate_condvar:               Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// Return the Call identifier.
    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    /// Return the Call direction.
    pub fn direction(&self) -> CallDirection {
        self.direction
    }

    /// Return the platform specific data, under a locked mutex.
    pub fn platform(&self) -> Result<MutexGuard<'_, T>> {
        self.platform.lock()
    }

    /// Return the current Call state.
    pub fn state(&self) -> Result<CallState> {
        let state = self.state.lock()?;
        Ok(*state)
    }

    /// Update the current Call state.
    pub fn set_state(&self, new_state: CallState) -> Result<()> {
        let mut state = self.state.lock()?;
        *state = new_state;
        Ok(())
    }

    /// Update the webrtc::PeerConnection interface.
    pub fn set_pc_interface(&self, pc_interface: PeerConnection) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        webrtc.pc_interface = Some(pc_interface);
        Ok(())
    }

    /// Update the webrtc::DataChannel interface.
    pub fn set_data_channel(&self, data_channel: DataChannel) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        webrtc.data_channel = Some(data_channel);
        Ok(())
    }

    /// Update the webrtc::DataChannelObserver interface.
    pub fn set_data_channel_observer(&self, observer: DataChannelObserver<T>) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        webrtc.data_channel_observer = Some(observer);
        Ok(())
    }

    /// Clone the CallConnection, Box it and return a raw pointer to the Box.
    pub fn create_call_connection_ptr(&self) -> *mut CallConnection<T> {
        let new_handle = self.clone();
        let cc_handle_box = Box::new(new_handle);
        Box::into_raw(cc_handle_box)
    }

    /// Returns `true` if the call is terminating.
    pub fn terminating(&self) -> Result<bool> {
        if let CallState::Terminating = self.state()? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Return the strong reference count on the platform `Arc<Mutex<>>`.
    fn ref_count(&self) -> usize {
        Arc::strong_count(&self.webrtc)
    }

    /// Create an SDP offer message.
    fn create_offer(&self) -> Result<SessionDescriptionInterface> {
        let csd_observer = create_csd_observer();

        let webrtc = self.webrtc.lock()?;
        webrtc.pc_interface()?.create_offer(csd_observer.as_ref());
        csd_observer.get_result()
    }

    /// Create an SDP answer message.
    fn create_answer(&self) -> Result<SessionDescriptionInterface> {
        let csd_observer = create_csd_observer();

        let webrtc = self.webrtc.lock()?;
        webrtc.pc_interface()?.create_answer(csd_observer.as_ref());
        csd_observer.get_result()
    }

    /// Set the local SPD decription in the PeerConnection interface.
    fn set_local_description(&self, desc: &SessionDescriptionInterface) -> Result<()> {
        let ssd_observer = create_ssd_observer();

        let webrtc = self.webrtc.lock()?;
        webrtc.pc_interface()?.set_local_description(ssd_observer.as_ref(), desc);
        ssd_observer.get_result()
    }

    /// Set the remote SPD decription in the PeerConnection interface.
    fn set_remote_description(&self, desc: &SessionDescriptionInterface) -> Result<()> {
        let ssd_observer = create_ssd_observer();

        let webrtc = self.webrtc.lock()?;
        webrtc.pc_interface()?.set_remote_description(ssd_observer.as_ref(), desc);
        ssd_observer.get_result()
    }

    /// Send an SDP offer message to the remote peer via the signaling
    /// channel.
    pub fn send_offer(&self) -> Result<()> {
        let offer = self.create_offer()?;
        self.set_local_description(&offer)?;

        info!("TX SDP offer:\n{}", redact_string(&offer.get_description()?));

        let platform = self.platform.lock()?;
        platform.app_send_offer(self.call_id, offer)
    }

    /// Handle an incoming SDP answer message.
    pub fn handle_answer(&self, answer: String) -> Result<()> {
        self.send_pending_ice_updates()?;

        let desc = SessionDescriptionInterface::create_sdp_answer(answer)?;
        self.set_remote_description(&desc)?;
        Ok(())
    }

    /// Handle an incoming SDP offer message.
    pub fn handle_offer(&self, offer: String) -> Result<()> {
        let desc = SessionDescriptionInterface::create_sdp_offer(offer)?;
        self.set_remote_description(&desc)?;

        let answer = self.create_answer()?;
        self.set_local_description(&answer)?;

        info!("TX SDP answer:\n{}", redact_string(&answer.get_description()?));

        let platform = self.platform.lock()?;
        platform.app_send_answer(self.call_id, answer)

    }

    /// Buffer local ICE candidates.
    pub fn buffer_local_ice_candidate(&self, candidate: IceCandidate) -> Result<()> {

        info!("Local ICE candidate: {}", candidate);

        let mut ice_candidates = self.pending_outbound_ice_candidates.lock()?;
        ice_candidates.push(candidate);
        Ok(())
    }

    /// Buffer remote ICE candidates.
    pub fn buffer_remote_ice_candidate(&self, candidate: IceCandidate) -> Result<()> {

        info!("Remote ICE candidate: {}", candidate);

        let mut ice_candidates = self.pending_inbound_ice_candidates.lock()?;
        ice_candidates.push(candidate);
        Ok(())
    }

    /// Send pending local ICE candidates to the remote peer.
    pub fn send_pending_ice_updates(&self) -> Result<()> {
        let mut copy_candidates;

        if let Ok(mut ice_candidates) = self.pending_outbound_ice_candidates.lock() {
            if ice_candidates.is_empty() {
                return Ok(());
            }
            info!("send_pending_ice_updates(): Local ICE candidates length: {}", ice_candidates.len());
            copy_candidates = ice_candidates.clone();
            ice_candidates.clear();
        } else {
            return Err(RingRtcError::MutexPoisoned("pending_outbound_ice_candidates".to_string()).into());
        }

        let platform = self.platform.lock()?;
        if let Err(e) = platform.app_send_ice_updates(self.call_id, &copy_candidates) {
            // Put the ICE candidates back in the buffer
            let mut ice_candidates = self.pending_outbound_ice_candidates.lock()?;
            ice_candidates.append(&mut copy_candidates);
            Err(e)
        } else {
            Ok(())
        }
    }

    /// Add any remote ICE candidates to the PeerConnection interface.
    pub fn handle_remote_ice_updates(&self) -> Result<()> {
        let mut ice_candidates = self.pending_inbound_ice_candidates.lock()?;

        if ice_candidates.is_empty() {
            return Ok(());
        }

        info!("handle_remote_ice_updates(): Remote ICE candidates length: {}", ice_candidates.len());
        let webrtc = self.webrtc.lock()?;
        for candidate in &(*ice_candidates) {
            webrtc.pc_interface()?.add_ice_candidate(candidate)?;
        }
        ice_candidates.clear();

        Ok(())

    }

    /// Send a hang-up message to the remote peer .
    ///
    /// The hang-up message is first sent via the PeerConnection
    /// DataChannel and then via the signaling channel.
    pub fn send_hang_up(&self) -> Result<()> {
        let webrtc = self.webrtc.lock()?;
        let platform = self.platform.lock()?;

        if let Ok(dc) = webrtc.data_channel() {
            if let Err(e) = dc.send_hang_up(self.call_id) {
                info!("dc.send_hang_up() failed: {}", e);
            }
        }

        platform.app_send_hangup(self.call_id)
    }

    /// Send a call connected message to the remote peer via the
    /// PeerConnection DataChannel.
    pub fn send_connected(&self) -> Result<()> {
        let webrtc = self.webrtc.lock()?;

        webrtc.data_channel()?.send_connected(self.call_id)?;
        Ok(())
    }

    /// Send the remote peer the current video status via the
    /// PeerConnection DataChannel.
    ///
    /// # Arguments
    ///
    /// * `enabled` - `true` when the local side is streaming video,
    /// otherwise `false`.
    pub fn send_video_status(&self, enabled: bool) -> Result<()> {
        let webrtc = self.webrtc.lock()?;

        webrtc.data_channel()?.send_video_status(self.call_id, enabled)
    }

    /// A notification of an available DataChannel.
    ///
    /// Called when the PeerConnectionObserver is notified of an
    /// available DataChannel.
    pub fn on_data_channel(&mut self,
                           data_channel:    DataChannel,
                           call_connection: CallConnection<T>) -> Result<()> {

        info!("on_data_channel()");
        let dc_observer = DataChannelObserver::new(call_connection)?;
        unsafe { data_channel.register_observer(dc_observer.rffi_interface())? } ;
        self.set_data_channel(data_channel)?;
        self.set_data_channel_observer(dc_observer)?;
        Ok(())

    }

    /// Notify the client application about an event.
    pub fn notify_client(&self, event: ClientEvent) -> Result<()> {
        let platform = self.platform.lock()?;
        platform.notify_client(event)
    }

    /// Notify the client application about an error.
    pub fn notify_error(&self, error: failure::Error) -> Result<()> {
        let platform = self.platform.lock()?;
        platform.notify_error(error)
    }

    /// Notify the client application about an avilable MediaStream.
    #[allow(clippy::map_entry)]
    pub fn notify_on_add_stream(&self, stream: MediaStream) -> Result<()> {
        let media_stream_interface = stream.rffi_interface();
        let mut webrtc = self.webrtc.lock()?;
        let platform = self.platform.lock()?;

        if !webrtc.stream_map.contains_key(&media_stream_interface) {
            let app_media_stream = platform.create_media_stream(stream)?;
            webrtc.stream_map.insert(media_stream_interface, app_media_stream);
        }
        let app_media_stream = &webrtc.stream_map[&media_stream_interface];

        platform.notify_on_add_stream(app_media_stream)
    }

    /// Send a CallEvent to the internal FSM.
    ///
    /// Using the `EventPump` send a CallEvent to the internal FSM.
    fn inject_event(&mut self, event: CallEvent) -> Result<()> {
        if self.event_pump.is_closed() {
            // The stream is closed, just eat the request
            debug!("cc.inject_event(): stream is closed while sending: {}", event);
            return Ok(());
        }
        self.event_pump.try_send((self.clone(), event))?;
        Ok(())
    }

    /// Shutdown the current call.
    ///
    /// Notify the internal FSM to shutdown.
    ///
    /// `Note:` The current thread is blocked while waiting for the
    /// FSM to signal that shutdown is complete.
    pub fn close(&mut self) -> Result<()> {
        let start_ref_count = self.ref_count();
        info!("terminate(): ref_count: {}", start_ref_count);
        self.set_state(CallState::Terminating)?;
        self.inject_event(CallEvent::EndCall)?;
        self.wait_for_terminate()?;

        let mut webrtc = self.webrtc.lock()?;

        // dispose of all the media stream objects
        webrtc.stream_map.clear();

        if let Some(data_channel) = webrtc.data_channel.take().as_mut() {
            if let Some(dc_observer) = webrtc.data_channel_observer.take().as_mut() {
                unsafe { data_channel.unregister_observer(dc_observer.rffi_interface()) } ;
            }
            data_channel.dispose();
        }

        self.set_state(CallState::Closed)?;

        Ok(())

    }

    /// Bottom half of `close()`
    ///
    /// Waits for the FSM shutdown condition variable to signal that
    /// shutdown is complete.
    fn wait_for_terminate(&mut self) -> Result<()> {
        // Wait for terminate operation to complete
        info!("terminate(): waiting for terminate complete...");
        let &(ref mutex, ref condvar) = &*self.terminate_condvar;
        if let Ok(mut terminate_complete) = mutex.lock() {
            while !*terminate_complete {
                terminate_complete = condvar.wait(terminate_complete).
                    map_err(|_| { RingRtcError::MutexPoisoned("CallConnection Terminate Condition Variable".to_string()) })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned("CallConnection Terminate Condition Variable".to_string()).into());
        }
        info!("terminate(): terminate complete: ref_count: {}", self.ref_count());
        Ok(())
    }

    /// Notification that the FSM shutdown is complete.
    ///
    /// `Note:` Called by the FSM on a worker thread after shutdown.
    pub fn notify_terminate_complete(&mut self) -> Result<()> {
        debug!("notify_terminate_complete(): notifying terminate complete...");
        let &(ref mutex, ref condvar) = &*self.terminate_condvar;
        if let Ok(mut terminate_complete) = mutex.lock() {
            *terminate_complete = true;
            condvar.notify_one();
            Ok(())
        } else {
            Err(RingRtcError::MutexPoisoned("CallConnection Terminate Condition Variable".to_string()).into())
        }
    }

    /// Inject a `SendOffer` event into the FSM.
    pub fn inject_send_offer(&mut self) -> Result<()> {
        let event = CallEvent::SendOffer;
        self.inject_event(event)
    }

    /// Inject a `HandleAnswer` event into the FSM
    ///
    /// `Called By:` Local application.
    ///
    /// # Arguments
    ///
    /// * `answer` - String containing the remote SDP answer.
    pub fn inject_handle_answer(&mut self, answer: String) -> Result<()> {
        info!("RX SDP answer:\n{}", redact_string(&answer));
        let event = CallEvent::HandleAnswer(answer);
        self.inject_event(event)
    }

    /// Inject an `HandleOffer` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// # Arguments
    ///
    /// * `offer` - String containing the remote SDP offer.
    pub fn inject_handle_offer(&mut self, offer: String) -> Result<()> {
        info!("RX SDP offer:\n{}", redact_string(&offer));
        let event = CallEvent::HandleOffer(offer);
        self.inject_event(event)
    }

    /// Inject a `LocalIceCandidate` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `candidate` - Locally generated IceCandidate.
    pub fn inject_local_ice_candidate(&mut self, candidate: IceCandidate) -> Result<()> {
        let event = CallEvent::LocalIceCandidate(candidate);
        self.inject_event(event)
    }

    /// Inject an `IceConnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connected(&mut self) -> Result<()> {
        self.inject_event(CallEvent::IceConnected)
    }

    /// Inject an `IceConnectionFailed` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connection_failed(&mut self) -> Result<()> {
        self.inject_event(CallEvent::IceConnectionFailed)
    }

    /// Inject an `IceConnectionDisconnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connection_disconnected(&mut self) -> Result<()> {
        self.inject_event(CallEvent::IceConnectionDisconnected)
    }

    /// Inject a `ClientError` event into the FSM.
    ///
    /// This is used to send an error notification to the client
    /// application.
    ///
    /// `Called By:` Various threads when errors occur.
    pub fn inject_client_error(&mut self, error: failure::Error) -> Result<()> {
        self.inject_event(CallEvent::ClientError(error))
    }

    /// Inject a `RemoteConnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_remote_connected(&mut self, call_id: CallId) -> Result<()> {
        self.inject_event(CallEvent::RemoteConnected(call_id))
    }

    /// Inject a `RemoteHangup` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_remote_hangup(&mut self, call_id: CallId) -> Result<()> {
        self.inject_event(CallEvent::RemoteHangup(call_id))
    }

    /// Inject a `RemoteVideoStatus` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    /// * `enabled` - `true` if the remote peer is streaming video.
    pub fn inject_remote_video_status(&mut self, call_id: CallId, enabled: bool) -> Result<()> {
        self.inject_event(CallEvent::RemoteVideoStatus(call_id, enabled))
    }

    /// Inject a local `HangUp` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_hang_up(&mut self) -> Result<()> {
        self.set_state(CallState::Terminating)?;
        self.inject_event(CallEvent::LocalHangup)
    }

    /// Inject a local `AcceptCall` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_accept_call(&mut self) -> Result<()> {
        self.inject_event(CallEvent::AcceptCall)
    }

    /// Inject a `LocalVideoStatus` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// * `enabled` - `true` if the local peer is streaming video.
    pub fn inject_local_video_status(&mut self, enabled: bool) -> Result<()> {
        self.inject_event(CallEvent::LocalVideoStatus(enabled))
    }

    /// Inject a local `CallTimeout` event into the FSM.
    ///
    /// `Called By:` Local timeout thread.
    ///
    /// * `enabled` - `true` if the local peer is streaming video.
    pub fn inject_call_timeout(&mut self, call_id: CallId) -> Result<()> {
        let event = CallEvent::CallTimeout(call_id);
        self.inject_event(event)
    }

    /// Inject a `RemoteIceCandidate` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// # Arguments
    ///
    /// * `candidate` - Remotely generated IceCandidate.
    pub fn inject_remote_ice_candidate(&mut self, candidate: IceCandidate) -> Result<()> {
        let event = CallEvent::RemoteIceCandidate(candidate);
        self.inject_event(event)
    }

    /// Inject an `OnAddStream` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` back thread.
    ///
    /// # Arguments
    ///
    /// * `stream` - WebRTC C++ MediaStream object.
    pub fn inject_on_add_stream(&mut self, stream: MediaStream) -> Result<()> {
        let event = CallEvent::OnAddStream(stream);
        self.inject_event(event)
    }

    /// Inject an `OnDataChannel` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` back thread.
    ///
    /// # Arguments
    ///
    /// * `data_channel` - WebRTC C++ `DataChannel` object.
    pub fn inject_on_data_channel(&mut self, data_channel: DataChannel) -> Result<()> {
        let event = CallEvent::OnDataChannel(data_channel);
        self.inject_event(event)
    }

    #[allow(clippy::mutex_atomic)]
    /// Inject a synchronizing event into the FSM.
    ///
    /// Blocks the caller while the event flushes through the FSM.
    ///
    /// Note: Events ahead of this event in the FSM pipeline can
    /// generate additional error events, which will be queued behind
    /// this synchronizing event.
    #[cfg(feature = "sim")]
    fn inject_synchronize(&mut self) -> Result<()> {
        let sync  = Arc::new((Mutex::new(false), Condvar::new()));
        let event = CallEvent::Synchronize(sync.clone());

        self.inject_event(event)?;

        info!("synchronize(): waiting for synchronize complete...");
        let &(ref mutex, ref condvar) = &*sync;
        if let Ok(mut sync_complete) = mutex.lock() {
            while !*sync_complete {
                sync_complete = condvar.wait(sync_complete).
                    map_err(|_| { RingRtcError::MutexPoisoned("CallConnection Synchronize Condition Variable".to_string()) })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned("CallConnection Synchronize Condition Variable".to_string()).into());
        }
        info!("synchronize(): complete");
        Ok(())
    }

    /// Synchronize the caller with the FSM event queue.
    ///
    /// Blocks the caller while the FSM event queue is flushed.
    ///
    /// `Called By:` Test infrastructure
    #[cfg(feature = "sim")]
    pub fn synchronize(&mut self) -> Result<()> {

        // The first sync flushes out any pending events.  This
        // event(s) could fail, which would enqueues another event to
        // the FSM, *behind* the sync event.
        self.inject_synchronize()?;

        // The second sync flushes out any error event(s) that might
        // have happened during the first sync.
        self.inject_synchronize()
    }

}
