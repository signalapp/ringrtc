//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! A peer-to-peer call connection interface.

extern crate tokio;

use std::fmt;
use std::sync::{
    Arc,
    Condvar,
    Mutex,
    MutexGuard,
};

use crate::common::{
    Result,
    CallState,
    CallDirection,
    CallId,
};
use crate::core::call_connection_factory::EventPump;
use crate::core::call_connection_observer::ClientEvent;
use crate::core::call_fsm::CallEvent;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media_stream::MediaStream;
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::sdp_observer::{
    SessionDescriptionInterface,
    create_csd_observer,
    create_ssd_observer,
};

/// A platform independent media stream.
pub trait ClientStreamTrait : Sync + Send + 'static {}

/// A platform independent recipient of a call.
pub trait ClientRecipientTrait : Sync + Send + 'static {}

/// A platform independent call connection.
///
/// The call connection is a fundamental object type representing the
/// connection between two peers.
pub trait CallConnectionInterface : fmt::Debug + fmt::Display + Sync + Send + Sized + 'static
where
    <Self as CallConnectionInterface>::ClientStream: ClientStreamTrait,
    <Self as CallConnectionInterface>::ClientRecipient: ClientRecipientTrait,
{

    type ClientStream;
    type ClientRecipient;

    /// Return the underlying WebRTC PeerConnection object.
    fn get_pc_interface(&self) -> Result<&PeerConnection>;

    /// Return the underlying WebRTC DataChannel object.
    fn get_data_channel(&self) -> Result<&DataChannel>;

    /// Return the unique call identifier.
    fn get_call_id(&self) -> CallId;

    /// Return the current call state.
    fn get_state(&self) -> CallState;

    /// Set the current call state.
    fn set_state(&mut self, state: CallState);

    /// Return the current call direction.
    fn get_direction(&self) -> CallDirection;

    /// Set the current call direction.
    fn set_direction(&mut self, direction: CallDirection);

    /// Returns `true` if the call is terminating.
    fn terminating(&self) -> bool {
        if let CallState::Terminating = self.get_state() {
            true
        } else {
            false
        }
    }

    /// Create a SDP offer message.
    fn create_offer(&self) -> Result<SessionDescriptionInterface> {
        let csd_observer = create_csd_observer();
        self.get_pc_interface()?.create_offer(csd_observer.as_ref());
        csd_observer.get_result()
    }

    /// Create a SDP answer message.
    fn create_answer(&self) -> Result<SessionDescriptionInterface> {
        let csd_observer = create_csd_observer();
        self.get_pc_interface()?.create_answer(csd_observer.as_ref());
        csd_observer.get_result()
    }

    /// Set the local SPD decription.
    fn set_local_description(&self, desc: &SessionDescriptionInterface) -> Result<()> {
        let ssd_observer = create_ssd_observer();
        self.get_pc_interface()?.set_local_description(ssd_observer.as_ref(), desc);
        ssd_observer.get_result()
    }

    /// Send a SDP offer message to the remote peer via the signaling
    /// channel.
    fn send_offer(&self) -> Result<()>;

    /// Set the remote SPD decription.
    fn set_remote_description(&self, desc: &SessionDescriptionInterface) -> Result<()> {
        let ssd_observer = create_ssd_observer();
        self.get_pc_interface()?.set_remote_description(ssd_observer.as_ref(), desc);
        ssd_observer.get_result()
    }

    /// Accept the incoming SDP answer message.
    fn accept_answer(&mut self, answer: String) -> Result<()>;

    /// Accept the incoming SDP offer message.
    fn accept_offer(&self, offer: String) -> Result<()>;

    /// Buffer local ICE candidates.
    fn add_local_candidate(&mut self, candidate: IceCandidate);

    /// Buffer remote ICE candidates.
    fn add_remote_candidate(&mut self, candidate: IceCandidate);

    /// Send any buffered local ICE candidates.
    ///
    /// Send the buffered ICE candidates to the remote peer using the
    /// signaling channel.
    fn send_pending_ice_updates(&mut self) -> Result<()>;

    /// Send a busy message to the remote peer via the signaling
    /// channel.
    fn send_busy(&self, recipient: Self::ClientRecipient, call_id: CallId) -> Result<()>;

    /// Add any remote ICE candidates to the PeerConnection interface
    fn process_remote_ice_updates(&mut self) -> Result<()>;

    /// Send a hang-up message to the remote peer.
    ///
    /// The hang-up message is first sent via the PeerConnection
    /// DataChannel and then via the signaling channel.
    fn send_hang_up(&self) -> Result<()> {
        if let Ok(dc) = self.get_data_channel() {
            if let Err(e) = dc.send_hang_up(self.get_call_id()) {
                info!("dc.send_hang_up() failed: {}", e);
            }
        }
        self.send_signal_message_hang_up()
    }

    /// Send a hang-up to the remote peer via the signaling channel.
    fn send_signal_message_hang_up(&self) -> Result<()>;

    /// Send a call connected message to the remote peer via the
    /// PeerConnection DataChannel.
    fn send_connected(&mut self) -> Result<()> {
        self.get_data_channel()?.send_connected(self.get_call_id())?;
        self.set_state(CallState::CallConnected);
        Ok(())
    }

    /// Send the remote peer the current video status via the
    /// PeerConnection DataChannel.
    ///
    /// # Arguments
    ///
    /// * `enabled` - `true` when the local side is streaming video,
    /// otherwise `false`.
    fn send_video_status(&self, enabled: bool) -> Result<()> {
        self.get_data_channel()?.send_video_status(self.get_call_id(), enabled)
    }

    /// A notification of an available DataChannel.
    ///
    /// Called when the PeerConnectionObserver is notified of an
    /// available DataChannel.
    fn on_data_channel(&mut self,
                       data_channel: DataChannel,
                       cc_handle:    CallConnectionHandle<Self>) -> Result<()>;

    /// Notify the client application about an event.
    fn notify_client(&self, event: ClientEvent) -> Result<()>;

    /// Notify the client application about an error.
    fn notify_error(&self, error: failure::Error) -> Result<()>;

    /// Notify the client application about an avilable MediaStream.
    fn notify_on_add_stream(&mut self, stream: MediaStream) -> Result<()>;

}

/// A thread-safe wrapper around a [CallConnectionInterface](trait.CallConnectionInterface.html) object
///
/// The handle wraps the CallConnection object in `Arc<Mutex<>>`,
/// allowing it to be sent across thread boundaries.
///
/// The handle is used to inject events into the
/// [CallStateMachine](../call_fsm/struct.CallStateMachine.html).
pub struct CallConnectionHandle<T>
where
    T: CallConnectionInterface,
{
    /// Injects events into the [CallStateMachine](../call_fsm/struct.CallStateMachine.html).
    event_pump:        EventPump<T>,
    /// Thread-safe wrapper around a CallConnection object.
    call_connection:   Arc<Mutex<T>>,
    /// Condition variable used at termination to quiesce and synchronize the FSM.
    terminate_condvar: Arc<(Mutex<bool>, Condvar)>,
}

impl<T> fmt::Debug for CallConnectionHandle<T>
where
    T: CallConnectionInterface,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.call_connection)
    }
}

impl<T> Clone for CallConnectionHandle<T>
where
    T: CallConnectionInterface,
{
    fn clone(&self) -> Self {
        CallConnectionHandle {
            event_pump:        self.event_pump.clone(),
            call_connection:   Arc::clone(&self.call_connection),
            terminate_condvar: Arc::clone(&self.terminate_condvar),
        }
    }
}

impl<T> CallConnectionHandle<T>
where
    T: CallConnectionInterface,
{

    /// Create a new CallConnectionHandle.
    #[allow(clippy::mutex_atomic)]
    pub fn create(event_pump: EventPump<T>, call_connection: Arc<Mutex<T>>) -> Self {
        Self {
            event_pump,
            call_connection,
            terminate_condvar:   Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// Take the mutex lock and return the CallConnection object.
    ///
    /// `Note:` The current thread is blocked while waiting on the
    /// mutex.
    pub fn lock(&self) -> Result<MutexGuard<T>> {
        match self.call_connection.lock() {
            Ok(v) => Ok(v),
            Err(_) => Err(RingRtcError::MutexPoisoned("Call Connection".to_string()).into()),
        }
    }

    /// Clone the handle, Box it and return a raw pointer to the Box.
    pub fn create_call_connection_ptr(&self) -> *mut CallConnectionHandle<T> {
        let new_handle = self.clone();
        let cc_handle_box = Box::new(new_handle);
        Box::into_raw(cc_handle_box)
    }

    /// Take the mutex lock and return the call ID number.
    ///
    /// `Note:` The current thread is blocked while waiting on the
    /// mutex.
    pub fn get_call_id(&self) -> Result<CallId> {
        let cc = self.lock()?;
        Ok(cc.get_call_id())
    }

    /// Take the mutex lock and return the call state.
    ///
    /// `Note:` The current thread is blocked while waiting on the
    /// mutex.
    pub fn get_state(&self) -> Result<CallState> {
        let cc = self.lock()?;
        Ok(cc.get_state())
    }

    /// Take the mutex lock and set the call state.
    ///
    /// `Note:` The current thread is blocked while waiting on the
    /// mutex.
    pub fn set_state(&self, state: CallState) -> Result<()> {
        let mut cc = self.lock()?;
        cc.set_state(state);
        Ok(())
    }

    /// Return the strong reference count on the `Arc<Mutex<>>`
    fn get_ref_count(&self) -> usize {
        Arc::strong_count(&self.call_connection)
    }

    /// Send a CallEvent to the internal FSM
    ///
    /// Using the `EventPump` send a CallEvent to the internal FSM.
    fn send_event(&mut self,
                  event: CallEvent<<T as CallConnectionInterface>::ClientRecipient>) -> Result<()> {
        if self.event_pump.is_closed() {
            // The stream is closed, just eat the request
            debug!("cc.send_event(): stream is closed while sending: {}", event);
            return Ok(());
        }
        self.event_pump.try_send((self.clone(), event))?;
        Ok(())
    }

    /// Terminate the current call.
    ///
    /// Notify the internal FSM to shutdown.
    ///
    /// `Note:` The current thread is blocked while waiting for the
    /// FSM to signal that shutdown is complete.
    pub fn terminate(&mut self) -> Result<()> {
        let start_ref_count = self.get_ref_count();
        info!("terminate(): ref_count: {}", start_ref_count);
        self.set_state(CallState::Terminating)?;
        self.send_event(CallEvent::EndCall)?;
        self.wait_for_terminate()
    }

    /// Bottom half of `terminate()`
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
        info!("terminate(): terminate complete: ref_count: {}", self.get_ref_count());
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
        self.send_event(event)
    }

    /// Inject an `AcceptAnswer` event into the FSM
    ///
    /// `Called By:` Local application.
    ///
    /// # Arguments
    ///
    /// * `answer` - String containing the remote SDP answer.
    pub fn inject_accept_answer(&mut self, answer: String) -> Result<()> {
        let event = CallEvent::AcceptAnswer(answer);
        self.send_event(event)
    }

    /// Inject an `AcceptOffer` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// # Arguments
    ///
    /// * `offer` - String containing the remote SDP offer.
    pub fn inject_accept_offer(&mut self, offer: String) -> Result<()> {
        let event = CallEvent::AcceptOffer(offer);
        self.send_event(event)
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
        self.send_event(event)
    }

    /// Inject an `IceConnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connected(&mut self) -> Result<()> {
        self.send_event(CallEvent::IceConnected)
    }

    /// Inject an `IceConnectionFailed` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connection_failed(&mut self) -> Result<()> {
        self.send_event(CallEvent::IceConnectionFailed)
    }

    /// Inject an `IceConnectionDisconnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connection_disconnected(&mut self) -> Result<()> {
        self.send_event(CallEvent::IceConnectionDisconnected)
    }

    /// Inject a `ClientError` event into the FSM.
    ///
    /// This is used to send an error notification to the client
    /// application.
    ///
    /// `Called By:` Various threads when errors occur.
    pub fn inject_client_error(&mut self, error: failure::Error) -> Result<()> {
        self.send_event(CallEvent::ClientError(error))
    }

    /// Inject a `RemoteConnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_remote_connected(&mut self, call_id: CallId) -> Result<()> {
        self.send_event(CallEvent::RemoteConnected(call_id))
    }

    /// Inject a `RemoteHangup` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_remote_hangup(&mut self, call_id: CallId) -> Result<()> {
        self.send_event(CallEvent::RemoteHangup(call_id))
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
        self.send_event(CallEvent::RemoteVideoStatus(call_id, enabled))
    }

    /// Inject a local `HangUp` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_hang_up(&mut self) -> Result<()> {
        self.send_event(CallEvent::LocalHangup)
    }

    /// Inject a local `AnswerCall` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_answer_call(&mut self) -> Result<()> {
        self.send_event(CallEvent::AnswerCall)
    }

    /// Inject a `LocalVideoStatus` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// * `enabled` - `true` if the local peer is streaming video.
    pub fn inject_local_video_status(&mut self, enabled: bool) -> Result<()> {
        self.send_event(CallEvent::LocalVideoStatus(enabled))
    }

    /// Inject a local `CallTimeout` event into the FSM.
    ///
    /// `Called By:` Local timeout thread.
    ///
    /// * `enabled` - `true` if the local peer is streaming video.
    pub fn inject_call_timeout(&mut self, call_id: CallId) -> Result<()> {
        let event = CallEvent::CallTimeout(call_id);
        self.send_event(event)
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
        self.send_event(event)
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
        self.send_event(event)
    }

    /// Inject a `SendBusy` event into the FSM.
    ///
    /// When currently in a call and another call comes in send a busy
    /// message to the new caller.
    ///
    /// `Called By:` Local application.
    ///
    /// # Arguments
    ///
    /// * `recipient` - Recipient of the busy message.
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_send_busy(&mut self, recipient: <T as CallConnectionInterface>::ClientRecipient, call_id: CallId) -> Result<()> {
        let event = CallEvent::SendBusy(recipient, call_id);
        self.send_event(event)
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
        self.send_event(event)
    }

}
