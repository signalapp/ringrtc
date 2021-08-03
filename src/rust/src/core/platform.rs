//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Platform trait describing the interface an operating system platform must
/// implement for calling.
use std::collections::HashMap;
use std::fmt;

use crate::common::{
    ApplicationEvent,
    CallDirection,
    CallId,
    CallMediaType,
    DeviceId,
    HttpMethod,
    Result,
};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::{group_call, signaling};
use crate::webrtc::media::{MediaStream, VideoTrack};

/// A trait encompassing the traits the platform associated types must
/// implement.
pub trait PlatformItem: Sync + Send + 'static {}

/// A trait describing the interface an operating system platform must
/// implement for calling.
pub trait Platform: fmt::Debug + fmt::Display + Send + Sized + 'static {
    /// Opaque application specific incoming media object.
    type AppIncomingMedia: PlatformItem;

    /// Opaque application specific remote peer.
    type AppRemotePeer: PlatformItem + Clone;

    /// Opaque application specific connection.
    type AppConnection: PlatformItem + Clone;

    /// Opaque application specific call context.
    type AppCallContext: PlatformItem + Clone;

    /// Create platform specific Connection object.
    fn create_connection(
        &mut self,
        call: &Call<Self>,
        remote_device: DeviceId,
        connection_type: ConnectionType,
        signaling_version: signaling::Version,
        bandwidth_mode: BandwidthMode,
    ) -> Result<Connection<Self>>;

    /// Inform the client application that a call should be started.
    fn on_start_call(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        direction: CallDirection,
        call_media_type: CallMediaType,
    ) -> Result<()>;

    /// Notify the client application about an event.
    fn on_event(&self, remote_peer: &Self::AppRemotePeer, event: ApplicationEvent) -> Result<()>;

    /// Send an offer to a remote peer using the signaling
    /// channel.  Offers are always broadcast to all devices.
    fn on_send_offer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        offer: signaling::Offer,
    ) -> Result<()>;

    /// Send an answer to a remote peer using the signaling
    /// channel.
    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendAnswer,
    ) -> Result<()>;

    /// Send an ICE message to a remote peer using the signaling
    /// channel.
    fn on_send_ice(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendIce,
    ) -> Result<()>;

    /// Send a hangup message to a remote peer using the
    /// signaling channel.
    fn on_send_hangup(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendHangup,
    ) -> Result<()>;

    /// Send a call busy message to a remote peer using the
    /// signaling channel.  This always broadcasts to all devices.
    fn on_send_busy(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()>;

    /// Send a generic call message to a recipient using the
    /// signaling channel.
    fn send_call_message(&self, recipient_uuid: Vec<u8>, message: Vec<u8>) -> Result<()>;

    /// Send a generic HTTP request to the service using the application's
    /// HTTP stack and connection.
    fn send_http_request(
        &self,
        request_id: u32,
        url: String,
        method: HttpMethod,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<()>;

    /// Create a platform dependent media stream from the base WebRTC
    /// MediaStream.
    fn create_incoming_media(
        &self,
        connection: &Connection<Self>,
        incoming_media: MediaStream,
    ) -> Result<Self::AppIncomingMedia>;

    /// Connect incoming media to the application.
    fn connect_incoming_media(
        &self,
        remote_peer: &Self::AppRemotePeer,
        app_call_context: &Self::AppCallContext,
        incoming_media: &Self::AppIncomingMedia,
    ) -> Result<()>;

    /// Close the media associated with the call.
    fn disconnect_incoming_media(&self, _app_call_context: &Self::AppCallContext) -> Result<()> {
        Ok(())
    }

    /// Compare two remote peers for equality, returning true if
    /// equal, false otherwise.
    fn compare_remotes(
        &self,
        remote_peer1: &Self::AppRemotePeer,
        remote_peer2: &Self::AppRemotePeer,
    ) -> Result<bool>;

    /// Notify the application that the call is completely concluded
    fn on_call_concluded(&self, remote_peer: &Self::AppRemotePeer) -> Result<()>;

    /// Return true if you want a CallManager to always assume you called
    /// message_sent() for every signaling message.
    fn assume_messages_sent(&self) -> bool {
        false
    }

    // Group Calls

    fn handle_peek_response(
        &self,
        request_id: u32,
        joined_members: &[group_call::UserId],
        creator: Option<group_call::UserId>,
        era_id: Option<&str>,
        max_devices: Option<u32>,
        device_count: u32,
    );

    fn request_membership_proof(&self, client_id: group_call::ClientId);

    fn request_group_members(&self, client_id: group_call::ClientId);

    fn handle_connection_state_changed(
        &self,
        client_id: group_call::ClientId,
        connection_state: group_call::ConnectionState,
    );

    fn handle_join_state_changed(
        &self,
        client_id: group_call::ClientId,
        join_state: group_call::JoinState,
    );

    fn handle_remote_devices_changed(
        &self,
        client_id: group_call::ClientId,
        remote_device_states: &[group_call::RemoteDeviceState],
        _reason: group_call::RemoteDevicesChangedReason,
    );

    fn handle_incoming_video_track(
        &self,
        client_id: group_call::ClientId,
        remote_demux_id: group_call::DemuxId,
        incoming_video_track: VideoTrack,
    );

    fn handle_peek_changed(
        &self,
        client_id: group_call::ClientId,
        joined_members: &[group_call::UserId],
        creator: Option<group_call::UserId>,
        era_id: Option<&str>,
        max_devices: Option<u32>,
        device_count: u32,
    );

    fn handle_ended(&self, client_id: group_call::ClientId, reason: group_call::EndReason);
}
