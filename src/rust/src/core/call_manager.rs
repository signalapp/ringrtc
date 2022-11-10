//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! The main Call Manager object definitions.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::stringify;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use bytes::{Bytes, BytesMut};
use futures::future::lazy;
use futures::future::TryFutureExt;
use futures::Future;
use lazy_static::lazy_static;
use prost::Message;

use crate::common::{
    ApplicationEvent, CallDirection, CallId, CallMediaType, CallState, DeviceId, Result, RingBench,
};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call::Call;
use crate::core::call_mutex::CallMutex;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::group_call::{HttpSfuClient, Observer};
use crate::core::platform::Platform;
use crate::core::signaling::ReceivedOffer;
use crate::core::util::{uuid_to_string, TaskQueueRuntime};
use crate::core::{group_call, signaling};
use crate::error::RingRtcError;
use crate::lite::{
    http, sfu,
    sfu::{DemuxId, GroupMember, MembershipProof, PeekInfo, UserId},
};
use crate::protobuf;
use crate::webrtc::media::{AudioTrack, MediaStream, VideoSink, VideoTrack};
use crate::webrtc::peer_connection::{AudioLevel, ReceivedAudioLevel};
use crate::webrtc::peer_connection_factory::PeerConnectionFactory;
use crate::webrtc::peer_connection_observer::NetworkRoute;

pub const MAX_MESSAGE_AGE: Duration = Duration::from_secs(60);
const TIME_OUT_PERIOD: Duration = Duration::from_secs(60);

lazy_static! {
    static ref INCOMING_GROUP_CALL_RING_TIME: Duration =
        std::env::var("INCOMING_GROUP_CALL_RING_SECS")
            .ok()
            .map(|secs| secs
                .parse()
                .expect("INCOMING_GROUP_CALL_RING_SECS must be an integer"))
            .map(Duration::from_secs)
            .unwrap_or(TIME_OUT_PERIOD);
}

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
        let future = lazy(move |_| $f(&mut call_manager $( , $a)*)).unwrap_or_else(move |err| {
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
                info!("{}(): active call_id: {}", $f, v.call_id());
                v
            }
            _ => {
                ringbenchx!(RingBench::Cm, RingBench::App, "inactive");
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
        let future = lazy(move |_| $f(&mut call_manager $( , $a)*)).unwrap_or_else(move |err| {
            error!("Future {} failed: {}", stringify!($f), err);
        });
        $s.worker_spawn(future)
    }};
}

/// Result from the message queue closures as to whether a message was
/// sent or not. If not, it is due to some non-error check and as a
/// result, no message is given to the application. In this case, no
/// message is in-flight and the next one can be sent right away, if
/// any.
#[derive(PartialEq)]
enum MessageSendResult {
    Sent,
    NotSent,
}

/// A structure to hold messages in the message_queue, identified by their CallId.
pub struct SignalingMessageItem<T>
where
    T: Platform,
{
    /// The CallId of the Call that the message belongs to.
    call_id: CallId,
    /// The type of message the item corresponds to.
    message_type: signaling::MessageType,
    /// The closure to be called which will send the message.
    #[allow(clippy::type_complexity)]
    message_closure: Box<dyn FnOnce(&CallManager<T>) -> Result<MessageSendResult> + Send>,
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
    queue: VecDeque<SignalingMessageItem<T>>,
    /// The type of the last message sent from the message queue.
    last_sent_message_type: Option<signaling::MessageType>,
    /// Whether or not a message is still being handled by the
    /// application (true if a message is currently in the process
    /// of being sent). We will only send one at a time to the
    /// application.
    messages_in_flight: bool,
}

impl<T> SignalingMessageQueue<T>
where
    T: Platform,
{
    /// Create a new SignalingMessageQueue.
    pub fn new() -> Result<Self> {
        Ok(Self {
            queue: VecDeque::new(),
            last_sent_message_type: None,
            messages_in_flight: false,
        })
    }
}

/// Information about a received group ring that hasn't yet been accepted or cancelled.
#[derive(Debug)]
struct OutstandingGroupRing {
    ring_id: group_call::RingId,
    received: Instant,
}

impl OutstandingGroupRing {
    fn has_expired(&self) -> bool {
        self.received.elapsed() >= TIME_OUT_PERIOD
    }
}

/// When receiving an offer, the possible collisions with the active call.
enum ReceivedOfferCollision {
    /// No active call, so we can proceed normally
    None,
    /// An active call with a different user, so act busy
    Busy,
    /// An active call with the same user, but we win so ignore the incoming call
    GlareWinner,
    /// An active call with the same user, but we lose so drop our call
    GlareLoser,
    /// An active call with the same user, but we both lose and drop both calls
    GlareDoubleLoser,
    /// An active call with the same user, but we were already connected but they
    /// are recalling us, so drop our call, no need to send hangup, they already ended
    ReCall,
}

/// Management of 1:1 call messages that arrive before the offer for a particular call.
///
/// We don't save all message kinds here, only the ones that can affect an incoming call.
enum PendingCallMessages {
    None,
    IceCandidates {
        call_id: CallId,
        received: Vec<signaling::ReceivedIce>,
    },
    Hangup {
        call_id: CallId,
        received: signaling::ReceivedHangup,
    },
}

impl PendingCallMessages {
    fn save_ice_candidates(&mut self, new_call_id: CallId, new_received: signaling::ReceivedIce) {
        info!("no active call; saving ice candidates for {}", new_call_id);
        match self {
            PendingCallMessages::IceCandidates { call_id, received } if call_id == &new_call_id => {
                // Avoid growing unbounded.
                if received.len() >= 30 {
                    received.remove(0);
                }
                received.push(new_received);
                return;
            }
            PendingCallMessages::Hangup { call_id, .. } if call_id == &new_call_id => {
                // Ice candidates arriving after a hangup are never needed.
                return;
            }
            PendingCallMessages::IceCandidates { call_id, .. }
            | PendingCallMessages::Hangup { call_id, .. } => {
                warn!("dropping pending messages for {}", call_id);
            }
            PendingCallMessages::None => {}
        }
        *self = PendingCallMessages::IceCandidates {
            call_id: new_call_id,
            received: vec![new_received],
        }
    }

    fn save_hangup(&mut self, new_call_id: CallId, new_received: signaling::ReceivedHangup) {
        info!("no active call; saving hangup for {}", new_call_id);
        match self {
            PendingCallMessages::IceCandidates { call_id, .. } if call_id == &new_call_id => {
                info!(
                    "discarding pending ice candidates for {} in favor of hangup",
                    call_id
                );
            }
            PendingCallMessages::Hangup { call_id, .. } if call_id == &new_call_id => {
                error!(
                    "received two hangup messages for {}; taking the later one",
                    call_id
                );
            }
            PendingCallMessages::IceCandidates { call_id, .. }
            | PendingCallMessages::Hangup { call_id, .. } => {
                warn!("dropping pending messages for {}", call_id);
            }
            PendingCallMessages::None => {}
        }

        *self = PendingCallMessages::Hangup {
            call_id: new_call_id,
            received: new_received,
        }
    }
}

impl Default for PendingCallMessages {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug)]
pub enum OfferValidationError {
    Expired,
}

/// Statelessly evaluate the given offer.
pub fn validate_offer(
    received: &signaling::ReceivedOffer,
) -> std::result::Result<(), OfferValidationError> {
    if received.age > MAX_MESSAGE_AGE {
        return Err(OfferValidationError::Expired);
    }
    Ok(())
}

#[derive(Debug)]
pub enum OpaqueRingValidationError {
    NotARing,
    Expired,
    RejectedByCallback,
}

pub fn validate_call_message_as_opaque_ring(
    message: &protobuf::signaling::CallMessage,
    message_age: Duration,
    validate_group_ring: impl FnOnce(group_call::GroupIdRef, group_call::RingId) -> bool,
) -> std::result::Result<(), OpaqueRingValidationError> {
    match message {
        protobuf::signaling::CallMessage {
            ring_intention:
                Some(protobuf::signaling::call_message::RingIntention {
                    group_id: Some(group_id),
                    r#type: Some(ring_type),
                    ring_id: Some(ring_id),
                }),
            ..
        } => {
            // Must match the implementation of handle_received_call_message for RingIntentions.
            use protobuf::signaling::call_message::ring_intention::Type as IntentionType;
            if IntentionType::from_i32(*ring_type) != Some(IntentionType::Ring) {
                return Err(OpaqueRingValidationError::NotARing);
            }
            if message_age > MAX_MESSAGE_AGE {
                return Err(OpaqueRingValidationError::Expired);
            }
            if !validate_group_ring(group_id, group_call::RingId::from(*ring_id)) {
                // Gives the app an opportunity to reject the ring based on group,
                // or on prior remembered cancellations.
                return Err(OpaqueRingValidationError::RejectedByCallback);
            }
            Ok(())
        }
        _ => Err(OpaqueRingValidationError::NotARing),
    }
}

pub struct CallManager<T>
where
    T: Platform,
{
    /// Interface to platform specific methods.
    platform: Arc<CallMutex<T>>,
    /// The current user's UUID, or None if it's unknown.
    self_uuid: Arc<CallMutex<Option<UserId>>>,
    /// Map of all 1:1 calls.
    call_by_call_id: Arc<CallMutex<HashMap<CallId, Call<T>>>>,
    /// CallId of the active call.
    active_call_id: Arc<CallMutex<Option<CallId>>>,
    /// 1:1 call messages that arrived before the Offer for a particular call.
    pending_call_messages: Arc<CallMutex<PendingCallMessages>>,
    /// Map of all group calls.
    group_call_by_client_id: Arc<CallMutex<HashMap<group_call::ClientId, group_call::Client>>>,
    /// Next value of the group call client id (sequential).
    next_group_call_client_id: Arc<CallMutex<u32>>,
    /// Recent outstanding group rings, keyed by group ID.
    outstanding_group_rings: Arc<CallMutex<HashMap<group_call::GroupId, OutstandingGroupRing>>>,
    /// Busy indication if in either a direct or group call.
    busy: Arc<CallMutex<bool>>,
    /// Tokio runtime for back ground task execution.
    worker_runtime: Arc<CallMutex<Option<TaskQueueRuntime>>>,
    /// Signaling message queue.
    message_queue: Arc<CallMutex<SignalingMessageQueue<T>>>,
    /// How to make HTTP requests to the SFU for group calls.
    http_client: http::DelegatingClient,
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
            platform: Arc::clone(&self.platform),
            self_uuid: Arc::clone(&self.self_uuid),
            call_by_call_id: Arc::clone(&self.call_by_call_id),
            active_call_id: Arc::clone(&self.active_call_id),
            pending_call_messages: Arc::clone(&self.pending_call_messages),
            group_call_by_client_id: Arc::clone(&self.group_call_by_client_id),
            next_group_call_client_id: Arc::clone(&self.next_group_call_client_id),
            outstanding_group_rings: Arc::clone(&self.outstanding_group_rings),
            busy: Arc::clone(&self.busy),
            worker_runtime: Arc::clone(&self.worker_runtime),
            message_queue: Arc::clone(&self.message_queue),
            http_client: self.http_client.clone(),
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

    pub fn new(platform: T, http_client: http::DelegatingClient) -> Result<Self> {
        info!(
            "RingRTC v{}",
            option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")
        );

        Ok(Self {
            platform: Arc::new(CallMutex::new(platform, "platform")),
            self_uuid: Arc::new(CallMutex::new(None, "self_uuid")),
            call_by_call_id: Arc::new(CallMutex::new(HashMap::new(), "call_by_call_id")),
            active_call_id: Arc::new(CallMutex::new(None, "active_call_id")),
            pending_call_messages: Arc::new(CallMutex::new(
                PendingCallMessages::None,
                "pending_individual_call_messages",
            )),
            group_call_by_client_id: Arc::new(CallMutex::new(
                HashMap::new(),
                "group_call_by_client_id",
            )),
            next_group_call_client_id: Arc::new(CallMutex::new(0, "next_group_call_client_id")),
            outstanding_group_rings: Arc::new(CallMutex::new(
                HashMap::new(),
                "outstanding_group_rings",
            )),
            busy: Arc::new(CallMutex::new(false, "busy")),
            worker_runtime: Arc::new(CallMutex::new(
                Some(TaskQueueRuntime::new("call-manager-worker")?),
                "worker_runtime",
            )),
            message_queue: Arc::new(CallMutex::new(
                SignalingMessageQueue::new()?,
                "message_queue",
            )),
            http_client,
        })
    }

    /// Updates the current user's UUID.
    pub fn set_self_uuid(&mut self, uuid: UserId) -> Result<()> {
        info!("set_self_uuid():");
        *self.self_uuid.lock()? = Some(uuid);
        Ok(())
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
        let future = lazy(move |_| {
            call_manager.handle_call(remote_peer, call_id, call_media_type, local_device_id)
        })
        .unwrap_or_else(move |err| {
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
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    ) -> Result<()> {
        handle_active_call_api!(
            self,
            CallManager::handle_proceed,
            call_id,
            app_call_context,
            bandwidth_mode,
            audio_levels_interval
        )
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

    fn remove_outstanding_group_ring(
        &mut self,
        group_id: group_call::GroupIdRef,
        ring_id: group_call::RingId,
    ) -> Result<()> {
        let mut outstanding_group_rings = self.outstanding_group_rings.lock()?;
        if let Some(ring) = outstanding_group_rings.get(group_id) {
            if ring.ring_id == ring_id {
                outstanding_group_rings.remove(group_id);
            }
        }
        Ok(())
    }

    /// Cancel a group ring.
    pub fn cancel_group_ring(
        &mut self,
        group_id: group_call::GroupId,
        ring_id: group_call::RingId,
        reason: Option<group_call::RingCancelReason>,
    ) -> Result<()> {
        info!("cancel_group_ring(): ring_id: {}", ring_id);

        self.remove_outstanding_group_ring(&group_id, ring_id)?;

        if let Some(reason) = reason {
            let self_uuid = self
                .self_uuid
                .lock()
                .expect("get self UUID")
                .as_ref()
                .cloned();
            if let Some(self_uuid) = self_uuid {
                use protobuf::signaling::call_message::ring_response::Type as ResponseType;
                let response_type = match reason {
                    group_call::RingCancelReason::DeclinedByUser => ResponseType::Declined,
                    group_call::RingCancelReason::Busy => ResponseType::Busy,
                };
                let message = protobuf::signaling::CallMessage {
                    ring_response: Some(protobuf::signaling::call_message::RingResponse {
                        group_id: Some(group_id),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(response_type.into()),
                    }),
                    ..Default::default()
                };
                self.send_signaling_message(
                    self_uuid,
                    message,
                    group_call::SignalingMessageUrgency::HandleImmediately,
                );
            } else {
                error!("self UUID unknown; cannot notify other devices of cancellation");
            }
        }

        Ok(())
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
            lazy(move |_| call_manager.handle_received_offer(remote_peer, call_id, received))
                .unwrap_or_else(move |err| {
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

    /// Received a call message from the application.
    pub fn received_call_message(
        &mut self,
        sender_uuid: Vec<u8>,
        sender_device_id: DeviceId,
        local_device_id: DeviceId,
        message: Vec<u8>,
        message_age: Duration,
    ) -> Result<()> {
        handle_api!(
            self,
            CallManager::handle_received_call_message,
            sender_uuid,
            sender_device_id,
            local_device_id,
            message,
            message_age
        )
    }

    /// Received a HTTP response from the application.
    pub fn received_http_response(&mut self, request_id: u32, response: Option<http::Response>) {
        let _ = handle_api!(
            self,
            CallManager::handle_received_http_response,
            request_id,
            response
        );
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
                let call_map = self.call_by_call_id.lock()?;
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

    /// Checks if the CallManager is busy with either a 1:1 or group call.
    #[cfg(feature = "sim")]
    pub fn busy(&self) -> bool {
        *self
            .busy
            .lock()
            .expect("panicked in busy lock during testing")
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

        // sync several times, as simulated error injection can put more
        // events on the FSMs.
        for i in 0..3 {
            info!("synchronize(): pass: {}", i);
            let mut calls = self.call_by_call_id.lock()?.clone();
            for (_, call) in calls.iter_mut() {
                info!("synchronize(): syncing call: {}", call.call_id());
                call.synchronize()?;
            }

            let mut group_calls = self.group_call_by_client_id.lock()?.clone();
            for (client_id, call) in group_calls.iter_mut() {
                info!("synchronize(): syncing group call: {}", client_id);
                call.synchronize();
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
        F: Future<Output = ()> + Send + 'static,
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
        let future = lazy(move |_| {
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
        .unwrap_or_else(move |err: anyhow::Error| {
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

        let result: Option<TaskQueueRuntime> = {
            let mut worker_runtime = self.worker_runtime.lock()?;
            worker_runtime.take()
        };

        if result.is_some() {
            // Dropping the runtime causes it to shut down.
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

    /// Releases busy so another call can begin
    fn release_busy(&mut self) -> Result<()> {
        let mut busy = self.busy.lock()?;
        *busy = false;

        Ok(())
    }

    /// Terminates Call and optionally notifies application of the reason why.
    /// Also removes/drops it from the map.
    fn terminate_and_drop_call(&mut self, call_id: CallId) -> Result<()> {
        info!("terminate_call(): call_id: {}", call_id);

        let mut call = match self.call_by_call_id.lock()?.remove(&call_id) {
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
                RingBench::Cm,
                RingBench::App,
                format!("send_hangup({:?})\t{}", send.hangup, call_id)
            );

            let remote_peer = call.remote_peer()?;

            let platform = cm.platform.lock()?;
            platform.on_send_hangup(&remote_peer, call_id, send)?;

            Ok(MessageSendResult::Sent)
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
    /// - [optional] sending hangup on all connections via RTP data
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
            self.notify_application(&remote_peer, call_id, event)?;
        }

        if let Some(hangup) = hangup {
            // All connections send hangup via RTP data.
            call.inject_send_hangup_via_rtp_data_to_all(hangup)?;
        }

        let mut call_manager = self.clone();
        let cm_error = self.clone();
        let call_error = call.clone();
        let future = lazy(move |_| {
            if let Some(hangup) = hangup {
                // If we want to send a hangup message, be sure that
                // the call actually should send one.
                if call.should_send_hangup() {
                    call.send_hangup_via_signaling_to_all(hangup)?;
                }
            }
            call_manager.terminate_and_drop_call(call_id)
        })
        .unwrap_or_else(move |err| {
            error!("Conclude call future failed: {}", err);
            if let Ok(remote_peer) = call_error.remote_peer() {
                let _ = cm_error.notify_application(
                    &remote_peer,
                    call_id,
                    ApplicationEvent::EndedInternalFailure,
                );
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
        self.release_busy()?;

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
            RingBench::Cm,
            format!(
                "call()\t{}\t{}\t{}",
                call_id, call_media_type, local_device_id
            )
        );

        // If not busy, create a new direct call.
        let mut busy = self.busy.lock()?;
        if *busy {
            Err(RingRtcError::CallManagerIsBusy.into())
        } else {
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
                        self.clone(),
                    )?;

                    // Whenever there is a new call, ensure that messages can flow.
                    self.reset_messages_in_flight()?;

                    let mut call_map = self.call_by_call_id.lock()?;
                    call_map.insert(call_id, call.clone());

                    *busy = true;
                    *active_call_id = Some(call_id);
                    call.start_timeout_timer(TIME_OUT_PERIOD)?;
                    call.inject_start_call()
                }
            }
        }
    }

    /// Handle accept_call() API from application.
    fn handle_accept_call(&mut self, call_id: CallId) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::Cm,
            format!("accept()\t{}", call_id)
        );

        let mut active_call = check_active_call!(self, "handle_accept_call");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::Cm, RingBench::App, "inactive call_id");
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
        self.release_busy()?;
        self.terminate_call(active_call, hangup, Some(event))
    }

    /// Handle drop_call() API from application.
    fn handle_drop_call(&mut self, call_id: CallId) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::Cm,
            format!("drop()\t{}", call_id)
        );

        let active_call = check_active_call!(self, "handle_drop_call");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::Cm, RingBench::App, "inactive call_id");
            return Ok(());
        }

        self.handle_terminate_active_call(active_call, None, ApplicationEvent::EndedAppDroppedCall)
    }

    /// Handle proceed() API from application.
    fn handle_proceed(
        &mut self,
        call_id: CallId,
        app_call_context: <T as Platform>::AppCallContext,
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::Cm,
            format!("proceed()\t{}", call_id)
        );

        let mut active_call = check_active_call!(self, "handle_proceed");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::Cm, RingBench::App, "inactive call_id");
            Ok(())
        } else {
            active_call.set_call_context(app_call_context)?;
            active_call.inject_proceed(bandwidth_mode, audio_levels_interval)
        }
    }

    /// Handle message_sent() API from application.
    fn handle_message_sent(&mut self, call_id: CallId) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::Cm,
            format!("message_sent()\t{}", call_id)
        );

        self.reset_messages_in_flight()?;
        self.send_next_message(None)
    }

    /// Handle message_send_failure() API from application.
    fn handle_message_send_failure(&mut self, call_id: CallId) -> Result<()> {
        let mut is_active_call = false;
        let mut should_handle = true;

        if let Ok(active_call) = self.active_call() {
            if active_call.call_id() == call_id {
                is_active_call = true;
                if let Ok(state) = active_call.state() {
                    if state.connected_or_reconnecting() {
                        // Get the last sent message type and see if it was for ICE.
                        // Since we are in a connected state, don't handle it if so.
                        if let Ok(message_queue) = self.message_queue.lock() {
                            if message_queue.last_sent_message_type
                                == Some(signaling::MessageType::Ice)
                            {
                                should_handle = false
                            }
                        }
                    }
                }
            }
        }

        if should_handle {
            if is_active_call {
                info!(
                    "handle_message_send_failure(): id: {}, concluding active call",
                    call_id
                );

                let _ = self.terminate_active_call(true, ApplicationEvent::EndedSignalingFailure);
            } else {
                // See if the associated call is in the call map.
                let mut call = None;
                {
                    if let Ok(call_map) = self.call_by_call_id.lock() {
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
        ringbench!(RingBench::App, RingBench::Cm, "hangup()");

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
        incoming_call_id: CallId,
        received: signaling::ReceivedOffer,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::Cm,
            format!(
                "received_offer()\t{}\t{}\tprimary={}\t{}\t{}",
                incoming_call_id,
                received.sender_device_id,
                received.receiver_device_is_primary,
                received.offer.to_info_string(),
                received.receiver_device_id,
            )
        );

        if let Err(e) = validate_offer(&received) {
            match e {
                OfferValidationError::Expired => {
                    ringbenchx!(RingBench::Cm, RingBench::App, "offer expired");
                    self.notify_offer_expired(&remote_peer, incoming_call_id, received.age)?;
                }
            }
            // Notify application we are completely done with this remote.
            self.notify_call_concluded(&remote_peer, incoming_call_id)?;
            return Ok(());
        }

        let cm_clone = self.clone();
        let mut busy = cm_clone.busy.lock()?;

        // Don't use self.active_call() because we need to know the active_call_id and active_call separately
        // to handle the case where the active_call_id is set but there is no active call in the map.
        let (active_call_id, active_call): (Option<CallId>, Option<Call<T>>) = {
            let active_call_id = self.active_call_id.lock()?;
            match *active_call_id {
                None => (None, None),
                Some(active_call_id) => {
                    let call_map = self.call_by_call_id.lock()?;
                    let active_call = call_map.get(&active_call_id).cloned();
                    (Some(active_call_id), active_call)
                }
            }
        };

        // Create the call object so that it will either be used as the
        // active call or properly concluded if dropped.
        let mut incoming_call = Call::new(
            remote_peer.clone(),
            incoming_call_id,
            CallDirection::InComing,
            received.offer.call_media_type,
            received.receiver_device_id,
            self.clone(),
        )?;

        let collision = match (active_call_id, &active_call, *busy) {
            (None, None, false) => ReceivedOfferCollision::None,
            (None, None, true) => {
                info!("Group call exists, sending busy for received offer");
                ReceivedOfferCollision::Busy
            }
            (_, Some(active_call), _) => {
                self.check_for_collision(active_call, &remote_peer, &incoming_call_id, &received)
            }
            (Some(_), None, _) => {
                warn!("There is an active call_id without an active call, sending busy");
                ReceivedOfferCollision::Busy
            }
        };

        enum ActiveCallAction {
            DontTerminate,
            TerminateAndSendHangup(ApplicationEvent),
            TerminateWithoutSendingHangup(ApplicationEvent),
        }

        enum IncomingCallAction {
            Ignore(ApplicationEvent),
            RejectAsBusy(ApplicationEvent),
            Start,
        }

        let (active_call_action, incoming_call_action) = match collision {
            ReceivedOfferCollision::None => {
                (ActiveCallAction::DontTerminate, IncomingCallAction::Start)
            }
            ReceivedOfferCollision::Busy => (
                ActiveCallAction::DontTerminate,
                IncomingCallAction::RejectAsBusy(ApplicationEvent::ReceivedOfferWhileActive),
            ),
            ReceivedOfferCollision::GlareWinner => (
                ActiveCallAction::DontTerminate,
                IncomingCallAction::Ignore(ApplicationEvent::ReceivedOfferWithGlare),
            ),
            ReceivedOfferCollision::GlareLoser => (
                ActiveCallAction::TerminateAndSendHangup(ApplicationEvent::EndedRemoteGlare),
                IncomingCallAction::Start,
            ),
            ReceivedOfferCollision::GlareDoubleLoser => (
                ActiveCallAction::TerminateAndSendHangup(ApplicationEvent::EndedRemoteGlare),
                IncomingCallAction::RejectAsBusy(ApplicationEvent::EndedGlareHandlingFailure),
            ),
            ReceivedOfferCollision::ReCall => (
                ActiveCallAction::TerminateWithoutSendingHangup(
                    ApplicationEvent::EndedRemoteReCall,
                ),
                IncomingCallAction::Start,
            ),
        };

        match active_call_action {
            ActiveCallAction::DontTerminate => {}
            ActiveCallAction::TerminateAndSendHangup(app_event) => {
                self.clear_active_call()?;
                *busy = false;
                self.terminate_call(
                    active_call.unwrap(),
                    Some(signaling::Hangup::Normal),
                    Some(app_event),
                )?;
            }
            ActiveCallAction::TerminateWithoutSendingHangup(app_event) => {
                self.clear_active_call()?;
                *busy = false;
                self.terminate_call(active_call.unwrap(), None, Some(app_event))?;
            }
        }

        match incoming_call_action {
            IncomingCallAction::Ignore(app_event) => {
                self.notify_application(&remote_peer, incoming_call_id, app_event)?;
            }
            IncomingCallAction::RejectAsBusy(app_event) => {
                self.notify_application(&remote_peer, incoming_call_id, app_event)?;
                self.send_busy(incoming_call)?;
            }
            IncomingCallAction::Start => {
                let mut active_call_id = self.active_call_id.lock()?;
                if let Some(active_call_id) = *active_call_id {
                    return Err(RingRtcError::CallAlreadyInProgress(active_call_id).into());
                }

                // Whenever there is a new call, ensure that messages can flow.
                self.reset_messages_in_flight()?;

                let mut call_map = self.call_by_call_id.lock()?;
                call_map.insert(incoming_call_id, incoming_call.clone());

                *busy = true;
                *active_call_id = Some(incoming_call_id);
                incoming_call.start_timeout_timer(TIME_OUT_PERIOD)?;
                incoming_call.handle_received_offer(received)?;
                incoming_call.inject_start_call()?;

                match std::mem::take(&mut *self.pending_call_messages.lock()?) {
                    PendingCallMessages::None => {}
                    PendingCallMessages::IceCandidates { call_id, received }
                        if call_id == incoming_call_id =>
                    {
                        for received in received {
                            incoming_call.inject_received_ice(received)?;
                        }
                    }
                    PendingCallMessages::Hangup { call_id, received }
                        if call_id == incoming_call_id =>
                    {
                        incoming_call.inject_received_hangup(received)?;
                    }
                    PendingCallMessages::IceCandidates { call_id, .. }
                    | PendingCallMessages::Hangup { call_id, .. } => {
                        info!("dropping pending messages for {}", call_id);
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle received_answer() API from application.
    fn handle_received_answer(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedAnswer,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::Cm,
            format!(
                "received_answer()\t{}\t{}\t{}",
                call_id,
                received.sender_device_id,
                received.answer.to_info_string(),
            )
        );

        let mut active_call = check_active_call!(self, "handle_received_answer");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::Cm, RingBench::App, "inactive call_id");
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
            RingBench::Cm,
            format!(
                "received_ice_candidates({})\t{}\t{}",
                received.ice.candidates.len(),
                call_id,
                received.sender_device_id,
            )
        );

        match self.active_call() {
            Ok(mut active_call) if active_call.call_id() == call_id => {
                active_call.inject_received_ice(received)?;
            }
            Ok(active_call) => {
                if active_call.direction() == CallDirection::OutGoing {
                    // Save the ICE candidates anyway, in case we have a glare scenario.
                    self.pending_call_messages
                        .lock()?
                        .save_ice_candidates(call_id, received);
                }
            }
            Err(_) => {
                if *self.busy.lock()? {
                    // We're in a group call. Discard the candidates.
                } else {
                    // Save it for later in case it's arriving out-of-order.
                    self.pending_call_messages
                        .lock()?
                        .save_ice_candidates(call_id, received);
                }
            }
        }

        Ok(())
    }

    /// Handle received_hangup() API from application.
    fn handle_received_hangup(
        &mut self,
        call_id: CallId,
        received: signaling::ReceivedHangup,
    ) -> Result<()> {
        ringbench!(
            RingBench::App,
            RingBench::Cm,
            format!(
                "received_hangup({})\t{}\t{}",
                received.hangup, call_id, received.sender_device_id
            )
        );

        match self.active_call() {
            Ok(mut active_call) if active_call.call_id() == call_id => {
                active_call.inject_received_hangup(received)?;
            }
            Ok(active_call) => {
                if active_call.direction() == CallDirection::OutGoing {
                    // Save the hangup anyway, in case we have a glare scenario.
                    self.pending_call_messages
                        .lock()?
                        .save_hangup(call_id, received);
                }
            }
            Err(_) => {
                if *self.busy.lock()? {
                    // We're in a group call. Discard the hangup.
                } else {
                    // Save it for later in case it's arriving out-of-order.
                    self.pending_call_messages
                        .lock()?
                        .save_hangup(call_id, received);
                }
            }
        }

        Ok(())
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
            RingBench::Cm,
            format!("received_busy()\t{}\t{}", call_id, sender_device_id)
        );

        let active_call = check_active_call!(self, "handle_received_busy");
        if active_call.call_id() != call_id {
            ringbenchx!(RingBench::Cm, RingBench::App, "inactive call_id");
            return Ok(());
        }

        // Invoke hangup_other for the call, which will inject hangup/busy
        // to all connections, if any.
        let hangup = signaling::Hangup::BusyOnAnotherDevice(sender_device_id);
        active_call
            .send_hangup_via_rtp_data_and_signaling_to_all_except(hangup, sender_device_id)?;

        // Handle the normal processing of busy by concluding the call locally.
        self.handle_terminate_active_call(active_call, None, ApplicationEvent::EndedRemoteBusy)
    }

    /// Handle received_call_message() API from the application.
    fn handle_received_call_message(
        &mut self,
        sender_uuid: Vec<u8>,
        _sender_device_id: DeviceId,
        _local_device_id: DeviceId,
        message: Vec<u8>,
        message_age: Duration,
    ) -> Result<()> {
        info!("handle_received_call_message():");

        let message = protobuf::signaling::CallMessage::decode(Bytes::from(message))?;
        match message {
            // Handle cases in the same order as classify_received_call_message_for_ringing,
            // so that a CallMessage that mistakenly has multiple fields populated
            // isn't treated differently between the two.
            protobuf::signaling::CallMessage {
                ring_intention: Some(mut ring_intention),
                ..
            } => {
                // Must be compatible with validate_received_call_message_for_ringing.
                use protobuf::signaling::call_message::ring_intention::Type as IntentionType;
                match (
                    &mut ring_intention.group_id,
                    ring_intention.r#type.and_then(IntentionType::from_i32),
                    ring_intention.ring_id,
                ) {
                    (Some(group_id), Some(ring_type), Some(ring_id)) => {
                        let ring_update = match ring_type {
                            IntentionType::Ring => {
                                if message_age > MAX_MESSAGE_AGE {
                                    group_call::RingUpdate::ExpiredRequest
                                } else if *self.busy.lock()? {
                                    // Let your other devices know.
                                    self.cancel_group_ring(
                                        group_id.clone(),
                                        ring_id.into(),
                                        Some(group_call::RingCancelReason::Busy),
                                    )?;
                                    group_call::RingUpdate::BusyLocally
                                } else {
                                    self.start_group_ring(
                                        group_id.clone(),
                                        ring_id.into(),
                                        sender_uuid.clone(),
                                    )?;
                                    group_call::RingUpdate::Requested
                                }
                            }
                            IntentionType::Cancelled => {
                                self.remove_outstanding_group_ring(group_id, ring_id.into())?;
                                group_call::RingUpdate::CancelledByRinger
                            }
                        };

                        self.platform.lock()?.group_call_ring_update(
                            std::mem::take(group_id),
                            ring_id.into(),
                            sender_uuid,
                            ring_update,
                        );
                    }
                    _ => {
                        warn!("Received malformed RingIntention: {:?}", ring_intention);
                    }
                }
            }
            protobuf::signaling::CallMessage {
                ring_response: Some(mut ring_response),
                ..
            } => {
                {
                    let self_uuid = self.self_uuid.lock().expect("get self UUID");
                    if self_uuid.as_ref() != Some(&sender_uuid) {
                        info!(
                            concat!(
                                "Discarding ring response from another user {} for ring ID {}.",
                                "If that's the current user, make sure you told CallManager the ",
                                "current user's UUID!"
                            ),
                            uuid_to_string(&sender_uuid),
                            ring_response.ring_id.unwrap_or(0)
                        );
                        return Ok(());
                    }
                }

                use protobuf::signaling::call_message::ring_response::Type as ResponseType;
                match (
                    &mut ring_response.group_id,
                    ring_response.r#type.and_then(ResponseType::from_i32),
                    ring_response.ring_id,
                ) {
                    (Some(_), Some(ResponseType::Ringing), Some(_)) => {
                        warn!("should not be notified of our own other devices ringing");
                    }
                    (Some(group_id), Some(response_type), Some(ring_id)) => {
                        let ring_update = match response_type {
                            ResponseType::Accepted => {
                                group_call::RingUpdate::AcceptedOnAnotherDevice
                            }
                            ResponseType::Busy => group_call::RingUpdate::BusyOnAnotherDevice,
                            ResponseType::Declined => {
                                group_call::RingUpdate::DeclinedOnAnotherDevice
                            }
                            ResponseType::Ringing => unreachable!("handled above"),
                        };
                        self.remove_outstanding_group_ring(group_id, ring_id.into())?;
                        self.platform.lock()?.group_call_ring_update(
                            std::mem::take(group_id),
                            ring_id.into(),
                            sender_uuid,
                            ring_update,
                        );
                    }
                    _ => {
                        warn!("Received malformed RingResponse: {:?}", ring_response);
                    }
                }
            }
            protobuf::signaling::CallMessage {
                group_call_message: Some(group_call_message),
                ..
            } => {
                if let Some(group_id) = group_call_message.group_id.as_ref() {
                    let group_calls = self
                        .group_call_by_client_id
                        .lock()
                        .expect("lock group_call_by_client_id");
                    let group_call = group_calls.values().find(|c| &c.group_id == group_id);
                    match group_call {
                        Some(call) => {
                            call.on_signaling_message_received(sender_uuid, group_call_message)
                        }
                        None => warn!("Received signaling message for unknown group ID"),
                    };
                }
            }
            _ => {
                warn!("Received unknown CallMessage - ignoring");
            }
        };
        Ok(())
    }

    fn start_group_ring(
        &mut self,
        group_id: group_call::GroupId,
        ring_id: group_call::RingId,
        sender_uuid: UserId,
    ) -> Result<()> {
        {
            let mut outstanding_group_rings = self.outstanding_group_rings.lock()?;
            // Take this opportunity to clear the outstanding rings table
            // (which should be small).
            outstanding_group_rings.retain(|_group_id, ring| !ring.has_expired());
            // If there's an existing, non-expired ring, replace it so that the
            // newly received ring will get cancelled upon joining.
            outstanding_group_rings.insert(
                group_id.clone(),
                OutstandingGroupRing {
                    ring_id,
                    received: Instant::now(),
                },
            );
        }

        {
            if let Ok(mut group_calls) = self.group_call_by_client_id.lock() {
                group_calls
                    .values_mut()
                    .filter(|call| call.group_id == group_id)
                    .for_each(|call| call.provide_ring_id_if_absent(ring_id))
            } else {
                // Ignore the failure to lock; it's more important that we cancel the ring.
            }
        }

        let mut self_for_timeout = self.clone();
        self.worker_spawn(
            async move {
                tokio::time::sleep(*INCOMING_GROUP_CALL_RING_TIME).await;
                self_for_timeout.remove_outstanding_group_ring(&group_id, ring_id)?;
                self_for_timeout.platform.lock()?.group_call_ring_update(
                    group_id,
                    ring_id,
                    sender_uuid,
                    group_call::RingUpdate::ExpiredRequest,
                );
                Ok(())
            }
            .unwrap_or_else(|err: anyhow::Error| {
                error!("error handling group ring timeout: {}", err);
            }),
        )?;

        Ok(())
    }

    #[cfg(feature = "sim")]
    pub fn age_all_outstanding_group_rings(&mut self, age: Duration) {
        for (_group_id, ring) in self.outstanding_group_rings.lock().unwrap().iter_mut() {
            ring.received -= age;
        }
    }

    /// Handle receiving an HTTP response from the application.
    fn handle_received_http_response(
        &mut self,
        request_id: u32,
        response: Option<http::Response>,
    ) -> Result<()> {
        info!(
            "handle_received_http_response(): request_id: {}",
            request_id
        );

        self.http_client.received_response(request_id, response);
        Ok(())
    }

    /// Handle reset() API from application.
    ///
    /// Terminate all calls and clear active callId.  Do not notify the
    /// application at the conclusion.
    fn handle_reset(&mut self) -> Result<()> {
        info!("handle_reset():");

        // gather all the calls from the call_map.
        let calls: Vec<Call<T>> = {
            let call_map = self.call_by_call_id.lock()?;
            call_map.values().cloned().collect()
        };

        // foreach call, terminate without notifying application
        for call in calls {
            info!("reset(): terminating call_id: {}", call.call_id());
            let _ = self.terminate_call(call, Some(signaling::Hangup::Normal), None);
        }

        self.clear_active_call()?;
        self.release_busy()?;

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
                RingBench::Cm,
                RingBench::App,
                format!("send_busy()\t{}", call_id)
            );

            let remote_peer = call.remote_peer()?;

            let platform = cm.platform.lock()?;
            platform.on_send_busy(&remote_peer, call_id)?;

            Ok(MessageSendResult::Sent)
        });

        let message_item = SignalingMessageItem {
            call_id,
            message_type: signaling::MessageType::Busy,
            message_closure: busy_closure,
        };

        self.send_next_message(Some(message_item))
    }

    /// If the remote peer of the active call equals the remote peer
    /// of an incoming offer, then we might have glare or recall.
    fn check_for_collision(
        &mut self,
        active_call: &Call<T>,
        remote_peer: &<T as Platform>::AppRemotePeer,
        incoming_call_id: &CallId,
        received: &ReceivedOffer,
    ) -> ReceivedOfferCollision {
        // Calculates the type of glare collision based on the call_id of each call leg.
        let glare_tiebreaker = || match active_call
            .call_id()
            .as_u64()
            .cmp(&incoming_call_id.as_u64())
        {
            Ordering::Greater => {
                info!("Glare winner, keeping the active call");
                ReceivedOfferCollision::GlareWinner
            }
            Ordering::Less => {
                info!("Glare loser, ending the active call");
                ReceivedOfferCollision::GlareLoser
            }
            Ordering::Equal => {
                warn!("Glare, unexpected call_id match!");
                ReceivedOfferCollision::GlareDoubleLoser
            }
        };

        if let Ok(active_call_state) = active_call.state() {
            if self.remote_peer_equals_active(active_call, remote_peer) {
                info!("Possible glare, remote peers match");
                if let Ok(active_device_id) = active_call.active_device_id() {
                    if received.sender_device_id == active_device_id {
                        if active_call_state >= CallState::ConnectedAndAccepted {
                            info!("Recall, already in-call but peer's device is calling again");
                            // Peer likely ended the call before we know about it:
                            // - Hungup and called faster than messages get handled, or
                            // - ICE failure on their end before ours
                            ReceivedOfferCollision::ReCall
                        } else {
                            info!("Glare, not yet accepted and peer devices match");
                            glare_tiebreaker()
                        }
                    } else {
                        info!("Call from different device, sending busy for received offer");
                        ReceivedOfferCollision::Busy
                    }
                } else {
                    info!("Glare, not yet connected so no active device");
                    glare_tiebreaker()
                }
            } else {
                info!("Active call exists, sending busy for received offer");
                ReceivedOfferCollision::Busy
            }
        } else {
            error!("No active_call state! End all calls.");
            ReceivedOfferCollision::GlareDoubleLoser
        }
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
        error: anyhow::Error,
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
        let _ =
            self.notify_application(remote_peer, call_id, ApplicationEvent::EndedInternalFailure);
        let _ = self.notify_call_concluded(remote_peer, call_id);
    }

    /// Internal error occurred on an API future.
    ///
    /// This shuts down the specified call if active and notifies the
    /// application.
    fn internal_api_error(&mut self, error: anyhow::Error) -> Result<()> {
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
            let mut closure_error: Option<(anyhow::Error, CallId)> = None;

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
                                    message_queue.messages_in_flight = (message_is_in_flight
                                        == MessageSendResult::Sent)
                                        && !assume_messages_sent;

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
            RingBench::Cm,
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
        call_id: CallId,
        event: ApplicationEvent,
    ) -> Result<()> {
        ringbench!(RingBench::Cm, RingBench::App, format!("event({})", event));

        let platform = self.platform.lock()?;
        platform.on_event(remote_peer, call_id, event)
    }

    /// Notify application that the network route changed
    pub(super) fn notify_network_route_changed(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        network_route: NetworkRoute,
    ) -> Result<()> {
        ringbench!(
            RingBench::Cm,
            RingBench::App,
            format!(
                "network_route_changed()\tnetwork_route: {:?}",
                network_route
            )
        );

        let platform = self.platform.lock()?;
        platform.on_network_route_changed(remote_peer, network_route)
    }

    /// Notify application of audio levels
    pub(super) fn notify_audio_levels(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        let platform = self.platform.lock()?;
        platform.on_audio_levels(remote_peer, captured_level, received_level)
    }

    /// Create a new connection to a remote device
    pub(super) fn create_connection(
        &self,
        call: &Call<T>,
        device_id: DeviceId,
        connection_type: ConnectionType,
        signaling_version: signaling::Version,
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    ) -> Result<Connection<T>> {
        let mut platform = self.platform.lock()?;
        platform.create_connection(
            call,
            device_id,
            connection_type,
            signaling_version,
            bandwidth_mode,
            audio_levels_interval,
        )
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
    pub(super) fn notify_offer_expired(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        call_id: CallId,
        age: Duration,
    ) -> Result<()> {
        ringbench!(
            RingBench::Cm,
            RingBench::App,
            format!("offer_expired()\t{}", call_id)
        );

        let platform = self.platform.lock()?;
        platform.on_offer_expired(remote_peer, call_id, age)
    }

    /// Notify application that the call is concluded.
    pub(super) fn notify_call_concluded(
        &self,
        remote_peer: &<T as Platform>::AppRemotePeer,
        call_id: CallId,
    ) -> Result<()> {
        ringbench!(
            RingBench::Cm,
            RingBench::App,
            format!("call_concluded()\t{}", call_id)
        );

        let platform = self.platform.lock()?;
        platform.on_call_concluded(remote_peer, call_id)
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

    /// Network failure occurred on the active call.
    pub(super) fn connection_failure(&mut self, call_id: CallId) -> Result<()> {
        info!("call_failed(): call_id: {}", call_id);

        if self.call_is_active(call_id)? {
            self.terminate_active_call(true, ApplicationEvent::EndedConnectionFailure)
        } else {
            info!("call_failed(): ignoring for inactive call");
            Ok(())
        }
    }

    /// Internal error occurred on the active call.
    ///
    /// This shuts down the specified call if active and notifies the
    /// application.
    pub(super) fn internal_error(&mut self, call_id: CallId, error: anyhow::Error) -> Result<()> {
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
                RingBench::Cm,
                RingBench::App,
                format!("send_offer()\t{}\t{}", call_id, offer.to_info_string())
            );

            let remote_peer = call.remote_peer()?;

            if connection.can_send_messages() {
                let platform = cm.platform.lock()?;
                platform.on_send_offer(&remote_peer, call_id, offer)?;
                Ok(MessageSendResult::Sent)
            } else {
                Ok(MessageSendResult::NotSent)
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
                RingBench::Cm,
                RingBench::App,
                format!(
                    "send_answer()\t{}\t{}",
                    call_id,
                    send.answer.to_info_string()
                )
            );

            let remote_peer = call.remote_peer()?;

            if connection.can_send_messages() {
                let platform = cm.platform.lock()?;
                platform.on_send_answer(&remote_peer, call_id, send)?;
                Ok(MessageSendResult::Sent)
            } else {
                Ok(MessageSendResult::NotSent)
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
                return Ok(MessageSendResult::NotSent);
            }

            ringbench!(
                RingBench::Cm,
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
                &remote_peer,
                call_id,
                signaling::SendIce {
                    receiver_device_id: if broadcast {
                        None
                    } else {
                        Some(connection.remote_device_id())
                    },
                    ice: signaling::Ice {
                        candidates: local_candidates,
                    },
                },
            )?;
            Ok(MessageSendResult::Sent)
        });

        let message_item = SignalingMessageItem {
            call_id,
            message_type: signaling::MessageType::Ice,
            message_closure: ice_closure,
        };

        self.send_next_message(Some(message_item))
    }
}

// Group Calls

macro_rules! platform_handler {
    (
        $s:ident,
        $f:tt
        $(, $a:expr)*
    ) => {{
        let platform = $s.platform.lock();
        match platform {
            Ok(platform) => {
                platform.$f($($a),*);
            }
            Err(error) => {
                error!("{}", error);
            }
        }
    }};
}

impl<T> group_call::Observer for CallManager<T>
where
    T: Platform,
{
    fn request_membership_proof(&self, client_id: group_call::ClientId) {
        info!("request_membership_proof():");
        platform_handler!(self, request_membership_proof, client_id);
    }

    fn request_group_members(&self, client_id: group_call::ClientId) {
        info!("request_group_members():");
        platform_handler!(self, request_group_members, client_id);
    }

    fn handle_connection_state_changed(
        &self,
        client_id: group_call::ClientId,
        connection_state: group_call::ConnectionState,
    ) {
        info!("handle_connection_state_changed():");
        platform_handler!(
            self,
            handle_connection_state_changed,
            client_id,
            connection_state
        );
    }

    fn handle_network_route_changed(
        &self,
        client_id: group_call::ClientId,
        network_route: NetworkRoute,
    ) {
        info!("handle_network_route_changed():");
        platform_handler!(self, handle_network_route_changed, client_id, network_route);
    }

    fn handle_join_state_changed(
        &self,
        client_id: group_call::ClientId,
        join_state: group_call::JoinState,
    ) {
        info!("handle_join_state_changed():");
        platform_handler!(self, handle_join_state_changed, client_id, join_state);
    }

    fn handle_remote_devices_changed(
        &self,
        client_id: group_call::ClientId,
        remote_device_states: &[group_call::RemoteDeviceState],
        reason: group_call::RemoteDevicesChangedReason,
    ) {
        info!("handle_remote_devices_changed(): {:?}", reason);
        platform_handler!(
            self,
            handle_remote_devices_changed,
            client_id,
            remote_device_states,
            reason
        );
    }

    fn handle_incoming_video_track(
        &mut self,
        client_id: group_call::ClientId,
        remote_demux_id: DemuxId,
        incoming_video_track: VideoTrack,
    ) {
        info!("handle_incoming_video_track():");
        platform_handler!(
            self,
            handle_incoming_video_track,
            client_id,
            remote_demux_id,
            incoming_video_track
        );
    }

    fn handle_peek_changed(
        &self,
        client_id: group_call::ClientId,
        peek_info: &PeekInfo,
        joined_members: &HashSet<UserId>,
    ) {
        info!("handle_peek_changed():");
        platform_handler!(
            self,
            handle_peek_changed,
            client_id,
            peek_info,
            joined_members
        );
    }

    fn handle_audio_levels(
        &self,
        client_id: group_call::ClientId,
        captured_level: AudioLevel,
        received_levels: Vec<ReceivedAudioLevel>,
    ) {
        trace!("handle_audio_levels():");
        platform_handler!(
            self,
            handle_audio_levels,
            client_id,
            captured_level,
            received_levels
        );
    }

    fn handle_ended(&self, client_id: group_call::ClientId, reason: group_call::EndReason) {
        info!("handle_ended({:?}):", reason);
        platform_handler!(self, handle_ended, client_id, reason);
    }

    fn send_signaling_message(
        &mut self,
        recipient: UserId,
        call_message: protobuf::signaling::CallMessage,
        urgency: group_call::SignalingMessageUrgency,
    ) {
        info!("send_signaling_message():");
        debug!("  recipient: {}", uuid_to_string(&recipient));

        let platform = self.platform.lock().expect("platform.lock()");
        let mut bytes = BytesMut::with_capacity(call_message.encoded_len());
        let result = call_message.encode(&mut bytes);
        match result {
            Ok(()) => {
                platform
                    .send_call_message(recipient, bytes.to_vec(), urgency)
                    .unwrap_or_else(|_| {
                        error!("failed to send signaling message",);
                    });
            }
            Err(_) => {
                error!("Failed to encode signaling message");
            }
        }
    }

    fn send_signaling_message_to_group(
        &mut self,
        group_id: group_call::GroupId,
        call_message: protobuf::signaling::CallMessage,
        urgency: group_call::SignalingMessageUrgency,
    ) {
        info!("send_signaling_messag_to_group():");
        debug!("  group ID: {}", uuid_to_string(&group_id));

        let platform = self.platform.lock().expect("platform.lock()");
        let mut bytes = BytesMut::with_capacity(call_message.encoded_len());
        let result = call_message.encode(&mut bytes);
        match result {
            Ok(()) => {
                platform
                    .send_call_message_to_group(group_id, bytes.to_vec(), urgency)
                    .unwrap_or_else(|_| {
                        error!("failed to send signaling message",);
                    });
            }
            Err(_) => {
                error!("Failed to encode signaling message");
            }
        }
    }
}

impl<T> CallManager<T>
where
    T: Platform,
{
    // The membership proof is need for authentication and the group members
    // are needed for the opaque ID => user UUID mapping.
    pub fn peek_group_call(
        &self,
        request_id: u32,
        sfu_url: String,
        membership_proof: MembershipProof,
        group_members: Vec<GroupMember>,
    ) {
        if let Some(auth_header) = sfu::auth_header_from_membership_proof(&membership_proof) {
            let opaque_user_id_mappings =
                sfu::opaque_user_id_mappings_from_group_members(&group_members);
            let call_manager = self.clone();
            sfu::peek(
                &self.http_client,
                &sfu_url,
                auth_header,
                opaque_user_id_mappings,
                Box::new(move |peek_result| {
                    info!("handle_peek_response");
                    platform_handler!(call_manager, handle_peek_result, request_id, peek_result);
                }),
            );
        } else {
            error!("Invalid membership proof: {:?}", membership_proof);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_group_call_client(
        &mut self,
        group_id: group_call::GroupId,
        sfu_url: String,
        hkdf_extra_info: Vec<u8>,
        audio_levels_interval: Option<Duration>,
        peer_connection_factory: Option<PeerConnectionFactory>,
        outgoing_audio_track: AudioTrack,
        outgoing_video_track: VideoTrack,
        incoming_video_sink: Option<Box<dyn VideoSink>>,
    ) -> Result<group_call::ClientId> {
        info!("create_group_call_client():");
        debug!(
            "  group_id: {} sfu_url: {}",
            uuid_to_string(&group_id),
            sfu_url
        );

        let mut next_group_call_client_id = self.next_group_call_client_id.lock()?;
        if *next_group_call_client_id == group_call::INVALID_CLIENT_ID {
            *next_group_call_client_id += 1;
        }
        let client_id = *next_group_call_client_id;
        *next_group_call_client_id += 1;

        let mut outstanding_group_rings = self.outstanding_group_rings.lock()?;
        // Take this opportunity to clear the outstanding rings table (which should be small).
        outstanding_group_rings.retain(|_group_id, ring| !ring.has_expired());
        let ring_id = outstanding_group_rings
            .get(&group_id)
            .map(|ring| ring.ring_id);

        let sfu_client =
            HttpSfuClient::new(Box::new(self.http_client.clone()), sfu_url, hkdf_extra_info);
        let client = group_call::Client::start(
            group_id,
            client_id,
            Box::new(sfu_client),
            Box::new(self.clone()),
            self.busy.clone(),
            self.self_uuid.clone(),
            peer_connection_factory,
            outgoing_audio_track,
            Some(outgoing_video_track),
            incoming_video_sink,
            ring_id,
            audio_levels_interval,
        )?;

        let mut client_by_id = self.group_call_by_client_id.lock()?;
        client_by_id.insert(client_id, client);

        info!("Group Client created with id: {}", client_id);

        Ok(client_id)
    }

    pub fn delete_group_call_client(&mut self, client_id: group_call::ClientId) {
        info!("delete_group_call_client(): id: {}", client_id);

        // Remove the group_call client from the map.
        let group_call_map = self.group_call_by_client_id.lock();
        match group_call_map {
            Ok(mut group_call_map) => {
                let group_call = group_call_map.remove(&client_id);
                match group_call {
                    Some(_group_call) => {
                        // Let group_call drop.
                    }
                    None => {
                        warn!("Group Client not found for id: {}", client_id);
                    }
                }
            }
            Err(error) => {
                error!("{}", error);
            }
        }
    }
}

macro_rules! group_call_api_handler {
    (
        $s:ident,
        $i:ident,
        $f:tt
        $(, $a:expr)*
        $(,)?
    ) => {{
        let group_call_map = $s.group_call_by_client_id.lock();
        match group_call_map {
            Ok(mut group_call_map) => {
                let group_call = group_call_map.get_mut(&$i);
                match group_call {
                    Some(group_call) => {
                        group_call.$f($($a),*);
                    }
                    None => {
                        warn!("Group Client not found for id: {}", $i);
                    }
                }
            }
            Err(error) => {
                error!("{}", error);
            }
        }
    }};
}

impl<T> CallManager<T>
where
    T: Platform,
{
    pub fn connect(&mut self, client_id: group_call::ClientId) {
        info!("connect(): id: {}", client_id);
        group_call_api_handler!(self, client_id, connect);
    }

    pub fn join(&mut self, client_id: group_call::ClientId) {
        info!("join(): id: {}", client_id);
        group_call_api_handler!(self, client_id, join);
    }

    pub fn leave(&mut self, client_id: group_call::ClientId) {
        info!("leave(): id: {}", client_id);
        group_call_api_handler!(self, client_id, leave);
    }

    pub fn disconnect(&mut self, client_id: group_call::ClientId) {
        info!("disconnect(): id: {}", client_id);
        group_call_api_handler!(self, client_id, disconnect);
    }

    pub fn group_ring(&mut self, client_id: group_call::ClientId, recipient: Option<UserId>) {
        info!("group_ring(): id: {}", client_id);
        group_call_api_handler!(self, client_id, ring, recipient);
    }

    pub fn set_outgoing_audio_muted(&mut self, client_id: group_call::ClientId, muted: bool) {
        info!("set_outgoing_audio_muted(): id: {}", client_id);
        group_call_api_handler!(self, client_id, set_outgoing_audio_muted, muted);
    }

    pub fn set_outgoing_video_muted(&mut self, client_id: group_call::ClientId, muted: bool) {
        info!("set_outgoing_video_muted(): id: {}", client_id);
        group_call_api_handler!(self, client_id, set_outgoing_video_muted, muted);
    }

    pub fn set_presenting(&mut self, client_id: group_call::ClientId, presenting: bool) {
        info!("set_presenting(): id: {}", client_id);
        group_call_api_handler!(self, client_id, set_presenting, presenting);
    }

    pub fn set_sharing_screen(&mut self, client_id: group_call::ClientId, sharing_screen: bool) {
        info!("set_sharing_screen(): id: {}", client_id);
        group_call_api_handler!(self, client_id, set_sharing_screen, sharing_screen);
    }

    pub fn resend_media_keys(&mut self, client_id: group_call::ClientId) {
        info!("resend_media_keys(): id: {}", client_id);
        group_call_api_handler!(self, client_id, resend_media_keys);
    }

    pub fn set_bandwidth_mode(
        &mut self,
        client_id: group_call::ClientId,
        bandwidth_mode: BandwidthMode,
    ) {
        info!("set_bandwidth_mode(): id: {}", client_id);
        group_call_api_handler!(self, client_id, set_bandwidth_mode, bandwidth_mode);
    }

    pub fn request_video(
        &mut self,
        client_id: group_call::ClientId,
        rendered_resolutions: Vec<group_call::VideoRequest>,
        active_speaker_height: u16,
    ) {
        info!("request_video(): id: {}", client_id);
        group_call_api_handler!(
            self,
            client_id,
            request_video,
            rendered_resolutions,
            active_speaker_height,
        );
    }

    pub fn set_group_members(
        &mut self,
        client_id: group_call::ClientId,
        members: Vec<GroupMember>,
    ) {
        info!("set_group_members(): id: {}", client_id);
        group_call_api_handler!(self, client_id, set_group_members, members);
    }

    pub fn set_membership_proof(&mut self, client_id: group_call::ClientId, proof: Vec<u8>) {
        info!("set_membership_proof(): id: {}", client_id);
        group_call_api_handler!(self, client_id, set_membership_proof, proof);
    }
}

#[cfg(test)]
mod tests {
    use protobuf::signaling::call_message::ring_intention::Type as IntentionType;
    use protobuf::signaling::{call_message::RingIntention, CallMessage};

    use super::*;

    #[test]
    fn test_validate_offer() {
        fn offer_with_age(age: Duration) -> ReceivedOffer {
            ReceivedOffer {
                offer: signaling::Offer::new(CallMediaType::Audio, vec![]).expect("valid"),
                age,
                sender_device_id: 1,
                receiver_device_id: 1,
                receiver_device_is_primary: true,
                sender_identity_key: vec![],
                receiver_identity_key: vec![],
            }
        }

        validate_offer(&offer_with_age(Duration::ZERO)).expect("valid");
        validate_offer(&offer_with_age(MAX_MESSAGE_AGE - Duration::from_secs(1))).expect("valid");
        validate_offer(&offer_with_age(MAX_MESSAGE_AGE)).expect("valid");
        assert!(matches!(
            validate_offer(&offer_with_age(MAX_MESSAGE_AGE + Duration::from_secs(1))),
            Err(OfferValidationError::Expired)
        ));
    }

    #[test]
    fn test_validate_group_ring_intention_based_on_age() {
        let valid_message = CallMessage {
            ring_intention: Some(RingIntention {
                r#type: Some(IntentionType::Ring.into()),
                group_id: Some(vec![1, 2]),
                ring_id: Some(5),
            }),
            ..Default::default()
        };
        fn check_group_and_ring_id(
            group_id: group_call::GroupIdRef,
            ring_id: group_call::RingId,
        ) -> bool {
            assert_eq!(group_id, &[1, 2]);
            assert_eq!(ring_id, 5.into());
            true
        }

        validate_call_message_as_opaque_ring(
            &valid_message,
            Duration::ZERO,
            check_group_and_ring_id,
        )
        .expect("valid");
        validate_call_message_as_opaque_ring(
            &valid_message,
            MAX_MESSAGE_AGE - Duration::from_secs(1),
            check_group_and_ring_id,
        )
        .expect("valid");
        validate_call_message_as_opaque_ring(
            &valid_message,
            MAX_MESSAGE_AGE,
            check_group_and_ring_id,
        )
        .expect("valid");

        assert!(matches!(
            validate_call_message_as_opaque_ring(
                &valid_message,
                MAX_MESSAGE_AGE + Duration::from_secs(1),
                check_group_and_ring_id,
            ),
            Err(OpaqueRingValidationError::Expired)
        ));
    }

    #[test]
    fn test_validate_group_ring_intention_based_on_callback() {
        let valid_message = CallMessage {
            ring_intention: Some(RingIntention {
                r#type: Some(IntentionType::Ring.into()),
                group_id: Some(vec![1, 2]),
                ring_id: Some(5),
            }),
            ..Default::default()
        };

        validate_call_message_as_opaque_ring(&valid_message, Duration::ZERO, |_, _| true)
            .expect("valid");

        assert!(matches!(
            validate_call_message_as_opaque_ring(&valid_message, Duration::ZERO, |_, _| { false }),
            Err(OpaqueRingValidationError::RejectedByCallback)
        ));
    }

    #[test]
    fn test_validate_group_ring_intention_for_non_rings() {
        #[track_caller]
        fn assert_rejected(message: CallMessage, description: &str) {
            assert!(
                matches!(
                    validate_call_message_as_opaque_ring(&message, Duration::ZERO, |_, _| { true }),
                    Err(OpaqueRingValidationError::NotARing)
                ),
                "{}",
                description
            );
        }

        assert_rejected(
            CallMessage {
                ring_intention: Some(RingIntention {
                    r#type: Some(IntentionType::Ring.into()),
                    group_id: Some(vec![1, 2]),
                    ring_id: None,
                }),
                ..Default::default()
            },
            "missing ring ID",
        );
        assert_rejected(
            CallMessage {
                ring_intention: Some(RingIntention {
                    r#type: Some(IntentionType::Ring.into()),
                    group_id: None,
                    ring_id: Some(5),
                }),
                ..Default::default()
            },
            "missing group ID",
        );
        assert_rejected(
            CallMessage {
                ring_intention: Some(RingIntention {
                    r#type: None,
                    group_id: Some(vec![1, 2]),
                    ring_id: Some(5),
                }),
                ..Default::default()
            },
            "missing type",
        );
        assert_rejected(
            CallMessage {
                ring_intention: Some(RingIntention {
                    r#type: Some(IntentionType::Cancelled.into()),
                    group_id: Some(vec![1, 2]),
                    ring_id: Some(5),
                }),
                ..Default::default()
            },
            "cancellation, not ring",
        );
        assert_rejected(
            CallMessage {
                ..Default::default()
            },
            "not a ring intention",
        );
    }
}
