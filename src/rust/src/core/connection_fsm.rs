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

use crate::common::{
    AnswerParameters,
    CallDirection,
    CallId,
    ConnectionState,
    HangupParameters,
    Result,
    RingBench,
};
use crate::core::connection::{Connection, EventStream, ObserverEvent};
use crate::core::platform::Platform;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media::MediaStream;

/// The different types of Connection Events.
pub enum ConnectionEvent {
    /// Send SDP offer to remote peer (caller only).
    SendOffer(String),
    /// Handle SDP answer from remote peer (caller only).
    HandleAnswer(AnswerParameters),
    /// Handle SDP offer from remote peer (callee only).
    HandleOffer(String),
    /// Connection has both local and remote SDP
    HaveLocalRemoteSdp,
    /// Accept incoming call (callee only).
    AcceptCall,
    /// Receive hangup from remote peer.
    RemoteHangup(CallId, HangupParameters),
    /// Receive call connected from remote peer.
    RemoteConnected(CallId),
    /// Receive video streaming status change from remote peer.
    RemoteVideoStatus(CallId, bool),
    /// Receive ICE candidate message from remote peer.
    ReceivedIceCandidates(Vec<IceCandidate>),
    /// Event from client application to send hangup via the data channel.
    SendHangupViaDataChannel(HangupParameters),
    /// Local video streaming status change from client application.
    LocalVideoStatus(bool),
    /// Local ICE candidate ready, from WebRTC observer.
    LocalIceCandidate(IceCandidate),
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
            ConnectionEvent::SendOffer(_) => "SendOffer".to_string(),
            ConnectionEvent::HandleAnswer(_) => "HandleAnswer".to_string(),
            ConnectionEvent::HandleOffer(_) => "HandleOffer".to_string(),
            ConnectionEvent::HaveLocalRemoteSdp => "HaveLocalRemoteSdp".to_string(),
            ConnectionEvent::AcceptCall => "AcceptCall".to_string(),
            ConnectionEvent::RemoteHangup(id, hangup_parameters) => format!(
                "RemoteHangup, call_id: {} hangup: {}",
                id, hangup_parameters
            ),
            ConnectionEvent::RemoteConnected(id) => format!("RemoteConnected, call_id: {}", id),
            ConnectionEvent::RemoteVideoStatus(id, enabled) => {
                format!("RemoteVideoStatus, call_id: {}, enabled: {}", id, enabled)
            }
            ConnectionEvent::ReceivedIceCandidates(_) => "RemoteIceCandidates".to_string(),
            ConnectionEvent::SendHangupViaDataChannel(hangup_parameters) => {
                format!("SendHangupViaDataChannel, hangup: {}", hangup_parameters)
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
    event_stream:   EventStream<T>,
    /// Runtime for processing long running requests.
    worker_runtime: Option<runtime::Runtime>,
    /// Runtime for processing observer notification events.
    notify_runtime: Option<runtime::Runtime>,
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
                    info!("state: {}, event: {}", state, event);
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
            ConnectionEvent::SendHangupViaDataChannel(hangup_parameters) => {
                return self.handle_send_hangup_via_data_channel(
                    connection,
                    state,
                    hangup_parameters,
                )
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
            ConnectionEvent::SendOffer(offer) => self.handle_send_offer(connection, state, offer),
            ConnectionEvent::HandleAnswer(answer) => self.handle_answer(connection, state, answer),
            ConnectionEvent::HandleOffer(offer) => self.handle_offer(connection, state, offer),
            ConnectionEvent::HaveLocalRemoteSdp => {
                self.handle_have_local_remote_sdp(connection, state)
            }
            ConnectionEvent::AcceptCall => self.handle_accept_call(connection, state),
            ConnectionEvent::RemoteHangup(id, hangup_parameters) => {
                self.handle_remote_hangup(connection, state, id, hangup_parameters)
            }
            ConnectionEvent::RemoteConnected(id) => {
                self.handle_remote_connected(connection, state, id)
            }
            ConnectionEvent::RemoteVideoStatus(id, enable) => {
                self.handle_remote_video_status(connection, state, id, enable)
            }
            ConnectionEvent::ReceivedIceCandidates(candidates) => {
                self.handle_received_ice_candidates(connection, state, candidates)
            }
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

    fn handle_send_offer(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        offer: String,
    ) -> Result<()> {
        if let ConnectionState::Idle = state {
            connection.set_state(ConnectionState::SendingOffer)?;

            let mut err_connection = connection.clone();
            let send_offer_future = lazy(move || {
                if connection.terminating()? {
                    return Ok(());
                }
                connection.send_offer(offer)?;
                Ok(())
            })
            .map_err(move |err| {
                err_connection.inject_internal_error(err, "SendOfferFuture failed")
            });

            self.worker_spawn(send_offer_future);
        } else {
            self.unexpected_state(state, "SendOffer");
        }
        Ok(())
    }

    fn handle_answer(
        &mut self,
        mut connection: Connection<T>,
        state: ConnectionState,
        answer: AnswerParameters,
    ) -> Result<()> {
        if let ConnectionState::SendingOffer = state {
            connection.set_state(ConnectionState::IceConnecting(false))?;
            connection.set_remote_feature_level(answer.remote_feature_level())?;

            let mut err_connection = connection.clone();
            let handle_answer_future = lazy(move || {
                if connection.terminating()? {
                    return Ok(());
                }
                connection.handle_answer(answer.sdp())
            })
            .map_err(move |err| {
                err_connection.inject_internal_error(err, "HandleAnswerFuture failed")
            });

            self.worker_spawn(handle_answer_future);
        } else {
            self.unexpected_state(state, "HandleAnswer");
        }
        Ok(())
    }

    fn handle_offer(
        &mut self,
        mut connection: Connection<T>,
        state: ConnectionState,
        offer: String,
    ) -> Result<()> {
        if let ConnectionState::Idle = state {
            connection.set_state(ConnectionState::IceConnecting(false))?;

            let mut err_connection = connection.clone();
            let handle_offer_future = lazy(move || {
                if connection.terminating()? {
                    return Ok(());
                }
                connection.handle_offer(offer)
            })
            .map_err(move |err| {
                err_connection.inject_internal_error(err, "HandleOfferFuture failed")
            });

            self.worker_spawn(handle_offer_future);
        } else {
            self.unexpected_state(state, "HandleOffer");
        }
        Ok(())
    }

    fn handle_have_local_remote_sdp(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        if let ConnectionState::IceConnecting(false) = state {
            connection.set_state(ConnectionState::IceConnecting(true))?;
            let mut err_connection = connection.clone();
            let handle_remote_ice_updates_future = lazy(move || {
                if connection.terminating()? {
                    return Ok(());
                }
                connection.handle_remote_ice_updates()
            })
            .map_err(move |err| {
                err_connection.inject_internal_error(err, "HandleRemoteIceUpdatesFuture failed")
            });

            self.worker_spawn(handle_remote_ice_updates_future);
        } else {
            self.unexpected_state(state, "HandleHaveLocalRemoteSdp");
        }
        Ok(())
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

    fn handle_remote_hangup(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        hangup_parameters: HangupParameters,
    ) -> Result<()> {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!("dc(hangup/{})\t{}", hangup_parameters, call_id)
        );

        if connection.call_id() != call_id {
            warn!("Remote hangup for non-active call");
            return Ok(());
        }
        match state {
            ConnectionState::IceConnecting(_)
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                self.notify_observer(connection, ObserverEvent::RemoteHangup(hangup_parameters))
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
        ringbench!(RingBench::WebRTC, RingBench::Conn, "dc(connected)");

        if connection.call_id() != call_id {
            warn!("Remote connected for non-active call");
            return Ok(());
        }
        match state {
            ConnectionState::IceConnecting(_) | ConnectionState::IceConnected => {
                connection.set_state(ConnectionState::CallConnected)?;
                self.notify_observer(connection, ObserverEvent::RemoteConnected);
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
    ) -> Result<()> {
        if connection.call_id() != call_id {
            warn!("Remote video status change for non-active call");
            return Ok(());
        }

        match state {
            ConnectionState::IceConnecting(_)
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => {
                self.notify_observer(connection, ObserverEvent::RemoteVideoStatus(enable))
            }
            _ => self.unexpected_state(state, "RemoteVideoStatus"),
        };
        Ok(())
    }

    fn handle_received_ice_candidates(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        ice_candidates: Vec<IceCandidate>,
    ) -> Result<()> {
        if let ConnectionState::Idle = state {
            warn!("State is now idle, ignoring remote ICE candidates...");
            return Ok(());
        }

        connection.buffer_remote_ice_candidates(ice_candidates)?;

        match state {
            ConnectionState::IceConnecting(false) => {}
            ConnectionState::IceConnecting(true)
            | ConnectionState::IceReconnecting
            | ConnectionState::IceConnected
            | ConnectionState::CallConnected => connection.handle_remote_ice_updates()?,
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
            ConnectionState::IceConnecting(_)
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
        hangup_parameters: HangupParameters,
    ) -> Result<()> {
        match state {
            ConnectionState::Idle => self.unexpected_state(state, "SendHangupViaDataChannel"),
            _ => {
                let mut err_connection = connection.clone();
                let hangup_future =
                    lazy(move || connection.send_hangup_via_data_channel(hangup_parameters))
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
            ConnectionState::IceConnecting(_)
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
        candidate: IceCandidate,
    ) -> Result<()> {
        ringbench!(
            RingBench::WebRTC,
            RingBench::Conn,
            format!("ice_candidate()\t{}", connection.id())
        );

        match state {
            ConnectionState::Idle | ConnectionState::Terminating | ConnectionState::Closed => {
                warn!("State is now idle or terminating, ignoring local ICE candidate...");
            }
            _ => {
                // send signal message to the other side with the ICE
                // candidate.
                let mut err_connection = connection.clone();
                let ice_update_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.buffer_local_ice_candidate(candidate)
                })
                .map_err(move |err| {
                    err_connection.inject_internal_error(err, "IceUpdateFuture failed")
                });

                self.worker_spawn(ice_update_future);
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
            ConnectionState::IceConnecting(_) => {
                connection.set_state(ConnectionState::IceConnected)?;
                // When ICE connects for the first time (or
                // reconnects before the call was completely
                // connected), notify only the *caller* about the
                // ringing event.
                if let CallDirection::OutGoing = connection.direction() {
                    self.notify_observer(connection, ObserverEvent::ConnectionRinging);
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
            ConnectionState::IceConnecting(_)
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
            ConnectionState::IceConnecting(_) | ConnectionState::IceConnected => {
                // ICE disconnected *before* the call was
                // connected, so simply go back to the
                // IceConnecting state.
                connection.set_state(ConnectionState::IceConnecting(true))?;
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
            ConnectionState::IceConnecting(_)
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
        mut connection: Connection<T>,
        state: ConnectionState,
        data_channel: DataChannel,
    ) -> Result<()> {
        ringbench!(RingBench::WebRTC, RingBench::Conn, "on_data_channel()");

        match state {
            ConnectionState::IceConnected | ConnectionState::CallConnected => {
                let dc_observer_handle = connection.clone();
                let notify_handle = connection.clone();
                debug_assert_eq!(
                    CallDirection::InComing,
                    connection.direction(),
                    "onDataChannel should only happen for incoming calls"
                );
                connection.on_data_channel(data_channel, dc_observer_handle)?;
                self.notify_observer(notify_handle, ObserverEvent::ConnectionRinging);
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
