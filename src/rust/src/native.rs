//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use crate::common::{
    ApplicationEvent,
    CallDirection,
    CallId,
    ConnectionId,
    DeviceId,
    Result,
    DATA_CHANNEL_NAME,
};
use crate::common::{CallMediaType, HangupParameters};
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionForkingType};
use crate::core::platform::{Platform, PlatformItem};
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media::MediaStream;
use crate::webrtc::media::{AudioTrack, VideoSink, VideoSource};
use crate::webrtc::peer_connection_factory::{Certificate, IceServer, PeerConnectionFactory};
use crate::webrtc::peer_connection_observer::PeerConnectionObserver;
use std::fmt;

// This serves as the Platform::AppCallContext
// Users of the native platform must provide these things
// for each call.
#[derive(Clone)]
pub struct NativeCallContext {
    certificate:    Certificate,
    hide_ip:        bool,
    ice_server:     IceServer,
    outgoing_audio: AudioTrack,
    outgoing_video: VideoSource,
}

impl NativeCallContext {
    pub fn new(
        certificate: Certificate,
        hide_ip: bool,
        ice_server: IceServer,
        outgoing_audio: AudioTrack,
        outgoing_video: VideoSource,
    ) -> Self {
        Self {
            certificate,
            hide_ip,
            ice_server,
            outgoing_audio,
            outgoing_video,
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

// This serves as the Platform::AppMediaStream
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
        recipient_id: &PeerId,
        connection_id: ConnectionId,
        broadcast: bool,
        msg: SignalingMessage,
    ) -> Result<()>;
}

pub trait CallStateHandler {
    fn handle_call_state(&self, remote_peer_id: &PeerId, state: CallState) -> Result<()>;
    fn handle_remote_video_state(&self, remote_peer_id: &PeerId, enabled: bool) -> Result<()>;
}

// These are the signaling messages native clients need to be able to send
// and receive.  Closely tied to call_manager::SignalingMessageType.
// TODO: Should unify this with SignalingMessageType?
#[derive(Clone)]
pub enum SignalingMessage {
    Offer(CallMediaType, DeviceId, String),
    Answer(String),
    IceCandidates(Vec<IceCandidate>),
    Hangup(HangupParameters, bool),
    Busy,
}

impl fmt::Display for SignalingMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            SignalingMessage::Offer(media_type, local_device_id, _) => {
                format!("Offer({:?}, {}, ...)", media_type, local_device_id)
            }
            SignalingMessage::Answer(_) => format!("Answer(...)"),
            SignalingMessage::IceCandidates(_) => format!("IceCandidates(...)"),
            SignalingMessage::Hangup(parameters, use_legacy) => {
                format!("Hangup({:?}, {:?})", parameters, use_legacy)
            }
            SignalingMessage::Busy => format!("Busy"),
        };
        write!(f, "({})", display)
    }
}

impl fmt::Debug for SignalingMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

// These are the different states a call can be in.
// Closely tied with call_manager::ConnectionState and
// call_manager::CallState.
// TODO: Should we unify with ConnectionState and CallState?
pub enum CallState {
    Incoming(CallId, CallMediaType), // !connected || !accepted
    Outgoing(CallId, CallMediaType), // !connected || !accepted
    Ringing, //  connected && !accepted  (currently can be stuck here if you accept incoming before Ringing)
    Connected, //  connected &&  accepted
    Connecting, // !connected &&  accepted  (currently won't happen until after Connected)
    Ended(EndReason),
    Concluded,
}

impl fmt::Display for CallState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            CallState::Incoming(call_id, call_media_type) => {
                format!("Incoming({}, {})", call_id, call_media_type)
            }
            CallState::Outgoing(call_id, call_media_type) => {
                format!("Outgoing({}, {})", call_id, call_media_type)
            }
            CallState::Connected => format!("Connected"),
            CallState::Connecting => format!("Connecting"),
            CallState::Ringing => format!("Ringing"),
            CallState::Ended(reason) => format!("Ended({})", reason),
            CallState::Concluded => format!("Concluded"),
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
pub enum EndReason {
    LocalHangup,
    RemoteHangup,
    Declined,
    Busy, // Remote side is busy
    Glare,
    ReceivedOfferExpired,
    ReceivedOfferWhileActive,
    SignalingFailure,
    ConnectionFailure,
    InternalFailure,
    Timeout,
    AcceptedOnAnotherDevice,
    DeclinedOnAnotherDevice,
    BusyOnAnotherDevice,
    CallerIsNotMultiring,
}

impl fmt::Display for EndReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            EndReason::LocalHangup => "LocalHangup",
            EndReason::RemoteHangup => "RemoteHangup",
            EndReason::Declined => "Declined",
            EndReason::Busy => "Busy",
            EndReason::Glare => "Glare",
            EndReason::ReceivedOfferExpired => "ReceivedOfferExpired",
            EndReason::ReceivedOfferWhileActive => "ReceivedOfferWhileActive",
            EndReason::SignalingFailure => "SignalingFailure",
            EndReason::ConnectionFailure => "ConnectionFailure",
            EndReason::InternalFailure => "InternalFailure",
            EndReason::Timeout => "Timeout",
            EndReason::AcceptedOnAnotherDevice => "AcceptedOnAnotherDevice",
            EndReason::DeclinedOnAnotherDevice => "DeclinedOnAnotherDevice",
            EndReason::BusyOnAnotherDevice => "BusyOnAnotherDevice",
            EndReason::CallerIsNotMultiring => "CallerIsNotMultiring",
        };
        write!(f, "({})", display)
    }
}

impl fmt::Debug for EndReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

pub struct NativePlatform {
    peer_connection_factory: PeerConnectionFactory,
    signaling_sender:        Box<dyn SignalingSender + Send>,
    state_handler:           Box<dyn CallStateHandler + Send>,
    incoming_video_sink:     Box<dyn VideoSink + Send>,
}

impl NativePlatform {
    pub fn new(
        peer_connection_factory: PeerConnectionFactory,
        signaling_sender: Box<dyn SignalingSender + Send>,
        state_handler: Box<dyn CallStateHandler + Send>,
        incoming_video_sink: Box<dyn VideoSink + Send>,
    ) -> Self {
        Self {
            peer_connection_factory,
            signaling_sender,
            state_handler,
            incoming_video_sink,
        }
    }

    fn send_state(&self, peer_id: &PeerId, state: CallState) -> Result<()> {
        self.state_handler.handle_call_state(peer_id, state)
    }

    fn send_remote_video_state(&self, peer_id: &PeerId, enabled: bool) -> Result<()> {
        self.state_handler
            .handle_remote_video_state(peer_id, enabled)
    }

    fn send_signaling(
        &self,
        recipient_id: &PeerId,
        connection_id: ConnectionId,
        broadcast: bool,
        msg: SignalingMessage,
    ) -> Result<()> {
        self.signaling_sender
            .send_signaling(recipient_id, connection_id, broadcast, msg)
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
    type AppMediaStream = NativeMediaStream;

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
        forking_type: ConnectionForkingType,
    ) -> Result<Connection<Self>> {
        info!(
            "NativePlatform::create_connection(): call: {} remote_device_id: {}",
            call, remote_device_id
        );

        // Like AndroidPlatform::create_connection
        let connection = Connection::new(call.clone(), remote_device_id, forking_type)?;
        let context = call.call_context()?;

        // Like android::call_manager::create_peer_connection
        let pc_observer = PeerConnectionObserver::new(connection.get_connection_ptr()?)?;
        let pc = self.peer_connection_factory.create_peer_connection(
            pc_observer,
            context.certificate.clone(),
            context.hide_ip,
            &context.ice_server,
            context.outgoing_audio.clone(),
            context.outgoing_video.clone(),
        )?;
        if call.direction() == CallDirection::OutGoing {
            let dc_observer = DataChannelObserver::new(connection.clone())?;
            let dc = pc.create_data_channel(DATA_CHANNEL_NAME.to_string())?;
            unsafe { dc.register_observer(dc_observer.rffi_interface())? };
            connection.set_data_channel(dc)?;
            connection.set_data_channel_observer(dc_observer)?;
        }

        connection.set_pc_interface(pc)?;
        Ok(connection)
    }

    fn create_media_stream(
        &self,
        _connection: &Connection<Self>,
        stream: MediaStream,
    ) -> Result<Self::AppMediaStream> {
        info!("NativePlatform::create_media_stream()");
        Ok(stream)
    }

    fn on_connect_media(
        &self,
        _remote_peer: &Self::AppRemotePeer,
        _call_context: &Self::AppCallContext,
        media_stream: &Self::AppMediaStream,
    ) -> Result<()> {
        info!("NativePlatform::on_connect_media()");
        if let Some(incoming_video_track) = media_stream.incoming_video_track() {
            self.incoming_video_sink.set_enabled(true);
            // Note: this is passing an unsafe reference that must outlive
            // the VideoTrack/MediaStream.
            incoming_video_track.add_sink(self.incoming_video_sink.as_ref());
        }
        Ok(())
    }

    fn on_close_media(&self, _app_call_context: &Self::AppCallContext) -> Result<()> {
        info!("NativePlatform::on_close_media()");
        self.incoming_video_sink.set_enabled(false);
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
            match direction {
                CallDirection::OutGoing => CallState::Outgoing(call_id, call_media_type),
                CallDirection::InComing => CallState::Incoming(call_id, call_media_type),
            },
        )?;
        Ok(())
    }

    fn on_event(&self, remote_peer: &Self::AppRemotePeer, event: ApplicationEvent) -> Result<()> {
        info!(
            "NativePlatform::on_event(): remote_peer: {}, event: {}",
            remote_peer, event
        );

        match event {
            ApplicationEvent::LocalRinging | ApplicationEvent::RemoteRinging => {
                self.send_state(remote_peer, CallState::Ringing)
            }
            ApplicationEvent::LocalConnected
            | ApplicationEvent::RemoteConnected
            | ApplicationEvent::Reconnected => self.send_state(remote_peer, CallState::Connected),
            ApplicationEvent::Reconnecting => self.send_state(remote_peer, CallState::Connecting),
            ApplicationEvent::EndedLocalHangup => {
                self.send_state(remote_peer, CallState::Ended(EndReason::LocalHangup))
            }
            ApplicationEvent::EndedRemoteHangup => {
                self.send_state(remote_peer, CallState::Ended(EndReason::RemoteHangup))
            }
            ApplicationEvent::EndedRemoteBusy => {
                self.send_state(remote_peer, CallState::Ended(EndReason::Busy))
            }
            ApplicationEvent::EndedRemoteGlare => {
                self.send_state(remote_peer, CallState::Ended(EndReason::Glare))
            }
            ApplicationEvent::EndedTimeout => {
                self.send_state(remote_peer, CallState::Ended(EndReason::Timeout))
            }
            ApplicationEvent::EndedInternalFailure => {
                self.send_state(remote_peer, CallState::Ended(EndReason::InternalFailure))
            }
            ApplicationEvent::EndedSignalingFailure => {
                self.send_state(remote_peer, CallState::Ended(EndReason::SignalingFailure))
            }
            ApplicationEvent::EndedConnectionFailure => {
                self.send_state(remote_peer, CallState::Ended(EndReason::ConnectionFailure))
            }
            ApplicationEvent::EndedAppDroppedCall => {
                self.send_state(remote_peer, CallState::Ended(EndReason::Declined))
            }
            ApplicationEvent::EndedReceivedOfferExpired => self.send_state(
                remote_peer,
                CallState::Ended(EndReason::ReceivedOfferExpired),
            ),
            ApplicationEvent::EndedReceivedOfferWhileActive => self.send_state(
                remote_peer,
                CallState::Ended(EndReason::ReceivedOfferWhileActive),
            ),
            ApplicationEvent::EndedRemoteHangupAccepted => self.send_state(
                remote_peer,
                CallState::Ended(EndReason::AcceptedOnAnotherDevice),
            ),
            ApplicationEvent::EndedRemoteHangupDeclined => self.send_state(
                remote_peer,
                CallState::Ended(EndReason::DeclinedOnAnotherDevice),
            ),
            ApplicationEvent::EndedRemoteHangupBusy => self.send_state(
                remote_peer,
                CallState::Ended(EndReason::BusyOnAnotherDevice),
            ),
            ApplicationEvent::EndedIgnoreCallsFromNonMultiringCallers => self.send_state(
                remote_peer,
                CallState::Ended(EndReason::CallerIsNotMultiring),
            ),
            ApplicationEvent::RemoteVideoEnable => self.send_remote_video_state(remote_peer, true),
            ApplicationEvent::RemoteVideoDisable => {
                self.send_remote_video_state(remote_peer, false)
            }
        }?;
        Ok(())
    }

    fn on_call_concluded(&self, remote_peer: &Self::AppRemotePeer) -> Result<()> {
        info!(
            "NativePlatform::on_call_concluded(): remote_peer: {}",
            remote_peer
        );

        self.send_state(remote_peer, CallState::Concluded)?;
        Ok(())
    }

    fn assume_messages_sent(&self) -> bool {
        // TODO: Figure out how we can call message_sent() and avoid needing this.
        true
    }

    fn on_send_offer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        connection_id: ConnectionId,
        broadcast: bool,
        offer: &str,
        call_media_type: CallMediaType,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_offer(): remote_peer: {}, connection_id: {}, broadcast: {}",
            remote_peer, connection_id, broadcast
        );
        self.send_signaling(
            remote_peer,
            connection_id,
            broadcast,
            SignalingMessage::Offer(call_media_type, 0, offer.to_string()),
        )?;
        Ok(())
    }

    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        connection_id: ConnectionId,
        broadcast: bool,
        answer: &str,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_answer(): remote_peer: {}, connection_id: {}, broadcast: {}",
            remote_peer, connection_id, broadcast
        );
        self.send_signaling(
            remote_peer,
            connection_id,
            broadcast,
            SignalingMessage::Answer(answer.to_string()),
        )?;
        Ok(())
    }

    fn on_send_ice_candidates(
        &self,
        remote_peer: &Self::AppRemotePeer,
        connection_id: ConnectionId,
        broadcast: bool,
        ice_candidates: &[IceCandidate],
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_ice_candidates(): remote_peer: {}, connection_id: {}, broadcast: {}, candidates: {}",
            remote_peer, connection_id, broadcast, ice_candidates.len()
        );
        self.send_signaling(
            remote_peer,
            connection_id,
            broadcast,
            SignalingMessage::IceCandidates(ice_candidates.to_vec()),
        )?;
        Ok(())
    }

    fn on_send_hangup(
        &self,
        remote_peer: &Self::AppRemotePeer,
        connection_id: ConnectionId,
        broadcast: bool,
        hangup_parameters: HangupParameters,
        use_legacy_hangup_message: bool,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_hangup(): remote_peer: {}, connection_id: {}, broadcast: {}",
            remote_peer, connection_id, broadcast
        );
        self.send_signaling(
            remote_peer,
            connection_id,
            broadcast,
            SignalingMessage::Hangup(hangup_parameters, use_legacy_hangup_message),
        )?;
        Ok(())
    }

    fn on_send_busy(
        &self,
        remote_peer: &Self::AppRemotePeer,
        connection_id: ConnectionId,
        broadcast: bool,
    ) -> Result<()> {
        info!(
            "NativePlatform::on_send_busy(): remote_peer: {}, connection_id: {}, broadcast: {}",
            remote_peer, connection_id, broadcast
        );

        self.send_signaling(
            remote_peer,
            connection_id,
            broadcast,
            SignalingMessage::Busy,
        )?;
        Ok(())
    }
}
