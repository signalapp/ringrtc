//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use crate::common::CallMediaType;
use crate::common::{ApplicationEvent, CallDirection, CallId, DeviceId, Result};
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
use crate::core::signaling;
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
        recipient_id: &PeerId,
        call_id: CallId,
        receiver_device_id: Option<DeviceId>,
        msg: signaling::Message,
    ) -> Result<()>;
}

pub trait CallStateHandler {
    fn handle_call_state(&self, remote_peer_id: &PeerId, state: CallState) -> Result<()>;
    fn handle_remote_video_state(&self, remote_peer_id: &PeerId, enabled: bool) -> Result<()>;
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
    RemoteHangupNeedPermission,
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
            EndReason::RemoteHangupNeedPermission => "RemoteHangupNeedPermission",
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
    should_assume_messages_sent: bool,
    peer_connection_factory:     PeerConnectionFactory,
    signaling_sender:            Box<dyn SignalingSender + Send>,
    state_handler:               Box<dyn CallStateHandler + Send>,
    incoming_video_sink:         Box<dyn VideoSink + Send>,
}

impl NativePlatform {
    pub fn new(
        should_assume_messages_sent: bool,
        peer_connection_factory: PeerConnectionFactory,
        signaling_sender: Box<dyn SignalingSender + Send>,
        state_handler: Box<dyn CallStateHandler + Send>,
        incoming_video_sink: Box<dyn VideoSink + Send>,
    ) -> Self {
        Self {
            should_assume_messages_sent,
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
    ) -> Result<Connection<Self>> {
        info!(
            "NativePlatform::create_connection(): call: {} remote_device_id: {} signaling_version: {:?}",
            call, remote_device_id, signaling_version
        );

        // Like AndroidPlatform::create_connection
        let connection = Connection::new(call.clone(), remote_device_id, connection_type)?;
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
            signaling_version.enable_dtls(),
            signaling_version.enable_rtp_data_channel(),
        )?;

        connection.set_pc_interface(pc)?;
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
        incoming_media: &Self::AppIncomingMedia,
    ) -> Result<()> {
        info!("NativePlatform::connect_incoming_media()");
        if let Some(incoming_video_track) = incoming_media.first_video_track() {
            self.incoming_video_sink.set_enabled(true);
            // Note: this is passing an unsafe reference that must outlive
            // the VideoTrack/MediaStream.
            incoming_video_track.add_sink(self.incoming_video_sink.as_ref());
        }
        Ok(())
    }

    fn disconnect_incoming_media(&self, _app_call_context: &Self::AppCallContext) -> Result<()> {
        info!("NativePlatform::disconnect_incoming_media()");
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
            ApplicationEvent::LocalAccepted
            | ApplicationEvent::RemoteAccepted
            | ApplicationEvent::Reconnected => self.send_state(remote_peer, CallState::Connected),
            ApplicationEvent::Reconnecting => self.send_state(remote_peer, CallState::Connecting),
            ApplicationEvent::EndedLocalHangup => {
                self.send_state(remote_peer, CallState::Ended(EndReason::LocalHangup))
            }
            ApplicationEvent::EndedRemoteHangup => {
                self.send_state(remote_peer, CallState::Ended(EndReason::RemoteHangup))
            }
            ApplicationEvent::EndedRemoteHangupNeedPermission => self.send_state(
                remote_peer,
                CallState::Ended(EndReason::RemoteHangupNeedPermission),
            ),
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
            remote_peer, call_id, send.receiver_device_id, send.ice.candidates_added.len()
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
        let message = if send.use_legacy {
            signaling::Message::LegacyHangup(send.hangup)
        } else {
            signaling::Message::Hangup(send.hangup)
        };
        let receiver_device_id = None; // always broadcast

        self.send_signaling(remote_peer, call_id, receiver_device_id, message)?;
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
}
