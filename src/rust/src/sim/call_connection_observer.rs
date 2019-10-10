//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Simulation CallConnectionObserver Implementation.

use std::collections::HashMap;
use std::sync::{
    Arc,
    Mutex,
};
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use crate::common::CallId;

use crate::core::call_connection_observer::{
    ClientEvent,
    CallConnectionObserver,
};
use crate::sim::sim_platform::SimMediaStream;

/// Simulation CallConnectionObserver
pub struct SimCallConnectionObserver {
    /// Unique identifier for the call
    call_id:      CallId,
    event_map:    Arc<Mutex<HashMap<ClientEvent, usize>>>,
    error_count:  AtomicUsize,
    stream_count: AtomicUsize,
}

impl SimCallConnectionObserver {

    /// Creates a new SimCallConnectionObserver
    pub fn new(call_id: CallId) -> Self {

        Self {
            call_id,
            event_map:    Arc::new(Mutex::new(Default::default())),
            error_count:  AtomicUsize::new(0),
            stream_count: AtomicUsize::new(0),
        }

    }

}

unsafe impl Sync for SimCallConnectionObserver {}
unsafe impl Send for SimCallConnectionObserver {}

impl CallConnectionObserver for SimCallConnectionObserver {

    type AppMediaStream = SimMediaStream;

    fn notify_event(&self, event: ClientEvent) {
        info!("notify_event: {}, call_id: {}", event, self.call_id);
        let mut map = self.event_map.lock().unwrap();
        map.entry(event)
            .and_modify(|e| { *e += 1 })
            .or_insert(1);
    }

    fn notify_error(&self, error: failure::Error) {
        info!("notify_error: {}, call_id: {}", error, self.call_id);
        let _ = self.error_count.fetch_add(1, Ordering::AcqRel);
    }

    fn notify_on_add_stream(&self, stream: Self::AppMediaStream) {
        info!("notify_on_add_stream(): {:?}, call_id: {}", stream, self.call_id);
        let _ = self.stream_count.fetch_add(1, Ordering::AcqRel);
    }

}

impl SimCallConnectionObserver {
    pub fn get_error_count(&self) -> usize {
        self.error_count.load(Ordering::Acquire)
    }

    pub fn clear_error_count(&self) {
        self.error_count.store(0, Ordering::Release)
    }

    pub fn get_event_count(&mut self, event: ClientEvent) -> usize {
        let mut map = self.event_map.lock().unwrap();
        *map.entry(event)
            .or_insert(0)

    }

    pub fn clear_event_count(&mut self, event: ClientEvent) {
        let mut map = self.event_map.lock().unwrap();
        *map.entry(event)
            .or_insert(0) = 0;

    }

    pub fn get_stream_count(&self) -> usize {
        self.stream_count.load(Ordering::Acquire)
    }

    pub fn clear_stream_count(&self) {
        self.stream_count.store(0, Ordering::Release)
    }

}
