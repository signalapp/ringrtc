//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
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
//! - UpdateSenderStatus
//! - SendReceiverStatusViaRtpData
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
//! - ReceivedAcceptedViaRtpData
//! - ReceivedSenderStatusViaRtpData
//! - ReceivedReceiverStatusViaRtpData
//! - ReceivedHangup
//!
//! # Asynchronous Outputs:
//!
//! ## To Call observer
//!
//! - [ConnectionObserverEvents](../connection/enum.ConnectionObserverEvent.html)
//! - ObserverErrors

use std::fmt;
use std::pin::Pin;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use futures::future::lazy;
use futures::future::TryFutureExt;
use futures::task::Poll;
use futures::{Future, Stream};

use crate::common::{units::DataRate, CallDirection, CallId, ConnectionState, Result, RingBench};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::connection::{Connection, ConnectionObserverEvent, EventStream};
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::core::util::TaskQueueRuntime;
use crate::error::RingRtcError;
use crate::webrtc::media::MediaStream;
use crate::webrtc::peer_connection_observer::NetworkRoute;

/// The different types of Connection Events.
pub enum ConnectionEvent {
    /// Receive ICE candidates from remote peer.
    /// Source: signaling
    /// Action: Add candidate to PeerConnection.
    ReceivedIce(signaling::Ice),
    /// Receive hangup from remote peer.
    /// Source: signaling or RTP data
    /// Action: Bubble up to the Call, which then terminates.
    ReceivedHangup(CallId, signaling::Hangup),
    /// Event from client application to send hangup message via RTP data.
    /// Source: app or internal decision to terminate call
    /// Action: Send a hangup message over RTP data.
    SendHangupViaRtpData(signaling::Hangup),
    /// Accept incoming call (callee only).
    /// Source: app (user action)
    /// Action: got to "accepted" state and send accept message via RTP data.
    Accept,
    /// Receive accepted message from remote peer.
    /// Source: RTP data
    /// Action: bubble up to Call and transition states
    ReceivedAcceptedViaRtpData(CallId),
    /// Receive sender status change from remote peer.
    /// Source: RTP data
    /// Action: Bubble up to app, which should change the "in call" screen.
    ReceivedSenderStatusViaRtpData(CallId, signaling::SenderStatus, u64),
    /// Receive receiver status change from remote peer.
    /// Source: RTP data
    /// Action: Make adjustments in connection if necessary.
    ReceivedReceiverStatusViaRtpData(CallId, DataRate, u64),
    /// Send sender status message via RTP data
    /// Source: app (user action)
    /// Action: Accumulate and send a sender status message via RTP data.
    UpdateSenderStatus(signaling::SenderStatus),
    /// Set bandwidth mode
    /// Source: app (user setting)
    /// Action: Update and send bitrate via a receiver status message via RTP data.
    UpdateBandwidthMode(BandwidthMode),
    /// Local ICE candidates added or removed from PeerConnection
    /// Source: PeerConnection
    /// Action: Send ICE candidate (addition or removal) over signaling.
    LocalIceCandidates(Vec<signaling::IceCandidate>),
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
    /// ICE network path (selected candidate pair) changed.
    /// Source: PeerConnection
    /// Action: Bubble up to Connection and Call objects.
    IceNetworkRouteChanged(NetworkRoute),
    /// Send the observer an internal error message.
    /// Source: all kinds of things that can go wrong internally
    /// Action: Terminate the call.
    InternalError(anyhow::Error),
    /// Receive incoming media from PeerConnection
    /// Source: PeerConnection (OnAddStream)
    /// Action: remember the MediaStream so we can "connect" to it after the call is accepted
    ReceivedIncomingMedia(MediaStream),
    /// Synchronize the FSM.
    /// Only used by unit tests
    Synchronize(Arc<(Mutex<bool>, Condvar)>),

    /// Terminate the connection.
    /// Source: Termination of the call or response to ICE failed
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
            ConnectionEvent::ReceivedAcceptedViaRtpData(id) => {
                format!("ReceivedAcceptedViaRtpData, call_id: {}", id)
            }
            ConnectionEvent::ReceivedSenderStatusViaRtpData(id, status, seqnum) => {
                format!(
                    "ReceivedSenderStatusViaRtpData, call_id: {}, status: {:?}, seqnum: {:?}",
                    id, status, seqnum
                )
            }
            ConnectionEvent::ReceivedReceiverStatusViaRtpData(id, max_bitrate, seqnum) => {
                format!(
                    "ReceivedReceiverStatusViaRtpData, call_id: {}, max_bitrate: {:?}, seqnum: {:?}",
                    id, max_bitrate, seqnum
                )
            }
            ConnectionEvent::ReceivedIce(_) => "RemoteIceCandidates".to_string(),
            ConnectionEvent::SendHangupViaRtpData(hangup) => {
                format!("SendHangupViaRtpData, hangup: {}", hangup)
            }
            ConnectionEvent::UpdateSenderStatus(status) => {
                format!("UpdateSenderStatus, status: {:?}", status)
            }
            ConnectionEvent::UpdateBandwidthMode(mode) => {
                format!("UpdateBandwidthMode, mode: {:?}", mode)
            }
            ConnectionEvent::LocalIceCandidates(_) => "LocalIceCandidates".to_string(),
            ConnectionEvent::IceConnected => "IceConnected".to_string(),
            ConnectionEvent::IceFailed => "IceConnectionFailed".to_string(),
            ConnectionEvent::IceDisconnected => "IceDisconnected".to_string(),
            ConnectionEvent::IceNetworkRouteChanged(network_route) => format!(
                "IceNetworkRouteChanged, network_route: {:?})",
                network_route
            ),
            ConnectionEvent::InternalError(e) => format!("InternalError: {}", e),
            ConnectionEvent::ReceivedIncomingMedia(stream) => {
                format!("ReceivedIncomingMedia, stream: {:?}", stream)
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
    worker_runtime: Option<TaskQueueRuntime>,
    /// Runtime for processing observer notification events.
    notify_runtime: Option<TaskQueueRuntime>,
    /// The sequence number and last received remote sender status.
    /// We process remote sender status messages larger than the seqnum
    /// and fire events when the status changes.
    last_remote_sender_status: Option<(u64, signaling::SenderStatus)>,
    /// The sequence number of the last received remote receiver bitrate.
    /// We process remote receiver status messages larger than the seqnum
    /// and use the bitrate when it changes.
    last_remote_receiver_status: Option<(u64, DataRate)>,
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
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut futures::task::Context) -> Poll<Self::Output> {
        loop {
            let pin_stream = Pin::new(&mut self.event_stream);
            match ready!(pin_stream.poll_next(cx)) {
                Some((cc, event)) => {
                    let state = cc.state()?;
                    match (state, &event) {
                        (
                            ConnectionState::ConnectedAndAccepted,
                            ConnectionEvent::ReceivedSenderStatusViaRtpData(_, _, _),
                        )
                        | (
                            ConnectionState::ConnectedAndAccepted,
                            ConnectionEvent::ReceivedReceiverStatusViaRtpData(_, _, _),
                        )
                        | (
                            ConnectionState::ConnectedAndAccepted,
                            ConnectionEvent::ReceivedAcceptedViaRtpData(_),
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
                    debug!("No more events!");
                    break;
                }
            }
        }

        // The event stream is closed and we are done
        Poll::Ready(Ok(()))
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
            worker_runtime: Some(TaskQueueRuntime::new("connection-fsm-worker")?),
            notify_runtime: Some(TaskQueueRuntime::new("connection-fsm-notify")?),
            last_remote_sender_status: None,
            last_remote_receiver_status: None,
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
    fn sync_thread(label: &'static str, runtime: &mut TaskQueueRuntime) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let future = lazy(move |_| {
            info!("syncing {} thread: {:?}", label, thread::current().id());
            let _ = tx.send(true);
        });
        runtime.spawn(future);
        let _ = rx.recv_timeout(Duration::from_secs(2))?;
        Ok(())
    }

    /// Spawn a future on the worker runtime if enabled.
    fn worker_spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if let Some(worker_runtime) = &mut self.worker_runtime {
            worker_runtime.spawn(future);
        }
    }

    /// Spawn a future on the notify runtime if enabled.
    fn notify_spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if let Some(notify_runtime) = &mut self.notify_runtime {
            notify_runtime.spawn(future);
        }
    }

    /// Shutdown the worker runtime.
    fn drain_worker_thread(&mut self) {
        debug!("draining worker thread");
        self.worker_runtime.take();
        debug!("draining worker thread: complete");
    }

    /// Shutdown the notify runtime.
    fn drain_notify_thread(&mut self) {
        debug!("draining notify thread");
        self.notify_runtime.take();
        debug!("draining notify thread: complete");
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
            ConnectionEvent::SendHangupViaRtpData(hangup) => {
                return self.handle_send_hangup_via_rtp_data(connection, state, hangup)
            }
            ConnectionEvent::Terminate => return self.handle_terminate(connection),
            ConnectionEvent::Synchronize(sync) => return self.handle_synchronize(sync),
            _ => {}
        }

        if state.terminating_or_terminated() {
            debug!("handle_event(): dropping event {} while terminating", event);
            return Ok(());
        }

        match event {
            ConnectionEvent::ReceivedHangup(call_id, hangup) => {
                self.handle_received_hangup(connection, state, call_id, hangup)
            }
            ConnectionEvent::Accept => self.handle_accept(connection, state),
            ConnectionEvent::ReceivedAcceptedViaRtpData(id) => {
                self.handle_received_accepted_via_rtp_data(connection, state, id)
            }
            ConnectionEvent::ReceivedSenderStatusViaRtpData(id, status, seqnum) => self
                .handle_received_sender_status_via_rtp_data(connection, state, id, status, seqnum),
            ConnectionEvent::ReceivedReceiverStatusViaRtpData(id, max_bitrate, seqnum) => self
                .handle_received_receiver_status_via_rtp_data(
                    connection,
                    state,
                    id,
                    max_bitrate,
                    seqnum,
                ),
            ConnectionEvent::ReceivedIce(ice) => self.handle_received_ice(connection, state, ice),
            ConnectionEvent::UpdateSenderStatus(status) => {
                self.handle_update_sender_status(connection, state, status)
            }
            ConnectionEvent::UpdateBandwidthMode(mode) => {
                self.handle_update_bandwidth_mode(connection, state, mode)
            }
            ConnectionEvent::LocalIceCandidates(candidates) => {
                self.handle_local_ice_candidates(connection, state, candidates)
            }
            ConnectionEvent::IceConnected => self.handle_ice_connected(connection, state),
            ConnectionEvent::IceFailed => self.handle_ice_failed(connection, state),
            ConnectionEvent::IceDisconnected => self.handle_ice_disconnected(connection, state),
            ConnectionEvent::IceNetworkRouteChanged(network_route) => {
                self.handle_ice_network_route_changed(connection, network_route)
            }
            ConnectionEvent::InternalError(error) => self.handle_internal_error(connection, error),
            ConnectionEvent::ReceivedIncomingMedia(stream) => {
                self.handle_received_incoming_media(connection, state, stream)
            }
            ConnectionEvent::SendHangupViaRtpData(_) => Ok(()),
            ConnectionEvent::Synchronize(_) => Ok(()),
            ConnectionEvent::Terminate => Ok(()),
        }
    }

    fn notify_observer(&mut self, connection: Connection<T>, event: ConnectionObserverEvent) {
        let mut err_connection = connection.clone();
        let notify_observer_future = lazy(move |_| {
            if connection.terminating()? {
                return Ok(());
            }
            connection.notify_observer(event)
        })
        .unwrap_or_else(move |err| {
            err_connection.inject_internal_error(err, "Notify Observer Future failed");
        });

        self.notify_spawn(notify_observer_future);
    }

    fn handle_connected_and_accepted_for_the_first_time(
        &mut self,
        connection: Connection<T>,
    ) -> Result<()> {
        connection.set_state(ConnectionState::ConnectedAndAccepted)?;
        // We may have received status messages while ringing, which we now must
        // process because they are ignored before the call is accepted.
        if let Some((_, max_bitrate)) = self.last_remote_receiver_status {
            Self::handle_remote_receiver_status_changed(&connection, max_bitrate)?;
        }
        if let Some((_, status)) = self.last_remote_sender_status {
            Self::handle_remote_sender_status_changed(&connection, status)?;
        }
        if connection.direction() == CallDirection::InComing {
            self.send_accepted_via_rtp_data(connection);
        }
        Ok(())
    }

    // This can happen either when it changes or when we process a cached one
    // when we are first ConnectedAndAccepted.
    fn handle_remote_receiver_status_changed(
        connection: &Connection<T>,
        max_bitrate: DataRate,
    ) -> Result<()> {
        connection.set_remote_max_bitrate(max_bitrate)
    }

    // This can happen either when it changes or when we process a cached one
    // when we are first ConnectedAndAccepted.
    fn handle_remote_sender_status_changed(
        connection: &Connection<T>,
        status: signaling::SenderStatus,
    ) -> Result<()> {
        connection.notify_observer(ConnectionObserverEvent::RemoteSenderStatusChanged(status))
    }

    fn send_accepted_via_rtp_data(&mut self, connection: Connection<T>) {
        let mut err_connection = connection.clone();
        let connected_future = lazy(move |_| {
            if connection.terminating()? {
                return Ok(());
            }
            connection.send_accepted_via_rtp_data()?;
            Ok(())
        })
        .unwrap_or_else(move |err| {
            err_connection.inject_internal_error(err, "Sending Connected failed");
        });
        self.worker_spawn(connected_future);
    }

    fn handle_received_hangup(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        ringbench!(
            RingBench::WebRtc,
            RingBench::Conn,
            format!("dc(hangup/{})\t{}", hangup, call_id)
        );

        if connection.call_id() != call_id {
            warn!("Remote hangup for non-active call");
            return Ok(());
        }
        if state.connecting_or_connected() {
            self.notify_observer(connection, ConnectionObserverEvent::ReceivedHangup(hangup))
        } else {
            self.unexpected_state(state, "RemoteHangup");
        }
        Ok(())
    }

    fn handle_received_accepted_via_rtp_data(
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
            ConnectionState::NotYetStarted
            | ConnectionState::Starting
            | ConnectionState::IceGathering => {
                // It shouldn't be possible to receive anything over RTP yet.
                self.unexpected_state(state, "ReceivedAcceptedViaRtpData");
            }
            ConnectionState::ConnectingBeforeAccepted => {
                ringbench!(
                    RingBench::WebRtc,
                    RingBench::Conn,
                    format!(
                        "dc(accepted)\t{} (before connected)",
                        connection.connection_id()
                    )
                );
                connection.set_state(ConnectionState::ConnectingAfterAccepted)?;
            }
            ConnectionState::ConnectedBeforeAccepted => {
                ringbench!(
                    RingBench::WebRtc,
                    RingBench::Conn,
                    format!(
                        "dc(accepted)\t{} (after connected)",
                        connection.connection_id()
                    )
                );
                self.handle_connected_and_accepted_for_the_first_time(connection)?;
            }
            ConnectionState::ConnectingAfterAccepted
            | ConnectionState::ConnectedAndAccepted
            | ConnectionState::ReconnectingAfterAccepted => {
                // Ignore Accepted notifications in already-accepted state. These may arise
                // because of expected RTP data retransmissions.
            }
            ConnectionState::IceFailed
            | ConnectionState::Terminating
            | ConnectionState::Terminated => {
                // It might be possible, but definitely unexpected.
                self.unexpected_state(state, "ReceivedAcceptedViaRtpData");
            }
        }
        Ok(())
    }

    fn handle_received_sender_status_via_rtp_data(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        status: signaling::SenderStatus,
        seqnum: u64,
    ) -> Result<()> {
        debug!(
            "handle_received_sender_status_via_rtp_data(): status: {:?}, seqnum: {:?}",
            status, seqnum
        );

        if connection.call_id() != call_id {
            warn!("Remote sender status change for non-active call");
            return Ok(());
        }

        let changed = match self.last_remote_sender_status {
            // This is the first sequence number
            None => true,
            Some((last_seqnum, last_status)) => {
                if seqnum < last_seqnum {
                    // Warn only when packets arrive out of order, but not on expected retransmits
                    // with the same sequence number.
                    warn!("Dropped remote sender status message because it arrived out of order.");
                };

                // If they are equal, we treat it as out of order as well.
                if seqnum <= last_seqnum {
                    // Just ignore out of order status messages.
                    return Ok(());
                }

                last_status != status
            }
        };
        self.last_remote_sender_status = Some((seqnum, status));

        match state {
            ConnectionState::ConnectedAndAccepted | ConnectionState::ReconnectingAfterAccepted => {
                if changed {
                    Self::handle_remote_sender_status_changed(&connection, status)?;
                }
            }
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ConnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted => {
                // Ignore before active
            }
            ConnectionState::NotYetStarted
            | ConnectionState::Starting
            | ConnectionState::IceGathering
            | ConnectionState::IceFailed
            | ConnectionState::Terminating
            | ConnectionState::Terminated => {
                self.unexpected_state(state, "ReceivedSenderStatusViaRtpData");
            }
        }
        Ok(())
    }

    fn handle_received_receiver_status_via_rtp_data(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        call_id: CallId,
        max_bitrate: DataRate,
        seqnum: u64,
    ) -> Result<()> {
        debug!(
            "handle_received_receiver_status_via_rtp_data(): max_bitrate: {:?}, seqnum: {:?}",
            max_bitrate, seqnum
        );

        if connection.call_id() != call_id {
            warn!("Remote sender status change for non-active call");
            return Ok(());
        }

        let changed = match self.last_remote_receiver_status {
            // This is the first sequence number
            None => true,
            Some((last_seqnum, last_max_bitrate)) => {
                if seqnum < last_seqnum {
                    // Warn only when packets arrive out of order, but not on expected retransmits
                    // with the same sequence number.
                    warn!(
                        "Dropped remote receiver status message because it arrived out of order."
                    );
                };

                // If they are equal, we treat it as out of order as well.
                if seqnum <= last_seqnum {
                    // Just ignore out of order status messages.
                    return Ok(());
                }

                max_bitrate != last_max_bitrate
            }
        };
        self.last_remote_receiver_status = Some((seqnum, max_bitrate));

        match state {
            ConnectionState::ConnectedAndAccepted | ConnectionState::ReconnectingAfterAccepted => {
                if changed {
                    Self::handle_remote_receiver_status_changed(&connection, max_bitrate)?;
                }
            }
            ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ConnectingAfterAccepted
            | ConnectionState::ConnectedBeforeAccepted => {
                // Ignore before active
            }
            ConnectionState::NotYetStarted
            | ConnectionState::Starting
            | ConnectionState::IceGathering
            | ConnectionState::IceFailed
            | ConnectionState::Terminating
            | ConnectionState::Terminated => {
                self.unexpected_state(state, "ReceivedReceiverStatusViaRtpData");
            }
        };
        Ok(())
    }

    fn handle_received_ice(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        ice: signaling::Ice,
    ) -> Result<()> {
        if state == ConnectionState::NotYetStarted {
            warn!("Connection has not yet started, so ignoring remote ICE candidates...");
            return Ok(());
        }

        if state.can_receive_ice_candidates() {
            connection.handle_received_ice(ice)?;
        } else {
            self.unexpected_state(state, "RemoteIceCandidate");
        }

        Ok(())
    }

    fn handle_accept(&mut self, connection: Connection<T>, state: ConnectionState) -> Result<()> {
        if state.can_be_accepted_locally() {
            self.handle_connected_and_accepted_for_the_first_time(connection)?;
        } else {
            self.unexpected_state(state, "AcceptCall");
        }
        Ok(())
    }

    fn handle_send_hangup_via_rtp_data(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        if state.can_send_hangup_via_rtp() {
            let mut err_connection = connection.clone();
            let hangup_future = lazy(move |_| connection.send_hangup_via_rtp_data(hangup))
                .unwrap_or_else(move |err| {
                    err_connection.inject_internal_error(err, "Sending Hangup failed");
                });

            self.worker_spawn(hangup_future);
        } else {
            self.unexpected_state(state, "SendHangupViaRtpData");
        }
        Ok(())
    }

    fn handle_update_sender_status(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        sender_status: signaling::SenderStatus,
    ) -> Result<()> {
        if state.connected_or_reconnecting() {
            // notify the peer via an RTP data message.
            let mut err_connection = connection.clone();
            let send_sender_status_future = lazy(move |_| {
                if connection.terminating()? {
                    return Ok(());
                }
                connection.update_sender_status_from_fsm(sender_status)
            })
            .unwrap_or_else(move |err| {
                err_connection.inject_internal_error(err, "Sending local sender status failed");
            });

            self.worker_spawn(send_sender_status_future);
        } else {
            self.unexpected_state(state, "UpdateSenderStatus");
        };
        Ok(())
    }

    fn handle_update_bandwidth_mode(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        bandwidth_mode: BandwidthMode,
    ) -> Result<()> {
        if state.connecting_or_connected() {
            let mut err_connection = connection.clone();
            let update_bandwidth_mode_future = lazy(move |_| {
                if connection.terminating()? {
                    return Ok(());
                }

                connection.update_bandwidth_mode(bandwidth_mode)
            })
            .unwrap_or_else(move |err| {
                err_connection.inject_internal_error(err, "Updating bandwidth mode failed");
            });

            self.worker_spawn(update_bandwidth_mode_future);
        };
        Ok(())
    }

    fn handle_local_ice_candidates(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
        candidates: Vec<signaling::IceCandidate>,
    ) -> Result<()> {
        ringbench!(
            RingBench::WebRtc,
            RingBench::Conn,
            format!("ice_candidate()\t{}", connection.id())
        );

        if state.can_send_ice_candidates() {
            // send signal message to the other side with the ICE
            // candidate.
            let mut err_connection = connection.clone();
            let ice_future = lazy(move |_| {
                if connection.terminating()? {
                    return Ok(());
                }
                connection.buffer_local_ice_candidates(candidates)
            })
            .unwrap_or_else(move |err| {
                err_connection.inject_internal_error(err, "IceFuture failed");
            });

            self.worker_spawn(ice_future);
        } else {
            self.unexpected_state(state, "LocalIceCandidate");
        }
        Ok(())
    }

    fn handle_ice_connected(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::NotYetStarted
            | ConnectionState::Starting
            | ConnectionState::IceGathering => {
                // This shouldn't be possible.
                self.unexpected_state(state, "IceConnected");
            }
            ConnectionState::ConnectingBeforeAccepted => {
                connection.set_state(ConnectionState::ConnectedBeforeAccepted)?;
            }
            ConnectionState::ConnectingAfterAccepted => {
                self.handle_connected_and_accepted_for_the_first_time(connection)?;
            }
            ConnectionState::ConnectedBeforeAccepted | ConnectionState::ConnectedAndAccepted => {
                // Already connected, so this shouldn't happen.
                self.unexpected_state(state, "IceConnected");
            }
            ConnectionState::ReconnectingAfterAccepted => {
                // ICE has reconnected after the call was
                // previously accepted (and connected).  Return to that state
                // now.
                connection.set_state(ConnectionState::ConnectedAndAccepted)?;
            }
            ConnectionState::IceFailed
            | ConnectionState::Terminating
            | ConnectionState::Terminated => {
                // This shouldn't be possible.
                self.unexpected_state(state, "IceConnected");
            }
        }
        Ok(())
    }

    fn handle_ice_failed(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        if state.connecting_or_connected() {
            // For callee -- the call was disconnected while answering/local_ringing
            // For caller -- the recipient was unreachable
            connection.set_state(ConnectionState::IceFailed)?;
        } else {
            self.unexpected_state(state, "IceFailed");
        };
        Ok(())
    }

    fn handle_ice_disconnected(
        &mut self,
        connection: Connection<T>,
        state: ConnectionState,
    ) -> Result<()> {
        match state {
            ConnectionState::NotYetStarted
            | ConnectionState::Starting
            | ConnectionState::IceGathering
            | ConnectionState::ConnectingBeforeAccepted
            | ConnectionState::ConnectingAfterAccepted => {
                // This shouldn't be possible.
                self.unexpected_state(state, "IceConnected");
            }
            ConnectionState::ConnectedBeforeAccepted => {
                connection.set_state(ConnectionState::ConnectingBeforeAccepted)?;
            }
            ConnectionState::ConnectedAndAccepted => {
                connection.set_state(ConnectionState::ReconnectingAfterAccepted)?;
            }
            ConnectionState::ReconnectingAfterAccepted
            | ConnectionState::IceFailed
            | ConnectionState::Terminating
            | ConnectionState::Terminated => {
                // This shouldn't be possible.
                self.unexpected_state(state, "IceConnected");
            }
        };
        Ok(())
    }

    fn handle_ice_network_route_changed(
        &mut self,
        connection: Connection<T>,
        network_route: NetworkRoute,
    ) -> Result<()> {
        if network_route.local_adapter_type
            == crate::webrtc::peer_connection_observer::NetworkAdapterType::Vpn
        {
            info!(
                "Local ICE network adapter type changed to {:?} going through a VPN",
                network_route.local_adapter_type_under_vpn
            );
        } else {
            info!(
                "Local ICE network adapter type changed to {:?}",
                network_route.local_adapter_type
            );
        }
        connection.set_network_route(network_route)?;
        self.notify_observer(
            connection,
            ConnectionObserverEvent::IceNetworkRouteChanged(network_route),
        );
        Ok(())
    }

    fn handle_internal_error(
        &mut self,
        connection: Connection<T>,
        error: anyhow::Error,
    ) -> Result<()> {
        let notify_error_future = lazy(move |_| {
            if connection.terminating()? {
                return Ok(());
            }
            connection.internal_error(error)
        })
        .unwrap_or_else(|err| {
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
        if state.connecting_or_connected() {
            let mut err_connection = connection.clone();
            let add_stream_future = lazy(move |_| {
                if connection.terminating()? {
                    return Ok(());
                }
                connection.handle_received_incoming_media(stream)
            })
            .unwrap_or_else(move |err| {
                err_connection.inject_internal_error(err, "Add Media Stream Future failed");
            });

            self.worker_spawn(add_stream_future);
        } else {
            self.unexpected_state(state, "ReceivedIncomingMedia");
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
