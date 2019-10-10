//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Simulation CallPlatform Interface.

use std::fmt;
use std::sync::{
    Arc,
    Mutex,
};
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use crate::common::{
    CallId,
    Result,
};
use crate::core::call_connection::{
    CallConnection,
    CallPlatform,
    AppMediaStreamTrait,
};
use crate::core::call_connection_observer::{
    CallConnectionObserver,
    ClientEvent,
};
use crate::sim::call_connection_observer::SimCallConnectionObserver;
use crate::sim::error::SimError;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media_stream::MediaStream;
use crate::webrtc::sdp_observer::SessionDescriptionInterface;

/// Concrete type for Simulation AppMediaStream objects.
pub type SimMediaStream = String;
impl AppMediaStreamTrait for SimMediaStream {}

/// Public type for Android CallConnection object.
pub type SimCallConnection = CallConnection<SimPlatform>;

/// Simulation implementation of core::CallPlatform.
pub struct SimPlatform {
    /// Recipient
    recipient:   String,
    /// CallConnectionObserver object.
    cc_observer: Arc<Mutex<SimCallConnectionObserver>>,
    /// True if the CallPlatform functions should fail
    should_fail:         bool,
    /// Number of offers sent
    offers_sent:         AtomicUsize,
    /// Number of answers sent
    answers_sent:        AtomicUsize,
    /// Number of ICE candidates sent
    ice_candidates_sent: AtomicUsize,
    /// Number of hang ups sent
    hangups_sent:        AtomicUsize,
}

impl fmt::Display for SimPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "recipient: {}", self.recipient)
    }
}

impl fmt::Debug for SimPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for SimPlatform {
    fn drop(&mut self) {
        info!("Dropping SimPlatform");
    }
}

impl CallPlatform for SimPlatform {

    type AppMediaStream = SimMediaStream;

    fn app_send_offer(&self,
                      call_id:   CallId,
                      offer:     SessionDescriptionInterface) -> Result<()> {

        info!("app_send_offer(): call_id: {:?}, offer:{:?}, desc:{}",
              call_id,
              offer,
              offer.get_description().unwrap());

        if self.should_fail {
            Err(SimError::SendOfferError.into())
        } else {
            let _ = self.offers_sent.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    fn app_send_answer(&self,
                       call_id:   CallId,
                       answer:    SessionDescriptionInterface) -> Result<()> {

        info!("app_send_answer(): call_id: {:?}, offer:{:?}", call_id, answer);

        if self.should_fail {
            Err(SimError::SendAnswerError.into())
        } else {
            let _ = self.answers_sent.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    fn app_send_ice_updates(&self,
                            call_id:    CallId,
                            candidates: &[IceCandidate]) -> Result<()> {

        if candidates.is_empty() {
            return Ok(());
        }

        info!("app_send_ice_updates(): call_id: {:?}, candidates:{:?}", call_id, candidates);

        if self.should_fail {
            Err(SimError::SendIceCandidateError.into())
        } else {
            let _ = self.ice_candidates_sent.fetch_add(candidates.len(), Ordering::AcqRel);
            Ok(())
        }
    }

    fn app_send_hangup(&self, call_id: CallId) -> Result<()> {

        info!("app_send_hangup(): call_id: {:?}", call_id);

        if self.should_fail {
            Err(SimError::SendHangupError.into())
        } else {
            let _ = self.hangups_sent.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    fn create_media_stream(&self, stream: MediaStream) -> Result<Self::AppMediaStream> {
        info!("create_media_stream(): stream: {:?}", stream);
        Ok("SimMediaStream".to_string())
    }

    fn notify_client(&self, event: ClientEvent) -> Result<()> {
        info!("sim:notify_client(): event: {}", event);
        let observer = self.cc_observer.lock().unwrap();
        observer.notify_event(event);
        Ok(())
    }

    fn notify_error(&self, error: failure::Error) -> Result<()> {
        info!("sim:notify_error(): error: {}", error);
        let observer = self.cc_observer.lock().unwrap();
        observer.notify_error(error);
        Ok(())
    }

    /// Notify the client application about an avilable MediaStream.
    fn notify_on_add_stream(&self, stream: &Self::AppMediaStream) -> Result<()> {
        info!("sim:notify_on_add_stream(): stream: {}", stream);
        let observer = self.cc_observer.lock().unwrap();
        observer.notify_on_add_stream(stream.to_string());
        Ok(())
    }
}

impl SimPlatform {

    /// Create a new SimPlatform object.
    pub fn new(recipient: String, cc_observer: Arc<Mutex<SimCallConnectionObserver>>) -> Self {

        Self {
            recipient,
            cc_observer,
            should_fail:         false,
            offers_sent:         AtomicUsize::new(0),
            answers_sent:        AtomicUsize::new(0),
            ice_candidates_sent: AtomicUsize::new(0),
            hangups_sent:        AtomicUsize::new(0),
        }
    }

    pub fn should_fail(&mut self, enable: bool) {
        self.should_fail = enable;
    }

    pub fn offers_sent(&self) -> usize {
        self.offers_sent.load(Ordering::Acquire)
    }

    pub fn answers_sent(&self) -> usize {
        self.answers_sent.load(Ordering::Acquire)
    }

    pub fn ice_candidates_sent(&self) -> usize {
        self.ice_candidates_sent.load(Ordering::Acquire)
    }

    pub fn hangups_sent(&self) -> usize {
        self.hangups_sent.load(Ordering::Acquire)
    }

}
