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
//! - ConnectionFailed
//! - Timeout
//! - Reconnecting
//!
//! ## Signaling events from client application
//! - ReceivedAnswer
//! - ReceivedIceCandidates
//!
//! ## From Internal runtime
//!
//! - CallTimeout
//! - InternalError

extern crate tokio;

use std::fmt;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use futures::future::lazy;
use futures::{Async, Future, Poll, Stream};
use tokio::runtime;

use crate::error::RingRtcError;

use crate::common::{
    AnswerParameters,
    ApplicationEvent,
    CallDirection,
    CallState,
    ConnectionId,
    DeviceId,
    FeatureLevel,
    HangupParameters,
    HangupType,
    Result,
    USE_LEGACY_HANGUP_MESSAGE,
};

use crate::core::call::{Call, EventStream};
use crate::core::connection::ObserverEvent;
use crate::core::platform::Platform;

use crate::webrtc::ice_candidate::IceCandidate;

/// The different types of CallEvents.
pub enum CallEvent {
    // Control events from client application
    /// Start a call (call struct has the direction attribute).
    StartCall,
    /// Accept incoming call (callee only).
    LocalAccept,
    /// Hangup call.
    LocalHangup,

    // Flow events from client application
    /// OK to proceed with call setup.
    Proceed(Vec<DeviceId>, bool),

    // Signaling events from client application
    /// Received SDP answer signal message from remote peer (caller
    /// only).
    ReceivedAnswer(DeviceId, AnswerParameters),
    /// Received ICE candidates signal message from remote peer.
    ReceivedIceCandidates(DeviceId, Vec<IceCandidate>),
    /// Received hangup signal message from remote peer.
    ReceivedHangup(DeviceId, HangupParameters),

    /// Connection observer event
    ConnectionEvent(ObserverEvent, DeviceId),
    /// Connection observer error
    ConnectionError(failure::Error, DeviceId),

    // Internally generated events
    /// Notify the call manager of an internal error condition.
    InternalError(failure::Error),
    /// The call timed out while establishing a connection.
    CallTimeout,
    /// Synchronize the FSM.
    Synchronize(Arc<(Mutex<bool>, Condvar)>),
    /// Shutdown the call.
    EndCall,
}

impl fmt::Display for CallEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            CallEvent::StartCall => "StartCall".to_string(),
            CallEvent::LocalAccept => "LocalAccept".to_string(),
            CallEvent::LocalHangup => "LocalHangup".to_string(),
            CallEvent::Proceed(devices, enable_forking) => format!(
                "Proceed, devices: {:?}, enable_forking: {}",
                devices, enable_forking
            ),
            CallEvent::ReceivedAnswer(d, answer) => format!(
                "ReceivedAnswer, device: {} remote_feature_level: {}",
                d,
                answer.remote_feature_level()
            ),
            CallEvent::ReceivedIceCandidates(d, _) => {
                format!("ReceivedIceCandidates, device: {}", d)
            }
            CallEvent::ReceivedHangup(d, hangup_parameters) => format!(
                "ReceivedHangup, device: {} hangup: {}",
                d, hangup_parameters
            ),
            CallEvent::ConnectionEvent(e, d) => {
                format!("ConnectionEvent, event: {}, device: {}", e, d)
            }
            CallEvent::ConnectionError(e, d) => {
                format!("ConnectionError, error: {}, device: {}", e, d)
            }
            CallEvent::InternalError(e) => format!("InternalError: {}", e),
            CallEvent::CallTimeout => "CallTimeout".to_string(),
            CallEvent::Synchronize(_) => "Synchronize".to_string(),
            CallEvent::EndCall => "EndCall".to_string(),
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
            CallEvent::LocalHangup => return self.handle_local_hangup(call, state),
            CallEvent::EndCall => return self.handle_end_call(call),
            CallEvent::Synchronize(sync) => return self.handle_synchronize(sync),
            _ => {}
        }

        // If in the process of terminating the call, drop all other
        // events.
        match state {
            CallState::Terminating | CallState::Closed => {
                debug!("handle_event(): dropping event {} while terminating", event);
                return Ok(());
            }
            _ => (),
        }

        match event {
            CallEvent::StartCall => self.handle_start_call(call, state),
            CallEvent::Proceed(remote_devices, enable_forking) => {
                self.handle_proceed(call, state, remote_devices, enable_forking)
            }
            CallEvent::LocalAccept => self.handle_local_accept(call, state),
            CallEvent::ReceivedAnswer(remote_device, answer) => {
                self.handle_received_answer(call, state, remote_device, answer)
            }
            CallEvent::ReceivedIceCandidates(remote_device, ice_candidates) => {
                self.handle_received_ice_candidates(call, state, ice_candidates, remote_device)
            }
            CallEvent::ReceivedHangup(remote_device, hangup_parameters) => {
                self.handle_received_hangup(call, state, remote_device, hangup_parameters)
            }
            CallEvent::ConnectionEvent(event, remote_device) => {
                self.handle_connection_event(call, state, event, remote_device)
            }
            CallEvent::ConnectionError(error, remote_device) => {
                self.handle_connection_error(call, error, remote_device)
            }
            CallEvent::InternalError(error) => self.handle_internal_error(call, error),
            CallEvent::CallTimeout => self.handle_call_timeout(call, state),
            CallEvent::LocalHangup => Ok(()),
            CallEvent::Synchronize(_) => Ok(()),
            CallEvent::EndCall => Ok(()),
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

        if let CallState::Idle = state {
            call.set_state(CallState::Starting)?;
            call.handle_start_call()
        } else {
            self.unexpected_state(state, "StartCall");

            Ok(())
        }
    }

    fn handle_proceed(
        &mut self,
        mut call: Call<T>,
        state: CallState,
        remote_devices: Vec<DeviceId>,
        enable_forking: bool,
    ) -> Result<()> {
        info!("handle_proceed():");

        if let CallState::Starting = state {
            call.set_state(CallState::Connecting)?;

            let mut err_call = call.clone();
            let proceed_future = lazy(move || {
                if call.terminating()? {
                    return Ok(());
                }
                call.proceed(remote_devices, enable_forking)
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
        remote_device: DeviceId,
        answer: AnswerParameters,
    ) -> Result<()> {
        // Accept answers when we are ringing so we can get answers for more than one connection.
        if state == CallState::Connecting || state == CallState::Ringing {
            let mut err_call = call.clone();
            let handle_answer_future = lazy(move || {
                if call.terminating()? {
                    return Ok(());
                }
                call.received_answer(remote_device, answer)
            })
            .map_err(move |err| {
                err_call.inject_internal_error(err, "Handle Received Answer Future failed")
            });

            self.worker_spawn(handle_answer_future);
        } else {
            self.unexpected_state(state, "HandleReceivedAnswer");
        }
        Ok(())
    }

    fn handle_received_ice_candidates(
        &mut self,
        call: Call<T>,
        state: CallState,
        ice_candidates: Vec<IceCandidate>,
        remote_device: DeviceId,
    ) -> Result<()> {
        match state {
            CallState::Starting
            | CallState::Connecting
            | CallState::Ringing
            | CallState::Connected
            | CallState::Reconnecting => {
                let mut err_call = call.clone();
                let handle_received_ice_future = lazy(move || {
                    if call.terminating()? {
                        return Ok(());
                    }
                    call.received_ice_candidates(remote_device, ice_candidates)
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
        remote_device: DeviceId,
        hangup_parameters: HangupParameters,
    ) -> Result<()> {
        info!(
            "handle_received_hangup(): remote_device: {}, hangup: {}",
            remote_device, hangup_parameters
        );

        let mut err_call = call.clone();

        let hangup_event = match hangup_parameters.hangup_type() {
            HangupType::Accepted => ApplicationEvent::EndedRemoteHangupAccepted,
            HangupType::Declined => ApplicationEvent::EndedRemoteHangupDeclined,
            HangupType::Busy => ApplicationEvent::EndedRemoteHangupBusy,
            HangupType::Normal => {
                match call.direction() {
                    CallDirection::InComing => {
                        // Callees will never need to send hangup messages in reaction
                        // to the received hangup since only callers perform such call
                        // management in multi-ring scenarios.

                        // Normal handling of the received hangup.
                        call.set_state(CallState::Terminating)?;
                    }
                    CallDirection::OutGoing => match state {
                        CallState::Idle
                        | CallState::Connected
                        | CallState::Reconnecting
                        | CallState::Terminating
                        | CallState::Closed => {
                            // We are already connected and can assume that the hangup
                            // is a traditional "in-call" hangup. Hangups to other
                            // callees/connections would have been sent at the time
                            // of transitioning to the Connected state. Therefore, we
                            // don't need to send anything out in reaction to the
                            // received hangup.
                            // @note Idle and Closed states shouldn't reach this function...
                            if remote_device == call.active_device_id()? {
                                // Normal handling of the received hangup.
                                call.set_state(CallState::Terminating)?;
                            } else {
                                info!("handle_received_hangup(): Ignoring hangup message from a device we aren't in a call with after connection");
                                return Ok(());
                            }
                        }
                        CallState::Starting | CallState::Connecting | CallState::Ringing => {
                            // A normal hangup has been received on a call for which
                            // we are the caller and which is not yet accepted. This
                            // means that the callee device is declining the call.

                            // Make sure the call is in a terminating state asap.
                            call.set_state(CallState::Terminating)?;

                            // Send out hangup/declined to all callees.
                            let hangup_parameters =
                                HangupParameters::new(HangupType::Declined, Some(remote_device));

                            // Send via the data channel except to the decliner.
                            call.send_hangup_via_data_channel_to_all_except(hangup_parameters)?;

                            // Also send out via signal messaging.
                            let mut call_manager = call.call_manager()?;
                            call_manager.send_hangup(
                                call.clone(),
                                call.call_id(),
                                hangup_parameters,
                                USE_LEGACY_HANGUP_MESSAGE,
                            )?;
                        }
                    },
                }

                // Continue with the normal handling of a received hangup, to hang up
                // the local device.
                let remote_hangup_future = lazy(move || {
                    let mut call_manager = call.call_manager()?;
                    call_manager.remote_hangup(call.call_id(), None)
                })
                .map_err(move |err| {
                    err_call.inject_internal_error(err, "Processing remote hangup request failed")
                });

                self.worker_spawn(remote_hangup_future);

                // Return for any HangupType::Normal case.
                return Ok(());
            }
        };

        match hangup_parameters.device_id() {
            Some(device_id) => {
                // If this is the device that caused the hangup, it can be ignored.
                if call.local_device_id() == device_id {
                    info!("handle_received_hangup(): Ignoring hangup message for the referenced device");
                    Ok(())
                } else {
                    let remote_hangup_future = lazy(move || {
                        let mut call_manager = call.call_manager()?;
                        call_manager.remote_hangup(call.call_id(), Some(hangup_event))
                    })
                    .map_err(move |err| {
                        err_call
                            .inject_internal_error(err, "Processing remote hangup request failed")
                    });

                    self.worker_spawn(remote_hangup_future);
                    Ok(())
                }
            }
            None => {
                info!("handle_received_hangup(): Missing device_id for hangup, ignoring");
                Ok(())
            }
        }
    }

    fn handle_local_accept(&mut self, call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_local_accept():");
        match state {
            CallState::Ringing => {
                call.set_state(CallState::Connected)?;
                let mut err_call = call.clone();
                let accept_future = lazy(move || {
                    if call.terminating()? {
                        return Ok(());
                    }
                    let mut connection = call.active_connection()?;
                    connection.inject_accept_call()?;
                    connection.connect_media()?;
                    call.notify_application(ApplicationEvent::LocalConnected)
                })
                .map_err(move |err| {
                    err_call.inject_internal_error(err, "Processing local accept request failed")
                });

                self.worker_spawn(accept_future);
            }
            _ => self.unexpected_state(state, "LocalAccept"),
        }
        Ok(())
    }

    fn handle_local_hangup(&mut self, mut call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_local_hangup():");
        match state {
            CallState::Idle => self.unexpected_state(state, "LocalHangup"),
            _ => {
                let mut err_call = call.clone();
                let hangup_future = lazy(move || call.send_hangup_via_data_channel_to_all())
                    .map_err(move |err| {
                        err_call
                            .inject_internal_error(err, "Processing local hangup request failed")
                    });

                self.worker_spawn(hangup_future);
            }
        }
        Ok(())
    }

    fn ignore_connection_event(&self, id: ConnectionId, state: CallState, event: ObserverEvent) {
        info!(
            "id: {}: Ignoring event: {}, while in state: {}",
            id, event, state
        );
    }

    fn handle_connection_event(
        &mut self,
        mut call: Call<T>,
        state: CallState,
        event: ObserverEvent,
        remote_device: DeviceId,
    ) -> Result<()> {
        let connection_id = ConnectionId::new(call.call_id(), remote_device);

        match event {
            ObserverEvent::ConnectionRinging => {
                match state {
                    CallState::Connecting => {
                        call.set_state(CallState::Ringing)?;
                        if let CallDirection::InComing = call.direction() {
                            self.notify_application(call, ApplicationEvent::LocalRinging)
                        } else {
                            self.notify_application(call, ApplicationEvent::RemoteRinging)
                        }
                    }
                    _ => {
                        self.ignore_connection_event(connection_id, state, event);
                    }
                }
                Ok(())
            }
            ObserverEvent::RemoteConnected => {
                match call.direction() {
                    CallDirection::OutGoing => match state {
                        CallState::Ringing => {
                            info!(
                                "handle_connection_event(): Connection from {}",
                                remote_device
                            );

                            call.set_state(CallState::Connected)?;
                            call.set_active_device_id(remote_device)?;

                            // Send out hangup/accepted to all callees.
                            let hangup_parameters =
                                HangupParameters::new(HangupType::Accepted, Some(remote_device));

                            // Send via the data channel except to the accepter.
                            call.send_hangup_via_data_channel_to_all_except(hangup_parameters)?;

                            let mut err_call = call.clone();
                            let connected_future = lazy(move || {
                                if call.terminating()? {
                                    return Ok(());
                                }

                                // Get the media and application working for the first connection.
                                let connection = call.active_connection()?;
                                connection.connect_media()?;
                                call.notify_application(ApplicationEvent::RemoteConnected)?;

                                // If the remote device of the active connection can support
                                // multi-ring, we send a "legacy" Hangup message. The callee
                                // that accepted the call will ignore it and all other callees,
                                // legacy or otherwise, will handle it and end.
                                //
                                // If the remote device is not multi-ring capable, then we send
                                // a new "non-legacy" Hangup message because the callee that
                                // accepted the call will ignore it as it is not defined in their
                                // protocol definition.
                                let use_legacy_hangup_message =
                                    match connection.remote_feature_level()? {
                                        FeatureLevel::Unspecified => !USE_LEGACY_HANGUP_MESSAGE,
                                        FeatureLevel::MultiRing => USE_LEGACY_HANGUP_MESSAGE,
                                    };

                                // Send the accepted indication via hangup signaling (it will be
                                // replicated to all remote peers).
                                let mut call_manager = call.call_manager()?;
                                call_manager.send_hangup(
                                    call.clone(),
                                    call.call_id(),
                                    hangup_parameters,
                                    use_legacy_hangup_message,
                                )?;

                                // Close all the other connections (this blocks).
                                let mut call_clone = call.clone();
                                call_clone.close_connections_except_accepted(remote_device)
                            })
                            .map_err(move |err| {
                                err_call.inject_internal_error(
                                    err,
                                    "Processing connect_media request failed",
                                )
                            });
                            self.worker_spawn(connected_future);
                        }
                        _ => {
                            self.ignore_connection_event(connection_id, state, event);
                        }
                    },
                    CallDirection::InComing => {
                        warn!(
                            "Ignoring RemoteConnected for incoming call: {}",
                            connection_id
                        );
                    }
                }
                Ok(())
            }
            ObserverEvent::RemoteHangup(hangup_parameters) => {
                self.handle_received_hangup(call, state, remote_device, hangup_parameters)
            }
            ObserverEvent::RemoteVideoStatus(enable) => {
                if call.active_device_id()? == remote_device {
                    match state {
                        CallState::Connected => {
                            if enable {
                                self.notify_application(call, ApplicationEvent::RemoteVideoEnable)
                            } else {
                                self.notify_application(call, ApplicationEvent::RemoteVideoDisable)
                            }
                        }
                        _ => {
                            self.ignore_connection_event(connection_id, state, event);
                        }
                    }
                } else {
                    info!(
                        "id: {}: Ignoring event: {}, from inactive connection.",
                        connection_id, event
                    );
                }
                Ok(())
            }
            ObserverEvent::ConnectionReconnecting => {
                if call.active_device_id()? == remote_device {
                    match state {
                        CallState::Connected => {
                            call.set_state(CallState::Reconnecting)?;
                            self.notify_application(call, ApplicationEvent::Reconnecting)
                        }
                        _ => {
                            self.ignore_connection_event(connection_id, state, event);
                        }
                    }
                } else {
                    info!(
                        "id: {}: Ignoring event: {}, from inactive connection.",
                        connection_id, event
                    );
                }
                Ok(())
            }
            ObserverEvent::ConnectionReconnected => {
                if call.active_device_id()? == remote_device {
                    match state {
                        CallState::Reconnecting => {
                            call.set_state(CallState::Connected)?;
                            self.notify_application(call, ApplicationEvent::Reconnected)
                        }
                        _ => {
                            self.ignore_connection_event(connection_id, state, event);
                        }
                    }
                } else {
                    info!(
                        "id: {}: Ignoring event: {}, from inactive connection.",
                        connection_id, event
                    );
                }
                Ok(())
            }
            ObserverEvent::ConnectionFailed => {
                let mut err_call = call.clone();
                let future = lazy(move || {
                    if call.terminating()? {
                        return Ok(());
                    }
                    call.connection_failed(remote_device)
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

    fn handle_connection_error(
        &mut self,
        call: Call<T>,
        error: failure::Error,
        remote_device: DeviceId,
    ) -> Result<()> {
        let id = ConnectionId::new(call.call_id(), remote_device);
        info!("handle_connection_error(): id: {}", id);

        // Treat a connection internal error as a call internal error,
        // i.e. ignore the remote_device ID.
        self.handle_internal_error(call, error)
    }

    fn handle_call_timeout(&mut self, call: Call<T>, state: CallState) -> Result<()> {
        info!("handle_call_timeout():");

        match state {
            CallState::Connected | CallState::Reconnecting => {} // Ok
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

    fn handle_end_call(&mut self, mut call: Call<T>) -> Result<()> {
        self.event_stream.close();
        self.drain_worker_thread();
        self.drain_notify_thread();

        call.set_state(CallState::Closed)?;
        call.terminate_complete()
    }

    fn unexpected_state(&self, state: CallState, event: &str) {
        warn!("Unexpected event {}, while in state {:?}", event, state);
    }
}
