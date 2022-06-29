//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Common types used throughout the library.

pub mod actor;
pub mod jni_signature;
pub mod units;

use std::fmt;

/// Common Result type, using `anyhow::Error` for Error.
pub type Result<T> = std::result::Result<T, anyhow::Error>;

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

/// Tracks the state of a call:
/// NotYetStarted
///      |
///      | start
///      V
/// WaitingToProceed -------------------------------->|
///      |                                            |
///      | proceed                                    |
///      V                                            |
/// ConnectingBeforeAccepted------------------------->|
///   |                        |                      |
///   | connected              | accepted             |
///   V                        |                      |
/// ConnectedBeforeAccepted--- V   ------------------>|
///   |                      ConnectingAfterAccepted->| terminate
///   |                        |                      |
///   | accepted               | connected            |
///   V                        V                      |
/// ConnectedAndAccepted           ------------------>|
///      |               ^                            |
///      | disconnected  | connected                  |
///      V               |                            |
/// ReconnectingAfterAccepted------------------------>|
///                                                   V
/// Terminating<---------------------------------------
///      |
///      | terminated
///      V
/// Terminated
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub enum CallState {
    /// The call has been created, but not yet started.
    NotYetStarted,

    /// The call has been started (via start_call() or handle_received_offer())
    /// but is waiting for the app to call proceed().
    /// Should transition to ConnectingBeforeAccepted when the app calls proceed().
    WaitingToProceed,

    /// Call is connecting (signaling and ICE) with the remote peer.
    /// We don't ring until we're connected with ICE to send an
    /// "accepted" message.
    ConnectingBeforeAccepted,

    /// The callee accepted before the caller is connected.
    /// This can happen, for example, if a remote callee gets ICE connected
    /// and accepts before the local caller gets ICE connected.
    ConnectingAfterAccepted,

    /// ICE is connected,
    /// But the callee has not yet accepted.
    ConnectedBeforeAccepted,

    /// ICE is connected and the callee has accepted.
    ConnectedAndAccepted,

    /// After ConnectedAndAccepted, has gone disconnected temporarily and is trying to reconnect.
    ReconnectingAfterAccepted,

    /// The call is in the process of terminating (hanging up).
    Terminating,

    /// The call is completely terminated.
    Terminated,
}

impl fmt::Display for CallState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// Note: The code below in both CallState and ConnectionState doesn't use "matches!" or "==" or
// default matching or similar because we want think about each state, especially for when
// we add more states in the future.

impl CallState {
    pub fn can_receive_ice_candidates(self) -> bool {
        match self {
            // Can't use ICE candidates because nothing has been started.
            CallState::NotYetStarted => false,

            // Can use ICE candidates before proceeding because they are stored until proceeding.
            CallState::WaitingToProceed => true,
            // Can use ICE candidates because connectivity is in progress.
            CallState::ConnectingBeforeAccepted => true,
            CallState::ConnectingAfterAccepted => true,
            CallState::ConnectedBeforeAccepted => true,
            CallState::ConnectedAndAccepted => true,
            CallState::ReconnectingAfterAccepted => true,

            // No point in ICE candidates any more.
            CallState::Terminating => false,
            CallState::Terminated => false,
        }
    }

    pub fn connected_or_reconnecting(self) -> bool {
        match self {
            CallState::NotYetStarted => false,
            CallState::WaitingToProceed => false,
            CallState::ConnectingBeforeAccepted => false,
            CallState::ConnectingAfterAccepted => false,

            CallState::ConnectedBeforeAccepted => true,
            CallState::ConnectedAndAccepted => true,
            CallState::ReconnectingAfterAccepted => true,

            CallState::Terminating => false,
            CallState::Terminated => false,
        }
    }

    pub fn can_be_accepted_locally(self) -> bool {
        match self {
            // Not even started/proceeding yet, so can't be accepted yet.
            CallState::NotYetStarted => false,
            CallState::WaitingToProceed => false,
            CallState::ConnectingBeforeAccepted => false,
            // We don't support accepting before connected yet, but we could in the future.
            CallState::ConnectingAfterAccepted => false,

            CallState::ConnectedBeforeAccepted => true,

            // Already accepted, so can't be accepted again.
            CallState::ConnectedAndAccepted => false,
            CallState::ReconnectingAfterAccepted => false,
            // Terminating, so can't be accepted any more.
            CallState::Terminating => false,
            CallState::Terminated => false,
        }
    }

    // AKA ConnectedAndAccepted or ReconnectingAfterAccepted
    pub fn active(self) -> bool {
        match self {
            CallState::NotYetStarted => false,
            CallState::WaitingToProceed => false,
            CallState::ConnectingBeforeAccepted => false,
            CallState::ConnectingAfterAccepted => false,
            CallState::ConnectedBeforeAccepted => false,

            CallState::ConnectedAndAccepted => true,
            CallState::ReconnectingAfterAccepted => true,

            CallState::Terminating => false,
            CallState::Terminated => false,
        }
    }

    pub fn can_send_hangup_via_rtp(self) -> bool {
        match self {
            CallState::NotYetStarted => false,

            // Not sure why these are true, but they preserve logic during refactoring.
            // Since the messasge won't go anywhere, we should consider removing it.
            CallState::WaitingToProceed => true,
            CallState::ConnectingBeforeAccepted => true,
            CallState::ConnectingAfterAccepted => true,

            CallState::ConnectedBeforeAccepted => true,
            CallState::ConnectedAndAccepted => true,
            CallState::ReconnectingAfterAccepted => true,
            CallState::Terminating => true,

            // Not sure why this is true, but it preserves logic during refactoring.
            // Since the messasge won't go anywhere, we should consider removing it.
            CallState::Terminated => true,
        }
    }

    pub fn should_propogate_hangup(self) -> bool {
        match self {
            CallState::NotYetStarted => false,

            CallState::WaitingToProceed => true,
            CallState::ConnectingBeforeAccepted => true,
            CallState::ConnectedBeforeAccepted => true,

            // Don't propagate if we're already accepted because a
            // Hangup/Accepted has been sent to the other callees.
            CallState::ConnectingAfterAccepted => false,
            CallState::ConnectedAndAccepted => false,
            CallState::ReconnectingAfterAccepted => false,
            // Don't propagate if we're already terminating/terminated because
            // we already sent out a Hangup to the other callees.
            CallState::Terminating => false,
            CallState::Terminated => false,
        }
    }

    pub fn can_be_terminated_remotely(self) -> bool {
        match self {
            // Can't be terminated if it hasn't started yet.
            CallState::NotYetStarted => false,

            CallState::WaitingToProceed => true,
            CallState::ConnectingBeforeAccepted => true,
            CallState::ConnectingAfterAccepted => true,
            CallState::ConnectedBeforeAccepted => true,
            CallState::ConnectedAndAccepted => true,
            CallState::ReconnectingAfterAccepted => true,

            // Already terminating or terminated, so can't be terminated again.
            CallState::Terminating => false,
            CallState::Terminated => false,
        }
    }

    pub fn terminating_or_terminated(self) -> bool {
        match self {
            CallState::NotYetStarted => false,
            CallState::WaitingToProceed => false,
            CallState::ConnectingBeforeAccepted => false,
            CallState::ConnectingAfterAccepted => false,
            CallState::ConnectedBeforeAccepted => false,
            CallState::ConnectedAndAccepted => false,
            CallState::ReconnectingAfterAccepted => false,

            CallState::Terminating => true,
            CallState::Terminated => true,
        }
    }
}

/// An enum representing the status notification types sent to the
/// client application.
///
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApplicationEvent {
    /// Inbound call only: The call signaling (ICE) is complete.
    LocalRinging = 0,

    /// Outbound call only: The call signaling (ICE) is complete.
    RemoteRinging,

    /// The local side has accepted the call.
    LocalAccepted,

    /// The remote side has accepted the call.
    RemoteAccepted,

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

    /// The call ended because of glare, receiving an offer from same remote
    /// while calling them.
    EndedRemoteGlare,

    /// The call ended because of recall, receiving an offer from same remote
    /// while still in an existing call with them.
    EndedRemoteReCall,

    /// The call ended because it timed out during setup.
    EndedTimeout,

    /// The call ended because of an internal error condition.
    EndedInternalFailure,

    /// The call ended because a signaling message couldn't be sent.
    EndedSignalingFailure,

    /// The call ended because setting up the connection failed.
    EndedConnectionFailure,

    /// The call ended because there was a failure during glare handling.
    EndedGlareHandlingFailure,

    /// The call ended because the application wanted to drop the call.
    EndedAppDroppedCall,

    /// The remote side has enabled video.
    RemoteVideoEnable,

    /// The remote side has disabled video.
    RemoteVideoDisable,

    /// The remote side has enabled screen sharing.
    RemoteSharingScreenEnable,

    /// The remote side has disabled screen sharing.
    RemoteSharingScreenDisable,

    /// The call dropped while connected and is now reconnecting.
    Reconnecting,

    /// The call dropped while connected and is now reconnected.
    Reconnected,

    /// The received offer is expired.
    ///
    /// This event should not be sent directly. Instead, use [`on_offer_expired`][],
    /// which preserves the age of the offer for platforms that need it.
    ///
    /// [`on_offer_expired`]: crate::core::platform::Platform::on_offer_expired
    ReceivedOfferExpired,

    /// Received an offer while already handling an active call.
    ReceivedOfferWhileActive,

    /// Received an offer while already handling an active call and glare
    /// was detected.
    ReceivedOfferWithGlare,
}

impl fmt::Display for ApplicationEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Tracks the state of a connection:
/// NotYetStarted
///      |
///      | start
///      V
/// Starting----------------------------------------------->|
///      |                              |                   |
///      | (outgoing child or incoming) | (outgoing parent) |
///      |                              V                   |
///      V                           IceGathering---------->|
/// ConnectingBeforeAccepted------------------------------->|
///   |                        |                      |
///   | connected              | accepted             |
///   V                        |                      |
/// ConnectedBeforeAccepted--- V   ------------------>|
///   |                      ConnectingAfterAccepted->| terminate
///   |                        |                      |
///   | accepted               | connected            |
///   V                        V                      |
/// ConnectedAndAccepted           ------------------>|
///      |               ^                            |
///      | disconnected  | connected                  |
///      V               |                            |
/// ReconnectingAfterAccepted------------------------>|
///                                                   V
/// Terminating<---------------------------------------
///      |                                            ^
///      | terminated                                 |
///      V                                            |
/// Terminated                                        |
///                                                   |
/// (this should go above, but it's hard to draw)     |
/// ConnectingBeforeAccepted|ConnectingAfterAccepted  |
///         |                                         |
///         | ICE failed                              |
///         V                                         |
/// IceFailed --------------------------------------->|

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionState {
    /// The connection has been created but not started
    /// (via start_outgoing_parent, start_outgoing_child, or start_incoming)
    NotYetStarted,

    /// The connection has been started, but the start method has not completed.
    /// After a connection is started, it will transition to either
    /// IceGathering (in the case of outgoing parent)
    /// or ConnectingBeforeAccepted (in the case of outgoing child or incoming)
    Starting,

    /// The connection is gathering and sending ICE candidates
    /// (only for outgoing parent).
    /// It has a local description but not a remote description.
    /// This can only transition to Terminating/Closed
    IceGathering,

    /// ICE is attempting to connect, but has not yet.
    /// It has both the local and remote descriptions.
    /// This can transition to ConnectedBeforeAccepted or IceFailed
    ConnectingBeforeAccepted,

    /// The call has been accepted, but ICE hasn't connected yet.
    ConnectingAfterAccepted,

    /// ICE has connected, but the call hasn't been accepted yet.
    ConnectedBeforeAccepted,

    /// ICE failed to connect.
    IceFailed,

    /// The callee has accepted the call and the call is connected.
    ConnectedAndAccepted,

    /// ICE is disconnected/reconnecting after the call is accepted.
    ReconnectingAfterAccepted,

    /// The connection is in the process of terminating
    Terminating,

    /// The connection is completely terminated
    Terminated,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ConnectionState {
    pub fn can_send_ice_candidates(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,

            ConnectionState::Starting => true,
            ConnectionState::IceGathering => true,
            ConnectionState::ConnectingBeforeAccepted => true,
            ConnectionState::ConnectingAfterAccepted => true,
            ConnectionState::ConnectedBeforeAccepted => true,
            ConnectionState::ConnectedAndAccepted => true,
            ConnectionState::ReconnectingAfterAccepted => true,
            // Not sure why this is true, but it preserves logic while refactoring
            // Consider changing it to false because it won't do anything.
            ConnectionState::IceFailed => true,

            ConnectionState::Terminating => false,
            ConnectionState::Terminated => false,
        }
    }

    pub fn can_receive_ice_candidates(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,
            ConnectionState::Starting => false,
            ConnectionState::IceGathering => false,

            ConnectionState::ConnectingBeforeAccepted => true,
            ConnectionState::ConnectingAfterAccepted => true,
            ConnectionState::ConnectedBeforeAccepted => true,
            ConnectionState::ConnectedAndAccepted => true,
            ConnectionState::ReconnectingAfterAccepted => true,

            ConnectionState::IceFailed => false,
            ConnectionState::Terminating => false,
            ConnectionState::Terminated => false,
        }
    }

    pub fn connecting_or_connected(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,
            ConnectionState::Starting => false,
            ConnectionState::IceGathering => false,

            ConnectionState::ConnectingBeforeAccepted => true,
            ConnectionState::ConnectingAfterAccepted => true,
            ConnectionState::ConnectedBeforeAccepted => true,
            ConnectionState::ConnectedAndAccepted => true,
            ConnectionState::ReconnectingAfterAccepted => true,

            ConnectionState::IceFailed => false,
            ConnectionState::Terminating => false,
            ConnectionState::Terminated => false,
        }
    }

    pub fn connected_or_reconnecting(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,
            ConnectionState::Starting => false,
            ConnectionState::IceGathering => false,
            ConnectionState::ConnectingBeforeAccepted => false,
            ConnectionState::ConnectingAfterAccepted => false,

            ConnectionState::ConnectedBeforeAccepted => true,
            ConnectionState::ConnectedAndAccepted => true,
            ConnectionState::ReconnectingAfterAccepted => true,

            ConnectionState::IceFailed => false,
            ConnectionState::Terminating => false,
            ConnectionState::Terminated => false,
        }
    }

    pub fn can_be_accepted_locally(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,
            ConnectionState::Starting => false,
            ConnectionState::IceGathering => false,

            // Not sure why this is true, but it preserves logic while refactoring.
            // We should consider changing it because we don't support
            // accepting locally before ICE is connected.
            ConnectionState::ConnectingBeforeAccepted => true,

            ConnectionState::ConnectingAfterAccepted => false,

            ConnectionState::ConnectedBeforeAccepted => true,

            ConnectionState::ConnectedAndAccepted => false,

            // Not sure why this is true, but it preserves logic while refactoring.
            // We should consider changing it because it doesn't
            // make sense to accept after accepting
            ConnectionState::ReconnectingAfterAccepted => true,

            ConnectionState::IceFailed => false,
            ConnectionState::Terminating => false,
            ConnectionState::Terminated => false,
        }
    }

    pub fn active(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,
            ConnectionState::Starting => false,
            ConnectionState::IceGathering => false,
            ConnectionState::ConnectingBeforeAccepted => false,
            ConnectionState::ConnectingAfterAccepted => false,
            ConnectionState::ConnectedBeforeAccepted => false,

            ConnectionState::ConnectedAndAccepted => true,
            ConnectionState::ReconnectingAfterAccepted => true,

            ConnectionState::IceFailed => false,
            ConnectionState::Terminating => false,
            ConnectionState::Terminated => false,
        }
    }

    pub fn can_send_hangup_via_rtp(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,

            // Not sure why these are true, but they preserve logic during refactoring.
            // We should consider making them false because the messages
            // can't go anywhere.
            ConnectionState::Starting => true,
            ConnectionState::IceGathering => true,
            ConnectionState::ConnectingBeforeAccepted => true,
            ConnectionState::ConnectingAfterAccepted => true,

            ConnectionState::ConnectedBeforeAccepted => true,
            ConnectionState::ConnectedAndAccepted => true,
            ConnectionState::ReconnectingAfterAccepted => true,

            // Not sure why these are true, but they preserve logic during refactoring.
            // We should consider making them false because the messages
            // can't go anywhere.
            ConnectionState::IceFailed => true,
            ConnectionState::Terminating => true,
            ConnectionState::Terminated => true,
        }
    }

    pub fn terminating_or_terminated(self) -> bool {
        match self {
            ConnectionState::NotYetStarted => false,
            ConnectionState::Starting => false,
            ConnectionState::IceGathering => false,
            ConnectionState::ConnectingBeforeAccepted => false,
            ConnectionState::ConnectingAfterAccepted => false,
            ConnectionState::ConnectedBeforeAccepted => false,
            ConnectionState::ConnectedAndAccepted => false,
            ConnectionState::ReconnectingAfterAccepted => false,
            ConnectionState::IceFailed => false,

            ConnectionState::Terminating => true,
            ConnectionState::Terminated => true,
        }
    }
}

/// The call direction.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// Type of media for a call at time of origination.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

// Benchmarking component list.
pub enum RingBench {
    App,
    Cm,
    Call,
    Conn,
    WebRtc,
    Network,
}

impl fmt::Display for RingBench {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RingBench::App => "app",
                RingBench::Cm => "cm",
                RingBench::Call => "call",
                RingBench::Conn => "conn",
                RingBench::WebRtc => "rtc",
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
