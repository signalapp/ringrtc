//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Call Connection Observer interface.
//!
//! The CallConnectionObserver is used to relay events and errors to
//! the client application.
use std::fmt;

/// An enum representing the status notification types sent to the
/// client application.
///
#[repr(i32)]
#[derive(Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClientEvent {
    /// The "ringing" event.
    ///
    /// ICE negotiation is complete, both sides communicating.
    Ringing = 0,
    /// The "connected" event.
    ///
    /// The remote side has connected the call.
    RemoteConnected,
    /// The "Video Disable" event.
    ///
    /// The remote has disabled video
    RemoteVideoEnable,
    /// The "Video Enable" event.
    ///
    /// The remote has enabled video
    RemoteVideoDisable,
    /// The "hangup" event.
    ///
    /// The remote side has hungup.
    RemoteHangup,
    /// The "call disconnected" event.
    ///
    /// The call failed to connect during the call setup phase.
    ConnectionFailed,
    /// The "call timeout" event.
    ///
    /// The call took too long to setup before connecting.
    CallTimeout,
    /// The "call reconnecting" event.
    ///
    /// The call dropped while connected and is now reconnecting.
    CallReconnecting,
}

impl Clone for ClientEvent {
    fn clone(&self) -> Self {
        *self
    }
}

impl fmt::Display for ClientEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Call Connection Observer trait
///
/// A platform implements this interface to send events and errors to
/// the client application.
pub trait CallConnectionObserver : Sync + Send + 'static {

    /// A platform specific type for holding a MediaStream
    type AppMediaStream;

    /// Notify the client application about an event
    fn notify_event(&self, event: ClientEvent);

    /// Notify the client application about an error
    fn notify_error(&self, error: failure::Error);

    /// Notify the client application about an available MediaStream
    fn notify_on_add_stream(&self, stream: Self::AppMediaStream);
}
