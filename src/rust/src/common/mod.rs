//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Common types used throughout the library.

use std::fmt;

/// Common Result type, using `failure::Error` for Error.
pub type Result<T> = std::result::Result<T, failure::Error>;

/// Unique call identification number.
pub type CallId = u64;

/// Tracks the state of the call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallState {
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

impl fmt::Display for CallState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// The call direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallDirection {
    /// Incoming call.
    InComing,
    /// Outgoing call.
    OutGoing,
}

impl fmt::Display for CallDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// The label of the WebRTC DataChannel.
pub const DATA_CHANNEL_NAME: &str = "signaling";
