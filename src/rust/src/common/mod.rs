//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Common types used throughout the library.

use std::fmt;

/// Common Result type, using `failure::Error` for Error.
pub type Result<T> = std::result::Result<T, failure::Error>;

/// Unique call identification number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CallId {
    id: u64,
}

impl CallId {
    pub fn as_u64(self) -> u64 {
        self.id
    }
}

impl fmt::Display for CallId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{:x}", self.id)
    }
}

impl From<CallId> for u64 {
    fn from(item: CallId) -> Self {
        item.id
    }
}

impl CallId {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn random() -> Self {
        Self::new(rand::random())
    }

    pub fn format(self, device_id: DeviceId) -> String {
        format!("0x{:x}-{}", self.id, device_id)
    }
}

impl From<i64> for CallId {
    fn from(item: i64) -> Self {
        CallId::new(item as u64)
    }
}

impl From<u64> for CallId {
    fn from(item: u64) -> Self {
        CallId::new(item)
    }
}

/// Unique remote device identification number.
pub type DeviceId = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConnectionId {
    call_id:       CallId,
    remote_device: DeviceId,
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.call_id, self.remote_device)
    }
}

impl ConnectionId {
    pub fn new(call_id: CallId, remote_device: DeviceId) -> Self {
        Self {
            call_id,
            remote_device,
        }
    }

    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    pub fn remote_device(&self) -> DeviceId {
        self.remote_device
    }
}

/// Tracks the state of a call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallState {
    /// No call in progress.
    Idle,

    /// Notifying app that a call is starting.
    Starting,

    /// Call is connecting (signaling) with the remote peer.
    Connecting,

    /// ICE is negotiated.
    Ringing,

    /// Incoming/Outgoing, the call is established.
    Connected,

    /// Incoming/Outgoing, the call is reconnecting.
    Reconnecting,

    /// The call is in the process of terminating (hanging up).
    Terminating,

    /// The call is completely closed.
    Closed,
}

impl fmt::Display for CallState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// An enum representing the status notification types sent to the
/// client application.
///
#[repr(C)]
#[derive(Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApplicationEvent {
    /// Inbound call only: The call signaling (ICE) is complete.
    LocalRinging = 0,

    /// Outbound call only: The call signaling (ICE) is complete.
    RemoteRinging,

    /// The local side has accepted and connected the call.
    LocalConnected,

    /// The remote side has accepted and connected the call.
    RemoteConnected,

    /// The call ended because of a local hangup.
    EndedLocalHangup,

    /// The call ended because of a remote hangup.
    EndedRemoteHangup,

    /// The call ended because the call was accepted by a different device.
    EndedRemoteHangupAccepted,

    /// The call ended because the call was declined by a different device.
    EndedRemoteHangupDeclined,

    /// The call ended because the call was declared busy by a different device.
    EndedRemoteHangupBusy,

    /// The call ended because of a remote busy message from a callee.
    EndedRemoteBusy,

    /// The call ended because of glare (received offer from same remote).
    EndedRemoteGlare,

    /// The call ended because it timed out during setup.
    EndedTimeout,

    /// The call ended because of an internal error condition.
    EndedInternalFailure,

    /// The call ended because a signaling message couldn't be sent.
    EndedSignalingFailure,

    /// The call ended because setting up the connection failed.
    EndedConnectionFailure,

    /// The call ended because the application wanted to drop the call.
    EndedAppDroppedCall,

    /// The remote side has enabled video.
    RemoteVideoEnable,

    /// The remote side has disabled video.
    RemoteVideoDisable,

    /// The call dropped while connected and is now reconnecting.
    Reconnecting,

    /// The call dropped while connected and is now reconnected.
    Reconnected,

    /// The received offer is expired.
    EndedReceivedOfferExpired,

    /// Received an offer while already handling an active call.
    EndedReceivedOfferWhileActive,

    /// Received an offer on a linked device from one that doesn't support multi-ring.
    EndedIgnoreCallsFromNonMultiringCallers,
}

impl Clone for ApplicationEvent {
    fn clone(&self) -> Self {
        *self
    }
}

impl fmt::Display for ApplicationEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Tracks the state of a connection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnectionState {
    /// No call in progress.
    Idle,

    /// Outgoing call is sending an offer.
    SendingOffer,

    /// Call is connecting ICE.  The `bool` is `true` if this end of
    /// the call has set both the *local* and *remote* SDP.
    IceConnecting(bool),

    /// ICE is connected.
    IceConnected,

    /// ICE failed to connect.
    IceConnectionFailed,

    /// ICE is reconnecting after an ICE disconnect event.
    IceReconnecting,

    /// The callee has accepted the call and the call is connected.
    CallConnected,

    /// The call is in the process of shutting down.
    Terminating,

    /// The call is completely closed.
    Closed,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// The call direction.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallDirection {
    /// Incoming call.
    InComing = 0,

    /// Outgoing call.
    OutGoing,
}

impl fmt::Display for CallDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl CallDirection {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => CallDirection::InComing,
            1 => CallDirection::OutGoing,
            _ => panic!("Unknown value: {}", value),
        }
    }
}

/// The supported feature level of the remote peer.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FeatureLevel {
    /// Unspecified by remote, usually means a legacy/older protocol.
    Unspecified = 0,

    /// Remote is multi-ring capable.
    MultiRing,
}

impl fmt::Display for FeatureLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FeatureLevel {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => FeatureLevel::Unspecified,
            1 => FeatureLevel::MultiRing,
            _ => panic!("Unknown value: {}", value),
        }
    }
}

/// Type of media for a call at time of origination.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallMediaType {
    /// Call should start as audio only.
    Audio = 0,

    /// Call should start as audio/video.
    Video,
}

impl fmt::Display for CallMediaType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl CallMediaType {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => CallMediaType::Audio,
            1 => CallMediaType::Video,
            _ => panic!("Unknown value: {}", value),
        }
    }
}

/// Type of hangup message.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HangupType {
    /// Normal hangup, typically remote user initiated.
    Normal = 0,

    /// Call was accepted elsewhere by a different device.
    Accepted,

    /// Call was declined elsewhere by a different device.
    Declined,

    // Call was declared busy elsewhere by a different device.
    Busy,
}

impl fmt::Display for HangupType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl HangupType {
    pub fn from_i32(value: i32) -> Self {
        match value {
            0 => HangupType::Normal,
            1 => HangupType::Accepted,
            2 => HangupType::Declined,
            3 => HangupType::Busy,
            _ => panic!("Unknown value: {}", value),
        }
    }
}

/// A grouping of parameters associated with a received Offer.
pub struct OfferParameters {
    /// The Offer SDP string.
    sdp:                     String,
    /// The approximate age of the offer message, in seconds.
    message_age_sec:         u64,
    /// The type of call indicated by the Offer.
    call_media_type:         CallMediaType,
    /// The local DeviceId of the client.
    local_device_id:         DeviceId,
    /// The feature level supported by the remote device.
    remote_feature_level:    FeatureLevel,
    /// If true, the local device is the primary device, otherwise a linked device.
    is_local_device_primary: bool,
}

impl OfferParameters {
    pub fn new(
        sdp: String,
        message_age_sec: u64,
        call_media_type: CallMediaType,
        local_device_id: DeviceId,
        remote_feature_level: FeatureLevel,
        is_local_device_primary: bool,
    ) -> Self {
        Self {
            sdp,
            message_age_sec,
            call_media_type,
            local_device_id,
            remote_feature_level,
            is_local_device_primary,
        }
    }

    pub fn sdp(&self) -> String {
        self.sdp.to_string()
    }

    pub fn message_age_sec(&self) -> u64 {
        self.message_age_sec
    }

    pub fn call_media_type(&self) -> CallMediaType {
        self.call_media_type
    }

    pub fn local_device_id(&self) -> DeviceId {
        self.local_device_id
    }

    pub fn remote_feature_level(&self) -> FeatureLevel {
        self.remote_feature_level
    }

    pub fn is_local_device_primary(&self) -> bool {
        self.is_local_device_primary
    }
}

/// A grouping of parameters associated with an Answer.
pub struct AnswerParameters {
    /// The Answer SDP string.
    sdp:                  String,
    /// The feature level supported by the remote device.
    remote_feature_level: FeatureLevel,
}

impl AnswerParameters {
    pub fn new(sdp: String, remote_feature_level: FeatureLevel) -> Self {
        Self {
            sdp,
            remote_feature_level,
        }
    }

    pub fn sdp(&self) -> String {
        self.sdp.to_string()
    }

    pub fn remote_feature_level(&self) -> FeatureLevel {
        self.remote_feature_level
    }
}

/// A grouping of parameters associated with a Hangup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HangupParameters {
    /// The type of hangup.
    hangup_type: HangupType,
    /// For some types, the id of the associated device for which the
    /// hangup refers to (e.g. the callee that accepted or declined
    /// a call).
    device_id:   Option<DeviceId>,
}

impl fmt::Display for HangupParameters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.device_id {
            Some(d) => write!(f, "{}/{}", self.hangup_type, d),
            None => write!(f, "{}/None", self.hangup_type),
        }
    }
}

impl HangupParameters {
    pub fn new(hangup_type: HangupType, device_id: Option<DeviceId>) -> Self {
        Self {
            hangup_type,
            device_id,
        }
    }

    pub fn hangup_type(&self) -> HangupType {
        self.hangup_type
    }

    pub fn device_id(&self) -> Option<DeviceId> {
        self.device_id
    }
}

/// A helper type to document when to use the legacy hangup message.
/// By default the legacy message definition will be used when we
/// want all devices to hangup.
pub const USE_LEGACY_HANGUP_MESSAGE: bool = true;

/// The label of the WebRTC DataChannel.
pub const DATA_CHANNEL_NAME: &str = "signaling";

// Benchmarking component list.
pub enum RingBench {
    Application,
    CallManager,
    Call,
    Connection,
    WebRTC,
    Network,
}

impl fmt::Display for RingBench {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RingBench::Application => "app",
                RingBench::CallManager => "cm",
                RingBench::Call => "call",
                RingBench::Connection => "conn",
                RingBench::WebRTC => "rtc",
                RingBench::Network => "net",
            }
        )
    }
}

#[macro_export]
macro_rules! ringbench {
    ($source:expr, $destination:expr, $operation:expr) => {
        info!(
            "ringrtc!\t{}\t{} -> {}: {}",
            match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                Ok(v) => v.as_millis(),
                Err(_) => 0,
            },
            $source,
            $destination,
            $operation
        );
    };
}
