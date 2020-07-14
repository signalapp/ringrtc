//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Call Finite State Machine
//!
//! The Call FSM mediates the state machine of the client application
//! with the state machines of the call's connection state machines.
//!
//! # Asynchronous Inputs:
//!
//! ## Control events from client application
//!
//! - StartOutgoingCall
//! - Accept
//! - LocalHangup
//!
//! ## Flow events from client application
//! - Proceed
//! - Drop
//! - Abort
//!
//! ## From connection observer interfaces
//!
//! - Ringing
//! - Connected
//! - RemoteVideoEnabled
//! - RemoteVideoDisabled
//! - RemoteHangup
//! - IceFailed
//! - Timeout
//! - Reconnecting
//!
//! ## Signaling events from client application
//! - ReceivedAnswer
//! - ReceivedIce
//!
//! ## From Internal runtime
//!
//! - CallTimeout
//! - InternalError

use std::fmt;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use futures::future::lazy;
use futures::{Async, Future, Poll, Stream};
use tokio::runtime;

use crate::error::RingRtcError;

use crate::common::{
    ApplicationEvent,
    BandwidthMode,
    CallDirection,
    CallId,
    CallState,
    DeviceId,
    FeatureLevel,
    Result,
};

use crate::core::call::{Call, EventStream};
use crate::core::connection::ConnectionObserverEvent;
use crate::core::platform::Platform;
use crate::core::signaling;

/// The different types of CallEvents.
pub enum CallEvent {
    // Control events from client application
    /// Start a call (call struct has the direction attribute).
    StartCall,
    /// Accept incoming call (callee only).
    AcceptCall,
    /// Send Hangup
    SendHangupViaDataChannelToAll(signaling::Hangup),

    // Flow events from client application
    /// OK to proceed with call setup including user options.
    Proceed,

    // Signaling events from client application
    /// Received answer from remote peer (caller only).
    ReceivedAnswer(signaling::ReceivedAnswer),
    /// Received ICE signaling from remote device.
    ReceivedIce(signaling::ReceivedIce),
    /// Received hangup signal message from remote peer.
    ReceivedHangup(signaling::ReceivedHangup),

    /// Connection observer event
    ConnectionObserverEvent(ConnectionObserverEvent, DeviceId),
    /// Connection observer error
    ConnectionObserverError(failure::Error, DeviceId),

    // Internally generated events
    /// Notify the call manager of an internal error condition.
    InternalError(failure::Error),
    /// The call timed out while establishing a connection.
    CallTimeout,
    /// Synchronize the FSM.
    Synchronize(Arc<(Mutex<bool>, Condvar)>),
    /// Terminate the call.
    Terminate,
}

impl fmt::Display for CallEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            CallEvent::StartCall => "StartCall".to_string(),
            CallEvent::AcceptCall => "AcceptCall".to_string(),
            CallEvent::SendHangupViaDataChannelToAll(hangup) => {
                format!("SendHangupViaDataChannelToAll, hangup: {}", hangup)
            }
            CallEvent::Proceed => "Proceed".to_string(),
            CallEvent::ReceivedAnswer(received) => format!(
                "ReceivedAnswer, device: {} feature_level: {}",
                received.sender_device_id, received.sender_device_feature_level
            ),
            CallEvent::ReceivedIce(received) => {
                format!("ReceivedIce, device: {}", received.sender_device_id)
            }
            CallEvent::ReceivedHangup(received) => format!(
                "ReceivedHangup, device: {} hangup: {}",
                received.sender_device_id, received.hangup
            ),
            CallEvent::ConnectionObserverEvent(e, d) => {
                format!("ConnectionObserverEvent, event: {}, device: {}", e, d)
            }
            CallEvent::ConnectionObserverError(e, d) => {
                format!("ConnectionObserverError, error: {}, device: {}", e, d)
            }
            CallEvent::InternalError(e) => format!("InternalError: {}", e),
            CallEvent::CallTimeout => "CallTimeout".to_string(),
            CallEvent::Synchronize(_) => "Synchronize".to_string(),
            CallEvent::Terminate => "Terminate".to_string(),
        };
        write!(f, "({})", display)
    }
}

impl fmt::Debug for CallEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

/// CallStateMachine Object.
///
/// The CallStateMachine object consumes incoming CallEvents and
/// either handles them immediately or dispatches them to other
/// runtimes for further processing.
///
/// The FSM itself is executing on a runtime managed by a Call object.
///
/// For "quick" reactions to incoming events, the FSM handles them
/// immediately on its own thread.
///
/// For "lengthy" reactions, typically involving network access, the
/// FSM dispatches the work to a "worker" thread.
///
/// For notification events targeted for the client application, the
/// FSM dispatches the work to a "notify" thread.
#[derive(Debug)]
pub struct CallStateMachine<T>
where
    T: Platform,
{
    /// Receiving end of EventPump.
    event_stream:   EventStream<T>,
    /// Runtime for processing long running requests.
    worker_runtime: Option<runtime::Runtime>,
    /// Runtime for processing client application notification events.
    notify_runtime: Option<runtime::Runtime>,
}

impl<T> fmt::Display for CallStateMachine<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(tid: {:?})", thread::current().id())
    }
}

impl<T> Drop for CallStateMachine<T>
where
    T: Platform,
{
    fn drop(&mut self) {
        info!("Dropping CallStateMachine:");
    }
}

impl<T> Future for CallStateMachine<T>
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
                Some((call, event)) => {
                    let state = call.state()?;
                    info!("state: {}, event: {}", state, event);
                    if let Err(e) = self.handle_event(call, state, event) {
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

impl<T> CallStateMachine<T>
where
    T: Platform,
{
    /// Creates a new CallStateMachine object.
    pub fn new(event_stream: EventStream<T>) -> Result<CallStateMachine<T>> {
        let mut fsm = CallStateMachine {
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
            CallStateMachine::<T>::sync_thread("worker", worker_runtime)?;
        }
        if let Some(notify_runtime) = &mut fsm.notify_runtime {
            CallStateMachine::<T>::sync_thread("notify", notify_runtime)?;
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
    fn handle_event(&mut self, call: Call<T>, state: CallState, event: CallEvent) -> Result<()> {
        // Handle these events even while terminating, as the remote
        // side needs to be informed.
        match event {
            CallEvent::SendHangupViaDataChannelToAll(hangup) => {
                return self.handle_send_hangup_via_data_channel_to_all(call, state, hangup)
            }
            CallEvent::Terminate => return self.handle_terminate(call),
            CallEvent::Synchronize(sync) => return self.handle_synchronize(sync),
            _ => {}
        }

        // If in the process of terminating the call, drop all other
        // events.
        match state {
            CallState::Terminating | CallState::Terminated => {
                debug!("handle_event(): dropping event {} while terminating", event);
                return Ok(());
            }
            _ => (),
        }

        match event {
            CallEvent::StartCall => self.handle_start_call(call, state),
            CallEvent::Proceed => self.handle_proceed(call, state),
            CallEvent::AcceptCall => self.handle_accept_call(call, state),
            CallEvent::ReceivedAnswer(received) => {
                self.handle_received_answer(call, state, received)
            }
            CallEvent::ReceivedIce(received) => self.handle_received_ice(call, state, received),
            CallEvent::ReceivedHangup(received) => {
                self.handle_received_hangup(call, state, received)
            }
            CallEvent::ConnectionObserverEvent(event, remote_device_id) => {
                self.handle_connection_observer_event(call, state, event, remote_device_id)
            }
            CallEvent::ConnectionObserverError(error, remote_device) => {
                self.handle_connection_observer_error(call, error, remote_device)
            }
            CallEvent::InternalError(error) => self.handle_internal_error(call, error),
            CallEvent::CallTimeout => self.handle_call_timeout(call, state),
            // Handled above
            CallEvent::SendHangupViaDataChannelToAll(_) => Ok(()),
            CallEvent::Synchronize(_) => Ok(()),
            CallEvent::Terminate => Ok(()),
        }
    }

    fn notify_application(&mut self, call: Call<T>, event: ApplicationEvent) {
        let mut err_call = call.clone();
        let notify_app_future = lazy(move || {
            if call.terminating()? {
                return Ok(());
            }
            call.notify_application(event)
        })
        .map_err(move |err| {
            err_call.inject_internal_error(err, "Notify Application Future failed")
        });

        self.notify_spawn(notify_app_future);
    }

    fn handle_start_call(&mut self, call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_start_call():");

        if let CallState::NotYetStarted = state {
            call.set_state(CallState::WaitingToProceed)?;
            call.handle_start_call()
        } else {
            self.unexpected_state(state, "StartCall");

            Ok(())
        }
    }

    fn handle_proceed(&mut self, mut call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_proceed():");

        if let CallState::WaitingToProceed = state {
            call.set_state(CallState::ConnectingBeforeAccepted)?;

            let mut err_call = call.clone();
            let proceed_future = lazy(move || {
                if call.terminating()? {
                    return Ok(());
                }
                call.proceed()
            })
            .map_err(move |err| err_call.inject_internal_error(err, "Proceed Future failed"));

            self.worker_spawn(proceed_future);
        } else {
            self.unexpected_state(state, "Proceed");
        }

        Ok(())
    }

    fn handle_received_answer(
        &mut self,
        call: Call<T>,
        state: CallState,
        received: signaling::ReceivedAnswer,
    ) -> Result<()> {
        // Accept answers when we are ringing so we can get answers for more than one connection.
        if state == CallState::ConnectingBeforeAccepted
            || state == CallState::ConnectedWithDataChannelBeforeAccepted
        {
            let mut err_call = call.clone();
            let received_answer_future = lazy(move || {
                if call.terminating()? {
                    return Ok(());
                }
                call.received_answer(received)
            })
            .map_err(move |err| {
                err_call.inject_internal_error(err, "Handle Received Answer Future failed")
            });

            self.worker_spawn(received_answer_future);
        } else {
            self.unexpected_state(state, "HandleReceivedAnswer");
        }
        Ok(())
    }

    fn handle_received_ice(
        &mut self,
        call: Call<T>,
        state: CallState,
        received: signaling::ReceivedIce,
    ) -> Result<()> {
        match state {
            CallState::WaitingToProceed
            | CallState::ConnectingBeforeAccepted
            | CallState::ConnectedWithDataChannelBeforeAccepted
            | CallState::ConnectedAndAccepted
            | CallState::ReconnectingAfterAccepted => {
                let mut err_call = call.clone();
                let handle_received_ice_future = lazy(move || {
                    if call.terminating()? {
                        return Ok(());
                    }
                    call.received_ice(received)
                })
                .map_err(move |err| {
                    err_call.inject_internal_error(err, "Handle Received Ice Future failed")
                });

                self.worker_spawn(handle_received_ice_future);
            }
            _ => self.unexpected_state(state, "HandleReceivedIceCandidates"),
        }
        Ok(())
    }

    fn handle_received_hangup(
        &mut self,
        call: Call<T>,
        state: CallState,
        received: signaling::ReceivedHangup,
    ) -> Result<()> {
        info!(
            "handle_received_hangup(): remote_device_id: {}, hangup: {}",
            received.sender_device_id, received.hangup
        );

        let direction = call.direction();
        let sender_device_id = received.sender_device_id;
        let (hangup_type, hangup_device_id) = received.hangup.to_type_and_device_id();

        // If the callee that originated the hangup, ignore messages that are propagated
        // back to us from the caller.
        if direction == CallDirection::InComing && Some(call.local_device_id()) == hangup_device_id
        {
            info!("handle_received_hangup(): Ignoring hangup message originated by this device");
            return Ok(());
        }

        // If already connected to device A, ignore hangup messages from device B.
        if let Ok(active_device_id) = call.active_device_id() {
            if sender_device_id != active_device_id {
                info!("handle_received_hangup(): Ignoring hangup message from devices we aren't connected with");
                return Ok(());
            }
        }

        // Setup helper tuples for common scenarios to handle.
        let no_app_event_and_no_propagation = (true, None, None);
        let app_event_without_propagation = |event| (true, None, Some(event));
        let propagate_without_app_event = |hangup_to_send| (true, Some(hangup_to_send), None);
        let propagate_with_app_event =
            |hangup_to_send, event| (true, Some(hangup_to_send), Some(event));
        let unexpected = (false, None, None);

        // Find out how we will handle the current hangup scenario.
        // - expected: true if an expected scenario
        // - hangup_to_propagate: If a caller, the hangup to send to other callees
        // - app_event_override: The event, if any, to return to the UX to override the default
        let (expected, hangup_to_propagate, app_event_override) = match (hangup_type, direction) {
            // Caller gets NeedsPermission: propagate it with specific app event.
            (signaling::HangupType::NeedPermission, CallDirection::OutGoing) => {
                propagate_with_app_event(
                    signaling::Hangup::NeedPermission(Some(sender_device_id)),
                    ApplicationEvent::EndedRemoteHangupNeedPermission,
                )
            }

            // Callee gets Normal: no propagation.
            (signaling::HangupType::Normal, CallDirection::InComing) => {
                no_app_event_and_no_propagation
            }

            // Caller gets Normal hangup: propagate it as Declined.
            (signaling::HangupType::Normal, CallDirection::OutGoing) => {
                propagate_without_app_event(signaling::Hangup::DeclinedOnAnotherDevice(
                    sender_device_id,
                ))
            }

            // Callee gets propagated hangup: use specific app event.
            (signaling::HangupType::AcceptedOnAnotherDevice, CallDirection::InComing) => {
                app_event_without_propagation(ApplicationEvent::EndedRemoteHangupAccepted)
            }
            (signaling::HangupType::DeclinedOnAnotherDevice, CallDirection::InComing) => {
                app_event_without_propagation(ApplicationEvent::EndedRemoteHangupDeclined)
            }
            (signaling::HangupType::BusyOnAnotherDevice, CallDirection::InComing) => {
                app_event_without_propagation(ApplicationEvent::EndedRemoteHangupBusy)
            }

            // Everything else is unexpected: warn, and mostly treat like normal, no propagation.
            // TODO: Isn't NeedPermission for incoming normal because it's propagated above?
            // Should we make this no_app_event_and_no_propagation?
            (signaling::HangupType::NeedPermission, CallDirection::InComing) => unexpected,
            (signaling::HangupType::AcceptedOnAnotherDevice, CallDirection::OutGoing) => unexpected,
            (signaling::HangupType::DeclinedOnAnotherDevice, CallDirection::OutGoing) => unexpected,
            (signaling::HangupType::BusyOnAnotherDevice, CallDirection::OutGoing) => unexpected,
        };

        if !expected {
            warn!(
                "handle_received_hangup(): Unexpected hangup type: {:?}",
                hangup_type,
            );
        }

        // Set the state to terminating here if not Idle | Terminating | Closed.
        if let CallState::WaitingToProceed
        | CallState::ConnectingBeforeAccepted
        | CallState::ConnectedWithDataChannelBeforeAccepted
        | CallState::ConnectedAndAccepted
        | CallState::ReconnectingAfterAccepted = state
        {
            call.set_state(CallState::Terminating)?;
        }

        // Only callers can propagate hangups to other callee devices.
        if let Some(hangup_to_propagate) = hangup_to_propagate {
            // Don't propagate if we're already accepted because a
            // Hangup/Accepted has been sent to the other callees.
            // Don't propagate if we're already terminating/terminated because
            // we already sent out a Hangup to the other callees.
            // Not for NotYetStarted | ConnectedAndAccepted | ReconnectingAfterAccepted | Terminating | Terminated states:
            if let CallState::WaitingToProceed
            | CallState::ConnectingBeforeAccepted
            | CallState::ConnectedWithDataChannelBeforeAccepted = state
            {
                let (_hangup_type, hangup_device_id) = hangup_to_propagate.to_type_and_device_id();
                let excluded_remote_device_id = hangup_device_id.unwrap_or(0 as DeviceId);
                call.send_hangup_via_data_channel_and_signaling_to_all_except(
                    hangup_to_propagate,
                    excluded_remote_device_id,
                )?;
            }
        }

        // Send a Hangup event to the UX, if a call is being remotely hungup, the user
        // should always know.
        let mut err_call = call.clone();
        self.worker_spawn(
            lazy(move || {
                call.call_manager()?
                    .remote_hangup(call.call_id(), app_event_override)
            })
            .map_err(move |err| {
                err_call.inject_internal_error(err, "Processing remote hangup event failed")
            }),
        );
        Ok(())
    }

    fn handle_accept_call(&mut self, call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_accept_call():");
        match state {
            CallState::ConnectedWithDataChannelBeforeAccepted => {
                call.set_state(CallState::ConnectedAndAccepted)?;
                let mut err_call = call.clone();
                let accept_future = lazy(move || {
                    if call.terminating()? {
                        return Ok(());
                    }
                    let mut connection = call.active_connection()?;
                    connection.inject_accept()?;
                    connection.connect_incoming_media()?;
                    connection.set_local_max_send_bitrate(BandwidthMode::Normal.max_bitrate())?;
                    connection.start_tick()?;
                    call.notify_application(ApplicationEvent::LocalAccepted)
                })
                .map_err(move |err| {
                    err_call.inject_internal_error(err, "Processing local accept request failed")
                });

                self.worker_spawn(accept_future);
            }
            _ => self.unexpected_state(state, "AcceptCall"),
        }
        Ok(())
    }

    fn handle_send_hangup_via_data_channel_to_all(
        &mut self,
        call: Call<T>,
        state: CallState,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        info!("handle_send_hangup_via_data_channel_to_all():");
        match state {
            CallState::NotYetStarted => self.unexpected_state(state, "LocalHangup"),
            _ => {
                let mut err_call = call.clone();
                let future = lazy(move || call.send_hangup_via_data_channel_to_all(hangup))
                    .map_err(move |err| {
                        err_call.inject_internal_error(err, "Send hangup request failed")
                    });

                self.worker_spawn(future);
            }
        }
        Ok(())
    }

    fn ignore_connection_observer_event(
        &self,
        call_id: CallId,
        remote_device_id: DeviceId,
        state: CallState,
        event: ConnectionObserverEvent,
    ) {
        info!(
            "call_id: {} remote_device_id: {} Ignoring event: {}, while in state: {}",
            call_id, remote_device_id, event, state
        );
    }

    fn handle_connection_observer_event(
        &mut self,
        mut call: Call<T>,
        state: CallState,
        event: ConnectionObserverEvent,
        remote_device_id: DeviceId,
    ) -> Result<()> {
        let call_id = call.call_id();

        match event {
            ConnectionObserverEvent::ConnectedWithDataChannelBeforeAccepted => {
                match state {
                    CallState::ConnectingBeforeAccepted => {
                        // We use the fact that we are connected with a data channel
                        // as a signal that the application should ring.
                        call.set_state(CallState::ConnectedWithDataChannelBeforeAccepted)?;
                        if let CallDirection::InComing = call.direction() {
                            self.notify_application(call, ApplicationEvent::LocalRinging)
                        } else {
                            self.notify_application(call, ApplicationEvent::RemoteRinging)
                        }
                    }
                    _ => {
                        self.ignore_connection_observer_event(
                            call_id,
                            remote_device_id,
                            state,
                            event,
                        );
                    }
                }
                Ok(())
            }
            ConnectionObserverEvent::ReceivedAcceptedViaDataChannel => {
                match call.direction() {
                    CallDirection::OutGoing => match state {
                        CallState::ConnectedWithDataChannelBeforeAccepted => {
                            info!(
                                "handle_connection_observer_event(): Accepted from {}",
                                remote_device_id
                            );

                            call.set_state(CallState::ConnectedAndAccepted)?;
                            call.set_active_device_id(remote_device_id)?;

                            // Send out hangup/accepted to all via the data channel except to the accepter.
                            let hangup =
                                signaling::Hangup::AcceptedOnAnotherDevice(remote_device_id);
                            call.send_hangup_via_data_channel_to_all_except(
                                hangup,
                                remote_device_id,
                            )?;

                            let mut err_call = call.clone();
                            let connected_future = lazy(move || {
                                if call.terminating()? {
                                    return Ok(());
                                }

                                // Get the media and application working for the first connection.
                                let connection = call.active_connection()?;
                                connection.connect_incoming_media()?;
                                connection.set_local_max_send_bitrate(
                                    BandwidthMode::Normal.max_bitrate(),
                                )?;
                                connection.start_tick()?;
                                call.notify_application(ApplicationEvent::RemoteAccepted)?;

                                // If the remote device of the active connection can support
                                // multi-ring, we send a "legacy" Hangup message. The callee
                                // that accepted the call will ignore it and all other callees,
                                // legacy or otherwise, will handle it and end.
                                //
                                // If the remote device is not multi-ring capable, then we send
                                // a new "non-legacy" Hangup message because the callee that
                                // accepted the call will ignore it as it is not defined in their
                                // protocol definition.
                                let use_legacy = match connection.remote_feature_level()? {
                                    FeatureLevel::Unspecified => false,
                                    FeatureLevel::MultiRing => true,
                                };

                                // Send the accepted indication via hangup signaling (it will be
                                // replicated to all remote peers).
                                let mut call_manager = call.call_manager()?;
                                call_manager.send_hangup(
                                    call.clone(),
                                    call.call_id(),
                                    signaling::SendHangup { hangup, use_legacy },
                                )?;

                                // Close all the other connections (this blocks).
                                let mut call_clone = call.clone();
                                call_clone.terminate_connections_except_accepted(remote_device_id)
                            })
                            .map_err(move |err| {
                                err_call.inject_internal_error(
                                    err,
                                    "Processing connect_incoming_media request failed",
                                )
                            });
                            self.worker_spawn(connected_future);
                        }
                        _ => {
                            self.ignore_connection_observer_event(
                                call_id,
                                remote_device_id,
                                state,
                                event,
                            );
                        }
                    },
                    CallDirection::InComing => {
                        warn!(
                            "Ignoring ReceivedAcceptedViaDataChannel for incoming call: {}",
                            call_id
                        );
                    }
                }
                Ok(())
            }
            ConnectionObserverEvent::ReceivedHangup(hangup) => self.handle_received_hangup(
                call,
                state,
                signaling::ReceivedHangup {
                    sender_device_id: remote_device_id,
                    hangup,
                },
            ),
            ConnectionObserverEvent::ReceivedSenderStatusViaDataChannel(enabled) => {
                if call.active_device_id()? == remote_device_id {
                    match state {
                        CallState::ConnectedAndAccepted => {
                            if enabled {
                                self.notify_application(call, ApplicationEvent::RemoteVideoEnable)
                            } else {
                                self.notify_application(call, ApplicationEvent::RemoteVideoDisable)
                            }
                        }
                        _ => {
                            self.ignore_connection_observer_event(
                                call_id,
                                remote_device_id,
                                state,
                                event,
                            );
                        }
                    }
                } else {
                    info!(
                        "call_id: {} remote_device_id: {} Ignoring event: {}, from inactive connection.",
                        call_id, remote_device_id, event
                    );
                }
                Ok(())
            }
            ConnectionObserverEvent::ReconnectingAfterAccepted => {
                if call.active_device_id()? == remote_device_id {
                    match state {
                        CallState::ConnectedAndAccepted => {
                            call.set_state(CallState::ReconnectingAfterAccepted)?;
                            self.notify_application(call, ApplicationEvent::Reconnecting)
                        }
                        _ => {
                            self.ignore_connection_observer_event(
                                call_id,
                                remote_device_id,
                                state,
                                event,
                            );
                        }
                    }
                } else {
                    info!(
                        "call_id: {} remote_device_id: {} Ignoring event: {}, from inactive connection.",
                        call_id, remote_device_id, event
                    );
                }
                Ok(())
            }
            ConnectionObserverEvent::ReconnectedAfterAccepted => {
                if call.active_device_id()? == remote_device_id {
                    match state {
                        CallState::ReconnectingAfterAccepted => {
                            call.set_state(CallState::ConnectedAndAccepted)?;
                            self.notify_application(call, ApplicationEvent::Reconnected)
                        }
                        _ => {
                            self.ignore_connection_observer_event(
                                call_id,
                                remote_device_id,
                                state,
                                event,
                            );
                        }
                    }
                } else {
                    info!(
                        "call_id: {} remote_device_id: {} Ignoring event: {}, from inactive connection.",
                        call_id, remote_device_id, event
                    );
                }
                Ok(())
            }
            ConnectionObserverEvent::IceFailed => {
                let mut err_call = call.clone();
                let future = lazy(move || {
                    if call.terminating()? {
                        return Ok(());
                    }
                    call.handle_ice_failed(remote_device_id)
                })
                .map_err(move |err| {
                    err_call
                        .inject_internal_error(err, "Processing connection_failed request failed")
                });
                self.worker_spawn(future);
                Ok(())
            }
        }
    }

    fn handle_internal_error(&mut self, call: Call<T>, error: failure::Error) -> Result<()> {
        info!("handle_internal_error():");

        let internal_error_future =
            lazy(move || call.internal_error(error)).map_err(move |err: failure::Error| {
                error!("Processing internal error future failed: {}", err);
                // Nothing else to do here
            });

        self.worker_spawn(internal_error_future);
        Ok(())
    }

    fn handle_connection_observer_error(
        &mut self,
        call: Call<T>,
        error: failure::Error,
        remote_device_id: DeviceId,
    ) -> Result<()> {
        info!(
            "handle_connection_observer_error(): call_id: {} remote_device_id: {}",
            call.call_id(),
            remote_device_id
        );

        // Treat a connection internal error as a call internal error,
        // i.e. ignore the remote_device ID.
        self.handle_internal_error(call, error)
    }

    fn handle_call_timeout(&mut self, call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_call_timeout():");

        match state {
            CallState::ConnectedAndAccepted | CallState::ReconnectingAfterAccepted => {} // Ok
            _ => {
                let mut err_call = call.clone();
                let timeout_future = lazy(move || {
                    let mut call_manager = call.call_manager()?;
                    call_manager.timeout(call.call_id())
                })
                .map_err(move |err| {
                    err_call.inject_internal_error(err, "Processing call timeout failed")
                });

                self.worker_spawn(timeout_future);
            }
        }
        Ok(())
    }

    fn handle_synchronize(&mut self, sync: Arc<(Mutex<bool>, Condvar)>) -> Result<()> {
        if let Some(worker_runtime) = &mut self.worker_runtime {
            CallStateMachine::<T>::sync_thread("worker", worker_runtime)?;
        }
        if let Some(notify_runtime) = &mut self.notify_runtime {
            CallStateMachine::<T>::sync_thread("notify", notify_runtime)?;
        }

        let &(ref mutex, ref condvar) = &*sync;
        if let Ok(mut sync_complete) = mutex.lock() {
            *sync_complete = true;
            condvar.notify_one();
            Ok(())
        } else {
            Err(RingRtcError::MutexPoisoned(
                "CallConnection Synchronize Condition Variable".to_string(),
            )
            .into())
        }
    }

    fn handle_terminate(&mut self, mut call: Call<T>) -> Result<()> {
        self.event_stream.close();
        self.drain_worker_thread();
        self.drain_notify_thread();

        call.set_state(CallState::Terminated)?;
        call.terminate_complete()
    }

    fn unexpected_state(&self, state: CallState, event: &str) {
        warn!("Unexpected event {}, while in state {:?}", event, state);
    }
}
