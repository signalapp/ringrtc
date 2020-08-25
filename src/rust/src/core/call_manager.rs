//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! The main Call Manager object defitions.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::stringify;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime};

use futures::future::lazy;
use futures::Future;
use tokio::runtime;

use crate::common::{
    ApplicationEvent,
    CallDirection,
    CallId,
    CallMediaType,
    CallState,
    DeviceId,
    FeatureLevel,
    Result,
    RingBench,
};
use crate::core::call::Call;
use crate::core::call_mutex::CallMutex;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::Platform;
use crate::core::signaling;
use crate::error::RingRtcError;

use crate::webrtc::media::MediaStream;

const TIME_OUT_PERIOD_SEC: u64 = 120;
pub const MAX_MESSAGE_AGE_SEC: u64 = 120;

/// Spawns a task on the worker runtime thread to handle an API
/// request with error handling.
///
/// If the future fails:
/// - log the failure
/// - conclude the call with EndedInternalFailure
///
macro_rules! handle_active_call_api {
    (
        $s:ident,
        $f:expr
            $( , $a:expr)*
    ) => {{
        info!("API:{}():", stringify!($f));
        let mut call_manager = $s.clone();
        let mut cm_error = $s.clone();
        let future = lazy(move || $f(&mut call_manager $( , $a)*)).map_err(move |err| {
            error!("Future {} failed: {}", stringify!($f), err);
            let _ = cm_error.internal_api_error( err);
        });
        $s.worker_spawn(future)
    }};
}

macro_rules! check_active_call {
    (
        $s:ident,
        $f:expr
    ) => {
        match $s.active_call() {
            Ok(v) => {
                info!("{}(): call_id: {}", $f, v.call_id());
                v
            }
            _ => {
                ringbenchx!(RingBench::CM, RingBench::App, "inactive");
                return Ok(());
            }
        }
    };
}

/// Spawns a task on the worker runtime thread to handle an API
/// request with no error handling.
///
/// If the future fails:
/// - log the failure
///
macro_rules! handle_api {
    (
        $s:ident,
        $f:expr
            $( , $a:expr)*
    ) => {{
        let mut call_manager = $s.clone();
        info!("API:{}():", stringify!($f));
        let future = lazy(move || $f(&mut call_manager $( , $a)*)).map_err(move |err| {
            error!("Future {} failed: {}", stringify!($f), err);
        });
        $s.worker_spawn(future)
    }};
}

/// A structure to hold messages in the message_queue, identified by their CallId.
pub struct SignalingMessageItem<T>
where
    T: Platform,
{
    /// The CallId of the Call that the message belongs to.
    call_id:         CallId,
    /// The type of message the item corresponds to.
    message_type:    signaling::MessageType,
    /// The closure to be called which will send the message. Returns Ok(true)
    /// if messages should be considered to be in-flight otherwise it can be
    /// assumed that messages weren't sent.
    message_closure: Box<dyn FnOnce(&CallManager<T>) -> Result<bool> + Send>,
}

/// A structure implementing a message queue used to control the
/// timing of sending Signaling messages. This helps ensure that
/// messages are sent with the same cadence that they can actually
/// be placed on-the-wire.
pub struct SignalingMessageQueue<T>
where
    T: Platform,
{
    /// The message queue.
    queue:                  VecDeque<SignalingMessageItem<T>>,
    /// The type of the last message sent from the message queue.
    last_sent_message_type: Option<signaling::MessageType>,
    /// Whether or not a message is still being handled by the
    /// application (true if a message is currently in the process
    /// of being sent). We will only send one at a time to the
    /// application.
    messages_in_flight:     bool,
}

impl<T> SignalingMessageQueue<T>
where
    T: Platform,
{
    /// Create a new SignalingMessageQueue.
    pub fn new() -> Result<Self> {
        Ok(Self {
            queue:                  VecDeque::new(),
            last_sent_message_type: None,
            messages_in_flight:     false,
        })
    }
}

pub struct CallManager<T>
where
    T: Platform,
{
    /// Interface to platform specific methods.
    platform:       Arc<CallMutex<T>>,
    /// Map of all calls, indexed by CallId.
    call_map:       Arc<CallMutex<HashMap<CallId, Call<T>>>>,
    /// CallId of the active call.
    active_call_id: Arc<CallMutex<Option<CallId>>>,
    /// Tokio runtime for back ground task execution.
    worker_runtime: Arc<CallMutex<Option<runtime::Runtime>>>,
    /// Signaling message queue.
    message_queue:  Arc<CallMutex<SignalingMessageQueue<T>>>,
}

impl<T> fmt::Display for CallManager<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let platform = match self.platform.lock() {
            Ok(v) => format!("{}", v),
            Err(_) => "unavailable".to_string(),
        };
        let active_call_id = match self.active_call_id.lock() {
            Ok(v) => format!("{:?}", v),
            Err(_) => "unavailable".to_string(),
        };
        write!(
            f,
            "thread: {:?}, platform: ({}), active_call_id: ({})",
            thread::current().id(),
            platform,
            active_call_id
        )
    }
}

impl<T> fmt::Debug for CallManager<T>
where
    T: Platform,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Drop for CallManager<T>
where
    T: Platform,
{
    fn drop(&mut self) {
        if self.ref_count() == 1 {
            info!("CallManager: Dropping last reference.");
        } else {
            debug!("Dropping CallManager: ref_count: {}", self.ref_count());
        }
    }
}

impl<T> Clone for CallManager<T>
where
    T: Platform,
{
    fn clone(&self) -> Self {
        Self {
            platform:       Arc::clone(&self.platform),
            call_map:       Arc::clone(&self.call_map),
            active_call_id: Arc::clone(&self.active_call_id),
            worker_runtime: Arc::clone(&self.worker_runtime),
            message_queue:  Arc::clone(&self.message_queue),
        }
    }
}

impl<T> CallManager<T>
where
    T: Platform,
{
    ////////////////////////////////////////////////////////////////////////
    // Public API (outside of this module) functions start here. These
    // functions are called by the application and need to be either
    // a) fast or b) asynchronous.
    ////////////////////////////////////////////////////////////////////////

    /// Create a new CallManager.
    pub fn new(platform: T) -> Result<Self> {
        info!(
            "RingRTC v{}",
            option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")
        );

        Ok(Self {
            platform:       Arc::new(CallMutex::new(platform, "platform")),
            call_map:       Arc::new(CallMutex::new(HashMap::new(), "hash_map")),
            active_call_id: Arc::new(CallMutex::new(None, "active_call_id")),
            worker_runtime: Arc::new(CallMutex::new(
                Some(
                    runtime::Builder::new()
                        .core_threads(1)
                        .name_prefix("worker-")
                        .build()?,
                ),
                "worker_runtime",
            )),
            message_queue:  Arc::new(CallMutex::new(
                SignalingMessageQueue::new()?,
                "message_queue",
            )),
        })
    }

    /// Create an outgoing call.
    pub fn call(
        &mut self,
        remote_peer: <T as Platform>::AppRemotePeer,
        call_media_type: CallMediaType,
        local_device_id: DeviceId,
    ) -> Result<()> {
        info!("API:call():");
        let call_id = CallId::random();
        self.create_outgoing_call(remote_peer, call_id, call_media_type, local_device_id)
    }

    /// Create an outgoing call with specified CallId.
    pub fn create_outgoing_call(
        &mut self,
        remote_peer: <T as Platform>::AppRemotePeer,
        call_id: CallId,
        call_media_type: CallMediaType,
        local_device_id: DeviceId,
    ) -> Result<()> {
        info!("API:create_outgoing_call({}):", call_id);

        let mut call_manager = self.clone();
        let mut cm_error = self.clone();
        let remote_peer_error = remote_peer.clone();
        let future = lazy(move || {
            call_manager.handle_call(remote_peer, call_id, call_media_type, local_device_id)
        })
        .map_err(move |err| {
            error!("Handle call failed: {}", err);
            cm_error.internal_create_api_error(&remote_peer_error, call_id, err);
        });
        self.worker_spawn(future)
    }

    /// Accept an incoming call.
    pub fn accept_call(&mut self, call_id: CallId) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_accept_call, call_id)
    }

    /// Drop the active call.
    pub fn drop_call(&mut self, call_id: CallId) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_drop_call, call_id)
    }

    /// Proceed with the outgoing call.
    pub fn proceed(
        &mut self,
        call_id: CallId,
        app_call_context: <T as Platform>::AppCallContext,
    ) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_proceed, call_id, app_call_context)
    }

    /// OK for the library to continue to send signaling messages.
    pub fn message_sent(&mut self, call_id: CallId) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_message_sent, call_id)
    }

    /// The previous message send failed. Handle, but continue to send signaling messages.
    pub fn message_send_failure(&mut self, call_id: CallId) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_message_send_failure, call_id)
    }

    /// Local hangup of the active call.
    pub fn hangup(&mut self) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_hangup)
    }

    /// Received offer from application.
    pub fn received_offer(
        &mut self,
        remote_peer: <T as Platform>::AppRemotePeer,
        call_id: CallId,
        received: signaling::ReceivedOffer,
    ) -> Result<()> {
        info!("API:received_offer():");

        let mut call_manager = self.clone();
        let mut cm_error = self.clone();
        let remote_peer_error = remote_peer.clone();
        let future =
            lazy(move || call_manager.handle_received_offer(remote_peer, call_id, received))
                .map_err(move |err| {
                    error!("Handle received offer failed: {}", err);
                    cm_error.internal_create_api_error(&remote_peer_error, call_id, err);
                });
        self.worker_spawn(future)
    }

    /// Received answer from application.
    pub fn received_answer(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedAnswer,
    ) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_received_answer, call_id, received)
    }

    /// Received ICE candidates from application.
    pub fn received_ice(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedIce,
    ) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_received_ice, call_id, received)
    }

    /// Received hangup message from application.
    pub fn received_hangup(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedHangup,
    ) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_received_hangup, call_id, received)
    }

    /// Received busy message from application.
    pub fn received_busy(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedBusy,
    ) -> Result<()> {
        handle_active_call_api!(self, CallManager::handle_received_busy, call_id, received)
    }

    /// Request to reset the Call Manager.
    ///
    /// Conclude all calls and clear active callId.  Do not notify the
    /// application at the conclusion.
    pub fn reset(&mut self) -> Result<()> {
        handle_api!(self, CallManager::handle_reset)
    }

    /// Close down the Call Manager.
    ///
    /// Close down the call manager and all the calls it is currently managing.
    ///
    /// This is a blocking call.
    #[allow(clippy::mutex_atomic)]
    pub fn close(&mut self) -> Result<()> {
        info!("close():");

        if self.worker_runtime.lock()?.is_some() {
            // Clear out any outstanding calls
            let _ = self.reset();

            self.sync_runtime()?;

            // close the runtime
            let _ = self.close_runtime();
            info!("close(): complete");
        } else {
            info!("close(): already closed.");
        }

        Ok(())
    }

    /// Returns the active Call
    pub fn active_call(&self) -> Result<Call<T>> {
        let active_call_id = self.active_call_id.lock()?;
        match *active_call_id {
            Some(call_id) => {
                let call_map = self.call_map.lock()?;
                match call_map.get(&call_id) {
                    Some(call) => Ok(call.clone()),
                    None => Err(RingRtcError::CallIdNotFound(call_id).into()),
                }
            }
            None => Err(RingRtcError::NoActiveCall.into()),
        }
    }

    /// Return active connection object.
    pub fn active_connection(&self) -> Result<Connection<T>> {
        info!("active_connection():");
        let active_call = self.active_call()?;
        active_call.active_connection()
    }

    /// Checks if a call is active.
    pub fn call_active(&self) -> Result<bool> {
        Ok(self.active_call_id.lock()?.is_some())
    }

    /// Check if call_id refers to the active call.
    pub fn call_is_active(&self, call_id: CallId) -> Result<bool> {
        let active_call_id = self.active_call_id.lock()?;
        match *active_call_id {
            Some(v) => Ok(v == call_id),
            None => Ok(false),
        }
    }

    /// Return the platform, under a locked mutex.
    pub fn platform(&self) -> Result<MutexGuard<'_, T>> {
        self.platform.lock()
    }

    /// Synchronize the call manager and all call FSMs.
    ///
    /// Blocks the caller while the call manager and call FSM event
    /// queues are flushed.
    ///
    /// `Called By:` Test infrastructure
    #[cfg(feature = "sim")]
    pub fn synchronize(&mut self) -> Result<()> {
        info!("synchronize():");

        self.sync_runtime()?;

        // sync twice, as simulated error injection can put more
        // events on the FSMs.
        for i in 0..2 {
            info!("synchronize(): pass: {}", i);
            let mut map_clone = self.call_map.lock()?.clone();
            for (_, call) in map_clone.iter_mut() {
                info!("synchronize(): syncing call: {}", call.call_id());
                call.synchronize()?;
            }

            self.sync_runtime()?;
        }

        info!("synchronize(): complete");
        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////
    // Private internal functions start here
    ////////////////////////////////////////////////////////////////////////

    /// Return the strong reference count on the platform.
    fn ref_count(&self) -> usize {
        Arc::strong_count(&self.platform)
    }

    /// Spawn a future on the worker runtime if enabled.
    fn worker_spawn<F>(&mut self, future: F) -> Result<()>
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        let mut worker_runtime = self.worker_runtime.lock()?;
        if let Some(worker_runtime) = &mut *worker_runtime {
            worker_runtime.spawn(future);
        } else {
            warn!("worker_spawn(): worker_runtime unavailable");
        }
        Ok(())
    }

    fn runtime_start_sync(&mut self, sync_condvar: Arc<(Mutex<bool>, Condvar)>) -> Result<()> {
        let future = lazy(move || {
            // signal the condvar
            info!("sync_runtime(): syncing runtime");
            let (mutex, condvar) = &*sync_condvar;
            if let Ok(mut terminate_complete) = mutex.lock() {
                *terminate_complete = true;
                condvar.notify_one();
                Ok(())
            } else {
                Err(RingRtcError::MutexPoisoned(
                    "Call Manager Close Condition Variable".to_string(),
                )
                .into())
            }
        })
        .map_err(move |err: failure::Error| {
            error!("Close call manager future failed: {}", err);
            // Not much else to do here.
        });
        self.worker_spawn(future)
    }

    fn wait_runtime_sync(&self, sync_condvar: Arc<(Mutex<bool>, Condvar)>) -> Result<()> {
        info!("wait_runtime_sync(): waiting for runtime sync...");

        let (mutex, condvar) = &*sync_condvar;
        if let Ok(mut terminate_complete) = mutex.lock() {
            while !*terminate_complete {
                terminate_complete = condvar.wait(terminate_complete).map_err(|_| {
                    RingRtcError::MutexPoisoned("Call Manager Close Condition Variable".to_string())
                })?;
            }
        } else {
            return Err(RingRtcError::MutexPoisoned(
                "Call Manager Close Condition Variable".to_string(),
            )
            .into());
        }

        info!("wait_runtime_sync(): runtime synchronized.");

        Ok(())
    }

    #[allow(clippy::mutex_atomic)]
    fn sync_runtime(&mut self) -> Result<()> {
        // cycle a condvar through the runtime
        let condvar = Arc::new((Mutex::new(false), Condvar::new()));
        self.runtime_start_sync(condvar.clone())?;

        // This blocks while the runtime synchronizes.
        self.wait_runtime_sync(condvar)
    }

    fn close_runtime(&mut self) -> Result<()> {
        info!("stopping worker runtime");

        let result: Option<runtime::Runtime> = {
            let mut worker_runtime = self.worker_runtime.lock()?;
            worker_runtime.take()
        };

        if let Some(worker_runtime) = result {
            let _ = worker_runtime
                .shutdown_now()
                .wait()
                .map_err(|_| warn!("Problems shutting down the worker runtime"));
        } else {
            error!("close_runtime(): worker_runtime is unavailable");
        }

        info!("stopping worker runtime: complete");
        Ok(())
    }

    /// Clears the active call_id
    fn clear_active_call(&mut self) -> Result<()> {
        let _ = self.active_call_id.lock()?.take();
        Ok(())
    }

    /// Terminates Call and optionally notifies application of the reason why.
    /// Also removes/drops it from the map.
    fn terminate_and_drop_call(&mut self, call_id: CallId) -> Result<()> {
        info!("terminate_call(): call_id: {}", call_id);

        let mut call = match self.call_map.lock()?.remove(&call_id) {
            Some(v) => v,
            None => return Err(RingRtcError::CallIdNotFound(call_id).into()),
        };

        // blocks while call FSM terminates
        call.terminate()
    }

    /// Sends a hangup message to a remote_peer via the application.
    pub(super) fn send_hangup(
        &mut self,
        call: Call<T>,
        call_id: CallId,
        send: signaling::SendHangup,
    ) -> Result<()> {
        info!("send_hangup(): call_id: {}", call_id);

        let hangup_closure = Box::new(move |cm: &CallManager<T>| {
            ringbench!(
                RingBench::CM,
                RingBench::App,
                format!("send_hangup({:?})\t{}", send.hangup, call_id)
            );

            let remote_peer = call.remote_peer()?;

            let platform = cm.platform.lock()?;
            platform.on_send_hangup(&*remote_peer, call_id, send)?;

            Ok(true)
        });

        let message_item = SignalingMessageItem {
            call_id,
            message_type: signaling::MessageType::Hangup,
            message_closure: hangup_closure,
        };

        self.send_next_message(Some(message_item))
    }

    /// Concludes the specified Call.
    ///
    /// Conclusion includes:
    /// - Trimming the message_queue, before possibly sending hangup message(s)
    /// - [optional] notifying application about call ended reason
    /// - closing down Call object
    /// - [optional] sending hangup on all connection data channels
    /// - [optional] sending Signal hangup message
    fn terminate_call(
        &mut self,
        mut call: Call<T>,
        hangup: Option<signaling::Hangup>,
        event: Option<ApplicationEvent>,
    ) -> Result<()> {
        let call_id = call.call_id();

        info!("conclude_call(): call_id: {}", call_id);

        self.trim_messages(call_id)?;

        if let Some(event) = event {
            let remote_peer = call.remote_peer()?;
            self.notify_application(&*remote_peer, event)?;
        }

        if let Some(hangup) = hangup {
            // All connections send hangup via data_channel.
            call.inject_send_hangup_via_data_channel_to_all(hangup)?;
        }

        let mut call_manager = self.clone();
        let cm_error = self.clone();
        let call_error = call.clone();
        let call_clone = call.clone();
        let future = lazy(move || {
            if let Some(hangup) = hangup {
                // If we want to send a hangup message, be sure that
                // the call actually should send one.
                if call.should_send_hangup() {
                    // Send hangup via signaling channel.
                    // Use legacy hangup signaling by default.
                    call_manager.send_hangup(
                        call_clone,
                        call_id,
                        signaling::SendHangup {
                            hangup,
                            use_legacy: true,
                        },
                    )?;
                }
            }
            call_manager.terminate_and_drop_call(call_id)
        })
        .map_err(move |err| {
            error!("Conclude call future failed: {}", err);
            if let Ok(remote_peer) = call_error.remote_peer() {
                let _ = cm_error
                    .notify_application(&*remote_peer, ApplicationEvent::EndedInternalFailure);
            }
        });
        self.worker_spawn(future)
    }

    /// Terminates the active call.
    fn terminate_active_call(&mut self, send_hangup: bool, event: ApplicationEvent) -> Result<()> {
        info!("terminate_active_call():");

        if !self.call_active()? {
            info!("terminate_active_call(): skipping, no active call");
            return Ok(());
        }

        let call = self.active_call()?;
        self.clear_active_call()?;

        let hangup = if send_hangup {
            Some(signaling::Hangup::Normal)
        } else {
            None
        };

        self.terminate_call(call, hangup, Some(event))
    }

    /// Handle call() API from application.
    fn handle_call(
        &mut self,
        remote_peer: <T as Platform>::AppRemotePeer,
        call_id: CallId,
        call_media_type: CallMediaType,
        local_device_id: DeviceId,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!("call()\t{}", call_id)
        );

        // if no active call, create a new call
        let mut active_call_id = self.active_call_id.lock()?;
        match *active_call_id {
            Some(v) => Err(RingRtcError::CallAlreadyInProgress(v).into()),
            None => {
                let mut call = Call::new(
                    remote_peer,
                    call_id,
                    CallDirection::OutGoing,
                    call_media_type,
                    local_device_id,
                    TIME_OUT_PERIOD_SEC,
                    self.clone(),
                )?;

                // Whenever there is a new call, ensure that messages can flow.
                self.reset_messages_in_flight()?;

                let mut call_map = self.call_map.lock()?;
                call_map.insert(call_id, call.clone());

                *active_call_id = Some(call_id);
                call.inject_start_call()
            }
        }
    }

    /// Handle accept_call() API from application.
    fn handle_accept_call(&mut self, call_id: CallId) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!("accept()\t{}", call_id)
        );

        let mut active_call = check_active_call!(self, "handle_accept_call");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::CM, RingBench::App, "inactive call_id");
            return Ok(());
        }

        active_call.inject_accept_call()
    }

    fn handle_terminate_active_call(
        &mut self,
        active_call: Call<T>,
        hangup: Option<signaling::Hangup>,
        event: ApplicationEvent,
    ) -> Result<()> {
        self.clear_active_call()?;
        self.terminate_call(active_call, hangup, Some(event))
    }

    /// Handle drop_call() API from application.
    fn handle_drop_call(&mut self, call_id: CallId) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!("drop()\t{}", call_id)
        );

        let active_call = check_active_call!(self, "handle_drop_call");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::CM, RingBench::App, "inactive call_id");
            return Ok(());
        }

        self.handle_terminate_active_call(active_call, None, ApplicationEvent::EndedAppDroppedCall)
    }

    /// Handle proceed() API from application.
    fn handle_proceed(
        &mut self,
        call_id: CallId,
        app_call_context: <T as Platform>::AppCallContext,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!("proceed()\t{}", call_id)
        );

        let mut active_call = check_active_call!(self, "handle_proceed");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::CM, RingBench::App, "inactive call_id");
            return Ok(());
        }

        active_call.set_call_context(app_call_context)?;
        active_call.inject_proceed()
    }

    /// Handle message_sent() API from application.
    fn handle_message_sent(&mut self, call_id: CallId) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!("message_sent()\t{}", call_id)
        );

        self.reset_messages_in_flight()?;
        self.send_next_message(None)
    }

    /// Handle message_send_failure() API from application.
    fn handle_message_send_failure(&mut self, call_id: CallId) -> Result<()> {
        // Get the last sent message type and see if it was for Ice.
        let mut last_sent_message_ice = false;
        if let Ok(message_queue) = self.message_queue.lock() {
            if message_queue.last_sent_message_type == Some(signaling::MessageType::Ice) {
                last_sent_message_ice = true
            }
        }

        let mut handle_active_call = false;
        if let Ok(active_call) = self.active_call() {
            if active_call.call_id() == call_id {
                handle_active_call = true;
                if let Ok(state) = active_call.state() {
                    match state {
                        CallState::ConnectedWithDataChannelBeforeAccepted
                        | CallState::ConnectedAndAccepted
                        | CallState::ReconnectingAfterAccepted => {
                            // We are in some connected state, ignore if the failed message
                            // was an Ice message.
                            if last_sent_message_ice {
                                handle_active_call = false;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if handle_active_call {
            info!(
                "handle_message_send_failure(): id: {}, concluding active call",
                call_id
            );

            let _ = self.terminate_active_call(true, ApplicationEvent::EndedSignalingFailure);
        } else {
            // See if the associated call is in the call map.
            let mut call = None;
            {
                if let Ok(call_map) = self.call_map.lock() {
                    if let Some(v) = call_map.get(&call_id) {
                        call = Some(v.clone());
                    };
                }
            }

            match call {
                Some(call) => {
                    info!(
                        "handle_message_send_failure(): id: {}, concluding call",
                        call_id
                    );
                    self.terminate_call(
                        call,
                        Some(signaling::Hangup::Normal),
                        Some(ApplicationEvent::EndedSignalingFailure),
                    )?;
                }
                None => {
                    info!("handle_message_send_failure(): no matching call found");
                }
            }
        }

        match self.message_queue.lock() {
            Ok(mut message_queue) => {
                message_queue.messages_in_flight = false;
            }
            Err(e) => {
                error!("Could not lock the message queue: {}", e);
                return Err(e);
            }
        }

        self.send_next_message(None)
    }

    /// Handle hangup() API from application.
    fn handle_hangup(&mut self) -> Result<()> {
        ringbench!(RingBench::App, RingBench::CM, "hangup()");

        let active_call = check_active_call!(self, "handle_hangup");

        self.handle_terminate_active_call(
            active_call,
            Some(signaling::Hangup::Normal),
            ApplicationEvent::EndedLocalHangup,
        )
    }

    /// Handle received_offer() API from application.
    fn handle_received_offer(
        &mut self,
        remote_peer: <T as Platform>::AppRemotePeer,
        call_id: CallId,
        received: signaling::ReceivedOffer,
    ) -> Result<()> {
        let sender_device_id = received.sender_device_id;
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!(
                "received_offer()\t{}\t{}\tfeature={}\tprimary={}\t{}",
                call_id,
                received.sender_device_id,
                received.sender_device_feature_level,
                received.receiver_device_is_primary,
                received.offer.to_info_string(),
            )
        );

        if received.age > Duration::from_secs(MAX_MESSAGE_AGE_SEC) {
            ringbenchx!(RingBench::CM, RingBench::App, "offer expired");
            self.notify_application(&remote_peer, ApplicationEvent::EndedReceivedOfferExpired)?;
            // Notify application we are completely done with this remote.
            self.notify_call_concluded(&remote_peer, call_id)?;
            return Ok(());
        }

        if (received.sender_device_feature_level == FeatureLevel::Unspecified)
            && !received.receiver_device_is_primary
        {
            ringbenchx!(
                RingBench::CM,
                RingBench::App,
                "offer not supported on linked device"
            );
            self.notify_application(
                &remote_peer,
                ApplicationEvent::EndedIgnoreCallsFromNonMultiringCallers,
            )?;
            // Notify application we are completely done with this remote.
            self.notify_call_concluded(&remote_peer, call_id)?;
            return Ok(());
        }

        if self.call_active()? {
            // Make a call object to ensure that the busy message can be sent
            // in the future. It does not go into the call map and should not
            // start a timeout timer.
            let call = Call::new(
                remote_peer.clone(),
                call_id,
                CallDirection::InComing,
                received.offer.call_media_type,
                received.receiver_device_id,
                0,
                self.clone(),
            )?;

            self.notify_application(
                &remote_peer,
                ApplicationEvent::EndedReceivedOfferWhileActive,
            )?;

            self.check_for_glare(&remote_peer, sender_device_id)?;

            // Send busy out after checking for glare to ensure that the call
            // object and the application's remote are still valid during the
            // glare check (and not concluded).
            self.send_busy(call)?;
            return Ok(());
        }

        let mut active_call_id = self.active_call_id.lock()?;
        match *active_call_id {
            Some(v) => Err(RingRtcError::CallAlreadyInProgress(v).into()),
            None => {
                let mut call = Call::new(
                    remote_peer,
                    call_id,
                    CallDirection::InComing,
                    received.offer.call_media_type,
                    received.receiver_device_id,
                    TIME_OUT_PERIOD_SEC,
                    self.clone(),
                )?;

                // Whenever there is a new call, ensure that messages can flow.
                self.reset_messages_in_flight()?;

                let mut call_map = self.call_map.lock()?;
                call_map.insert(call_id, call.clone());

                *active_call_id = Some(call_id);
                call.handle_received_offer(received)?;
                call.inject_start_call()
            }
        }
    }

    /// Handle received_answer() API from application.
    fn handle_received_answer(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedAnswer,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!(
                "received_answer()\t{}\t{}\t{}",
                call_id,
                received.sender_device_id,
                received.answer.to_info_string(),
            )
        );

        let mut active_call = check_active_call!(self, "handle_received_answer");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::CM, RingBench::App, "inactive call_id");
            return Ok(());
        }

        active_call.inject_received_answer(received)
    }

    /// Handle received_ice() API from application.
    fn handle_received_ice(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedIce,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!(
                "received_ice_candidates({})\t{}\t{}",
                received.ice.candidates_added.len(),
                call_id,
                received.sender_device_id,
            )
        );

        let mut active_call = check_active_call!(self, "handle_received_ice");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::CM, RingBench::App, "inactive call_id");
            return Ok(());
        }

        active_call.inject_received_ice(received)
    }

    /// Handle received_hangup() API from application.
    fn handle_received_hangup(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedHangup,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!(
                "received_hangup({})\t{}\t{}",
                received.hangup, call_id, received.sender_device_id
            )
        );

        let mut active_call = check_active_call!(self, "handle_received_hangup");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::CM, RingBench::App, "inactive call_id");
            return Ok(());
        }

        active_call.inject_received_hangup(received)
    }

    /// Handle received_busy() API from application.
    fn handle_received_busy(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedBusy,
    ) -> Result<()> {
        let sender_device_id = received.sender_device_id;
        ringbench!(
            RingBench::App,
            RingBench::CM,
            format!("received_busy()\t{}\t{}", call_id, sender_device_id)
        );

        let active_call = check_active_call!(self, "handle_received_busy");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::CM, RingBench::App, "inactive call_id");
            return Ok(());
        }

        // Invoke hangup_other for the call, which will inject hangup/busy
        // to all connections, if any.
        let hangup = signaling::Hangup::BusyOnAnotherDevice(sender_device_id);
        active_call.send_hangup_via_data_channel_to_all_except(hangup, sender_device_id)?;

        // Send out hangup/busy to all callees via signal messaging.
        // Use legacy signaling since the busy device, legacy or
        // otherwise, should ignore the message.
        let mut call_manager = active_call.call_manager()?;
        call_manager.send_hangup(
            active_call.clone(),
            active_call.call_id(),
            signaling::SendHangup {
                hangup,
                use_legacy: true,
            },
        )?;

        // Handle the normal processing of busy by concluding the call locally.
        self.handle_terminate_active_call(
            active_call.clone(),
            None,
            ApplicationEvent::EndedRemoteBusy,
        )
    }

    /// Handle reset() API from application.
    ///
    /// Terminate all calls and clear active callId.  Do not notify the
    /// application at the conclusion.
    fn handle_reset(&mut self) -> Result<()> {
        info!("handle_reset():");

        // gather all the calls from the call_map.
        let calls: Vec<Call<T>> = {
            let call_map = self.call_map.lock()?;
            call_map.values().cloned().collect()
        };

        // foreach call, termiante without notifying application
        for call in calls {
            info!("reset(): termianting call_id: {}", call.call_id());
            let _ = self.terminate_call(call, Some(signaling::Hangup::Normal), None);
        }

        self.clear_active_call()?;

        // clear out the message queue, the app gave up on everything
        let mut message_queue = self.message_queue.lock()?;
        message_queue.queue.clear();
        message_queue.messages_in_flight = false;

        info!("reset(): complete");
        Ok(())
    }

    fn send_busy(&mut self, call: Call<T>) -> Result<()> {
        let call_id = call.call_id();
        info!("send_busy(): call_id: {}", call_id);

        let busy_closure = Box::new(move |cm: &CallManager<T>| {
            ringbench!(
                RingBench::CM,
                RingBench::App,
                format!("send_busy()\t{}", call_id)
            );

            let remote_peer = call.remote_peer()?;

            let platform = cm.platform.lock()?;
            platform.on_send_busy(&*remote_peer, call_id)?;

            Ok(true)
        });

        let message_item = SignalingMessageItem {
            call_id,
            message_type: signaling::MessageType::Busy,
            message_closure: busy_closure,
        };

        self.send_next_message(Some(message_item))
    }

    /// If the remote peer of the active call equals the remote peer
    /// of an incoming offer, then we might have a glare situation.
    ///
    /// - If there is no active device id, this is glare since the
    ///   peers are calling each other at the same time and still in
    ///   the session setup, including the ringing state.
    /// - If there is an active device id and it equals the device id
    ///   of the incoming offer, this is an invalid state and will
    ///   be treated as glare (two devices can't be in more than one
    ///   call with one-another at the same time).
    /// - If there is an active device id and it is different than the
    ///   device id of the incoming offer, this is a valid state and
    ///   will be allowed. In this case, the caller might be calling
    ///   from one of their other devices. The incoming call will get
    ///   a busy but here we ensure that the active call isn't ended.
    ///
    /// If glare is detected, the active call will be concluded. It is
    /// assumed that a busy message would have already been sent to
    /// reject the incoming offer.
    fn check_for_glare(
        &mut self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        remote_device_id: DeviceId,
    ) -> Result<()> {
        if let Ok(active_call) = self.active_call() {
            info!("check_for_glare(): active call detected");
            if self.remote_peer_equals_active(&active_call, remote_peer) {
                info!("check_for_glare(): remote peers match");
                if let Ok(active_device_id) = active_call.active_device_id() {
                    info!("check_for_glare(): active device exists");
                    if remote_device_id == active_device_id {
                        info!("check_for_glare(): peer device matches, hangup active call");
                        return self.handle_terminate_active_call(
                            active_call,
                            Some(signaling::Hangup::Normal),
                            ApplicationEvent::EndedRemoteGlare,
                        );
                    }
                } else {
                    info!("check_for_glare(): no active device, hangup active call");
                    return self.handle_terminate_active_call(
                        active_call,
                        Some(signaling::Hangup::Normal),
                        ApplicationEvent::EndedRemoteGlare,
                    );
                }
            }
        }
        Ok(())
    }

    /// Check if the remote_peer matches the remote_peer in the active
    /// call.
    fn remote_peer_equals_active(
        &self,
        active_call: &Call<T>,
        remote_peer: &<T as Platform>::AppRemotePeer,
    ) -> bool {
        if let Ok(active_remote_peer) = active_call.remote_peer() {
            if let Ok(platform) = self.platform.lock() {
                if let Ok(result) = platform.compare_remotes(&active_remote_peer, remote_peer) {
                    return result;
                }
            }
        }
        false
    }

    /// Internal failure during API future that creates a call.
    ///
    /// The APIs call() and received_offer() use this error handler as
    /// it is unknown exactly where the API failed, before or after
    /// creating an active call.
    fn internal_create_api_error(
        &mut self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        call_id: CallId,
        error: failure::Error,
    ) {
        info!("internal_create_api_error(): error: {}", error);
        if let Ok(active_call) = self.active_call() {
            if self.remote_peer_equals_active(&active_call, remote_peer) {
                // The future managed to create the active call and then
                // hit problems.  Error out with active call clean up.
                let _ = self.internal_api_error(error);
                return;
            }
        }

        // The future hit problems before creating or accessing
        // an active call. Simply notify the application with no
        // call clean up.
        let _ = self.notify_application(remote_peer, ApplicationEvent::EndedInternalFailure);
        let _ = self.notify_call_concluded(remote_peer, call_id);
    }

    /// Internal error occured on an API future.
    ///
    /// This shuts down the specified call if active and notifies the
    /// application.
    fn internal_api_error(&mut self, error: failure::Error) -> Result<()> {
        info!("internal_api_error(): error: {}", error);
        if let Ok(call) = self.active_call() {
            self.internal_error(call.call_id(), error)
        } else {
            info!("internal_api_error(): ignoring for inactive call");
            Ok(())
        }
    }

    fn reset_messages_in_flight(&self) -> Result<()> {
        match self.message_queue.lock() {
            Ok(mut message_queue) => {
                message_queue.messages_in_flight = false;
                Ok(())
            }
            Err(e) => {
                error!("Could not lock the message queue: {}", e);
                Err(e)
            }
        }
    }

    /// Push the given message and send the next message in
    /// the queue if no other message is currently in the
    /// process of being sent (in flight).
    fn send_next_message(
        &mut self,
        message_item_option: Option<SignalingMessageItem<T>>,
    ) -> Result<()> {
        info!("send_next_message():");

        // Push the optional message we got to the queue.
        if let Some(message_item) = message_item_option {
            match self.message_queue.lock() {
                Ok(mut message_queue) => {
                    message_queue.queue.push_back(message_item);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        let assume_messages_sent = {
            let platform = self.platform.lock()?;
            platform.assume_messages_sent()
        };

        // Loop in case a sending error is encountered and jump to the next
        // message item if so.
        loop {
            let mut closure_error: Option<(failure::Error, CallId)> = None;

            match self.message_queue.lock() {
                Ok(mut message_queue) => {
                    if message_queue.messages_in_flight {
                        info!("send_next_message(): messages are in flight already");
                        return Ok(());
                    }

                    match message_queue.queue.pop_front() {
                        Some(message_item) => {
                            info!(
                                "send_next_message(): sending message, len: {}",
                                message_queue.queue.len()
                            );

                            // Execute the closure and match its return value.
                            match (message_item.message_closure)(self) {
                                Ok(message_is_in_flight) => {
                                    // We have attempted to deliver the message. If a message
                                    // is actually in flight, set the in flight flag. But
                                    // check to see if the platform overrides it (in which
                                    // case the platform doesn't want messages to be queued).
                                    message_queue.messages_in_flight =
                                        message_is_in_flight && !assume_messages_sent;

                                    message_queue.last_sent_message_type =
                                        Some(message_item.message_type);

                                    if message_queue.messages_in_flight {
                                        // If there are messages in flight, exit the loop and
                                        // wait for confirmation that they actually got sent.
                                        return Ok(());
                                    }
                                }
                                Err(e) => {
                                    error!("send_next_message(): closure failed {}", e);
                                    closure_error = Some((e, message_item.call_id));
                                }
                            }
                        }
                        None => {
                            info!("send_next_message(): no messages to send");
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }

            if let Some((error, call_id)) = closure_error {
                // Generate an error on the active call (if any) and
                // continue trying to send the next message.
                if let Err(e) = self.internal_error(call_id, error) {
                    error!("send_next_message(): unrecoverable {}", e);
                    return Err(e);
                }
            }
        }
    }

    /// Remove all messages in the queue by call_id. Ignore Busy
    /// messages as they might have been sent on behalf of the
    /// call before termination. Also ignore Hangup messages, since
    /// they should always be sent as backup for callees to end
    /// their side of the call.
    fn trim_messages(&self, call_id: CallId) -> Result<()> {
        let mut message_queue = self.message_queue.lock()?;
        let mq = &mut *message_queue;

        debug!(
            "trim_messages(): start id: {} len: {}",
            call_id,
            mq.queue.len()
        );
        mq.queue.retain(|x| {
            (x.call_id != call_id)
                || (x.message_type == signaling::MessageType::Busy)
                || (x.message_type == signaling::MessageType::Hangup)
        });
        debug!("trim_messages(): end len: {}", mq.queue.len());

        Ok(())
    }

    ////////////////////////////////////////////////////////////////////////
    // Module level public functions start here
    ////////////////////////////////////////////////////////////////////////

    /// Inform the application that a call should be started.
    pub(super) fn start_call(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        call_id: CallId,
        direction: CallDirection,
        call_media_type: CallMediaType,
    ) -> Result<()> {
        ringbench!(
            RingBench::CM,
            RingBench::App,
            format!("start()\t{}", call_id)
        );

        let platform = self.platform.lock()?;
        platform.on_start_call(remote_peer, call_id, direction, call_media_type)
    }

    /// Notify application of an event.
    pub(super) fn notify_application(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        event: ApplicationEvent,
    ) -> Result<()> {
        ringbench!(RingBench::CM, RingBench::App, format!("event({})", event));

        let platform = self.platform.lock()?;
        platform.on_event(remote_peer, event)
    }

    /// Create a new connection to a remote device
    pub(super) fn create_connection(
        &self,
        call: &Call<T>,
        device_id: DeviceId,
        connection_type: ConnectionType,
        signaling_version: signaling::Version,
    ) -> Result<Connection<T>> {
        let mut platform = self.platform.lock()?;
        platform.create_connection(call, device_id, connection_type, signaling_version)
    }

    /// Create a new application specific media stream
    pub(super) fn create_incoming_media(
        &self,
        connection: &Connection<T>,
        incoming_media: MediaStream,
    ) -> Result<<T as Platform>::AppIncomingMedia> {
        let platform = self.platform.lock()?;
        platform.create_incoming_media(connection, incoming_media)
    }

    /// Connect incoming media
    pub(super) fn connect_incoming_media(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        app_call_context: &<T as Platform>::AppCallContext,
        incoming_media: &<T as Platform>::AppIncomingMedia,
    ) -> Result<()> {
        let platform = self.platform.lock()?;
        platform.connect_incoming_media(remote_peer, app_call_context, incoming_media)
    }

    /// Disconnect incoming media
    pub(super) fn disconnect_incoming_media(
        &self,
        app_call_context: &<T as Platform>::AppCallContext,
    ) -> Result<()> {
        let platform = self.platform.lock()?;
        platform.disconnect_incoming_media(app_call_context)
    }

    /// Received hangup from remote for the active call.
    pub(super) fn remote_hangup(
        &mut self,
        call_id: CallId,
        app_event_override: Option<ApplicationEvent>,
    ) -> Result<()> {
        info!("remote_hangup(): call_id: {}", call_id);

        if self.call_is_active(call_id)? {
            match app_event_override {
                Some(event) => self.terminate_active_call(false, event),
                None => self.terminate_active_call(false, ApplicationEvent::EndedRemoteHangup),
            }
        } else {
            info!("remote_hangup(): ignoring for inactive call");
            Ok(())
        }
    }

    /// Notify application that the call is concluded.
    pub(super) fn notify_call_concluded(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        _call_id: CallId,
    ) -> Result<()> {
        ringbench!(
            RingBench::CM,
            RingBench::App,
            format!("call_concluded()\t{}", _call_id)
        );

        let platform = self.platform.lock()?;
        platform.on_call_concluded(remote_peer)
    }

    /// Local timeout of the active call.
    pub(super) fn timeout(&mut self, call_id: CallId) -> Result<()> {
        info!("timeout(): call_id: {}", call_id);

        if self.call_is_active(call_id)? {
            self.terminate_active_call(true, ApplicationEvent::EndedTimeout)
        } else {
            info!("timeout(): ignoring for inactive call");
            Ok(())
        }
    }

    /// Network failure occured on the active call.
    pub(super) fn connection_failure(&mut self, call_id: CallId) -> Result<()> {
        info!("call_failed(): call_id: {}", call_id);

        if self.call_is_active(call_id)? {
            self.terminate_active_call(true, ApplicationEvent::EndedConnectionFailure)
        } else {
            info!("call_failed(): ignoring for inactive call");
            Ok(())
        }
    }

    /// Internal error occured on the active call.
    ///
    /// This shuts down the specified call if active and notifies the
    /// application.
    pub(super) fn internal_error(&mut self, call_id: CallId, error: failure::Error) -> Result<()> {
        info!("internal_error(): call_id: {}, error: {}", call_id, error);

        if self.call_is_active(call_id)? {
            self.terminate_active_call(true, ApplicationEvent::EndedInternalFailure)
        } else {
            info!("internal_error(): ignoring for inactive call");
            Ok(())
        }
    }

    /// Send offer to remote_peer via the application.
    pub(super) fn send_offer(
        &mut self,
        call: Call<T>,
        connection: Connection<T>,
        offer: signaling::Offer,
    ) -> Result<()> {
        let call_id = call.call_id();
        info!("send_offer(): call_id: {}", call_id);

        let offer_closure = Box::new(move |cm: &CallManager<T>| {
            ringbench!(
                RingBench::CM,
                RingBench::App,
                format!("send_offer()\t{}\t{}", call_id, offer.to_info_string())
            );

            info!(
                "call_id: {}, TX offer:\n{}",
                call_id,
                offer.to_redacted_string()
            );

            let remote_peer = call.remote_peer()?;

            if connection.can_send_messages() {
                let platform = cm.platform.lock()?;
                platform.on_send_offer(&*remote_peer, call_id, offer)?;
                Ok(true)
            } else {
                Ok(false)
            }
        });

        let message_item = SignalingMessageItem {
            call_id,
            message_type: signaling::MessageType::Offer,
            message_closure: offer_closure,
        };

        self.send_next_message(Some(message_item))
    }

    /// Send answer to remote_peer via the application.
    pub(super) fn send_answer(
        &mut self,
        call: Call<T>,
        connection: Connection<T>,
        send: signaling::SendAnswer,
    ) -> Result<()> {
        let call_id = call.call_id();
        info!("send_answer(): call_id: {}", call_id);

        let answer_closure = Box::new(move |cm: &CallManager<T>| {
            ringbench!(
                RingBench::CM,
                RingBench::App,
                format!(
                    "send_answer()\t{}\t{}",
                    call_id,
                    send.answer.to_info_string()
                )
            );

            info!(
                "call_id: {}, TX answer:\n{}",
                call_id,
                send.answer.to_redacted_string(),
            );

            let remote_peer = call.remote_peer()?;

            if connection.can_send_messages() {
                let platform = cm.platform.lock()?;
                platform.on_send_answer(&*remote_peer, call_id, send)?;
                Ok(true)
            } else {
                Ok(false)
            }
        });

        let message_item = SignalingMessageItem {
            call_id,
            message_type: signaling::MessageType::Answer,
            message_closure: answer_closure,
        };

        self.send_next_message(Some(message_item))
    }

    /// Send ICE candidates to remote_peer via the application.
    pub(super) fn send_buffered_local_ice_candidates(
        &mut self,
        call: Call<T>,
        connection: Connection<T>,
        broadcast: bool,
    ) -> Result<()> {
        let call_id = call.call_id();
        info!("send_ice_candidates(): call_id: {}", call_id);

        let ice_closure = Box::new(move |cm: &CallManager<T>| {
            let local_candidates = connection.take_buffered_local_ice_candidates()?;

            if local_candidates.is_empty() {
                return Ok(false);
            }

            ringbench!(
                RingBench::CM,
                RingBench::App,
                format!(
                    "send_ice_candidates({})\t{}",
                    local_candidates.len(),
                    call_id,
                )
            );

            let remote_peer = call.remote_peer()?;

            let platform = cm.platform.lock()?;
            platform.on_send_ice(
                &*remote_peer,
                call_id,
                signaling::SendIce {
                    receiver_device_id: if broadcast {
                        None
                    } else {
                        Some(connection.remote_device_id())
                    },
                    ice:                signaling::Ice {
                        candidates_added: local_candidates,
                    },
                },
            )?;
            Ok(true)
        });

        let message_item = SignalingMessageItem {
            call_id,
            message_type: signaling::MessageType::Ice,
            message_closure: ice_closure,
        };

        self.send_next_message(Some(message_item))
    }
}
