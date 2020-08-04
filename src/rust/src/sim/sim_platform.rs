//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Simulation CallPlatform Interface.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::common::{ApplicationEvent, CallDirection, CallId, CallMediaType, DeviceId, Result};
use crate::core::call::Call;
use crate::core::call_manager::CallManager;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
use crate::core::signaling;
use crate::sim::error::SimError;
use crate::webrtc::media::MediaStream;
use crate::webrtc::peer_connection::PeerConnection;
use crate::webrtc::sim::peer_connection::RffiPeerConnectionInterface;

/// Simulation implmentation for platform::Platform::{AppIncomingMedia,
/// AppRemotePeer, AppCallContext}
type SimPlatformItem = String;
impl PlatformItem for SimPlatformItem {}

#[derive(Default)]
struct SimStats {
    /// Number of offers sent
    offers_sent:                  AtomicUsize,
    /// Number of answers sent
    answers_sent:                 AtomicUsize,
    /// Number of ICE candidates sent
    ice_candidates_sent:          AtomicUsize,
    /// Number of normal hangups sent
    normal_hangups_sent:          AtomicUsize,
    /// Number of accepted hangups sent
    accepted_hangups_sent:        AtomicUsize,
    /// Number of declined hangups sent
    declined_hangups_sent:        AtomicUsize,
    /// Number of busy hangups sent
    busy_hangups_sent:            AtomicUsize,
    /// Number of need permission hangups sent
    need_permission_hangups_sent: AtomicUsize,
    /// Number of busy messages sent
    busys_sent:                   AtomicUsize,
    /// Number of start outgoing call events
    start_outgoing:               AtomicUsize,
    /// Number of start incoming call events
    start_incoming:               AtomicUsize,
    /// Number of call concluded events
    call_concluded:               AtomicUsize,
    /// Track stream counts
    stream_count:                 AtomicUsize,
}

/// Simulation implementation of platform::Platform.
#[derive(Clone, Default)]
pub struct SimPlatform {
    /// Platform API statistics
    stats:                        Arc<SimStats>,
    /// True if the CallPlatform functions should simulate an internal failure.
    force_internal_fault:         Arc<AtomicBool>,
    /// True if the signaling functions should indicate a signaling
    /// failure to the call manager.
    force_signaling_fault:        Arc<AtomicBool>,
    /// Track event frequencies
    event_map:                    Arc<Mutex<HashMap<ApplicationEvent, usize>>>,
    /// Track whether disconnecting of incoming media happened
    incoming_media_disconnected:  Arc<AtomicBool>,
    /// Call Manager
    call_manager:                 Arc<Mutex<Option<CallManager<Self>>>>,
    /// True to manually require message_sent() to be invoked for Ice messages.
    no_auto_message_sent_for_ice: Arc<AtomicBool>,
}

impl fmt::Display for SimPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SimPlatform")
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

impl Platform for SimPlatform {
    type AppIncomingMedia = SimPlatformItem;
    type AppRemotePeer = SimPlatformItem;
    type AppConnection = RffiPeerConnectionInterface;
    type AppCallContext = SimPlatformItem;

    fn create_connection(
        &mut self,
        call: &Call<Self>,
        remote_device_id: DeviceId,
        connection_type: ConnectionType,
        signaling_version: signaling::Version,
    ) -> Result<Connection<Self>> {
        info!(
            "create_connection(): call_id: {} remote_device_id: {}, signaling_version: {:?}",
            call.call_id(),
            remote_device_id,
            signaling_version,
        );

        let fake_pc = RffiPeerConnectionInterface::new();

        let connection = Connection::new(call.clone(), remote_device_id, connection_type).unwrap();
        connection.set_app_connection(fake_pc).unwrap();

        let pc_interface = PeerConnection::unowned(connection.app_connection_ptr_for_tests());

        connection.set_pc_interface(pc_interface).unwrap();

        Ok(connection)
    }

    fn on_start_call(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        direction: CallDirection,
        call_media_type: CallMediaType,
    ) -> Result<()> {
        info!(
            "on_start_call(): remote_peer: {}, call_id: {}, direction: {}, call_media_type {}",
            remote_peer, call_id, direction, call_media_type
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::StartCallError.into())
        } else {
            let _ = match direction {
                CallDirection::OutGoing => self.stats.start_outgoing.fetch_add(1, Ordering::AcqRel),
                CallDirection::InComing => self.stats.start_incoming.fetch_add(1, Ordering::AcqRel),
            };
            Ok(())
        }
    }

    fn on_event(&self, remote_peer: &Self::AppRemotePeer, event: ApplicationEvent) -> Result<()> {
        info!("on_event(): {}, remote_peer: {}", event, remote_peer);

        let mut map = self.event_map.lock().unwrap();
        map.entry(event).and_modify(|e| *e += 1).or_insert(1);

        Ok(())
    }

    fn on_send_offer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        offer: signaling::Offer,
    ) -> Result<()> {
        info!(
            "on_send_offer(): remote_peer: {}, call_id: {}, offer: {}",
            remote_peer, call_id, offer
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::SendOfferError.into())
        } else {
            let _ = self.stats.offers_sent.fetch_add(1, Ordering::AcqRel);
            if self.force_internal_fault.load(Ordering::Acquire) {
                self.message_send_failure(call_id).unwrap();
            } else {
                self.message_sent(call_id).unwrap();
            }
            Ok(())
        }
    }

    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendAnswer,
    ) -> Result<()> {
        info!(
            "on_send_answer(): remote_peer: {}, call_id: {}, receiver_device_id: {}, answer: {}",
            remote_peer, call_id, send.receiver_device_id, send.answer
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::SendAnswerError.into())
        } else {
            let _ = self.stats.answers_sent.fetch_add(1, Ordering::AcqRel);
            if self.force_internal_fault.load(Ordering::Acquire) {
                self.message_send_failure(call_id).unwrap();
            } else {
                self.message_sent(call_id).unwrap();
            }
            Ok(())
        }
    }

    fn on_send_ice(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendIce,
    ) -> Result<()> {
        let (_broadcast, receiver_device_id) = match send.receiver_device_id {
            // The DeviceId doesn't matter if we're broadcasting
            None => (true, 0 as DeviceId),
            Some(receiver_device_id) => (false, receiver_device_id),
        };

        info!(
            "on_send_ice_candidates(): remote_peer: {}, call_id: {}, receiver_device_id: {}",
            remote_peer, call_id, receiver_device_id
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::SendIceCandidateError.into())
        } else {
            let _ = self
                .stats
                .ice_candidates_sent
                .fetch_add(send.ice.candidates_added.len(), Ordering::AcqRel);
            if self.force_internal_fault.load(Ordering::Acquire) {
                if !self.no_auto_message_sent_for_ice.load(Ordering::Acquire) {
                    self.message_send_failure(call_id).unwrap();
                }
            } else if !self.no_auto_message_sent_for_ice.load(Ordering::Acquire) {
                self.message_sent(call_id).unwrap();
            }
            Ok(())
        }
    }

    fn on_send_hangup(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendHangup,
    ) -> Result<()> {
        info!(
            "on_send_hangup(): remote_peer: {}, call_id: {}",
            remote_peer, call_id
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::SendHangupError.into())
        } else {
            match send.hangup {
                signaling::Hangup::Normal => {
                    let _ = self
                        .stats
                        .normal_hangups_sent
                        .fetch_add(1, Ordering::AcqRel);
                }
                signaling::Hangup::AcceptedOnAnotherDevice(_) => {
                    let _ = self
                        .stats
                        .accepted_hangups_sent
                        .fetch_add(1, Ordering::AcqRel);
                }
                signaling::Hangup::DeclinedOnAnotherDevice(_) => {
                    let _ = self
                        .stats
                        .declined_hangups_sent
                        .fetch_add(1, Ordering::AcqRel);
                }
                signaling::Hangup::BusyOnAnotherDevice(_) => {
                    let _ = self.stats.busy_hangups_sent.fetch_add(1, Ordering::AcqRel);
                }
                signaling::Hangup::NeedPermission(_) => {
                    let _ = self
                        .stats
                        .need_permission_hangups_sent
                        .fetch_add(1, Ordering::AcqRel);
                }
            }
            if self.force_internal_fault.load(Ordering::Acquire) {
                self.message_send_failure(call_id).unwrap();
            } else {
                self.message_sent(call_id).unwrap();
            }
            Ok(())
        }
    }

    fn on_send_busy(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()> {
        info!(
            "on_send_busy(): remote_peer: {}, call_id: {}",
            remote_peer, call_id
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::SendBusyError.into())
        } else {
            let _ = self.stats.busys_sent.fetch_add(1, Ordering::AcqRel);
            if self.force_internal_fault.load(Ordering::Acquire) {
                self.message_send_failure(call_id).unwrap();
            } else {
                self.message_sent(call_id).unwrap();
            }
            Ok(())
        }
    }

    fn create_incoming_media(
        &self,
        _connection: &Connection<Self>,
        _incoming_media: MediaStream,
    ) -> Result<Self::AppIncomingMedia> {
        Ok("MediaStream".to_owned())
    }

    fn connect_incoming_media(
        &self,
        remote_peer: &Self::AppRemotePeer,
        app_call_context: &Self::AppCallContext,
        _incoming_media: &Self::AppIncomingMedia,
    ) -> Result<()> {
        info!(
            "connect_incoming_media(): remote_peer: {}, call_context: {}",
            remote_peer, app_call_context
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::MediaStreamError.into())
        } else {
            let _ = self.stats.stream_count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    fn disconnect_incoming_media(&self, app_call_context: &Self::AppCallContext) -> Result<()> {
        info!(
            "disconnect_incoming_media(): call_context: {}",
            app_call_context
        );

        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::CloseMediaError.into())
        } else {
            self.incoming_media_disconnected
                .store(true, Ordering::Release);
            Ok(())
        }
    }

    fn compare_remotes(
        &self,
        remote_peer1: &Self::AppRemotePeer,
        remote_peer2: &Self::AppRemotePeer,
    ) -> Result<bool> {
        info!(
            "compare_remotes(): remote1: {}, remote2: {}",
            remote_peer1, remote_peer2
        );

        Ok(remote_peer1 == remote_peer2)
    }

    fn on_call_concluded(&self, _remote_peer: &Self::AppRemotePeer) -> Result<()> {
        info!("on_call_concluded():");
        if self.force_internal_fault.load(Ordering::Acquire) {
            Err(SimError::CallConcludedError.into())
        } else {
            let _ = self.stats.call_concluded.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }
}

impl SimPlatform {
    /// Create a new SimPlatform object.
    pub fn new() -> Self {
        SimPlatform::default()
    }

    pub fn close(&mut self) {
        info!("close(): dropping Call Manager object");
        let mut cm = self.call_manager.lock().unwrap();
        let _ = cm.take();
    }

    pub fn set_call_manager(&mut self, call_manager: CallManager<Self>) {
        let mut cm = self.call_manager.lock().unwrap();
        *cm = Some(call_manager);
    }

    fn message_sent(&self, call_id: CallId) -> Result<()> {
        let mut cm = self.call_manager.lock().unwrap();
        cm.as_mut().unwrap().message_sent(call_id).unwrap();
        Ok(())
    }

    fn message_send_failure(&self, call_id: CallId) -> Result<()> {
        let mut cm = self.call_manager.lock().unwrap();
        cm.as_mut().unwrap().message_send_failure(call_id).unwrap();
        Ok(())
    }

    pub fn force_internal_fault(&mut self, enable: bool) {
        self.force_internal_fault.store(enable, Ordering::Release);
    }

    pub fn force_signaling_fault(&mut self, enable: bool) {
        self.force_signaling_fault.store(enable, Ordering::Release);
    }

    pub fn no_auto_message_sent_for_ice(&mut self, enable: bool) {
        self.no_auto_message_sent_for_ice
            .store(enable, Ordering::Release);
    }

    pub fn event_count(&self, event: ApplicationEvent) -> usize {
        let mut errors = 0;
        let map = self.event_map.lock().unwrap();

        if let Some(entry) = map.get(&event) {
            errors += entry;
        }

        errors
    }

    pub fn error_count(&self) -> usize {
        self.event_count(ApplicationEvent::EndedInternalFailure)
    }

    pub fn clear_error_count(&self) {
        let mut map = self.event_map.lock().unwrap();
        let _ = map.remove(&ApplicationEvent::EndedInternalFailure);
    }

    pub fn ended_count(&self) -> usize {
        let mut ends = 0;

        let ended_events = vec![
            ApplicationEvent::EndedLocalHangup,
            ApplicationEvent::EndedRemoteHangup,
            ApplicationEvent::EndedRemoteBusy,
            ApplicationEvent::EndedTimeout,
            ApplicationEvent::EndedInternalFailure,
            ApplicationEvent::EndedConnectionFailure,
            ApplicationEvent::EndedAppDroppedCall,
        ];
        for event in ended_events {
            ends += self.event_count(event);
        }

        ends
    }

    pub fn offers_sent(&self) -> usize {
        self.stats.offers_sent.load(Ordering::Acquire)
    }

    pub fn answers_sent(&self) -> usize {
        self.stats.answers_sent.load(Ordering::Acquire)
    }

    pub fn ice_candidates_sent(&self) -> usize {
        self.stats.ice_candidates_sent.load(Ordering::Acquire)
    }

    pub fn normal_hangups_sent(&self) -> usize {
        self.stats.normal_hangups_sent.load(Ordering::Acquire)
    }

    pub fn accepted_hangups_sent(&self) -> usize {
        self.stats.accepted_hangups_sent.load(Ordering::Acquire)
    }

    pub fn declined_hangups_sent(&self) -> usize {
        self.stats.declined_hangups_sent.load(Ordering::Acquire)
    }

    pub fn busy_hangups_sent(&self) -> usize {
        self.stats.busy_hangups_sent.load(Ordering::Acquire)
    }

    pub fn need_permission_hangups_sent(&self) -> usize {
        self.stats
            .need_permission_hangups_sent
            .load(Ordering::Acquire)
    }

    pub fn busys_sent(&self) -> usize {
        self.stats.busys_sent.load(Ordering::Acquire)
    }

    pub fn stream_count(&self) -> usize {
        self.stats.stream_count.load(Ordering::Acquire)
    }

    pub fn incoming_media_disconnected(&self) -> bool {
        self.incoming_media_disconnected.load(Ordering::Acquire)
    }

    pub fn start_outgoing_count(&self) -> usize {
        self.stats.start_outgoing.load(Ordering::Acquire)
    }

    pub fn start_incoming_count(&self) -> usize {
        self.stats.start_incoming.load(Ordering::Acquire)
    }

    pub fn call_concluded_count(&self) -> usize {
        self.stats.call_concluded.load(Ordering::Acquire)
    }
}
