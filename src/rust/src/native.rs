//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

use crate::common::{ApplicationEvent, CallDirection, CallId, CallMediaType, DeviceId, Result};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
use crate::core::{
    group_call::{self, GroupId, SignalingMessageUrgency},
    signaling,
};
use crate::lite::{
    sfu,
    sfu::{DemuxId, PeekInfo, PeekResult, UserId},
};
use crate::webrtc::media::MediaStream;
use crate::webrtc::media::{AudioTrack, VideoSink, VideoTrack};
use crate::webrtc::peer_connection::{AudioLevel, ReceivedAudioLevel};
use crate::webrtc::peer_connection_factory::{IceServer, PeerConnectionFactory};
use crate::webrtc::peer_connection_observer::{NetworkRoute, PeerConnectionObserver};

// This serves as the Platform::AppCallContext
// Users of the native platform must provide these things
// for each call.
#[derive(Clone)]
pub struct NativeCallContext {
    hide_ip: bool,
    ice_server: IceServer,
    outgoing_audio_track: AudioTrack,
    outgoing_video_track: VideoTrack,
    incoming_video_sink: Box<dyn VideoSink>,
}

impl NativeCallContext {
    pub fn new(
        hide_ip: bool,
        ice_server: IceServer,
        outgoing_audio_track: AudioTrack,
        outgoing_video_track: VideoTrack,
        incoming_video_sink: Box<dyn VideoSink>,
    ) -> Self {
        Self {
            hide_ip,
            ice_server,
            outgoing_audio_track,
            outgoing_video_track,
            incoming_video_sink,
        }
    }
}

impl PlatformItem for NativeCallContext {}

// This is how we refer to remote peers.
// You can think of every call as being identified by (PeerId, CallId)
// and every connection by (PeerId, CallId, DeviceId)
// This also serves as the Platform::AppRemotePeer
// TODO: Rename AppRemotePeer to AppRemoteUser and PeerId to UserId.
pub type PeerId = String;

impl PlatformItem for PeerId {}

// This serves as the Platform::AppConnection
// But since native PeerConnections are just PeerConnections,
// we don't need anything here.
#[derive(Clone)]
pub struct NativeConnection;

impl PlatformItem for NativeConnection {}

// This serves as the Platform::AppIncomingMedia
// But since native MediaStreams are just MediaStreams,
// we don't need anything here.
type NativeMediaStream = MediaStream;

impl PlatformItem for NativeMediaStream {}

// These are the callbacks that come from a NetworkPlatform:
// - signaling to send (SignalingSender)
// - state (CallStateHandler)
pub trait SignalingSender {
    fn send_signaling(
        &self,
        recipient_id: &str,
        call_id: CallId,
        receiver_device_id: Option<DeviceId>,
        msg: signaling::Message,
    ) -> Result<()>;

    fn send_call_message(
        &self,
        recipient_id: UserId,
        msg: Vec<u8>,
        urgency: SignalingMessageUrgency,
    ) -> Result<()>;

    fn send_call_message_to_group(
        &self,
        group_id: GroupId,
        msg: Vec<u8>,
        urgency: SignalingMessageUrgency,
    ) -> Result<()>;
}

pub trait CallStateHandler {
    fn handle_call_state(
        &self,
        remote_peer_id: &str,
        call_id: CallId,
        state: CallState,
    ) -> Result<()>;
    fn handle_remote_video_state(&self, remote_peer_id: &str, enabled: bool) -> Result<()>;
    fn handle_remote_sharing_screen(&self, remote_peer_id: &str, enabled: bool) -> Result<()>;
    fn handle_network_route(&self, remote_peer_id: &str, network_route: NetworkRoute)
        -> Result<()>;
    fn handle_audio_levels(
        &self,
        remote_peer_id: &str,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()>;
}

// These are the different states a call can be in.
// Closely tied with call_manager::ConnectionState and
// call_manager::CallState.
// TODO: Should we unify with ConnectionState and CallState?
pub enum CallState {
    Incoming(CallMediaType), // !connected || !accepted
    Outgoing(CallMediaType), // !connected || !accepted
    Ringing, //  connected && !accepted  (currently can be stuck here if you accept incoming before Ringing)
    Connected, //  connected &&  accepted
    Connecting, // !connected &&  accepted  (currently won't happen until after Connected)
    Ended(EndReason),
    Concluded,
}

impl fmt::Display for CallState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            CallState::Incoming(call_media_type) => {
                format!("Incoming({})", call_media_type)
            }
            CallState::Outgoing(call_media_type) => {
                format!("Outgoing({})", call_media_type)
            }
            CallState::Connected => "Connected".to_string(),
            CallState::Connecting => "Connecting".to_string(),
            CallState::Ringing => "Ringing".to_string(),
            CallState::Ended(reason) => format!("Ended({})", reason),
            CallState::Concluded => "Concluded".to_string(),
        };
        write!(f, "({})", display)
    }
}

impl fmt::Debug for CallState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

// These are the different reasons a call can end.
// Closely tied to call_manager::ApplicationEvent.
// TODO: Should we unify with ApplicationEvent?
#[derive(Debug)]
pub enum EndReason {
    LocalHangup,
    RemoteHangup,
    RemoteHangupNeedPermission,
    Declined,
    Busy, // Remote side is busy
    Glare,
    ReCall,
    ReceivedOfferExpired { age: Duration },
    ReceivedOfferWhileActive,
    ReceivedOfferWithGlare,
    SignalingFailure,
    GlareFailure,
    ConnectionFailure,
    InternalFailure,
    Timeout,
    AcceptedOnAnotherDevice,
    DeclinedOnAnotherDevice,
    BusyOnAnotherDevice,
}

impl fmt::Display for EndReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            EndReason::LocalHangup => "LocalHangup",
            EndReason::RemoteHangup => "RemoteHangup",
            EndReason::RemoteHangupNeedPermission => "RemoteHangupNeedPermission",
            EndReason::Declined => "Declined",
            EndReason::Busy => "Busy",
            EndReason::Glare => "Glare",
            EndReason::ReCall => "ReCall",
            EndReason::ReceivedOfferExpired { .. } => "ReceivedOfferExpired",
            EndReason::ReceivedOfferWhileActive => "ReceivedOfferWhileActive",
            EndReason::ReceivedOfferWithGlare => "ReceivedOfferWithGlare",
            EndReason::SignalingFailure => "SignalingFailure",
            EndReason::GlareFailure => "GlareFailure",
            EndReason::ConnectionFailure => "ConnectionFailure",
            EndReason::InternalFailure => "InternalFailure",
            EndReason::Timeout => "Timeout",
            EndReason::AcceptedOnAnotherDevice => "AcceptedOnAnotherDevice",
            EndReason::DeclinedOnAnotherDevice => "DeclinedOnAnotherDevice",
            EndReason::BusyOnAnotherDevice => "BusyOnAnotherDevice",
        };
        write!(f, "({})", display)
    }
}

// Group Calls

pub trait GroupUpdateHandler {
    fn handle_group_update(&self, update: GroupUpdate) -> Result<()>;
}

pub enum GroupUpdate {
    RequestMembershipProof(group_call::ClientId),
    RequestGroupMembers(group_call::ClientId),
    ConnectionStateChanged(group_call::ClientId, group_call::ConnectionState),
    JoinStateChanged(group_call::ClientId, group_call::JoinState),
    RemoteDeviceStatesChanged(group_call::ClientId, Vec<group_call::RemoteDeviceState>),
    PeekChanged {
        client_id: group_call::ClientId,
        peek_info: PeekInfo,
    },
    PeekResult {
        request_id: u32,
        peek_result: PeekResult,
    },
    Ended(group_call::ClientId, group_call::EndReason),
    Ring {
        group_id: group_call::GroupId,
        ring_id: group_call::RingId,
        sender: UserId,
        update: group_call::RingUpdate,
    },
    NetworkRouteChanged(group_call::ClientId, NetworkRoute),
    AudioLevels(group_call::ClientId, AudioLevel, Vec<ReceivedAudioLevel>),
}

impl fmt::Display for GroupUpdate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            GroupUpdate::RequestMembershipProof(_) => "GroupMembershipProof".to_string(),
            GroupUpdate::RequestGroupMembers(_) => "GroupMembers".to_string(),
            GroupUpdate::ConnectionStateChanged(_, _) => "ConnectionStateChanged".to_string(),
            GroupUpdate::JoinStateChanged(_, _) => "JoinStateChanged".to_string(),
            GroupUpdate::RemoteDeviceStatesChanged(_, _) => "RemoteDeviceStatesChanged".to_string(),
            GroupUpdate::PeekChanged { .. } => "PeekChanged".to_string(),
            GroupUpdate::PeekResult { .. } => "PeekResult".to_string(),
            GroupUpdate::Ended(_, reason) => format!("Ended({:?})", reason),
            GroupUpdate::Ring { update, .. } => format!("Ring({:?})", update),
            GroupUpdate::NetworkRouteChanged(_, network_route) => {
                format!("NetworkRouteChanged({:?})", network_route)
            }
            GroupUpdate::AudioLevels(_, captured_level, received_levels) => {
                format!("AudioLevels({:?}, {:?})", captured_level, received_levels)
            }
        };
        write!(f, "({})", display)
    }
}

impl fmt::Debug for GroupUpdate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

pub struct NativePlatform {
    // Relevant for both group calls and 1:1 calls
    peer_connection_factory: PeerConnectionFactory,

    // Only relevant for 1:1 calls
    signaling_sender: Box<dyn SignalingSender + Send>,
    should_assume_messages_sent: bool,
    state_handler: Box<dyn CallStateHandler + Send>,

    // Only relevant for group calls
    group_handler: Box<dyn GroupUpdateHandler + Send>,
}

impl NativePlatform {
    pub fn new(
        peer_connection_factory: PeerConnectionFactory,

        signaling_sender: Box<dyn SignalingSender + Send>,
        should_assume_messages_sent: bool,
        state_handler: Box<dyn CallStateHandler + Send>,

        group_handler: Box<dyn GroupUpdateHandler + Send>,
    ) -> Self {
        Self {
            peer_connection_factory,

            signaling_sender,
            should_assume_messages_sent,
            state_handler,

            group_handler,
        }
    }

    fn send_state(&self, peer_id: &str, call_id: CallId, state: CallState) -> Result<()> {
        self.state_handler
            .handle_call_state(peer_id, call_id, state)
    }

    fn send_network_route(&self, peer_id: &str, network_route: NetworkRoute) -> Result<()> {
        self.state_handler
            .handle_network_route(peer_id, network_route)
    }

    fn send_audio_levels(
        &self,
        peer_id: &str,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        self.state_handler
            .handle_audio_levels(peer_id, captured_level, received_level)
    }

    fn send_group_update(&self, update: GroupUpdate) -> Result<()> {
        self.group_handler.handle_group_update(update)
    }

    fn send_remote_video_state(&self, peer_id: &str, enabled: bool) -> Result<()> {
        self.state_handler
            .handle_remote_video_state(peer_id, enabled)
    }

    fn send_remote_sharing_screen(&self, peer_id: &str, enabled: bool) -> Result<()> {
        self.state_handler
            .handle_remote_sharing_screen(peer_id, enabled)
    }

    fn send_signaling(
        &self,
        recipient_id: &str,
        call_id: CallId,
        receiver_device_id: Option<DeviceId>,
        msg: signaling::Message,
    ) -> Result<()> {
        self.signaling_sender
            .send_signaling(recipient_id, call_id, receiver_device_id, msg)
    }
}

impl fmt::Display for NativePlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NativePlatform")
    }
}

impl fmt::Debug for NativePlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Platform for NativePlatform {
    type AppRemotePeer = PeerId;
    type AppCallContext = NativeCallContext;
    type AppConnection = NativeConnection;
    type AppIncomingMedia = NativeMediaStream;

    fn compare_remotes(
        &self,
        remote_peer1: &Self::AppRemotePeer,
        remote_peer2: &Self::AppRemotePeer,
    ) -> Result<bool> {
        info!(
            "NativePlatform::compare_remotes(): remote1: {}, remote2: {}",
            remote_peer1, remote_peer2
        );

        Ok(remote_peer1 == remote_peer2)
    }

    fn create_connection(
        &mut self,
        call: &Call<Self>,
        remote_device_id: DeviceId,
        connection_type: ConnectionType,
        signaling_version: signaling::Version,
        bandwidth_mode: BandwidthMode,
        audio_levels_interval: Option<Duration>,
    ) -> Result<Connection<Self>> {
        info!(
            "NativePlatform::create_connection(): call: {} remote_device_id: {} signaling_version: {:?}",
            call, remote_device_id, signaling_version
        );

        let context = call.call_context()?;
        // Like AndroidPlatform::create_connection
        let connection = Connection::new(
            call.clone(),
            remote_device_id,
            connection_type,
            bandwidth_mode,
            audio_levels_interval,
            Some(context.incoming_video_sink),
        )?;

        // Like android::call_manager::create_peer_connection
        let pc_observer = PeerConnectionObserver::new(
            connection.get_connection_ptr()?,
            false, /* enable_frame_encryption */
            true,  /* enable_video_frame_event */
            true,  /* enable_video_frame_content */
        )?;
        let pc = self.peer_connection_factory.create_peer_connection(
            pc_observer,
            context.hide_ip,
            &context.ice_server,
            context.outgoing_audio_track.clone(),
            Some(context.outgoing_video_track.clone()),
        )?;

        connection.set_peer_connection(pc)?;
        Ok(connection)
    }

    fn create_incoming_media(
        &self,
        _connection: &Connection<Self>,
        incoming_media: MediaStream,
    ) -> Result<Self::AppIncomingMedia> {
        info!("NativePlatform::create_incoming_media()");
        Ok(incoming_media)
    }

    fn connect_incoming_media(
        &self,
        _remote_peer: &Self::AppRemotePeer,
        _call_context: &Self::AppCallContext,
        _incoming_media: &Self::AppIncomingMedia,
    ) -> Result<()> {
        info!("NativePlatform::connect_incoming_media()");
        Ok(())
    }

    fn disconnect_incoming_media(&self, _call_context: &Self::AppCallContext) -> Result<()> {
        info!("NativePlatform::disconnect_incoming_media()");
        Ok(())
    }

    fn on_start_call(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        direction: CallDirection,
        call_media_type: CallMediaType,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_start_call(): remote_peer: {}, call_id: {}, direction: {}, call_media_type: {}",
            remote_peer, call_id, direction, call_media_type
        );
        self.send_state(
            remote_peer,
            call_id,
            match direction {
                CallDirection::OutGoing => CallState::Outgoing(call_media_type),
                CallDirection::InComing => CallState::Incoming(call_media_type),
            },
        )?;
        Ok(())
    }

    fn on_event(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        event: ApplicationEvent,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_event(): remote_peer: {}, event: {}",
            remote_peer, event
        );

        match event {
            ApplicationEvent::LocalRinging | ApplicationEvent::RemoteRinging => {
                self.send_state(remote_peer, call_id, CallState::Ringing)
            }
            ApplicationEvent::LocalAccepted
            | ApplicationEvent::RemoteAccepted
            | ApplicationEvent::Reconnected => {
                self.send_state(remote_peer, call_id, CallState::Connected)
            }
            ApplicationEvent::Reconnecting => {
                self.send_state(remote_peer, call_id, CallState::Connecting)
            }
            ApplicationEvent::EndedLocalHangup => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::LocalHangup),
            ),
            ApplicationEvent::EndedRemoteHangup => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::RemoteHangup),
            ),
            ApplicationEvent::EndedRemoteHangupNeedPermission => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::RemoteHangupNeedPermission),
            ),
            ApplicationEvent::EndedRemoteBusy => {
                self.send_state(remote_peer, call_id, CallState::Ended(EndReason::Busy))
            }
            ApplicationEvent::EndedRemoteGlare => {
                self.send_state(remote_peer, call_id, CallState::Ended(EndReason::Glare))
            }
            ApplicationEvent::EndedRemoteReCall => {
                self.send_state(remote_peer, call_id, CallState::Ended(EndReason::ReCall))
            }
            ApplicationEvent::EndedTimeout => {
                self.send_state(remote_peer, call_id, CallState::Ended(EndReason::Timeout))
            }
            ApplicationEvent::EndedInternalFailure => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::InternalFailure),
            ),
            ApplicationEvent::EndedSignalingFailure => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::SignalingFailure),
            ),
            ApplicationEvent::EndedGlareHandlingFailure => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::GlareFailure),
            ),
            ApplicationEvent::EndedConnectionFailure => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::ConnectionFailure),
            ),
            ApplicationEvent::EndedAppDroppedCall => {
                self.send_state(remote_peer, call_id, CallState::Ended(EndReason::Declined))
            }
            ApplicationEvent::ReceivedOfferExpired => {
                debug_assert!(false, "should use on_offer_expired instead");
                self.on_offer_expired(remote_peer, call_id, Duration::ZERO)
            }
            ApplicationEvent::ReceivedOfferWhileActive => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::ReceivedOfferWhileActive),
            ),
            ApplicationEvent::ReceivedOfferWithGlare => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::ReceivedOfferWithGlare),
            ),
            ApplicationEvent::EndedRemoteHangupAccepted => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::AcceptedOnAnotherDevice),
            ),
            ApplicationEvent::EndedRemoteHangupDeclined => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::DeclinedOnAnotherDevice),
            ),
            ApplicationEvent::EndedRemoteHangupBusy => self.send_state(
                remote_peer,
                call_id,
                CallState::Ended(EndReason::BusyOnAnotherDevice),
            ),
            ApplicationEvent::RemoteVideoEnable => self.send_remote_video_state(remote_peer, true),
            ApplicationEvent::RemoteVideoDisable => {
                self.send_remote_video_state(remote_peer, false)
            }
            ApplicationEvent::RemoteSharingScreenEnable => {
                self.send_remote_sharing_screen(remote_peer, true)
            }
            ApplicationEvent::RemoteSharingScreenDisable => {
                self.send_remote_sharing_screen(remote_peer, false)
            }
        }?;
        Ok(())
    }

    fn on_network_route_changed(
        &self,
        remote_peer: &Self::AppRemotePeer,
        network_route: NetworkRoute,
    ) -> Result<()> {
        trace!(
            "NativePlatform::on_network_route_changed(): {:?}",
            network_route
        );

        self.send_network_route(remote_peer, network_route)
    }

    fn on_audio_levels(
        &self,
        remote_peer: &Self::AppRemotePeer,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        trace!(
            "NativePlatform::on_audio_levels(): remote_peer: {}, captured_level: {}, received_level: {}",
            remote_peer, captured_level, received_level
        );

        self.send_audio_levels(remote_peer, captured_level, received_level)?;
        Ok(())
    }

    fn on_offer_expired(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        age: Duration,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_offer_expired(): remote_peer: {}, age: {:?}",
            remote_peer, age
        );

        self.send_state(
            remote_peer,
            call_id,
            CallState::Ended(EndReason::ReceivedOfferExpired { age }),
        )?;
        Ok(())
    }

    fn on_call_concluded(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()> {
        info!(
            "NativePlatform::on_call_concluded(): remote_peer: {}",
            remote_peer
        );

        self.send_state(remote_peer, call_id, CallState::Concluded)?;
        Ok(())
    }

    fn assume_messages_sent(&self) -> bool {
        self.should_assume_messages_sent
    }

    fn on_send_offer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        offer: signaling::Offer,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_offer(): remote_peer: {}, call_id: {}",
            remote_peer, call_id
        );
        let receiver_device_id = None; // always broadcast
        self.send_signaling(
            remote_peer,
            call_id,
            receiver_device_id,
            signaling::Message::Offer(offer),
        )?;
        Ok(())
    }

    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendAnswer,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_answer(): remote_peer: {}, call_id: {}",
            remote_peer, call_id
        );
        self.send_signaling(
            remote_peer,
            call_id,
            Some(send.receiver_device_id),
            signaling::Message::Answer(send.answer),
        )?;
        Ok(())
    }

    fn on_send_ice(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendIce,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_ice(): remote_peer: {}, call_id: {}, receiver_device_id: {:?}, candidates: {}",
            remote_peer, call_id, send.receiver_device_id, send.ice.candidates.len()
        );
        self.send_signaling(
            remote_peer,
            call_id,
            send.receiver_device_id,
            signaling::Message::Ice(send.ice),
        )?;
        Ok(())
    }

    fn on_send_hangup(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendHangup,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_hangup(): remote_peer: {}, call_id: {}",
            remote_peer, call_id
        );
        let receiver_device_id = None; // always broadcast
        self.send_signaling(
            remote_peer,
            call_id,
            receiver_device_id,
            signaling::Message::Hangup(send.hangup),
        )?;
        Ok(())
    }

    fn on_send_busy(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()> {
        info!(
            "NativePlatform::on_send_busy(): remote_peer: {}, call_id: {} ",
            remote_peer, call_id
        );
        let receiver_device_id = None; // always broadcast
        self.send_signaling(
            remote_peer,
            call_id,
            receiver_device_id,
            signaling::Message::Busy,
        )?;
        Ok(())
    }

    fn send_call_message(
        &self,
        recipient_uuid: Vec<u8>,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        info!("NativePlatform::send_call_message():");
        self.signaling_sender
            .send_call_message(recipient_uuid, message, urgency)
    }

    fn send_call_message_to_group(
        &self,
        group_id: Vec<u8>,
        message: Vec<u8>,
        urgency: group_call::SignalingMessageUrgency,
    ) -> Result<()> {
        info!("NativePlatform::send_call_message_to_group():");
        self.signaling_sender
            .send_call_message_to_group(group_id, message, urgency)
    }

    // Group Calls

    fn request_membership_proof(&self, client_id: group_call::ClientId) {
        info!(
            "NativePlatform::request_membership_proof(): id: {}",
            client_id
        );

        let result = self.send_group_update(GroupUpdate::RequestMembershipProof(client_id));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn request_group_members(&self, client_id: group_call::ClientId) {
        info!("NativePlatform::request_group_members(): id: {}", client_id);

        let result = self.send_group_update(GroupUpdate::RequestGroupMembers(client_id));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn handle_connection_state_changed(
        &self,
        client_id: group_call::ClientId,
        connection_state: group_call::ConnectionState,
    ) {
        info!(
            "NativePlatform::handle_connection_state_changed(): id: {}",
            client_id
        );

        let result = self.send_group_update(GroupUpdate::ConnectionStateChanged(
            client_id,
            connection_state,
        ));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn handle_network_route_changed(
        &self,
        client_id: group_call::ClientId,
        network_route: NetworkRoute,
    ) {
        info!(
            "NativePlatform::handle_network_route_changed(): {:?}",
            network_route
        );
        let result =
            self.send_group_update(GroupUpdate::NetworkRouteChanged(client_id, network_route));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn handle_audio_levels(
        &self,
        client_id: group_call::ClientId,
        captured_level: AudioLevel,
        received_levels: Vec<ReceivedAudioLevel>,
    ) {
        trace!("NativePlatform::handle_audio_levels(): id: {}", client_id);
        let result = self.send_group_update(GroupUpdate::AudioLevels(
            client_id,
            captured_level,
            received_levels,
        ));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn handle_join_state_changed(
        &self,
        client_id: group_call::ClientId,
        join_state: group_call::JoinState,
    ) {
        info!(
            "NativePlatform::handle_join_state_changed(): id: {}",
            client_id
        );

        let result = self.send_group_update(GroupUpdate::JoinStateChanged(client_id, join_state));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn handle_remote_devices_changed(
        &self,
        client_id: group_call::ClientId,
        remote_device_states: &[group_call::RemoteDeviceState],
        _reason: group_call::RemoteDevicesChangedReason,
    ) {
        info!(
            "NativePlatform::handle_remote_devices_changed(): id: {}",
            client_id
        );

        let result = self.send_group_update(GroupUpdate::RemoteDeviceStatesChanged(
            client_id,
            remote_device_states.to_vec(),
        ));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn handle_incoming_video_track(
        &self,
        client_id: group_call::ClientId,
        remote_demux_id: DemuxId,
        _incoming_video_track: VideoTrack,
    ) {
        info!(
            "NativePlatform::handle_incoming_video_track(): id: {}; remote_demux_id: {}",
            client_id, remote_demux_id
        );
    }

    fn handle_peek_changed(
        &self,
        client_id: group_call::ClientId,
        peek_info: &PeekInfo,
        _joined_members: &HashSet<UserId>,
    ) {
        info!(
            "NativePlatform::handle_peek_changed(): id: {}, era_id: {:?}, max_devices: {:?}, device_count: {}",
            client_id,
            peek_info.era_id,
            peek_info.max_devices,
            peek_info.devices.len()
        );

        let result = self.send_group_update(GroupUpdate::PeekChanged {
            client_id,
            peek_info: peek_info.clone(),
        });
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn handle_ended(&self, client_id: group_call::ClientId, reason: group_call::EndReason) {
        info!("NativePlatform::handle_ended(): id: {}", client_id);

        let result = self.send_group_update(GroupUpdate::Ended(client_id, reason));
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }

    fn group_call_ring_update(
        &self,
        group_id: group_call::GroupId,
        ring_id: group_call::RingId,
        sender: UserId,
        update: group_call::RingUpdate,
    ) {
        info!("NativePlatform::group_call_ring_update(): id: {}", ring_id);

        let result = self.send_group_update(GroupUpdate::Ring {
            group_id,
            ring_id,
            sender,
            update,
        });
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }
}

impl sfu::Delegate for NativePlatform {
    fn handle_peek_result(&self, request_id: u32, peek_result: PeekResult) {
        info!("NativePlatform::handle_peek_result(): id: {}", request_id);

        let result = self.send_group_update(GroupUpdate::PeekResult {
            request_id,
            peek_result,
        });
        if result.is_err() {
            error!("{:?}", result.err());
        }
    }
}
