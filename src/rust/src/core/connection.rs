//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! A peer-to-peer connection interface.

extern crate tokio;

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

use futures::sync::mpsc::{Receiver, Sender};
use futures::Future;
use tokio::runtime;

use crate::common::{CallDirection, CallId, ConnectionId, ConnectionState, DeviceId, Result};
use crate::core::call::Call;
use crate::core::call_mutex::CallMutex;
use crate::core::connection_fsm::{ConnectionEvent, ConnectionStateMachine};
use crate::core::platform::Platform;
use crate::core::util::{ptr_as_box, redact_string};

use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media_stream::MediaStream;
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::sdp_observer::{
    create_csd_observer,
    create_ssd_observer,
    SessionDescriptionInterface,
};

/// Connection observer status notification types
///
#[derive(Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObserverEvent {
    /// ICE negotiation is complete and in the case of incoming calls
    /// the Data Channel is also ready, so both sides can begin
    /// communicating.
    ConnectionRinging,

    /// The remote side has connected the call.
    RemoteConnected,

    /// The remote video status.
    RemoteVideoStatus(bool),

    /// The remote side has hungup.
    RemoteHangup,

    /// The call failed to connect during ICE negotiation.
    ConnectionFailed,

    /// The call dropped while connected and is now reconnecting.
    ConnectionReconnecting,

    /// The call dropped while connected and is now reconnected.
    ConnectionReconnected,
}

impl Clone for ObserverEvent {
    fn clone(&self) -> Self {
        *self
    }
}

impl fmt::Display for ObserverEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Encapsulates several WebRTC objects associated with the
/// Connection object.
struct WebRtcData<T>
where
    T: Platform,
{
    /// PeerConnection object
    pc_interface:          Option<PeerConnection>,
    /// DataChannel object
    data_channel:          Option<DataChannel>,
    /// DataChannelObserver object
    data_channel_observer: Option<DataChannelObserver<T>>,
    /// Raw pointer to Connection object for PeerConnectionObserver
    connection_ptr:        Option<*mut Connection<T>>,
    /// Application specific media stream
    app_media_stream:      Option<<T as Platform>::AppMediaStream>,
    /// Application specific peer connection
    app_connection:        Option<<T as Platform>::AppConnection>,
}

// Send and Sync needed to share *const pointer types across threads.
unsafe impl<T> Send for WebRtcData<T> where T: Platform {}

unsafe impl<T> Sync for WebRtcData<T> where T: Platform {}

impl<T> fmt::Display for WebRtcData<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "pc_interface: {:?}, data_channel: {:?}, data_channel_observer: {:?}, connection_ptr: {:?}",
               self.pc_interface,
               self.data_channel,
               self.data_channel_observer,
               self.connection_ptr)
    }
}

impl<T> fmt::Debug for WebRtcData<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> WebRtcData<T>
where
    T: Platform,
{
    fn pc_interface(&self) -> Result<&PeerConnection> {
        match self.pc_interface.as_ref() {
            Some(v) => Ok(v),
            None => Err(RingRtcError::OptionValueNotSet(
                "pc_interface".to_string(),
                "pc_interface".to_string(),
            )
            .into()),
        }
    }

    fn data_channel(&self) -> Result<&DataChannel> {
        match self.data_channel.as_ref() {
            Some(v) => Ok(v),
            None => Err(RingRtcError::OptionValueNotSet(
                "data_channel".to_string(),
                "data_channel".to_string(),
            )
            .into()),
        }
    }
}

/// Encapsulates the FSM and runtime upon which a Connection runs.
struct Context {
    /// Runtime upon which the ConnectionStateMachine runs.
    pub worker_runtime: runtime::Runtime,
}

impl Context {
    fn new() -> Result<Self> {
        Ok(Self {
            worker_runtime: runtime::Builder::new()
                .core_threads(1)
                .name_prefix("worker".to_string())
                .build()?,
        })
    }
}

/// A mpsc::Sender for injecting ConnectionEvents into the
/// [ConnectionStateMachine](../call_fsm/struct.CallStateMachine.html)
///
/// The event pump injects the tuple (Connection, ConnectionEvent)
/// into the FSM.
type EventPump<T> = Sender<(Connection<T>, ConnectionEvent)>;

/// A mpsc::Receiver for receiving ConnectionEvents in the
/// [ConnectionStateMachine](../call_fsm/struct.CallStateMachine.html)
///
/// The event stream is the tuple (Connection, ConnectionEvent).
pub type EventStream<T> = Receiver<(Connection<T>, ConnectionEvent)>;

/// Represents the connection between a local client and one remote
/// peer.
///
/// This object is thread-safe.
pub struct Connection<T>
where
    T: Platform,
{
    /// The parent Call object of this connection.
    call:                            Arc<CallMutex<Call<T>>>,
    /// Injects events into the [ConnectionStateMachine](../call_fsm/struct.CallStateMachine.html).
    event_pump:                      EventPump<T>,
    /// Unique 64-bit number identifying the call.
    call_id:                         CallId,
    /// Device ID of the remote device.
    remote_device:                   DeviceId,
    /// Connection ID, identifying the call and remote_device.
    connection_id:                   ConnectionId,
    /// The call direction, inbound or outbound.
    direction:                       CallDirection,
    /// The current state of the call connection
    state:                           Arc<CallMutex<ConnectionState>>,
    /// Execution context for the call connection FSM
    context:                         Arc<CallMutex<Context>>,
    /// Ancillary WebRTC data.
    webrtc:                          Arc<CallMutex<WebRtcData<T>>>,
    /// Outbound ICE candidates, awaiting transmission.
    pending_outbound_ice_candidates: Arc<CallMutex<Vec<IceCandidate>>>,
    /// Inbound ICE candidates, awaiting processing.
    pending_inbound_ice_candidates:  Arc<CallMutex<Vec<IceCandidate>>>,
    /// Condition variable used at termination to quiesce and synchronize the FSM.
    terminate_condvar:               Arc<(Mutex<bool>, Condvar)>,
}

impl<T> fmt::Display for Connection<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let webrtc = match self.webrtc.lock() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        let state = match self.state() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        write!(
            f,
            "thread: {:?}, connection_id: {}, direction: {}, state: {}, webrtc: ({})",
            thread::current().id(),
            self.connection_id,
            self.direction,
            state,
            webrtc
        )
    }
}

impl<T> fmt::Debug for Connection<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Drop for Connection<T>
where
    T: Platform,
{
    fn drop(&mut self) {
        if self.ref_count() == 1 {
            info!(
                "Connection: Dropping last reference: {}",
                self.connection_id
            );
        } else {
            debug!(
                "Dropping Connection: {}, ref_count: {}",
                self.connection_id,
                self.ref_count()
            );
        }
    }
}

impl<T> Clone for Connection<T>
where
    T: Platform,
{
    fn clone(&self) -> Self {
        Connection {
            call:                            Arc::clone(&self.call),
            event_pump:                      self.event_pump.clone(),
            call_id:                         self.call_id,
            remote_device:                   self.remote_device,
            connection_id:                   self.connection_id,
            direction:                       self.direction,
            state:                           Arc::clone(&self.state),
            context:                         Arc::clone(&self.context),
            webrtc:                          Arc::clone(&self.webrtc),
            pending_outbound_ice_candidates: Arc::clone(&self.pending_outbound_ice_candidates),
            pending_inbound_ice_candidates:  Arc::clone(&self.pending_inbound_ice_candidates),
            terminate_condvar:               Arc::clone(&self.terminate_condvar),
        }
    }
}

impl<T> Connection<T>
where
    T: Platform,
{
    /// Create a new Connection.
    #[allow(clippy::mutex_atomic)]
    pub fn new(call: Call<T>, remote_device: DeviceId) -> Result<Self> {
        // create a FSM runtime for this connection
        let mut context = Context::new()?;
        let (event_pump, receiver) = futures::sync::mpsc::channel(256);
        let call_fsm = ConnectionStateMachine::new(receiver)?
            .map_err(|e| info!("call state machine returned error: {}", e));
        context.worker_runtime.spawn(call_fsm);

        let call_id = call.call_id();
        let direction = call.direction();

        let webrtc = WebRtcData {
            pc_interface:          None,
            data_channel:          None,
            data_channel_observer: None,
            connection_ptr:        None,
            app_media_stream:      None,
            app_connection:        None,
        };

        let connection = Self {
            event_pump,
            call_id,
            remote_device,
            direction,
            connection_id: ConnectionId::new(call_id, remote_device),
            call: Arc::new(CallMutex::new(call, "call")),
            state: Arc::new(CallMutex::new(ConnectionState::Idle, "state")),
            context: Arc::new(CallMutex::new(context, "context")),
            webrtc: Arc::new(CallMutex::new(webrtc, "webrtc")),
            pending_outbound_ice_candidates: Arc::new(CallMutex::new(
                Vec::new(),
                "pending_outbound_ice_candidates",
            )),
            pending_inbound_ice_candidates: Arc::new(CallMutex::new(
                Vec::new(),
                "pending_inbound_ice_candidates",
            )),
            terminate_condvar: Arc::new((Mutex::new(false), Condvar::new())),
        };

        connection.init_connection_ptr()?;

        Ok(connection)
    }

    /// Return the Call identifier.
    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    /// Return the remote device identifier.
    pub fn remote_device(&self) -> DeviceId {
        self.remote_device
    }

    /// Return the connection identifier.
    pub fn id(&self) -> ConnectionId {
        self.connection_id
    }

    /// Return the Call direction.
    pub fn direction(&self) -> CallDirection {
        self.direction
    }

    /// Return the parent call, under a locked mutex.
    pub fn call(&self) -> Result<MutexGuard<'_, Call<T>>> {
        self.call.lock()
    }

    /// Return the current Call state.
    pub fn state(&self) -> Result<ConnectionState> {
        let state = self.state.lock()?;
        Ok(*state)
    }

    /// Update the current Call state.
    pub fn set_state(&self, new_state: ConnectionState) -> Result<()> {
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

    /// Update the media stream.
    pub fn set_app_media_stream(
        &self,
        app_media_stream: <T as Platform>::AppMediaStream,
    ) -> Result<()> {
        // In the current application we only expect one media stream
        // per connection.
        let mut webrtc = self.webrtc.lock()?;
        match webrtc.app_media_stream {
            Some(_) => Err(RingRtcError::ActiveMediaStreamAlreadySet(self.remote_device).into()),
            None => {
                webrtc.app_media_stream = Some(app_media_stream);
                Ok(())
            }
        }
    }

    /// Set the application peer connection.
    pub fn set_app_connection(&self, app_connection: <T as Platform>::AppConnection) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        match webrtc.app_connection {
            Some(_) => Err(RingRtcError::AppConnectionAlreadySet(self.remote_device).into()),
            None => {
                webrtc.app_connection = Some(app_connection);
                Ok(())
            }
        }
    }

    /// Return a clone of the application peer connection.
    pub fn app_connection(&self) -> Result<<T as Platform>::AppConnection> {
        let webrtc = self.webrtc.lock()?;
        match webrtc.app_connection.as_ref() {
            Some(v) => Ok(v.clone()),
            None => Err(RingRtcError::OptionValueNotSet(
                String::from("app_connection()"),
                String::from("app_connection"),
            )
            .into()),
        }
    }

    /// Returns `true` if the call is terminating.
    pub fn terminating(&self) -> Result<bool> {
        if let ConnectionState::Terminating = self.state()? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Clone the Connection, Box it and return a raw pointer to the Box.
    pub fn create_connection_ptr(&self) -> *mut Connection<T> {
        let connection_box = Box::new(self.clone());
        Box::into_raw(connection_box)
    }

    /// Return the internally tracked connection object pointer, for
    /// use by the PeerConnectionObserver call backs.
    pub fn get_connection_ptr(&self) -> Result<*mut Connection<T>> {
        let webrtc = self.webrtc.lock()?;
        match webrtc.connection_ptr.as_ref() {
            Some(v) => Ok(*v),
            None => Err(RingRtcError::OptionValueNotSet(
                String::from("connection_ptr()"),
                String::from("connection_ptr"),
            )
            .into()),
        }
    }

    /// Create a connection object on the heap, for use by the
    /// PeerConnectionObserver call backs.  Track it, as it needs to
    /// be freed after closing down the PeerConnection.
    fn init_connection_ptr(&self) -> Result<()> {
        let mut webrtc = self.webrtc.lock()?;
        webrtc.connection_ptr = Some(self.create_connection_ptr());
        Ok(())
    }

    /// Return the strong reference count on the webrtc `Arc<Mutex<>>`.
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
        webrtc
            .pc_interface()?
            .set_local_description(ssd_observer.as_ref(), desc);
        ssd_observer.get_result()
    }

    /// Set the remote SPD decription in the PeerConnection interface.
    fn set_remote_description(&self, desc: &SessionDescriptionInterface) -> Result<()> {
        let ssd_observer = create_ssd_observer();

        let webrtc = self.webrtc.lock()?;
        webrtc
            .pc_interface()?
            .set_remote_description(ssd_observer.as_ref(), desc);
        ssd_observer.get_result()
    }

    /// Send an SDP offer message to the remote peer via the signaling
    /// channel.
    pub fn send_offer(&self) -> Result<()> {
        let offer = self.create_offer()?;
        self.set_local_description(&offer)?;

        info!(
            "id: {}, TX SDP offer:\n{}",
            self.id(),
            redact_string(&offer.get_description()?)
        );

        let call = self.call()?;
        call.send_offer(self.clone(), offer)
    }

    /// Check to see if this Connection is able to send messages.
    /// Once it is terminated it shouldn't be able to.
    pub fn can_send_messages(&self) -> bool {
        let state_result = self.state();

        match state_result {
            Ok(state) => match state {
                ConnectionState::Terminating | ConnectionState::Closed => false,
                _ => true,
            },
            Err(_) => false,
        }
    }

    /// Handle an incoming SDP answer message.
    pub fn handle_answer(&mut self, answer: String) -> Result<()> {
        let desc = SessionDescriptionInterface::create_sdp_answer(answer)?;
        self.set_remote_description(&desc)?;
        self.inject_have_local_remote_sdp()
    }

    /// Handle an incoming SDP offer message.
    pub fn handle_offer(&mut self, offer: String) -> Result<()> {
        let desc = SessionDescriptionInterface::create_sdp_offer(offer)?;
        self.set_remote_description(&desc)?;

        let answer = self.create_answer()?;
        self.set_local_description(&answer)?;
        self.inject_have_local_remote_sdp()?;

        info!(
            "id: {}, TX SDP answer:\n{}",
            self.id(),
            redact_string(&answer.get_description()?)
        );

        let call = self.call()?;
        call.send_answer(self.clone(), answer)
    }

    /// Buffer local ICE candidates.
    pub fn buffer_local_ice_candidate(&self, candidate: IceCandidate) -> Result<()> {
        info!("Local ICE candidate: {}", candidate);

        let num_ice_candidates = {
            let mut ice_candidates = self.pending_outbound_ice_candidates.lock()?;
            ice_candidates.push(candidate);
            ice_candidates.len()
        };

        // Only when we transition from no candidates to one do we
        // need to signal the message queue that there is something
        // to send for this Connection.
        if num_ice_candidates == 1 {
            let call = self.call()?;
            call.send_ice_candidates(self.clone())?
        }

        Ok(())
    }

    /// Buffer remote ICE candidates.
    pub fn buffer_remote_ice_candidates(&self, ice_candidates: Vec<IceCandidate>) -> Result<()> {
        let mut pending_ice_candidates = self.pending_inbound_ice_candidates.lock()?;
        for ice_candidate in ice_candidates {
            info!("Remote ICE candidates: {}", ice_candidate);
            pending_ice_candidates.push(ice_candidate);
        }
        Ok(())
    }

    /// Get the current local ICE candidates to send to the remote peer.
    pub fn get_pending_ice_updates(&self) -> Result<Vec<IceCandidate>> {
        info!("get_pending_ice_updates():");

        let mut ice_candidates = self.pending_outbound_ice_candidates.lock()?;

        let copy_candidates = ice_candidates.clone();
        ice_candidates.clear();

        info!(
            "get_pending_ice_updates(): Local ICE candidates length: {}",
            copy_candidates.len()
        );

        Ok(copy_candidates)
    }

    /// Add any remote ICE candidates to the PeerConnection interface.
    pub fn handle_remote_ice_updates(&self) -> Result<()> {
        let mut ice_candidates = self.pending_inbound_ice_candidates.lock()?;

        if ice_candidates.is_empty() {
            return Ok(());
        }

        info!(
            "handle_remote_ice_updates(): Remote ICE candidates length: {}",
            ice_candidates.len()
        );
        let webrtc = self.webrtc.lock()?;
        for candidate in &(*ice_candidates) {
            webrtc.pc_interface()?.add_ice_candidate(candidate)?;
        }
        ice_candidates.clear();

        Ok(())
    }

    /// Send a hangup message to the remote peer via the
    /// PeerConnection DataChannel.
    pub fn send_hangup(&self) -> Result<()> {
        info!("send_hangup(): id: {}", self.connection_id);
        let webrtc = self.webrtc.lock()?;
        if let Ok(data_channel) = webrtc.data_channel() {
            if let Err(e) = data_channel.send_hang_up(self.call_id) {
                info!("data_channel.send_hang_up() failed: {}", e);
            }
        } else {
            info!(
                "send_hangup(): id: {}, skipping, data_channel not present",
                self.connection_id
            );
        }
        Ok(())
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

        webrtc
            .data_channel()?
            .send_video_status(self.call_id, enabled)
    }

    /// A notification of an available DataChannel.
    ///
    /// Called when the PeerConnectionObserver is notified of an
    /// available DataChannel.
    pub fn on_data_channel(
        &mut self,
        data_channel: DataChannel,
        call_connection: Connection<T>,
    ) -> Result<()> {
        info!("on_data_channel()");
        let dc_observer = DataChannelObserver::new(call_connection)?;
        unsafe { data_channel.register_observer(dc_observer.rffi_interface())? };
        self.set_data_channel(data_channel)?;
        self.set_data_channel_observer(dc_observer)?;
        Ok(())
    }

    /// Notify the parent call observer about an event.
    pub fn notify_observer(&self, event: ObserverEvent) -> Result<()> {
        let mut call = self.call.lock()?;
        call.on_connection_event(self.connection_id, event)
    }

    /// Notify the parent call observer about an internal error.
    pub fn internal_error(&self, error: failure::Error) -> Result<()> {
        let mut call = self.call.lock()?;
        call.on_connection_error(self.connection_id, error)
    }

    /// Create an application specific MediaStream object and store it
    /// for later.
    pub fn on_add_stream(&mut self, stream: MediaStream) -> Result<()> {
        info!("on_add_stream(): id: {}", self.connection_id);

        let call = self.call.lock()?;
        let app_media_stream = call.create_media_stream(self, stream)?;
        self.set_app_media_stream(app_media_stream)
    }

    /// Connect our media stream to the application connection
    pub fn connect_media(&self) -> Result<()> {
        info!("connect_media(): id: {}", self.connection_id);

        let webrtc = self.webrtc.lock()?;
        let app_media_stream = match webrtc.app_media_stream.as_ref() {
            Some(v) => v,
            None => {
                return Err(RingRtcError::OptionValueNotSet(
                    String::from("connect_media()"),
                    String::from("app_media_stream"),
                )
                .into())
            }
        };

        let call = self.call()?;
        call.connect_media(app_media_stream)
    }

    /// Send a ConnectionEvent to the internal FSM.
    ///
    /// Using the `EventPump` send a ConnectionEvent to the internal FSM.
    fn inject_event(&mut self, event: ConnectionEvent) -> Result<()> {
        if self.event_pump.is_closed() {
            // The stream is closed, just eat the request
            debug!(
                "cc.inject_event(): stream is closed while sending: {}",
                event
            );
            return Ok(());
        }
        self.event_pump.try_send((self.clone(), event))?;
        Ok(())
    }

    /// Shutdown the connection.
    ///
    /// Notify the internal FSM to shutdown.
    ///
    /// `Note:` The current thread is blocked while waiting for the
    /// FSM to signal that shutdown is complete.
    pub fn close(&mut self) -> Result<()> {
        info!("close(): ref_count: {}", self.ref_count());

        self.set_state(ConnectionState::Terminating)?;

        self.inject_event(ConnectionEvent::EndCall)?;
        self.wait_for_terminate()?;

        self.set_state(ConnectionState::Closed)?;

        // Free up webrtc related resources.
        let mut webrtc = self.webrtc.lock()?;

        // dispose of the media stream
        let _ = webrtc.app_media_stream.take();

        // unregister the data channel observer
        if let Some(data_channel) = webrtc.data_channel.take().as_mut() {
            if let Some(dc_observer) = webrtc.data_channel_observer.take().as_mut() {
                unsafe { data_channel.unregister_observer(dc_observer.rffi_interface()) };
            }
            data_channel.dispose();
        }

        // Free the application connection object, which is in essence
        // the PeerConnection object.  It is important to dispose of
        // the app_connection before the connection_ptr.  The
        // app_connection refers to the real PeerConnection object,
        // whose observer is using the connection_ptr.  Once the
        // PeerConnection is completely shutdown it is safe to free up
        // the connection_ptr.
        let _ = webrtc.app_connection.take();

        // Free the connection object previously used by the
        // PeerConnectionObserver.  Convert the pointer back into a
        // Box and let it go out of scope.
        match webrtc.connection_ptr.take() {
            Some(v) => {
                let _ = unsafe { ptr_as_box(v)? };
                Ok(())
            }
            None => Err(RingRtcError::OptionValueNotSet(
                String::from("close()"),
                String::from("connection_ptr"),
            )
            .into()),
        }
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
                terminate_complete = condvar.wait(terminate_complete).map_err(|_| {
                    RingRtcError::MutexPoisoned(
                        "Connection Terminate Condition Variable".to_string(),
                    )
                })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned(
                "Connection Terminate Condition Variable".to_string(),
            )
            .into());
        }
        info!(
            "terminate(): terminate complete: ref_count: {}",
            self.ref_count()
        );
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
            Err(
                RingRtcError::MutexPoisoned("Connection Terminate Condition Variable".to_string())
                    .into(),
            )
        }
    }

    /// Inject a `SendOffer` event into the FSM.
    pub fn inject_send_offer(&mut self) -> Result<()> {
        let event = ConnectionEvent::SendOffer;
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
        info!(
            "id: {}, RX SDP answer:\n{}",
            self.id(),
            redact_string(&answer)
        );
        let event = ConnectionEvent::HandleAnswer(answer);
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
        info!(
            "id: {}, RX SDP offer:\n{}",
            self.id(),
            redact_string(&offer)
        );
        let event = ConnectionEvent::HandleOffer(offer);
        self.inject_event(event)
    }

    /// Inject a `HaveLocalRemoteSdp` event into the FSM.
    ///
    /// `Called By:` handle_offer() and handle_answer().
    pub fn inject_have_local_remote_sdp(&mut self) -> Result<()> {
        let event = ConnectionEvent::HaveLocalRemoteSdp;
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
        let event = ConnectionEvent::LocalIceCandidate(candidate);
        self.inject_event(event)
    }

    /// Inject an `IceConnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connected(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::IceConnected)
    }

    /// Inject an `IceConnectionFailed` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connection_failed(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::IceConnectionFailed)
    }

    /// Inject an `IceConnectionDisconnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `PeerConnectionObserver` call back thread.
    pub fn inject_ice_connection_disconnected(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::IceConnectionDisconnected)
    }

    /// Inject a `InternalError` event into the FSM.
    ///
    /// This is used to send an internal error notification to the
    /// observer.
    ///
    /// `Called By:` FSM when internal errors occur.
    ///
    /// Note: this function does not fail, as there is not much one
    /// can do in this case.
    pub fn inject_internal_error(&mut self, error: failure::Error, msg: &str) {
        error!("{}: {}", msg, error);
        let _ = self.inject_event(ConnectionEvent::InternalError(error));
    }

    /// Inject a `RemoteConnected` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_remote_connected(&mut self, call_id: CallId) -> Result<()> {
        self.inject_event(ConnectionEvent::RemoteConnected(call_id))
    }

    /// Inject a `RemoteHangup` event into the FSM.
    ///
    /// `Called By:` WebRTC `DataChannelObserver` call back thread.
    ///
    /// # Arguments
    ///
    /// * `call_id` - Call ID from the remote peer.
    pub fn inject_remote_hangup(&mut self, call_id: CallId) -> Result<()> {
        self.inject_event(ConnectionEvent::RemoteHangup(call_id))
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
        self.inject_event(ConnectionEvent::RemoteVideoStatus(call_id, enabled))
    }

    /// Inject a local `HangUp` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_hangup(&mut self) -> Result<()> {
        self.set_state(ConnectionState::Terminating)?;
        self.inject_event(ConnectionEvent::LocalHangup)
    }

    /// Inject a local `AcceptCall` event into the FSM.
    ///
    /// `Called By:` Local application.
    pub fn inject_accept_call(&mut self) -> Result<()> {
        self.inject_event(ConnectionEvent::AcceptCall)
    }

    /// Inject a `LocalVideoStatus` event into the FSM.
    ///
    /// `Called By:` Local application.
    ///
    /// * `enabled` - `true` if the local peer is streaming video.
    pub fn inject_local_video_status(&mut self, enabled: bool) -> Result<()> {
        self.inject_event(ConnectionEvent::LocalVideoStatus(enabled))
    }

    /// Inject a `RemoteIceCandidates` event into the FSM.
    ///
    /// `Called By:` Call object.
    ///
    /// # Arguments
    ///
    /// * `candidates` - Vector of remotely generated IceCandidates.
    pub fn inject_received_ice_candidates(
        &mut self,
        ice_candidates: Vec<IceCandidate>,
    ) -> Result<()> {
        let event = ConnectionEvent::ReceivedIceCandidates(ice_candidates);
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
        let event = ConnectionEvent::OnAddStream(stream);
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
        let event = ConnectionEvent::OnDataChannel(data_channel);
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
        match self.state()? {
            ConnectionState::Closed | ConnectionState::Terminating => {
                info!(
                    "connection-synchronize(): skipping synchronize while terminating or closed..."
                );
                return Ok(());
            }
            _ => {}
        }

        let sync = Arc::new((Mutex::new(false), Condvar::new()));
        let event = ConnectionEvent::Synchronize(sync.clone());

        self.inject_event(event)?;

        info!("connection-synchronize(): waiting for synchronize complete...");
        let &(ref mutex, ref condvar) = &*sync;
        if let Ok(mut sync_complete) = mutex.lock() {
            while !*sync_complete {
                sync_complete = condvar.wait(sync_complete).map_err(|_| {
                    RingRtcError::MutexPoisoned(
                        "Connection Synchronize Condition Variable".to_string(),
                    )
                })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned(
                "Connection Synchronize Condition Variable".to_string(),
            )
            .into());
        }
        info!("connection-synchronize(): complete");
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
