//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Connection Finite State Machine
//!
//! The Connection FSM mediates the state machine of the multi-device
//! Call with the state machine of WebRTC.  The FSM implements the ICE
//! negotiation protocol without the need for the client application
//! to intervene.
//!
//! # Asynchronous Inputs:
//!
//! ## From Call object
//!
//! - SendOffer
//! - AcceptAnswer
//! - AcceptOffer
//! - AnswerCall
//! - LocalHangup
//! - LocalVideoStatus
//! - SendBusy
//! - RemoteIceCandidate
//! - RemoteHangup
//!
//! ## From WebRTC observer interfaces
//!
//! - LocalIceCandidate
//! - IceConnected
//! - IceConnectionFailed
//! - IceConnectionDisconnected
//! - OnAddStream
//! - OnDataChannel
//! - RemoteConnected
//! - RemoteVideoStatus
//! - RemoteHangup
//!
//! # Asynchronous Outputs:
//!
//! ## To Call observer
//!
//! - [ObserverEvents](../connection/enum.ObserverEvent.html)
//! - ObserverErrors

use std::fmt;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use futures::future::lazy;
use futures::{Async, Future, Poll, Stream};
use tokio::runtime;

use crate::common::{CallDirection, CallId, ConnectionState, Result, RingBench};
use crate::core::connection::{Connection, EventStream, ObserverEvent};
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::media::MediaStream;

/// The different types of Connection Events.
pub enum ConnectionEvent {
    ReceivedIce(signaling::Ice),
    /// Receive hangup from remote peer.
    ReceivedHangup(CallId, signaling::Hangup),
    /// Event from client application to send hangup via the data channel.
    SendHangupViaDataChannel(signaling::Hangup),
    /// Accept incoming call (callee only).
    AcceptCall,
    /// Receive call connected from remote peer.
    RemoteConnected(CallId),
    /// Receive video streaming status change from remote peer.
    RemoteVideoStatus(CallId, bool, Option<u64>),
    /// Local video streaming status change from client application.
    LocalVideoStatus(bool),
    /// Local ICE candidate ready, from WebRTC observer.
    LocalIceCandidate(signaling::IceCandidate),
    /// Local ICE status is connected, from WebRTC observer.
    IceConnected,
    /// Local ICE connection failed, from WebRTC observer.
    IceConnectionFailed,
    /// Local ICE connection disconnected, from WebRTC observer.
    IceConnectionDisconnected,
    /// Send the observer an internal error message.
    InternalError(failure::Error),
    /// Receive local media stream from WebRTC observer.
    OnAddStream(MediaStream),
    /// Receive new available data channel from WebRTC observer (callee).
    OnDataChannel(DataChannel),
    /// Synchronize the FSM.
    Synchronize(Arc<(Mutex<bool>, Condvar)>),
    /// Shutdown the call.
    EndCall,
}

impl fmt::Display for ConnectionEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            ConnectionEvent::AcceptCall => "AcceptCall".to_string(),
            ConnectionEvent::ReceivedHangup(call_id, hangup) => {
                format!("RemoteHangup, call_id: {} hangup: {}", call_id, hangup)
            }
            ConnectionEvent::RemoteConnected(id) => format!("RemoteConnected, call_id: {}", id),
            ConnectionEvent::RemoteVideoStatus(id, enabled, sequence_number) => format!(
                "RemoteVideoStatus, call_id: {}, enabled: {}, seqnum: {:?}",
                id, enabled, sequence_number
            ),
            ConnectionEvent::ReceivedIce(_) => "RemoteIceCandidates".to_string(),
            ConnectionEvent::SendHangupViaDataChannel(hangup) => {
                format!("SendHangupViaDataChannel, hangup: {}", hangup)
            }
            ConnectionEvent::LocalVideoStatus(enabled) => {
                format!("LocalVideoStatus, enabled: {}", enabled)
            }
            ConnectionEvent::LocalIceCandidate(_) => "LocalIceCandidate".to_string(),
            ConnectionEvent::IceConnected => "IceConnected".to_string(),
            ConnectionEvent::IceConnectionFailed => "IceConnectionFailed".to_string(),
            ConnectionEvent::IceConnectionDisconnected => "IceConnectionDisconnected".to_string(),
            ConnectionEvent::InternalError(e) => format!("InternalError: {}", e),
            ConnectionEvent::OnAddStream(stream) => format!("OnAddStream, stream: {:}", stream),
            ConnectionEvent::OnDataChannel(dc) => format!("OnDataChannel, dc: {:?}", dc),
            ConnectionEvent::Synchronize(_) => "Synchronize".to_string(),
            ConnectionEvent::EndCall => "EndCall".to_string(),
        };
        write!(f, "({})", display)
    }
}

impl fmt::Debug for ConnectionEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

/// ConnectionStateMachine Object.
///
/// The ConnectionStateMachine object consumes incoming ConnectionEvents and
/// either handles them immediately or dispatches them to other
/// runtimes for further processing.
///
/// For "quick" reactions to incoming events, the FSM handles them
/// immediately on its own thread.
///
/// For "lengthy" reactions, typically involving worker access, the
/// FSM dispatches the work to a "worker" thread.
///
/// For notification events targeted for the observer, the FSM
/// dispatches the work to a "notify" thread.
#[derive(Debug)]
pub struct ConnectionStateMachine<T>
where
    T: Platform,
{
    /// Receiving end of EventPump.
    event_stream:                             EventStream<T>,
    /// Runtime for processing long running requests.
    worker_runtime:                           Option<runtime::Runtime>,
    /// Runtime for processing observer notification events.
    notify_runtime:                           Option<runtime::Runtime>,
    /// The sequence number of the last received remote video status
    /// We process remote video status messages larger than this value.
    last_remote_video_status_sequence_number: Option<u64>,
}

impl<T> fmt::Display for ConnectionStateMachine<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(tid: {:?})", thread::current().id())
    }
}

impl<T> Future for ConnectionStateMachine<T>
where
    T: Platform,
{
    type Item = ();
    type Error = failure::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match try_ready!(self
                .event_stream
                .poll()
                .map_err(|_| { RingRtcError::FsmStreamPoll }))
            {
                Some((cc, event)) => {
                    let state = cc.state()?;
                    match (state, &event) {
                        (
                            ConnectionState::CallConnected,
                            ConnectionEvent::RemoteVideoStatus(_, _, _),
                        )
                        | (ConnectionState::CallConnected, ConnectionEvent::RemoteConnected(_)) => {
                            // Don't log periodic, ignored events at high verbosity
                            debug!("state: {}, event: {}", state, event)
                        }
                        _ => info!("state: {}, event: {}", state, event),
                    };
                    if let Err(e) = self.handle_event(cc, state, event) {
                        error!("Handling event failed: {:?}", e);
                    }
                }
                None => {
                    info!("No more events!");
                    break;
                }
            }
        }

        // The event stream is closed and we are done
        Ok(Async::Ready(()))
    }
}

impl<T> ConnectionStateMachine<T>
where
    T: Platform,
{
    /// Creates a new ConnectionStateMachine object.
    pub fn new(event_stream: EventStream<T>) -> Result<ConnectionStateMachine<T>> {
        let mut fsm = ConnectionStateMachine {
            event_stream,
            worker_runtime: Some(
                runtime::Builder::new()
                    .core_threads(1)
                    .name_prefix("worker-")
                    .build()?,
            ),
            notify_runtime: Some(
                runtime::Builder::new()
                    .core_threads(1)
                    .name_prefix("notify-")
                    .build()?,
            ),
            last_remote_video_status_sequence_number: None,
        };

        if let Some(worker_runtime) = &mut fsm.worker_runtime {
            ConnectionStateMachine::<T>::sync_thread("worker", worker_runtime)?;
        }
        if let Some(notify_runtime) = &mut fsm.notify_runtime {
            ConnectionStateMachine::<T>::sync_thread("notify", notify_runtime)?;
        }

        Ok(fsm)
    }

    /// Synchronize a runtime with the main FSM thread.
    fn sync_thread(label: &'static str, runtime: &mut runtime::Runtime) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let future = lazy(move || {
            info!("syncing {} thread: {:?}", label, thread::current().id());
            let _ = tx.send(true);
            Ok(())
        });
        runtime.spawn(future);
        let _ = rx.recv_timeout(Duration::from_secs(2))?;
        Ok(())
    }

    /// Spawn a future on the worker runtime if enabled.
    fn worker_spawn<F>(&mut self, future: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        if let Some(worker_runtime) = &mut self.worker_runtime {
            worker_runtime.spawn(future);
        }
    }

    /// Spawn a future on the notify runtime if enabled.
    fn notify_spawn<F>(&mut self, future: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        if let Some(notify_runtime) = &mut self.notify_runtime {
            notify_runtime.spawn(future);
        }
    }

    /// Shutdown the worker runtime.
    fn drain_worker_thread(&mut self) {
        info!("draining worker thread");
        if let Some(worker_runtime) = self.worker_runtime.take() {
            let _ = worker_runtime
                .shutdown_on_idle()
                .wait()
                .map_err(|_| warn!("Problems shutting down the worker runtime"));
        }
        info!("draining worker thread: complete");
    }

    /// Shutdown the notify runtime.
    fn drain_notify_thread(&mut self) {
        info!("draining notify thread");
        if let Some(notify_runtime) = self.notify_runtime.take() {
            let _ = notify_runtime
                .shutdown_on_idle()
                .wait()
                .map_err(|_| warn!("Problems shutting down the notify runtime"));
        }
        info!("draining notify thread: complete");
    }

    /// Top level event dispatch.
    fn handle_event(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        event: ConnectionEvent,
    ) -> Result<()> {
        // Handle these events even while terminating, as the remote
        // side needs to be informed.
        match event {
            ConnectionEvent::SendHangupViaDataChannel(hangup) => {
                return self.handle_send_hangup_via_data_channel(connection, state, hangup)
            }
            ConnectionEvent::EndCall => return self.handle_end_call(connection),
            ConnectionEvent::Synchronize(sync) => return self.handle_synchronize(sync),
            _ => {}
        }

        // If in the process of terminating the call, drop all other
        // events.
        match state {
            ConnectionState::Terminating | ConnectionState::Closed => {
                debug!("handle_event(): dropping event {} while terminating", event);
                return Ok(());
            }
            _ => (),
        }

        match event {
            ConnectionEvent::ReceivedHangup(call_id, hangup) => {
                self.handle_received_hangup(connection, state, call_id, hangup)
            }
            ConnectionEvent::AcceptCall => self.handle_accept_call(connection, state),
            ConnectionEvent::RemoteConnected(id) => {
                self.handle_remote_connected(connection, state, id)
            }
            ConnectionEvent::RemoteVideoStatus(id, enable, sequence_number) => {
                self.handle_remote_video_status(connection, state, id, enable, sequence_number)
            }
            ConnectionEvent::ReceivedIce(ice) => self.handle_received_ice(connection, state, ice),
            ConnectionEvent::LocalVideoStatus(enabled) => {
                self.handle_local_video_status(connection, state, enabled)
            }
            ConnectionEvent::LocalIceCandidate(candidate) => {
                self.handle_local_ice_candidate(connection, state, candidate)
            }
            ConnectionEvent::IceConnected => self.handle_ice_connected(connection, state),
            ConnectionEvent::IceConnectionFailed => {
                self.handle_ice_connection_failed(connection, state)
            }
            ConnectionEvent::IceConnectionDisconnected => {
                self.handle_ice_connection_disconnected(connection, state)
            }
            ConnectionEvent::InternalError(error) => self.handle_internal_error(connection, error),
            ConnectionEvent::OnAddStream(stream) => {
                self.handle_on_add_stream(connection, state, stream)
            }
            ConnectionEvent::OnDataChannel(dc) => {
                self.handle_on_data_channel(connection, state, dc)
            }
            ConnectionEvent::SendHangupViaDataChannel(_) => Ok(()),
            ConnectionEvent::Synchronize(_) => Ok(()),
            ConnectionEvent::EndCall => Ok(()),
        }
    }

    fn notify_observer(&mut self, connection: Connection<T>, event: ObserverEvent) {
        let mut err_connection = connection.clone();
        let notify_observer_future = lazy(move || {
            if connection.terminating()? {
                return Ok(());
            }
            connection.notify_observer(event)
        })
        .map_err(move |err| {
            err_connection.inject_internal_error(err, "Notify Observer Future failed")
        });

        self.notify_spawn(notify_observer_future);
    }

    fn handle_received_hangup(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!("dc(hangup/{})\t{}", hangup, call_id)
        );

        if connection.call_id() != call_id {
            warn!("Remote hangup for non-active call");
            return Ok(());
        }
        match state {
            ConnectionState::IceConnecting
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                self.notify_observer(connection, ObserverEvent::ReceivedHangup(hangup))
            }
            _ => self.unexpected_state(state, "RemoteHangup"),
        };
        Ok(())
    }

    fn handle_remote_connected(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
    ) -> Result<()> {
        if connection.call_id() != call_id {
            warn!("Remote connected for non-active call");
            return Ok(());
        }
        match state {
            ConnectionState::IceConnecting | ConnectionState::IceConnected => {
                ringbench!(RingBench::WebRTC, RingBench::Conn, "dc(connected)");
                connection.set_state(ConnectionState::CallConnected)?;
                self.notify_observer(connection, ObserverEvent::RemoteConnected);
            }
            ConnectionState::CallConnected => {
                // Ignore Connected notifications in already-connected state. These may arise
                // because of expected data channel retransmissions.
            }
            _ => self.unexpected_state(state, "RemoteConnected"),
        }
        Ok(())
    }

    fn handle_remote_video_status(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        enable: bool,
        sequence_number: Option<u64>,
    ) -> Result<()> {
        debug!(
            "handle_remote_video_status(): enable: {}, sequence_number: {:?}",
            enable, sequence_number
        );

        if connection.call_id() != call_id {
            warn!("Remote video status change for non-active call");
            return Ok(());
        }

        let out_of_order = match (
            sequence_number,
            self.last_remote_video_status_sequence_number,
        ) {
            // If no sequence number was sent, we assume this is a legacy client that is only
            // using ordered delivery.
            (None, _) => false,
            // This is the first sequence number
            (Some(_), None) => false,
            // If they are equal, we treat it as out of order as well.
            (Some(seqnum), Some(last_seqnum)) => {
                if seqnum < last_seqnum {
                    // Warn only when packets arrive out of order, but not on expected retransmits with the same
                    // sequence number.
                    warn!("Dropped remote video status message because it arrived out of order.");
                };
                seqnum <= last_seqnum
            }
        };
        if out_of_order {
            // Just ignore out of order status messages.
            return Ok(());
        }
        if sequence_number.is_some() {
            self.last_remote_video_status_sequence_number = sequence_number;
        }

        match state {
            ConnectionState::IceConnecting
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                self.notify_observer(connection, ObserverEvent::RemoteVideoStatus(enable))
            }
            _ => self.unexpected_state(state, "RemoteVideoStatus"),
        };
        Ok(())
    }

    fn handle_received_ice(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        ice: signaling::Ice,
    ) -> Result<()> {
        if let ConnectionState::NotYetStarted = state {
            warn!("Connection has not yet started, so ignoring remote ICE candidates...");
            return Ok(());
        }

        match state {
            ConnectionState::IceConnecting
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                connection.add_remote_ice_candidates(&ice.candidates_added)?
            }
            _ => self.unexpected_state(state, "RemoteIceCandidate"),
        }

        Ok(())
    }

    fn handle_accept_call(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::IceConnecting
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected => {
                // notify the peer via a data channel message.
                let mut err_connection = connection.clone();
                let connected_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.send_connected()?;
                    connection.set_state(ConnectionState::CallConnected)
                })
                .map_err(move |err| {
                    err_connection.inject_internal_error(err, "Sending Connected failed")
                });

                self.worker_spawn(connected_future);
            }
            _ => self.unexpected_state(state, "AcceptCall"),
        }
        Ok(())
    }

    fn handle_send_hangup_via_data_channel(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        match state {
            ConnectionState::NotYetStarted => {
                self.unexpected_state(state, "SendHangupViaDataChannel")
            }
            _ => {
                let mut err_connection = connection.clone();
                let hangup_future = lazy(move || connection.send_hangup_via_data_channel(hangup))
                    .map_err(move |err| {
                        err_connection.inject_internal_error(err, "Sending Hangup failed")
                    });

                self.worker_spawn(hangup_future);
            }
        }
        Ok(())
    }

    fn handle_local_video_status(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        enabled: bool,
    ) -> Result<()> {
        match state {
            ConnectionState::IceConnecting
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                // notify the peer via a data channel message.
                let mut err_connection = connection.clone();
                let local_video_status_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.send_video_status(enabled)
                })
                .map_err(move |err| {
                    err_connection.inject_internal_error(err, "Sending local video status failed")
                });

                self.worker_spawn(local_video_status_future);
            }
            _ => self.unexpected_state(state, "LocalVideoStatus"),
        };
        Ok(())
    }

    fn handle_local_ice_candidate(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        candidate: signaling::IceCandidate,
    ) -> Result<()> {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!("ice_candidate()\t{}", connection.id())
        );

        match state {
            ConnectionState::NotYetStarted
            | ConnectionState::Terminating
            | ConnectionState::Closed => {
                warn!("State is now idle or terminating, ignoring local ICE candidate...");
            }
            _ => {
                // send signal message to the other side with the ICE
                // candidate.
                let mut err_connection = connection.clone();
                let ice_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.buffer_local_ice_candidate(candidate)
                })
                .map_err(move |err| err_connection.inject_internal_error(err, "IceFuture failed"));

                self.worker_spawn(ice_future);
            }
        }
        Ok(())
    }

    fn handle_ice_connected(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::IceConnecting => {
                connection.set_state(ConnectionState::IceConnected)?;
                match connection.direction() {
                    CallDirection::OutGoing => {
                        // For outgoing calls, beging ringing the moment ICE is connected.
                        self.notify_observer(connection, ObserverEvent::ConnectionRinging);
                    }
                    CallDirection::InComing => {
                        // Incoming calls don't start ringing until both ICE is connected,
                        // and a data channel is available to send the acceptance request.
                        if connection.has_data_channel()? {
                            self.notify_observer(connection, ObserverEvent::ConnectionRinging);
                        }
                    }
                }
            }
            ConnectionState::IceReconnecting => {
                // ICE has reconnected after the call was
                // previously connected.  Return to that state
                // now.
                connection.set_state(ConnectionState::CallConnected)?;
                self.notify_observer(connection, ObserverEvent::ConnectionReconnected);
            }
            _ => (),
        }
        Ok(())
    }

    fn handle_ice_connection_failed(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::IceConnecting
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                connection.set_state(ConnectionState::IceConnectionFailed)?;
                // For callee -- the call was disconnected while answering/local_ringing
                // For caller -- the recipient was unreachable
                self.notify_observer(connection, ObserverEvent::ConnectionFailed);
            }
            _ => self.unexpected_state(state, "IceConnectionFailed"),
        };
        Ok(())
    }

    fn handle_ice_connection_disconnected(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::IceConnected => {
                // ICE disconnected *before* the call was
                // connected, so simply go back to the
                // IceConnecting state.
                connection.set_state(ConnectionState::IceConnecting)?;
            }
            ConnectionState::CallConnected => {
                // ICE disconnected *after* the call was
                // connected, go to IceReconnecting state.
                connection.set_state(ConnectionState::IceReconnecting)?;
                self.notify_observer(connection, ObserverEvent::ConnectionReconnecting);
            }
            _ => self.unexpected_state(state, "IceConnectionDisconnected"),
        };
        Ok(())
    }

    fn handle_internal_error(
        &mut self,
        connection: Connection<T>,
        error: failure::Error,
    ) -> Result<()> {
        let notify_error_future = lazy(move || {
            if connection.terminating()? {
                return Ok(());
            }
            connection.internal_error(error)
        })
        .map_err(|err| {
            error!("Notify Error Future failed: {}", err);
            // Nothing else we can do here.
        });

        self.notify_spawn(notify_error_future);
        Ok(())
    }

    fn handle_on_add_stream(
        &mut self,
        mut connection: Connection<T>,
        state: ConnectionState,
        stream: MediaStream,
    ) -> Result<()> {
        match state {
            // We allow adding the stream to the connection while we are starting because this gets fired
            // when we call set_remote_description inside of the start_XXX methods.
            // This ends up being called between when we call set_remote_description and when we get the result.
            // This is a little subtle, but seems to work.
            // In the long-term, we should probably not handle this event and instead iterate over the
            // RtpReceivers after calling set_remote_description.
            ConnectionState::Starting
            | ConnectionState::IceConnecting
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                let mut err_connection = connection.clone();
                let add_stream_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.on_add_stream(stream)
                })
                .map_err(move |err| {
                    err_connection.inject_internal_error(err, "Add Media Stream Future failed")
                });

                self.worker_spawn(add_stream_future);
            }
            _ => self.unexpected_state(state, "OnAddStream"),
        }
        Ok(())
    }

    fn handle_on_data_channel(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        data_channel: DataChannel,
    ) -> Result<()> {
        ringbench!(RingBench::WebRTC, RingBench::Conn, "on_data_channel()");

        match state {
            // We allow adding the data channel to the connection while we are starting because this gets fired
            // when we call set_remote_description inside of the start_XXX methods (when using the RTP data channel).
            // This ends up being called between when we call set_remote_description and when we get the result.
            // This is a little subtle, but seems to work.
            // In the long-term, we should probably not handle this event and instead
            // change WebRTC so we can call create_data_channel on both sides with the same SSRC
            // and not rely on the signaling of SSRCs.
            ConnectionState::Starting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                let notify_handle = connection.clone();
                debug_assert_eq!(
                    CallDirection::InComing,
                    connection.direction(),
                    "onDataChannel should only happen for incoming calls"
                );
                connection.set_data_channel(data_channel)?;
                if state == ConnectionState::IceConnected {
                    // Incoming calls don't start ringing until both ICE is connected,
                    // and a data channel is available to send the acceptance request.
                    self.notify_observer(notify_handle, ObserverEvent::ConnectionRinging);
                }
            }
            _ => self.unexpected_state(state, "OnDataChannel"),
        }
        Ok(())
    }

    fn handle_synchronize(&mut self, sync: Arc<(Mutex<bool>, Condvar)>) -> Result<()> {
        if let Some(worker_runtime) = &mut self.worker_runtime {
            ConnectionStateMachine::<T>::sync_thread("worker", worker_runtime)?;
        }
        if let Some(notify_runtime) = &mut self.notify_runtime {
            ConnectionStateMachine::<T>::sync_thread("notify", notify_runtime)?;
        }

        let &(ref mutex, ref condvar) = &*sync;
        if let Ok(mut sync_complete) = mutex.lock() {
            *sync_complete = true;
            condvar.notify_one();
            Ok(())
        } else {
            Err(RingRtcError::MutexPoisoned(
                "Connection Synchronize Condition Variable".to_string(),
            )
            .into())
        }
    }

    fn handle_end_call(&mut self, mut connection: Connection<T>) -> Result<()> {
        self.event_stream.close();
        self.drain_worker_thread();
        self.drain_notify_thread();

        connection.notify_terminate_complete()
    }

    fn unexpected_state(&self, state: ConnectionState, event: &str) {
        warn!("Unexpected event {}, while in state {:?}", event, state);
    }
}
