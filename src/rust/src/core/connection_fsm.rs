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
//! - SendSenderStatusViaDataChannel
//! - SendReceiverStatusViaDataChannel
//! - SendBusy
//! - ReceivedIce
//! - ReceivedHangup
//!
//! ## From WebRTC observer interfaces
//!
//! - LocalIceCandidate
//! - ConnectedBeforeAccepted
//! - IceFailed
//! - IceDisconnected
//! - ReceivedIncomingMedia
//! - ReceivedDataChannel
//! - ReceivedAcceptedViaDataChannel
//! - ReceivedSenderStatusViaDataChannel
//! - ReceivedReceiverStatusViaDataChannel
//! - ReceivedHangup
//!
//! # Asynchronous Outputs:
//!
//! ## To Call observer
//!
//! - [ConnectionObserverEvents](../connection/enum.ConnectionObserverEvent.html)
//! - ObserverErrors

use std::fmt;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use futures::future::lazy;
use futures::{Async, Future, Poll, Stream};
use tokio::runtime;

use crate::common::{
    units::DataRate,
    BandwidthMode,
    CallDirection,
    CallId,
    ConnectionState,
    Result,
    RingBench,
};
use crate::core::connection::{Connection, ConnectionObserverEvent, EventStream};
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::media::MediaStream;

/// The different types of Connection Events.
pub enum ConnectionEvent {
    /// Receive ICE candidates from remote peer.
    /// Source: signaling
    /// Action: Add candidate to PeerConnection.
    ReceivedIce(signaling::Ice),
    /// Receive hangup from remote peer.
    /// Source: signaling or data channel (PeerConnection)
    /// Action: Bubble up to the Call, which then terminates.
    ReceivedHangup(CallId, signaling::Hangup),
    /// Event from client application to send hangup message via the data channel.
    /// Source: app or internal decision to terminate call
    /// Action: Send a hangup message over the data channel.
    SendHangupViaDataChannel(signaling::Hangup),
    /// Accept incoming call (callee only).
    /// Source: app (user action)
    /// Action: got to "accepted" state and send accept message over the data channel
    Accept,
    /// Receive accepted message from remote peer.
    /// Source: data channel (PeerConnection)
    /// Action: bubble up to Call and transition states
    ReceivedAcceptedViaDataChannel(CallId),
    /// Receive sender status change from remote peer.
    /// Source: data channel (PeerConnection)
    /// Action: Bubble up to app, which should change the "in call" screen.
    ReceivedSenderStatusViaDataChannel(CallId, bool, Option<u64>),
    /// Receive receiver status change from remote peer.
    /// Source: data channel (PeerConnection)
    /// Action: Make adjustments in connection if necessary.
    ReceivedReceiverStatusViaDataChannel(CallId, DataRate, Option<u64>),
    /// Send sender status message via the data channel
    /// Source: app (user action)
    /// Action: Send a sender status message over the data channel.
    SendSenderStatusViaDataChannel(bool),
    /// Set bandwidth mode
    /// Source: app (user setting)
    /// Action: Set bitrate and send a receiver status message over the data channel.
    SetBandwidthMode(BandwidthMode),
    /// Local ICE candidate from PeerConnection
    /// Source: PeerConnection
    /// Action: Send ICE candidate over signaling.
    LocalIceCandidate(signaling::IceCandidate),
    /// ICE state changed.
    /// Source: PeerConnection
    /// Action: Bubble up to Connection and Call objects.
    IceConnected,
    /// ICE state changed.
    /// Source: PeerConnection
    /// Action: Bubble up to Connection and Call objects.
    IceFailed,
    /// ICE state changed.
    /// Source: PeerConnection
    /// Action: Bubble up to Connection and Call objects.
    IceDisconnected,
    /// Send the observer an internal error message.
    /// Source: all kinds of things that can go wrong internally
    /// Action: Terminate the call.
    InternalError(failure::Error),
    /// Receive incoming media from PeerConnection
    /// Source: PeerConnection (OnAddStream)
    /// Action: remember the MediaStream so we can "connect" to it after the call is accepted
    ReceivedIncomingMedia(MediaStream),
    /// Received data channel from PeerConnection
    /// Source: PeerConnection
    /// Action: Use the DataChannel to send and receive messages.
    ReceivedDataChannel(DataChannel),
    /// Synchronize the FSM.
    /// Only used by unit tests
    Synchronize(Arc<(Mutex<bool>, Condvar)>),

    /// Terminate the connection.
    /// Source: Termination of the call or reponse to ICE failed
    /// Action: Drain threads of tasks and wait for them
    Terminate,
}

impl fmt::Display for ConnectionEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            ConnectionEvent::Accept => "Accept".to_string(),
            ConnectionEvent::ReceivedHangup(call_id, hangup) => {
                format!("RemoteHangup, call_id: {} hangup: {}", call_id, hangup)
            }
            ConnectionEvent::ReceivedAcceptedViaDataChannel(id) => {
                format!("ReceivedAcceptedViaDataChannel, call_id: {}", id)
            }
            ConnectionEvent::ReceivedSenderStatusViaDataChannel(id, video_enabled, sequence_number) => {
                format!(
                    "ReceivedSenderStatusViaDataChannel, call_id: {}, video_enabled: {}, seqnum: {:?}",
                    id, video_enabled, sequence_number
                )
            }
            ConnectionEvent::ReceivedReceiverStatusViaDataChannel(id, max_bitrate, sequence_number) => {
                format!(
                    "ReceivedReceiverStatusViaDataChannel, call_id: {}, max_bitrate: {:?}, seqnum: {:?}",
                    id, max_bitrate, sequence_number
                )
            }
            ConnectionEvent::ReceivedIce(_) => "RemoteIceCandidates".to_string(),
            ConnectionEvent::SendHangupViaDataChannel(hangup) => {
                format!("SendHangupViaDataChannel, hangup: {}", hangup)
            }
            ConnectionEvent::SendSenderStatusViaDataChannel(enabled) => format!(
                "SendSenderStatusViaDataChannel, enabled: {}",
                enabled
            ),
            ConnectionEvent::SetBandwidthMode(mode) => format!(
                "SetBandwidthMode, mode: {:?}",
                mode
            ),
            ConnectionEvent::LocalIceCandidate(_) => "LocalIceCandidate".to_string(),
            ConnectionEvent::IceConnected => "IceConnected".to_string(),
            ConnectionEvent::IceFailed => "IceConnectionFailed".to_string(),
            ConnectionEvent::IceDisconnected => "IceDisconnected".to_string(),
            ConnectionEvent::InternalError(e) => format!("InternalError: {}", e),
            ConnectionEvent::ReceivedIncomingMedia(stream) => {
                format!("ReceivedIncomingMedia, stream: {:}", stream)
            }
            ConnectionEvent::ReceivedDataChannel(dc) => {
                format!("ReceivedDataChannel, dc: {:?}", dc)
            }
            ConnectionEvent::Synchronize(_) => "Synchronize".to_string(),
            ConnectionEvent::Terminate => "Terminate".to_string(),
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
    event_stream: EventStream<T>,
    /// Runtime for processing long running requests.
    worker_runtime: Option<runtime::Runtime>,
    /// Runtime for processing observer notification events.
    notify_runtime: Option<runtime::Runtime>,
    /// The sequence number of the last received remote sender status
    /// We process remote sender status messages larger than this value.
    last_remote_sender_status_sequence_number: Option<u64>,
    /// The sequence number of the last received remote receiver status
    /// We process remote receiver status messages larger than this value.
    last_remote_receiver_status_sequence_number: Option<u64>,
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
                            ConnectionState::ConnectedAndAccepted,
                            ConnectionEvent::ReceivedSenderStatusViaDataChannel(_, _, _),
                        )
                        | (
                            ConnectionState::ConnectedAndAccepted,
                            ConnectionEvent::ReceivedReceiverStatusViaDataChannel(_, _, _),
                        )
                        | (
                            ConnectionState::ConnectedAndAccepted,
                            ConnectionEvent::ReceivedAcceptedViaDataChannel(_),
                        ) => {
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
            last_remote_sender_status_sequence_number: None,
            last_remote_receiver_status_sequence_number: None,
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
            ConnectionEvent::Terminate => return self.handle_terminate(connection),
            ConnectionEvent::Synchronize(sync) => return self.handle_synchronize(sync),
            _ => {}
        }

        // If in the process of terminating the call, drop all other
        // events.
        match state {
            ConnectionState::Terminating | ConnectionState::Terminated => {
                debug!("handle_event(): dropping event {} while terminating", event);
                return Ok(());
            }
            _ => (),
        }

        match event {
            ConnectionEvent::ReceivedHangup(call_id, hangup) => {
                self.handle_received_hangup(connection, state, call_id, hangup)
            }
            ConnectionEvent::Accept => self.handle_accept(connection, state),
            ConnectionEvent::ReceivedAcceptedViaDataChannel(id) => {
                self.handle_received_accepted_via_data_channel(connection, state, id)
            }
            ConnectionEvent::ReceivedSenderStatusViaDataChannel(
                id,
                video_enable,
                sequence_number,
            ) => self.handle_received_sender_status_via_data_channel(
                connection,
                state,
                id,
                video_enable,
                sequence_number,
            ),
            ConnectionEvent::ReceivedReceiverStatusViaDataChannel(
                id,
                max_bitrate,
                sequence_number,
            ) => self.handle_received_receiver_status_via_data_channel(
                connection,
                state,
                id,
                max_bitrate,
                sequence_number,
            ),
            ConnectionEvent::ReceivedIce(ice) => self.handle_received_ice(connection, state, ice),
            ConnectionEvent::SendSenderStatusViaDataChannel(enabled) => {
                self.handle_send_sender_status_via_data_channel(connection, state, enabled)
            }
            ConnectionEvent::SetBandwidthMode(mode) => {
                self.handle_set_bandwidth_mode(connection, state, mode)
            }
            ConnectionEvent::LocalIceCandidate(candidate) => {
                self.handle_local_ice_candidate(connection, state, candidate)
            }
            ConnectionEvent::IceConnected => self.handle_ice_connected(connection, state),
            ConnectionEvent::IceFailed => self.handle_ice_failed(connection, state),
            ConnectionEvent::IceDisconnected => self.handle_ice_disconnected(connection, state),
            ConnectionEvent::InternalError(error) => self.handle_internal_error(connection, error),
            ConnectionEvent::ReceivedIncomingMedia(stream) => {
                self.handle_received_incoming_media(connection, state, stream)
            }
            ConnectionEvent::ReceivedDataChannel(dc) => {
                self.handle_received_data_channel(connection, state, dc)
            }
            ConnectionEvent::SendHangupViaDataChannel(_) => Ok(()),
            ConnectionEvent::Synchronize(_) => Ok(()),
            ConnectionEvent::Terminate => Ok(()),
        }
    }

    fn notify_observer(&mut self, connection: Connection<T>, event: ConnectionObserverEvent) {
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
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                self.notify_observer(connection, ConnectionObserverEvent::ReceivedHangup(hangup))
            }
            _ => self.unexpected_state(state, "RemoteHangup"),
        };
        Ok(())
    }

    fn handle_received_accepted_via_data_channel(
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
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ConnectedBeforeAccepted => {
                ringbench!(RingBench::WebRTC, RingBench::Conn, "dc(accepted)");
                connection.set_state(ConnectionState::ConnectedAndAccepted)?;
                self.notify_observer(
                    connection,
                    ConnectionObserverEvent::ReceivedAcceptedViaDataChannel,
                );
            }
            ConnectionState::ConnectedAndAccepted => {
                // Ignore Accepted notifications in already-accepted state. These may arise
                // because of expected data channel retransmissions.
            }
            _ => self.unexpected_state(state, "ReceivedAcceptedViaDataChannel"),
        }
        Ok(())
    }

    fn handle_received_sender_status_via_data_channel(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        video_enable: bool,
        sequence_number: Option<u64>,
    ) -> Result<()> {
        debug!(
            "handle_received_sender_status_via_data_channel(): video_enable: {}, sequence_number: {:?}",
            video_enable, sequence_number
        );

        if connection.call_id() != call_id {
            warn!("Remote sender status change for non-active call");
            return Ok(());
        }

        let out_of_order = match (
            sequence_number,
            self.last_remote_sender_status_sequence_number,
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
                    warn!("Dropped remote sender status message because it arrived out of order.");
                };
                seqnum <= last_seqnum
            }
        };
        if out_of_order {
            // Just ignore out of order status messages.
            return Ok(());
        }
        if sequence_number.is_some() {
            self.last_remote_sender_status_sequence_number = sequence_number;
        }

        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => self.notify_observer(
                connection,
                ConnectionObserverEvent::ReceivedSenderStatusViaDataChannel(video_enable),
            ),
            _ => self.unexpected_state(state, "ReceivedSenderStatusViaDataChannel"),
        };
        Ok(())
    }

    fn handle_received_receiver_status_via_data_channel(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        max_bitrate: DataRate,
        sequence_number: Option<u64>,
    ) -> Result<()> {
        debug!(
            "handle_received_receiver_status_via_data_channel(): max_bitrate: {:?}, sequence_number: {:?}",
            max_bitrate, sequence_number
        );

        if connection.call_id() != call_id {
            warn!("Remote sender status change for non-active call");
            return Ok(());
        }

        let out_of_order = match (
            sequence_number,
            self.last_remote_receiver_status_sequence_number,
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
                    warn!(
                        "Dropped remote receiver status message because it arrived out of order."
                    );
                };
                seqnum <= last_seqnum
            }
        };
        if out_of_order {
            // Just ignore out of order status messages.
            return Ok(());
        }
        if sequence_number.is_some() {
            self.last_remote_receiver_status_sequence_number = sequence_number;
        }

        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                connection.set_remote_max_bitrate(max_bitrate)?
            }
            _ => self.unexpected_state(state, "ReceivedReceiverStatusViaDataChannel"),
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
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                connection.add_remote_ice_candidates(&ice.candidates_added)?
            }
            _ => self.unexpected_state(state, "RemoteIceCandidate"),
        }

        Ok(())
    }

    fn handle_accept(&mut self, connection: Connection<T>, state: ConnectionState) -> Result<()> {
        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted => {
                // notify the peer via a data channel message.
                let mut err_connection = connection.clone();
                let connected_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.set_state(ConnectionState::ConnectedAndAccepted)?;
                    connection.send_accepted_via_data_channel()
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

    fn handle_send_sender_status_via_data_channel(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        video_enabled: bool,
    ) -> Result<()> {
        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                // notify the peer via a data channel message.
                let mut err_connection = connection.clone();
                let send_sender_status_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.send_sender_status_via_data_channel(video_enabled)
                })
                .map_err(move |err| {
                    err_connection.inject_internal_error(err, "Sending local sender status failed")
                });

                self.worker_spawn(send_sender_status_future);
            }
            _ => self.unexpected_state(state, "SendSenderStatusViaDataChannel"),
        };
        Ok(())
    }

    fn handle_set_bandwidth_mode(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        mode: BandwidthMode,
    ) -> Result<()> {
        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                let mut err_connection = connection.clone();
                let set_bandwidth_mode_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }

                    connection.set_local_max_bitrate(mode.max_bitrate())
                })
                .map_err(move |err| {
                    err_connection.inject_internal_error(err, "Setting low bandwidth mode failed")
                });

                self.worker_spawn(set_bandwidth_mode_future);
            }
            _ => self.unexpected_state(state, "SetBandwidthMode"),
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
            | ConnectionState::Terminated => {
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
            ConnectionState::ConnectingBeforeAccepted => {
                connection.set_state(ConnectionState::ConnectedBeforeAccepted)?;
                match connection.direction() {
                    CallDirection::OutGoing => {
                        // For outgoing calls, we assume we have a data channel.
                        self.notify_observer(
                            connection,
                            ConnectionObserverEvent::ConnectedWithDataChannelBeforeAccepted,
                        );
                    }
                    CallDirection::InComing => {
                        if connection.has_data_channel()? {
                            self.notify_observer(
                                connection,
                                ConnectionObserverEvent::ConnectedWithDataChannelBeforeAccepted,
                            );
                        }
                    }
                }
            }
            ConnectionState::ReconnectingAfterAccepted => {
                // ICE has reconnected after the call was
                // previously accepted (and connected).  Return to that state
                // now.
                connection.set_state(ConnectionState::ConnectedAndAccepted)?;
                self.notify_observer(
                    connection,
                    ConnectionObserverEvent::ReconnectedAfterAccepted,
                );
            }
            _ => self.unexpected_state(state, "IceConnected"),
        }
        Ok(())
    }

    fn handle_ice_failed(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                connection.set_state(ConnectionState::IceFailed)?;
                // For callee -- the call was disconnected while answering/local_ringing
                // For caller -- the recipient was unreachable
                self.notify_observer(connection, ConnectionObserverEvent::IceFailed);
            }
            _ => self.unexpected_state(state, "IceFailed"),
        };
        Ok(())
    }

    fn handle_ice_disconnected(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::ConnectedBeforeAccepted => {
                connection.set_state(ConnectionState::ConnectingBeforeAccepted)?;
            }
            ConnectionState::ConnectedAndAccepted => {
                connection.set_state(ConnectionState::ReconnectingAfterAccepted)?;
                self.notify_observer(
                    connection,
                    ConnectionObserverEvent::ReconnectingAfterAccepted,
                );
            }
            _ => self.unexpected_state(state, "IceDisconnected"),
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

    fn handle_received_incoming_media(
        &mut self,
        mut connection: Connection<T>,
        state: ConnectionState,
        stream: MediaStream,
    ) -> Result<()> {
        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                let mut err_connection = connection.clone();
                let add_stream_future = lazy(move || {
                    if connection.terminating()? {
                        return Ok(());
                    }
                    connection.handle_received_incoming_media(stream)
                })
                .map_err(move |err| {
                    err_connection.inject_internal_error(err, "Add Media Stream Future failed")
                });

                self.worker_spawn(add_stream_future);
            }
            _ => self.unexpected_state(state, "ReceivedIncomingMedia"),
        }
        Ok(())
    }

    fn handle_received_data_channel(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        data_channel: DataChannel,
    ) -> Result<()> {
        ringbench!(RingBench::WebRTC, RingBench::Conn, "on_data_channel()");

        match state {
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ConnectedBeforeAccepted
            | ConnectionState::ConnectedAndAccepted => {
                let notify_handle = connection.clone();
                debug_assert_eq!(
                    CallDirection::InComing,
                    connection.direction(),
                    "ReceivedDataChannel should only happen for incoming calls"
                );
                connection.set_data_channel(data_channel)?;
                if state == ConnectionState::ConnectedBeforeAccepted {
                    self.notify_observer(
                        notify_handle,
                        ConnectionObserverEvent::ConnectedWithDataChannelBeforeAccepted,
                    );
                }
            }
            _ => self.unexpected_state(state, "ReceivedDataChannel"),
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

    fn handle_terminate(&mut self, mut connection: Connection<T>) -> Result<()> {
        self.event_stream.close();
        self.drain_worker_thread();
        self.drain_notify_thread();

        connection.notify_terminate_complete()
    }

    fn unexpected_state(&self, state: ConnectionState, event: &str) {
        warn!("Unexpected event {}, while in state {:?}", event, state);
    }
}
