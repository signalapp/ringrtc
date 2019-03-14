//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Definitions for managing the
//! [CallStateMachine](../call_fsm/struct.CallStateMachine.html) and
//! [CallConnectionHandle](../call_connection/struct.CallConnectionHandle.html)
//! objects.

extern crate tokio;

use std::fmt;
use std::thread;
use std::sync::{
    Arc,
    Mutex,
};
use std::time::{
    Duration,
    Instant,
};

use futures::Future;
use futures::sync::mpsc::{
    Receiver,
    Sender,
};
use tokio::runtime;
use tokio::timer::Delay;

use crate::common::Result;

use crate::core::call_fsm::{
    CallEvent,
    CallStateMachine,
};
use crate::core::call_connection::{
    CallConnectionInterface,
    CallConnectionHandle,
};
use crate::core::util::CppObject;

/// A mpsc::Sender for injecting CallEvents into the
/// [CallStateMachine](../call_fsm/struct.CallStateMachine.html)
///
/// The event pump injects the tuple (CallConnectionHandle, CallEvent)
/// into the FSM.
pub type EventPump<T> = Sender<(
    CallConnectionHandle<T>,
    CallEvent<<T as CallConnectionInterface>::ClientRecipient>
)>;

/// A mpsc::Receiver for receiving CallEvents in the
/// [CallStateMachine](../call_fsm/struct.CallStateMachine.html)
///
/// The event stream is the tuple (CallConnectionHandle, CallEvent).
pub type EventStream<T> = Receiver<(
    CallConnectionHandle<T>,
    CallEvent<<T as CallConnectionInterface>::ClientRecipient>
)>;

/// A factory object for creating a
/// [CallConnectionHandle](../call_connection/struct.CallConnectionHandle.html)
/// object and the associated
/// [CallStateMachine](../call_fsm/struct.CallStateMachine.html)
/// object.
///
/// The factory has two primary responsibilities:
///
/// - create a finite state machine object,
///   [CallStateMachine](../call_fsm/struct.CallStateMachine.html).
/// - create a CallConnectionHandle.
pub struct CallConnectionFactory<T>
where
    T: CallConnectionInterface,
{
    /// Runtime upon which the CallStateMachine runs.
    worker_runtime: runtime::Runtime,
    /// Runtime that manages timing out a call.
    timeout_runtime: Option<runtime::Runtime>,
    /// Native pointer to WebRTC C++ PeerConnectionFactory object.
    native_peer_connection_factory: CppObject,
    /// EventPump for sending events into the CallStateMachine.
    event_pump: EventPump<T>,
}

impl<T> fmt::Display for CallConnectionFactory<T>
where
    T: CallConnectionInterface,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(tid: {:?}, native_peer_connection_factory: 0x{:p})",
               thread::current().id(), self.native_peer_connection_factory)
    }
}

impl<T> fmt::Debug for CallConnectionFactory<T>
where
    T: CallConnectionInterface,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl<T> Drop for CallConnectionFactory<T>
where
    T: CallConnectionInterface,
{
    fn drop(&mut self) {
        info!("Dropping CallConnectionFactory");
    }
}

impl<T> CallConnectionFactory<T>
where
    T: CallConnectionInterface,
{
    /// Creates a new CallConnectionFactory
    ///
    /// # Arguments
    ///
    /// * `native_peer_connection_factory` - Raw pointer to WebRTC C++
    /// PeerConnectionFactory object.
    pub fn new(native_peer_connection_factory: CppObject) -> Result<Self> {
        let (sender, receiver) = futures::sync::mpsc::channel(256);
        let mut ccf = CallConnectionFactory {
            worker_runtime: runtime::Builder::new()
                .core_threads(1)
                .name_prefix("worker-")
                .build()?,
            timeout_runtime: Some(
                runtime::Builder::new()
                    .core_threads(1)
                    .name_prefix("timeout-")
                    .build()?
            ),
            native_peer_connection_factory,
            event_pump: sender,
        };

        let call_fsm = CallStateMachine::new(receiver)?
            .map_err(|e| info!("call state machine returned error: {}", e));
        ccf.worker_runtime.spawn(call_fsm);
        Ok(ccf)
    }

    /// Creates a new CallConnectionHandle
    ///
    /// # Arguments
    ///
    /// * `call_connection` - A platform specific CallConnection object
    pub fn create_call_connection_handle(&mut self, call_connection: T) -> Result<CallConnectionHandle<T>> {
        let cc_handle = CallConnectionHandle::create(self.event_pump.clone(), Arc::new(Mutex::new(call_connection)));

        let call_id = cc_handle.get_call_id()?;
        let mut cc_handle_clone = cc_handle.clone();

        let when = Instant::now() + Duration::from_secs(120);
        let call_timeout_future = Delay::new(when)
            .map_err(|e| error!("Call timeout Delay failed: {:?}", e))
            .and_then(move |_| {
                cc_handle_clone.inject_call_timeout(call_id)
                    .map_err(|e| error!("Inject call timeout failed: {:?}", e))
            });
        info!("create_call_connection_handle(): spawning call timeout task");
        if let Some(timeout_runtime) = &mut self.timeout_runtime {
            timeout_runtime.spawn(call_timeout_future);
        }

        Ok(cc_handle)
    }

    /// Return the raw WebRTC C++ PeerConnectionFactory pointer
    pub fn get_native_peer_connection_factory(&self) -> CppObject {
        self.native_peer_connection_factory
    }

    /// Clean up and close down the factory object
    pub fn close(&mut self) -> Result<()> {
        info!("stopping timeout thread");
        if let Some(timeout_runtime) = self.timeout_runtime.take() {
            let _ = timeout_runtime.shutdown_now().wait()
                .map_err(|_| warn!("Problems shutting down the timeout runtime"));
        }
        info!("stopping timeout thread: complete");
        Ok(())
    }

}
