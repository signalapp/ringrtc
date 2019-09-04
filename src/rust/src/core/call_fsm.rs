//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Call Connection Finite State Machine
//!
//! The Call FSM mediates the state machine of the client application
//! with the state machine of WebRTC.  The FSM implements the ICE
//! negotiation protocol without the need for the client application
//! to intervene.
//!
//! # Asynchronous Inputs:
//!
//! ## From Client application
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
//! ## From Internal runtime
//!
//! - CallTimeout
//!
//! # Asynchronous Outputs:
//!
//! ## To Client application
//!
//! - [ClientEvents](../call_connection_observer/enum.ClientEvent.html)
//! - ClientErrors

extern crate tokio;

use std::fmt;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use futures::{
    Future,
    Async,
    Poll,
    Stream,
};
use futures::future::lazy;
use tokio::runtime;

use crate::common::{
    Result,
    CallDirection,
    CallId,
    CallState,
};
use crate::core::call_connection_factory::EventStream;
use crate::core::call_connection::{
    CallConnectionInterface,
    CallConnectionHandle,
    ClientRecipientTrait,
};
use crate::core::call_connection_observer::ClientEvent;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::DataChannel;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media_stream::MediaStream;

/// The different types of CallEvents.
pub enum CallEvent<T>
where
    T: ClientRecipientTrait,
{
    /// Send SDP offer to remote peer (caller only).
    SendOffer,
    /// Accept SDP answer from remote peer (caller only).
    AcceptAnswer(String),
    /// Accept SDP offer from remote peer (callee only).
    AcceptOffer(String),
    /// Accept incoming call (callee only).
    AnswerCall,
    /// Receive hangup from remote peer.
    RemoteHangup(CallId),
    /// Receive call connected from remote peer.
    RemoteConnected(CallId),
    /// Receive video streaming status change from remote peer.
    RemoteVideoStatus(CallId, bool),
    /// Receive ICE candidate message from remote peer.
    RemoteIceCandidate(IceCandidate),
    /// Local hangup event from client application.
    LocalHangup,
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
    /// Send the client application an error message.
    ClientError(failure::Error),
    /// Receive local media stream from WebRTC observer.
    OnAddStream(MediaStream),
    /// Receive local call time from timeout runtime.
    CallTimeout(CallId),
    /// Receive new available data channel from WebRTC observer (callee).
    OnDataChannel(DataChannel),
    /// Send busy signal to third-party.
    SendBusy(T, CallId),
    /// Shutdown the call.
    EndCall,
}

impl<T> fmt::Display for CallEvent<T>
where
    T: ClientRecipientTrait,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            CallEvent::SendOffer                      => "SendOffer".to_string(),
            CallEvent::AcceptAnswer(answer)           => format!("AcceptAnswer, answer: {}", answer),
            CallEvent::AcceptOffer(offer)             => format!("AcceptOffer, offer: {}", offer),
            CallEvent::AnswerCall                     => "AnswerCall".to_string(),
            CallEvent::RemoteHangup(id)               => format!("RemoteHangup, call_id: {}", id),
            CallEvent::RemoteConnected(id)            => format!("RemoteConnected, call_id: {}", id),
            CallEvent::RemoteVideoStatus(id, enabled) => format!("RemoteVideoStatus, call_id: {}, enabled: {}", id, enabled),
            CallEvent::RemoteIceCandidate(_)          => "RemoteIceCandidate".to_string(),
            CallEvent::LocalHangup                    => "LocalHangup".to_string(),
            CallEvent::LocalVideoStatus(enabled)      => format!("LocalVideoStatus, enabled: {}", enabled),
            CallEvent::LocalIceCandidate(_)           => "LocalIceCandidate".to_string(),
            CallEvent::IceConnected                   => "IceConnected".to_string(),
            CallEvent::IceConnectionFailed            => "IceConnectionFailed".to_string(),
            CallEvent::IceConnectionDisconnected      => "IceConnectionDisconnected".to_string(),
            CallEvent::ClientError(e)                 => format!("ClientError: {}", e),
            CallEvent::CallTimeout(id)                => format!("CallTimeout, call_id: {}", id),
            CallEvent::OnAddStream(stream)            => format!("OnAddStream, stream: {:}", stream),
            CallEvent::OnDataChannel(dc)              => format!("OnDataChannel, dc: {:?}", dc),
            CallEvent::SendBusy(_, id)                => format!("SendBusy, call_id: {}", id),
            CallEvent::EndCall                        => "EndCall".to_string(),
        };
        write!(f, "({})", display)
    }
}

impl<T> fmt::Debug for CallEvent<T>
where
    T: ClientRecipientTrait,
{
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
/// The FSM itself is executing on a runtime managed by the CallConnectionFactory object.
///
/// For "quick" reactions to incoming events, the FSM handles them
/// immediately on its own thread.
///
/// For "lengthy" reactions, typically involving network access, the
/// FSM dispatches the work to a "network" thread.
///
/// For notification events targeted for the client application, the
/// FSM dispatches the work to a "notify" thread.
#[derive(Debug)]
pub struct CallStateMachine<T>
where
    T: CallConnectionInterface,
{
    /// Receiving end of EventPump.
    event_stream: EventStream<T>,
    /// Runtime for processing long running requests.
    network_runtime: Option<runtime::Runtime>,
    /// Runtime for processing client application notification events.
    notify_runtime: Option<runtime::Runtime>,
}

impl<T> fmt::Display for CallStateMachine<T>
where
    T: CallConnectionInterface,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(tid: {:?})", thread::current().id())
    }
}

impl<T> Future for CallStateMachine<T>
where
    T: CallConnectionInterface,
{
    type Item = ();
    type Error = failure::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {

        loop {
            match try_ready!(self.event_stream.poll().map_err(|_| { RingRtcError::FsmStreamPoll })) {
                Some((cc_handle, event)) => {
                    let state = cc_handle.get_state()?;
                    info!("CallStateMachine: rx state: {}, event: {}", state, event);
                    if let Err(e) = self.handle_event(cc_handle, state, event) {
                        error!("Handling event failed: {:?}", e);
                    }
                },
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
    T: CallConnectionInterface,
{
    /// Creates a new CallStateMachine object.
    pub fn new(event_stream: EventStream<T>) -> Result<CallStateMachine<T>> {
        info!("CallStateMachine constructor:");
        let mut fsm = CallStateMachine {
            event_stream,
            network_runtime: Some(
                runtime::Builder::new()
                    .core_threads(1)
                    .name_prefix("network-")
                    .build()?
            ),
            notify_runtime: Some(
                runtime::Builder::new()
                    .core_threads(1)
                    .name_prefix("notify-")
                    .build()?
            ),
        };

        if let Some(network_runtime) = &mut fsm.network_runtime {
            CallStateMachine::<T>::sync_thread("network", network_runtime)?;
        }
        if let Some(notify_runtime) = &mut fsm.notify_runtime {
            CallStateMachine::<T>::sync_thread("notify", notify_runtime)?;
        }

        Ok(fsm)
    }

    /// Synchronize a runtime with the main FSM thread.
    fn sync_thread(label: &'static str, runtime: &mut runtime::Runtime) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let future = lazy(move ||
                          {
                              info!("syncing {} thread: {:?}", label, thread::current().id());
                              let _ = tx.send(true);
                              Ok(())
                          });
        runtime.spawn(future);
        let _ = rx.recv_timeout(Duration::from_secs(2))?;
        Ok(())
    }

    /// Spawn a future on the network runtime if enabled.
    fn network_spawn<F>(&mut self, future: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        if let Some(network_runtime) = &mut self.network_runtime {
            network_runtime.spawn(future);
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

    /// Shutdown the network runtime.
    fn drain_network_thread(&mut self) {
        info!("draining network thread");
        if let Some(network_runtime) = self.network_runtime.take() {
            let _ = network_runtime.shutdown_on_idle().wait()
                .map_err(|_| warn!("Problems shutting down the network runtime"));
        }
        info!("draining network thread: complete");
    }

    /// Shutdown the notify runtime.
    fn drain_notify_thread(&mut self) {
        info!("draining notify thread");
        if let Some(notify_runtime) = self.notify_runtime.take() {
            let _ = notify_runtime.shutdown_on_idle().wait()
                .map_err(|_| warn!("Problems shutting down the notify runtime"));
        }
        info!("draining notify thread: complete");
    }

    /// Top level event dispatch.
    fn handle_event(&mut self,
                    cc_handle: CallConnectionHandle<T>,
                    state:     CallState,
                    event:     CallEvent<<T as CallConnectionInterface>::ClientRecipient>) -> Result<()> {

        // Handle these events even while terminating, as the remote
        // side needs to be informed.
        match event {
            CallEvent::LocalHangup             => return self.handle_local_hangup(cc_handle, state),
            CallEvent::SendBusy(recipient, id) => return self.handle_send_busy(cc_handle, state, recipient, id),
            CallEvent::EndCall                 => return self.handle_end_call(cc_handle),
            _ => {},
        }

        // If in the process of terminating the call, drop all other
        // events.
        if let CallState::Terminating = state {
            debug!("handle_event(): dropping event {} while terminating", event);
            return Ok(());
        }

        match event {
            CallEvent::SendOffer                      => self.handle_send_offer(cc_handle, state),
            CallEvent::AcceptAnswer(answer)           => self.handle_accept_answer(cc_handle, state, answer),
            CallEvent::AcceptOffer(offer)             => self.handle_accept_offer(cc_handle, state, offer),
            CallEvent::AnswerCall                     => self.handle_answer_call(cc_handle, state),
            CallEvent::RemoteHangup(id)               => self.handle_remote_hangup(cc_handle, state, id),
            CallEvent::RemoteConnected(id)            => self.handle_remote_connected(cc_handle, state, id),
            CallEvent::RemoteVideoStatus(id, enabled) => self.handle_remote_video_status(cc_handle, state, id, enabled),
            CallEvent::RemoteIceCandidate(candidate)  => self.handle_remote_ice_candidate(cc_handle, state, candidate),
            CallEvent::LocalVideoStatus(enabled)      => self.handle_local_video_status(cc_handle, state, enabled),
            CallEvent::LocalIceCandidate(candidate)   => self.handle_local_ice_candidate(cc_handle, state, candidate),
            CallEvent::IceConnected                   => self.handle_ice_connected(cc_handle, state),
            CallEvent::IceConnectionFailed            => self.handle_ice_connection_failed(cc_handle, state),
            CallEvent::IceConnectionDisconnected      => self.handle_ice_connection_disconnected(cc_handle, state),
            CallEvent::ClientError(error)             => self.handle_client_error(cc_handle, error),
            CallEvent::CallTimeout(id)                => self.handle_call_timeout(cc_handle, state, id),
            CallEvent::OnAddStream(stream)            => self.handle_on_add_stream(cc_handle, state, stream),
            CallEvent::OnDataChannel(dc)              => self.handle_on_data_channel(cc_handle, state, dc),
            CallEvent::LocalHangup                    => Ok(()),
            CallEvent::SendBusy(_recipient, _id)      => Ok(()),
            CallEvent::EndCall                        => Ok(()),
        }
    }

    fn handle_send_offer(&mut self,
                         cc_handle: CallConnectionHandle<T>,
                         state:     CallState) -> Result<()> {

        if let CallState::Idle = state {

            cc_handle.set_state(CallState::SendingOffer)?;

            let mut error_handle  = cc_handle.clone();
            let send_offer_future = lazy(move ||
                                         {
                                             let cc = cc_handle.lock()?;
                                             if cc.terminating() {
                                                 return Ok(())
                                             }
                                             cc.send_offer()?;
                                             Ok(())
                                         })
            .map_err(move |err: failure::Error| {
                error!("SendOfferFuture failed: {}", err);
                let _ = error_handle.inject_client_error(err);
            });

            debug!("handle_send_offer(): spawning network task");
            self.network_spawn(send_offer_future);
        } else {
            self.unexpected_state(state, "SendOffer");
        }
        Ok(())
    }

    fn handle_accept_answer(&mut self,
                            cc_handle: CallConnectionHandle<T>,
                            state:     CallState,
                            answer:    String) -> Result<()> {

        if let CallState::SendingOffer = state {

            cc_handle.set_state(CallState::IceConnecting(false))?;

            let mut error_handle     = cc_handle.clone();
            let accept_answer_future = lazy(move ||
                                            {
                                                let mut cc = cc_handle.lock()?;
                                                if cc.terminating() {
                                                    return Ok(())
                                                }
                                                cc.accept_answer(answer)?;
                                                // we have local and remote sdp now
                                                cc.set_state(CallState::IceConnecting(true));
                                                cc.process_remote_ice_updates()?;
                                                Ok(())
                                            })
                .map_err(move |err: failure::Error| {
                    error!("AcceptAnswerFuture failed: {}", err);
                    let _ = error_handle.inject_client_error(err);
                });

            debug!("handle_accept_answer(): spawning network task");
            self.network_spawn(accept_answer_future);
        } else {
            self.unexpected_state(state, "AcceptAnswer");
        }
        Ok(())
    }

    fn handle_accept_offer(&mut self,
                           cc_handle: CallConnectionHandle<T>,
                           state:     CallState,
                           offer:     String) -> Result<()> {

        if let CallState::Idle = state {

            cc_handle.set_state(CallState::IceConnecting(false))?;

            let mut error_handle    = cc_handle.clone();
            let accept_offer_future = lazy(move ||
                                           {
                                               let mut cc = cc_handle.lock()?;
                                               if cc.terminating() {
                                                   return Ok(())
                                               }
                                               cc.accept_offer(offer)?;
                                               // we have local and remote sdp now
                                               cc.set_state(CallState::IceConnecting(true));
                                               cc.process_remote_ice_updates()?;
                                               Ok(())
                                           })
                .map_err(move |err: failure::Error| {
                    error!("AcceptOfferFuture failed: {}", err);
                    let _ = error_handle.inject_client_error(err);
                });

            debug!("handle_accept_offer(): spawning network task");
            self.network_spawn(accept_offer_future);
        } else {
            self.unexpected_state(state, "AcceptOffer");
        }
        Ok(())
    }

    fn notify_client(&mut self, cc_handle: CallConnectionHandle<T>, event: ClientEvent) {

        let mut error_handle     = cc_handle.clone();
        let notify_client_future = lazy(move ||
                                        {
                                            let cc = cc_handle.lock()?;
                                            if cc.terminating() {
                                                return Ok(())
                                            }
                                            cc.notify_client(event)
                                        })
            .map_err(move |err| {
                error!("Notify Client Future failed: {}", err);
                let _ = error_handle.inject_client_error(err);
            });
        debug!("fsm:notify_client(): spawning notify task, event: {}", event);
        self.notify_spawn(notify_client_future);
    }

    fn handle_remote_hangup(&mut self,
                            cc_handle: CallConnectionHandle<T>,
                            state:     CallState,
                            call_id:   CallId) -> Result<()> {

        if cc_handle.get_call_id()? != call_id {
            warn!("Remote hangup for non-active call");
            return Ok(());
        }
        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected     |
            CallState::CallConnected
                => self.notify_client(cc_handle, ClientEvent::RemoteHangup),
            _ => self.unexpected_state(state, "RemoteHangup"),
        };
        Ok(())
    }

    fn handle_remote_connected(&mut self,
                               cc_handle: CallConnectionHandle<T>,
                               state:     CallState,
                               call_id:   CallId) -> Result<()> {

        if cc_handle.get_call_id()? != call_id {
            warn!("Remote connected for non-active call");
            return Ok(());
        }
        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected
            => {
                cc_handle.set_state(CallState::CallConnected)?;
                self.notify_client(cc_handle, ClientEvent::RemoteConnected);
            },
            _ => self.unexpected_state(state, "RemoteConnected"),
        }
        Ok(())
    }

    fn handle_remote_video_status(&mut self,
                                  cc_handle: CallConnectionHandle<T>,
                                  state:     CallState,
                                  call_id:   CallId,
                                  enabled:   bool) -> Result<()> {

        if cc_handle.get_call_id()? != call_id {
            warn!("Remote video status change for non-active call");
            return Ok(());
        }

        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected     |
            CallState::CallConnected
                => {
                    if enabled {
                        self.notify_client(cc_handle, ClientEvent::RemoteVideoEnable);
                    } else {
                        self.notify_client(cc_handle, ClientEvent::RemoteVideoDisable);
                    }
                },
            _ => self.unexpected_state(state, "RemoteVideoStatus"),
        };
        Ok(())
    }

    fn handle_remote_ice_candidate(&mut self,
                                   cc_handle: CallConnectionHandle<T>,
                                   state:     CallState,
                                   candidate: IceCandidate) -> Result<()> {

        if let CallState::Idle = state {
            warn!("State is now idle, ignoring remote ICE candidates...");
            return Ok(());
        }

        if let Ok(mut cc) = cc_handle.lock() {
            cc.add_remote_candidate(candidate);
        }

        match state {
            CallState::IceConnecting(false) => {},
            CallState::IceConnecting(true) |
            CallState::IceConnected        |
            CallState::CallConnected
                => {
                    let mut cc = cc_handle.lock()?;
                    cc.process_remote_ice_updates()?;
                },
            _ => self.unexpected_state(state, "RemoteIceCandidate"),
        }

        Ok(())
    }

    fn handle_answer_call(&mut self,
                          cc_handle: CallConnectionHandle<T>,
                          state:     CallState) -> Result<()> {

        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected
                => {
                    // notify the peer via a data channel message.
                    let mut error_handle = cc_handle.clone();
                    let connected_future = lazy(move ||
                                                {
                                                    let mut cc = cc_handle.lock()?;
                                                    if cc.terminating() {
                                                        return Ok(())
                                                    }
                                                    cc.send_connected()
                                                })
                        .map_err(move |err| {
                            error!("Sending Connected failed: {}", err);
                            let _ = error_handle.inject_client_error(err);
                        });
                    debug!("handle_answer_call(): spawning network task");
                    self.network_spawn(connected_future);
                },
            _ => self.unexpected_state(state, "AnswerCall"),
        }
        Ok(())
    }

    fn handle_local_hangup(&mut self,
                           cc_handle: CallConnectionHandle<T>,
                           state:     CallState) -> Result<()> {

        match state {
            CallState::Idle => self.unexpected_state(state, "LocalHangup"),
            _               => {
                let mut error_handle = cc_handle.clone();
                let hang_up_future   = lazy(move ||
                                          {
                                              let cc = cc_handle.lock()?;
                                              cc.send_hang_up()
                                          })
                    .map_err(move |err| {
                        error!("Sending Hang Up failed: {}", err);
                        let _ = error_handle.inject_client_error(err);
                    });
                debug!("handle_local_hangup(): spawning network task");
                self.network_spawn(hang_up_future);
            },
        }
        Ok(())
    }

    fn handle_local_video_status(&mut self,
                                 cc_handle: CallConnectionHandle<T>,
                                 state:     CallState,
                                 enabled:   bool) -> Result<()> {

        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected     |
            CallState::CallConnected
                => {
                    // notify the peer via a data channel message.
                    let mut error_handle          = cc_handle.clone();
                    let local_video_status_future = lazy(move ||
                                                         {
                                                             let cc = cc_handle.lock()?;
                                                             if cc.terminating() {
                                                                 return Ok(())
                                                             }
                                                             cc.send_video_status(enabled)
                                                         })
                        .map_err(move |err| {
                            error!("Sending local video status failed: {}", err);
                            let _ = error_handle.inject_client_error(err);
                        });
                    debug!("handle_local_video_status(): spawning network task");
                    self.network_spawn(local_video_status_future);
                },
            _ => self.unexpected_state(state, "LocalVideoStatus"),
        };
        Ok(())
    }

    fn handle_local_ice_candidate(&mut self,
                                  cc_handle: CallConnectionHandle<T>,
                                  state:     CallState,
                                  candidate: IceCandidate) -> Result<()> {

        if let CallState::Idle = state {
            warn!("State is now idle, ignoring local ICE candidates...");
            return Ok(());
        }

        if let Ok(mut cc) = cc_handle.lock() {
            cc.add_local_candidate(candidate);
        }

        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected     |
            CallState::CallConnected
                => {
                // send signal message to the other side with the ICE
                // candidate.
                let mut error_handle  = cc_handle.clone();
                let ice_update_future = lazy(move ||
                                             {
                                                 let mut cc = cc_handle.lock()?;
                                                 if cc.terminating() {
                                                     return Ok(())
                                                 }
                                                 cc.send_pending_ice_updates()?;
                                                 Ok(())
                                             })
                        .map_err(move |err: failure::Error| {
                            error!("IceUpdateFuture failed: {}", err);
                            let _ = error_handle.inject_client_error(err);
                        });
                debug!("handle_local_ice_candidate(): spawning network task");
                self.network_spawn(ice_update_future);
            },
            _ => (),
        }
        Ok(())
    }

    fn handle_ice_connected(&mut self,
                            cc_handle: CallConnectionHandle<T>,
                            state:     CallState) -> Result<()> {

        if let CallState::IceConnecting(_) = state {
            cc_handle.set_state(CallState::IceConnected)?;
            let notify_handle = cc_handle.clone();
            let cc = cc_handle.lock()?;
            if let CallDirection::OutGoing = cc.get_direction() {
                self.notify_client(notify_handle, ClientEvent::Ringing);
            }
        }
        Ok(())
    }

    fn handle_ice_connection_failed(&mut self,
                                    cc_handle: CallConnectionHandle<T>,
                                    state:     CallState) -> Result<()> {

        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected     |
            CallState::CallConnected
                => {
                    cc_handle.set_state(CallState::IceDisconnected)?;
                    // For callee -- the call was disconnected while answering/local_ringing
                    // For caller -- the recipient was unreachable
                    self.notify_client(cc_handle, ClientEvent::ConnectionFailed);
                },
            _ => self.unexpected_state(state, "IceConnectionFailed"),
        };
        Ok(())
    }

    fn handle_ice_connection_disconnected(&mut self,
                                          cc_handle: CallConnectionHandle<T>,
                                          state:     CallState) -> Result<()> {

        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected     |
            CallState::CallConnected
                => {
                    cc_handle.set_state(CallState::IceConnecting(true))?;
                    self.notify_client(cc_handle, ClientEvent::CallReconnecting);
                },
            _ => self.unexpected_state(state, "IceConnectionDisconnected"),
        };
        Ok(())
    }

    fn handle_client_error(&mut self,
                           cc_handle: CallConnectionHandle<T>,
                           error:     failure::Error) -> Result<()> {

        let notify_error_future = lazy(move ||
                                       {
                                           let cc = cc_handle.lock()?;
                                           if cc.terminating() {
                                               return Ok(())
                                           }
                                           cc.notify_error(error)
                                        })
            .map_err(|err| {
                error!("Notify Error Future failed: {}", err);
                // Nothing else we can do here.
            });
        debug!("fsm:notify_client(): spawning notify task, notify error");
        self.notify_spawn(notify_error_future);
        Ok(())
    }

    fn handle_call_timeout(&mut self,
                           cc_handle: CallConnectionHandle<T>,
                           state:     CallState,
                           call_id:   CallId) -> Result<()> {

        if cc_handle.get_call_id()? != call_id {
            warn!("Call timeout received for non-active call");
            return Ok(());
        }
        match state {
            CallState::CallConnected => {}, // Ok
            _ => self.notify_client(cc_handle, ClientEvent::CallTimeout),
        };
        Ok(())
    }

    fn handle_on_add_stream(&mut self,
                            cc_handle: CallConnectionHandle<T>,
                            state:     CallState,
                            stream:    MediaStream) -> Result<()> {

        match state {
            CallState::IceConnecting(_) |
            CallState::IceConnected     |
            CallState::CallConnected
                => {
                    let mut error_handle = cc_handle.clone();
                    let notify_future    = lazy(move ||
                                             {
                                                 let mut cc = cc_handle.lock()?;
                                                 if cc.terminating() {
                                                     return Ok(())
                                                 }
                                                 cc.notify_on_add_stream(stream)
                                             })
                        .map_err(move |err| {
                            error!("Notify On Media Stream Future failed: {}", err);
                            let _ = error_handle.inject_client_error(err);
                        });
                    debug!("handle_on_add_stream(): spawning notify task");
                    self.notify_spawn(notify_future);
                },
            _ => self.unexpected_state(state, "OnAddStream"),
        }
        Ok(())
    }

    fn handle_on_data_channel(&mut self,
                              cc_handle:    CallConnectionHandle<T>,
                              state:        CallState,
                              data_channel: DataChannel) -> Result<()> {

        match state {
            CallState::IceConnected     |
            CallState::CallConnected
                => {
                    let dc_observer_handle = cc_handle.clone();
                    let notify_handle = cc_handle.clone();
                    let mut cc = cc_handle.lock()?;
                    assert_eq!(CallDirection::InComing, cc.get_direction());
                    cc.on_data_channel(data_channel, dc_observer_handle)?;
                    self.notify_client(notify_handle, ClientEvent::Ringing);
                },
            _ => self.unexpected_state(state, "OnDataChannel"),
        }
        Ok(())
    }

    fn handle_send_busy(&mut self,
                        cc_handle: CallConnectionHandle<T>,
                        state:     CallState,
                        recipient: <T as CallConnectionInterface>::ClientRecipient,
                        call_id:   CallId) -> Result<()> {

        if let CallState::Idle = state {
            self.unexpected_state(state, "SendBusy");
        } else {
            let mut error_handle = cc_handle.clone();
            let send_busy_future = lazy(move ||
                                        {
                                            let cc = cc_handle.lock()?;
                                            cc.send_busy(recipient, call_id)
                                         })
                .map_err(move |err: failure::Error| {
                    error!("SendBusyFuture failed: {}", err);
                    let _ = error_handle.inject_client_error(err);
                });

            debug!("handle_send_busy(): spawning network task");
            self.network_spawn(send_busy_future);
        }
        Ok(())
    }

    fn handle_end_call(&mut self, mut cc_handle: CallConnectionHandle<T>) -> Result<()> {
        self.terminate();

        self.drain_network_thread();
        self.drain_notify_thread();

        cc_handle.notify_terminate_complete()
    }

    fn terminate(&mut self) {
        info!("terminate: closing event stream");
        self.event_stream.close();
    }

    fn unexpected_state(&self, state: CallState, event: &str) {
        warn!("Unexpected event {}, while in state {:?}", event, state);
    }

}
