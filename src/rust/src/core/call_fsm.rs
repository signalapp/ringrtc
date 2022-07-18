//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
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
//! - RemoteSharingScreenEnabled
//! - RemoteSharingScreenDisabled
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
use std::pin::Pin;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use futures::future::lazy;
use futures::future::TryFutureExt;
use futures::task::Poll;
use futures::{Future, Stream};

use crate::error::RingRtcError;

use crate::common::{
    ApplicationEvent, CallDirection, CallState, ConnectionState, DeviceId, Result,
};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call::{Call, EventStream};
use crate::core::connection::ConnectionObserverEvent;
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::core::util::TaskQueueRuntime;
use crate::webrtc::peer_connection::AudioLevel;
use crate::webrtc::peer_connection_observer::NetworkRoute;

/// The different types of CallEvents.
#[allow(clippy::large_enum_variant)]
pub enum CallEvent {
    // Control events from client application
    /// Start a call (call struct has the direction attribute).
    StartCall,
    /// Accept incoming call (callee only).
    AcceptCall,
    /// Send Hangup
    SendHangupViaRtpDataToAll(signaling::Hangup),

    // Flow events from client application
    /// OK to proceed with call setup including user options.
    Proceed {
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    },

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
    ConnectionObserverError(anyhow::Error, DeviceId),
    // Internally generated events
    /// Notify the call manager of an internal error condition.
    InternalError(anyhow::Error),
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
            CallEvent::SendHangupViaRtpDataToAll(hangup) => {
                format!("SendHangupViaRtpDataToAll, hangup: {}", hangup)
            }
            CallEvent::Proceed {
                bandwidth_mode,
                audio_levels_interval,
            } => {
                format!(
                    "Proceed, bandwidth_mode: {}, audio_levels_interval: {:?}",
                    bandwidth_mode, audio_levels_interval
                )
            }
            CallEvent::ReceivedAnswer(received) => {
                format!("ReceivedAnswer, device: {}", received.sender_device_id)
            }
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

impl CallEvent {
    // If an event is frequent, avoid logging it.
    pub fn is_frequent(&self) -> bool {
        if let CallEvent::ConnectionObserverEvent(event, _) = self {
            event.is_frequent()
        } else {
            false
        }
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
    event_stream: EventStream<T>,
    /// Runtime for processing long running requests.
    worker_runtime: Option<TaskQueueRuntime>,
    /// Runtime for processing client application notification events.
    notify_runtime: Option<TaskQueueRuntime>,
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
        debug!("Dropping CallStateMachine:");
    }
}

impl<T> Future for CallStateMachine<T>
where
    T: Platform,
{
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut futures::task::Context) -> Poll<Self::Output> {
        loop {
            let pin_stream = Pin::new(&mut self.event_stream);
            match ready!(pin_stream.poll_next(cx)) {
                Some((call, event)) => {
                    let state = call.state()?;
                    if !event.is_frequent() {
                        info!("state: {}, event: {}", state, event);
                    }
                    if let Err(e) = self.handle_event(call, state, event) {
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

impl<T> CallStateMachine<T>
where
    T: Platform,
{
    /// Creates a new CallStateMachine object.
    pub fn new(event_stream: EventStream<T>) -> Result<CallStateMachine<T>> {
        let mut fsm = CallStateMachine {
            event_stream,
            worker_runtime: Some(TaskQueueRuntime::new("call-worker")?),
            notify_runtime: Some(TaskQueueRuntime::new("call-notify")?),
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

    fn schedule_work_until_terminating(
        &mut self,
        call: Call<T>,
        error_msg: &'static str,
        do_work: impl (FnOnce(Call<T>) -> Result<()>) + Send + 'static,
    ) {
        let mut err_call = call.clone();
        self.worker_spawn(
            lazy(move |_| {
                if call.terminating()? {
                    Ok(())
                } else {
                    do_work(call)
                }
            })
            .unwrap_or_else(move |err| {
                err_call.inject_internal_error(err, error_msg);
            }),
        );
    }

    fn schedule_work_even_when_terminating(
        &mut self,
        call: Call<T>,
        error_msg: &'static str,
        do_work: impl (FnOnce(Call<T>) -> Result<()>) + Send + 'static,
    ) {
        let mut err_call = call.clone();
        self.worker_spawn(lazy(move |_| do_work(call)).unwrap_or_else(move |err| {
            err_call.inject_internal_error(err, error_msg);
        }));
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

    // For remotely-accepted (outgoing) calls, this is the first step: let other connections know to stop ringing,
    // and drop the useless state.
    fn handle_remotely_accepted_connection(
        &mut self,
        call: Call<T>,
        accepted_remote_device_id: DeviceId,
    ) -> Result<()> {
        call.set_active_device_id(accepted_remote_device_id)?;

        let hangup = signaling::Hangup::AcceptedOnAnotherDevice(accepted_remote_device_id);
        call.send_hangup_via_rtp_data_to_all_except(hangup, accepted_remote_device_id)?;

        self.schedule_work_until_terminating(
            call,
            "terminate_and_send_hangups_to_all_except_accepted failed",
            move |mut call| {
                call.send_hangup_via_signaling_to_all(hangup)?;
                // This blocks.
                call.terminate_connections_except_accepted(accepted_remote_device_id)?;
                Ok(())
            },
        );
        Ok(())
    }

    // For remotely-accepted (outgoing) calls, this is the second step: enable media and notify the application.
    fn activate_remotely_accepted_connection(&mut self, call: Call<T>) {
        self.schedule_work_until_terminating(
            call,
            "active_accepted_connection failed",
            move |call| {
                call.accept_remotely()?;
                Ok(())
            },
        );
    }

    /// Top level event dispatch.
    fn handle_event(&mut self, call: Call<T>, state: CallState, event: CallEvent) -> Result<()> {
        // Handle these events even while terminating, as the remote
        // side needs to be informed.
        match event {
            CallEvent::SendHangupViaRtpDataToAll(hangup) => {
                return self.handle_send_hangup_via_rtp_data_to_all(call, state, hangup)
            }
            CallEvent::Terminate => return self.handle_terminate(call),
            CallEvent::Synchronize(sync) => return self.handle_synchronize(sync),
            _ => {}
        }

        // If in the process of terminating the call, drop all other
        // events.
        if state.terminating_or_terminated() {
            debug!("handle_event(): dropping event {} while terminating", event);
            return Ok(());
        }
        match event {
            CallEvent::StartCall => self.handle_start_call(call, state),
            CallEvent::Proceed {
                bandwidth_mode,
                audio_levels_interval,
            } => self.handle_proceed(call, state, bandwidth_mode, audio_levels_interval),
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
            CallEvent::SendHangupViaRtpDataToAll(_) => Ok(()),
            CallEvent::Synchronize(_) => Ok(()),
            CallEvent::Terminate => Ok(()),
        }
    }

    fn notify_application(&mut self, call: Call<T>, event: ApplicationEvent) {
        let mut err_call = call.clone();
        let notify_app_future = async move {
            if call.terminating()? {
                return Ok(());
            }
            call.notify_application(event)
        }
        .unwrap_or_else(move |err| {
            err_call.inject_internal_error(err, "Notify Application Future failed");
        });

        self.notify_spawn(notify_app_future);
    }

    fn notify_network_route_changed(&mut self, call: Call<T>, network_route: NetworkRoute) {
        let mut err_call = call.clone();
        let notify_app_future = async move {
            if call.terminating()? {
                return Ok(());
            }
            call.notify_network_route_changed(network_route)
        }
        .unwrap_or_else(move |err| {
            err_call.inject_internal_error(err, "Notify Network Route Changed Future failed");
        });

        self.notify_spawn(notify_app_future);
    }

    fn notify_audio_levels(
        &mut self,
        call: Call<T>,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) {
        let mut err_call = call.clone();
        let notify_app_future = async move {
            if call.terminating()? {
                return Ok(());
            }
            call.notify_audio_levels(captured_level, received_level)
        }
        .unwrap_or_else(move |err| {
            err_call.inject_internal_error(err, "Notify Audio Levels Future failed");
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

    fn handle_proceed(
        &mut self,
        call: Call<T>,
        state: CallState,
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    ) -> Result<()> {
        info!("handle_proceed():");

        if state == CallState::WaitingToProceed {
            call.set_state(CallState::ConnectingBeforeAccepted)?;
            self.schedule_work_until_terminating(call, "Proceed failed", move |mut call| {
                call.proceed(bandwidth_mode, audio_levels_interval)
            });
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
        if matches!(
            state,
            CallState::ConnectingBeforeAccepted | CallState::ConnectedBeforeAccepted
        ) {
            self.schedule_work_until_terminating(
                call,
                "Handle Received Answer failed",
                move |call| call.received_answer(received),
            )
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
        if state.can_receive_ice_candidates() {
            self.schedule_work_until_terminating(call, "Handle Received Ice failed", move |call| {
                call.received_ice(received)
            });
        } else {
            self.unexpected_state(state, "HandleReceivedIceCandidates");
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

        if state.can_be_terminated_remotely() {
            call.set_state(CallState::Terminating)?;
        }

        // Only callers can propagate hangups to other callee devices.
        if let Some(hangup_to_propagate) = hangup_to_propagate {
            if state.should_propogate_hangup() {
                let (_hangup_type, hangup_device_id) = hangup_to_propagate.to_type_and_device_id();
                let excluded_remote_device_id = hangup_device_id.unwrap_or(0);
                call.send_hangup_via_rtp_data_and_signaling_to_all_except(
                    hangup_to_propagate,
                    excluded_remote_device_id,
                )?;
            }
        }

        // Send a Hangup event to the UX, if a call is being remotely hungup, the user
        // should always know.
        self.schedule_work_even_when_terminating(
            call,
            "Processing remote hangup event failed",
            move |call| {
                call.call_manager()?
                    .remote_hangup(call.call_id(), app_event_override)
            },
        );
        Ok(())
    }

    fn handle_accept_call(&mut self, call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_accept_call():");
        if state.can_be_accepted_locally() {
            call.set_state(CallState::ConnectedAndAccepted)?;
            self.schedule_work_until_terminating(
                call,
                "Processing local accept request failed",
                move |call| call.accept_locally(),
            );
        } else {
            self.unexpected_state(state, "AcceptCall");
        }
        Ok(())
    }

    fn handle_send_hangup_via_rtp_data_to_all(
        &mut self,
        call: Call<T>,
        state: CallState,
        hangup: signaling::Hangup,
    ) -> Result<()> {
        info!("handle_send_hangup_via_rtp_data_to_all():");
        if state.can_send_hangup_via_rtp() {
            self.schedule_work_even_when_terminating(
                call,
                "Send hangup request failed",
                move |call| call.send_hangup_via_rtp_data_to_all(hangup),
            );
        } else {
            self.unexpected_state(state, "LocalHangup")
        }
        Ok(())
    }

    fn handle_connection_observer_event(
        &mut self,
        call: Call<T>,
        state: CallState,
        event: ConnectionObserverEvent,
        remote_device_id: DeviceId,
    ) -> Result<()> {
        let call_id = call.call_id();
        let direction = call.direction();

        match event {
            ConnectionObserverEvent::StateChanged(connection_state) => {
                match (direction, state, connection_state) {
                    (
                        CallDirection::InComing,
                        CallState::ConnectingBeforeAccepted,
                        ConnectionState::ConnectedBeforeAccepted,
                    ) => {
                        // We use the fact that we are connected via ICE
                        // as a signal that the application should ring.
                        call.set_state(CallState::ConnectedBeforeAccepted)?;
                        self.notify_application(call, ApplicationEvent::LocalRinging)
                    }
                    (
                        CallDirection::OutGoing,
                        CallState::ConnectingBeforeAccepted,
                        ConnectionState::ConnectedBeforeAccepted,
                    ) => {
                        call.set_state(CallState::ConnectedBeforeAccepted)?;
                        self.notify_application(call, ApplicationEvent::RemoteRinging)
                    }
                    | (
                        CallDirection::OutGoing,
                        // Note: It's possible to have one connection in ConnectedBeforeAccepted and another in ConnectingAfterAccepted.
                        CallState::ConnectingBeforeAccepted | CallState::ConnectedBeforeAccepted,
                        ConnectionState::ConnectingAfterAccepted,
                    ) => {
                        info!(
                            "handle_connection_observer_event(): Accepted before connected from {}",
                            remote_device_id
                        );
                        call.set_state(CallState::ConnectingAfterAccepted)?;
                        self.handle_remotely_accepted_connection(call, remote_device_id)?;
                        // Don't activate the remotely accepted connection until it's connected.
                    }
                    (
                        CallDirection::OutGoing,
                        CallState::ConnectingAfterAccepted,
                        ConnectionState::ConnectedAndAccepted,
                    ) => {
                        if call.active_device_id()? == remote_device_id {
                            info!(
                                "handle_connection_observer_event(): Connected after accepted from {}",
                                remote_device_id
                            );
                            // We need to tell the application we're ringing even though it won't last long
                            // so that it goes through the same state machine it expects.
                            self.notify_application(call.clone(), ApplicationEvent::RemoteRinging);
                            call.set_state(CallState::ConnectedAndAccepted)?;
                            // The other connections were already terminated and sent hangup messages.
                            self.activate_remotely_accepted_connection(call);
                        } else {
                            info!(
                                "call_id: {} remote_device_id: {} Ignoring event: {}, from inactive connection.",
                                call_id, remote_device_id, event
                            );
                        }
                    }
                    (
                        CallDirection::OutGoing,
                        CallState::ConnectedBeforeAccepted,
                        ConnectionState::ConnectedAndAccepted,
                    ) => {
                        info!(
                            "handle_connection_observer_event(): Accepted after connected from {}",
                            remote_device_id
                        );
                        call.set_state(CallState::ConnectedAndAccepted)?;
                        self.handle_remotely_accepted_connection(call.clone(), remote_device_id)?;
                        self.activate_remotely_accepted_connection(call);
                    }
                    (
                        CallDirection::InComing,
                        CallState::ConnectedBeforeAccepted,
                        ConnectionState::ConnectedAndAccepted,
                    ) => {
                        // In Call::handle_accept_call, the CallState is set to ConnectedAndAccepted
                        // before the ConnectionState is set to ConnectedAndAccepted, so there's nothing to do here.
                    }
                    (
                        _,
                        CallState::ConnectedAndAccepted,
                        ConnectionState::ReconnectingAfterAccepted,
                    ) => {
                        if call.active_device_id()? == remote_device_id {
                            call.set_state(CallState::ReconnectingAfterAccepted)?;
                            self.notify_application(call, ApplicationEvent::Reconnecting)
                        } else {
                            info!(
                                "call_id: {} remote_device_id: {} Ignoring event: {}, from inactive connection.",
                                call_id, remote_device_id, event
                            );
                        }
                    }
                    (
                        _,
                        CallState::ReconnectingAfterAccepted,
                        ConnectionState::ConnectedAndAccepted,
                    ) => {
                        if call.active_device_id()? == remote_device_id {
                            call.set_state(CallState::ConnectedAndAccepted)?;
                            self.notify_application(call, ApplicationEvent::Reconnected)
                        } else {
                            info!(
                                "call_id: {} remote_device_id: {} Ignoring event: {}, from inactive connection.",
                                call_id, remote_device_id, event
                            );
                        }
                    }
                    (_, _, ConnectionState::IceFailed) => {
                        self.schedule_work_until_terminating(call, "Processing connection_failed request failed", move |mut call| {
                            call.handle_ice_failed(remote_device_id)
                        });
                    }
                    // The following state tranisitions make sense but aren't interesting.
                    (
                        _,
                        _,
                        ConnectionState::NotYetStarted
                        | ConnectionState::Starting
                        | ConnectionState::IceGathering
                        | ConnectionState::ConnectingBeforeAccepted
                        | ConnectionState::Terminating
                        | ConnectionState::Terminated,
                    )
                    // This is possible for outgoing calls when multiple connections can all reach the connected state.
                    | (
                        CallDirection::OutGoing,
                        CallState::ConnectedBeforeAccepted,
                        ConnectionState::ConnectedBeforeAccepted,
                    ) => {
                    }
                    // This is possible if we hit "reconnecting" while ringing
                    // and then go back to "connected" after accepting.
                    // We could avoid it by having a ReconnectingBeforeAccepted
                    // state but that doesn't seem worth it.
                    | (
                        _,
                        CallState::ConnectedAndAccepted,
                        ConnectionState::ConnectedAndAccepted,
                    ) => {
                    }
                    // None of the following state transitions make any sense
                    (
                        _,
                        CallState::NotYetStarted
                        | CallState::WaitingToProceed
                        | CallState::Terminating
                        | CallState::Terminated,
                        ConnectionState::ConnectedBeforeAccepted
                        | ConnectionState::ConnectingAfterAccepted
                        | ConnectionState::ConnectedAndAccepted
                        | ConnectionState::ReconnectingAfterAccepted,
                    )
                    | (
                        _,
                        CallState::ConnectingBeforeAccepted,
                        ConnectionState::ConnectedAndAccepted | ConnectionState::ReconnectingAfterAccepted,
                    )
                    // Note: This is possible for outgoing calls when multiple connections can all reach the connected state.
                    // (See that handled case above)
                    | (
                        CallDirection::InComing,
                        CallState::ConnectedBeforeAccepted,
                        ConnectionState::ConnectedBeforeAccepted,
                    )
                    | (
                        _,
                        CallState::ConnectedBeforeAccepted,
                        ConnectionState::ReconnectingAfterAccepted,
                    )
                    | (
                        CallDirection::InComing,
                        CallState::ConnectingAfterAccepted,
                        _,
                    )
                    | (
                        CallDirection::InComing,
                        _,
                        ConnectionState::ConnectingAfterAccepted,
                    )
                    | (
                        CallDirection::OutGoing,
                        CallState::ConnectingAfterAccepted,
                        ConnectionState::ConnectedBeforeAccepted | ConnectionState::ConnectingAfterAccepted | ConnectionState::ReconnectingAfterAccepted,
                    )
                    | (
                        _,
                        CallState::ConnectedAndAccepted,
                        ConnectionState::ConnectedBeforeAccepted | ConnectionState::ConnectingAfterAccepted,
                    )
                    | (
                        _,
                        CallState::ReconnectingAfterAccepted,
                        ConnectionState::ConnectedBeforeAccepted | ConnectionState::ConnectingAfterAccepted | ConnectionState::ReconnectingAfterAccepted,
                    ) => {
                        error!(
                            "call_id: {} remote_device_id: {} direction: {}, call state: {} unexpected connection state: {}",
                            call_id, remote_device_id, direction, state, connection_state
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
            ConnectionObserverEvent::RemoteSenderStatusChanged(status) => {
                if state.active() && call.active_device_id()? == remote_device_id {
                    if let Some(video_enabled) = status.video_enabled {
                        if video_enabled {
                            self.notify_application(
                                call.clone(),
                                ApplicationEvent::RemoteVideoEnable,
                            )
                        } else {
                            self.notify_application(
                                call.clone(),
                                ApplicationEvent::RemoteVideoDisable,
                            )
                        }
                    }
                    if let Some(sharing_screen) = status.sharing_screen {
                        if sharing_screen {
                            self.notify_application(
                                call,
                                ApplicationEvent::RemoteSharingScreenEnable,
                            )
                        } else {
                            self.notify_application(
                                call,
                                ApplicationEvent::RemoteSharingScreenDisable,
                            )
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
            ConnectionObserverEvent::IceNetworkRouteChanged(network_route) => {
                match call.active_device_id() {
                    Err(_) => {
                        // Wait until we've settled on one Connection and then
                        // report the network route of that Connection.
                    }
                    Ok(active_device_id) if active_device_id == remote_device_id => {
                        self.notify_network_route_changed(call, network_route);
                    }
                    _ => {
                        debug!(
                            "call_id: {} remote_device_id: {} Ignoring network route changed from inactive connection.",
                            call_id, remote_device_id
                        );
                    }
                }
                Ok(())
            }
            ConnectionObserverEvent::AudioLevels {
                captured_level,
                received_level,
            } => {
                self.notify_audio_levels(call, captured_level, received_level);
                Ok(())
            }
        }
    }

    fn handle_internal_error(&mut self, call: Call<T>, error: anyhow::Error) -> Result<()> {
        info!("handle_internal_error():");

        let internal_error_future =
            lazy(move |_| call.internal_error(error)).unwrap_or_else(move |err: anyhow::Error| {
                error!("Processing internal error future failed: {}", err);
            });

        self.worker_spawn(internal_error_future);
        Ok(())
    }

    fn handle_connection_observer_error(
        &mut self,
        call: Call<T>,
        error: anyhow::Error,
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

        if !state.active() {
            self.schedule_work_even_when_terminating(
                call,
                "Processing call timeout failed",
                move |call| call.call_manager()?.timeout(call.call_id()),
            );
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
