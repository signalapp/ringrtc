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

    /// The call ended because the remote needs permission.
    EndedRemoteHangupNeedPermission,

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
    /// The connection has been created but not started
    /// (via start_outgoing_parent, start_outgoing_child, or start_incoming)
    NotYetStarted,

    /// The connection has been started, but the start method has not completed.
    /// After a connection is started, it will transition to either
    /// IceGathering (in the case of outgoing parent)
    /// or IceConnecting (in the case of outgoing child or incoming)
    Starting,

    /// The connection is gathering and sending ICE candidates
    /// (only for outgoing parent).
    /// It has a local description but not a remote description.
    /// This can only transition to Terminating/Closed
    IceGathering,

    /// ICE is attempting to connect, but has not yet.
    /// It has both the local and remote descriptions.
    /// This can transition to IceConnected or IceConnectionFailed
    IceConnecting,

    /// ICE has connected.
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

/// The label of the WebRTC DataChannel.
pub const DATA_CHANNEL_NAME: &str = "signaling";

// Benchmarking component list.
pub enum RingBench {
    App,
    CM,
    Call,
    Conn,
    WebRTC,
    Network,
}

impl fmt::Display for RingBench {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RingBench::App => "app",
                RingBench::CM => "cm",
                RingBench::Call => "call",
                RingBench::Conn => "conn",
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

#[macro_export]
macro_rules! ringbenchx {
    ($source:expr, $destination:expr, $operation:expr) => {
        info!(
            "ringrtc!\t{}\t{} -x {}: {}",
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
