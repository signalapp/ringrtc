//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Platform

use std::ffi::c_void;
use std::fmt;
use std::sync::Arc;

use crate::common::{ApplicationEvent, CallDirection, CallId, CallMediaType, DeviceId, Result};
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
use crate::core::signaling;
use crate::ios::api::call_manager_interface::{
    AppCallContext,
    AppConnectionInterface,
    AppIceCandidate,
    AppIceCandidateArray,
    AppInterface,
    AppObject,
};
use crate::ios::error::IOSError;
use crate::ios::ios_media_stream::IOSMediaStream;
use crate::ios::ios_util::*;

use crate::webrtc::media::MediaStream;
use crate::webrtc::peer_connection::{PeerConnection, RffiPeerConnectionInterface};
use crate::webrtc::peer_connection_observer::PeerConnectionObserver;

/// Concrete type for iOS AppIncomingMedia objects.
impl PlatformItem for IOSMediaStream {}

/// Concrete type for iOS AppConnection objects.
// @todo Make a type of connection with an Arc. Finalize better naming...
pub type AppConnectionX = Arc<AppConnectionInterface>;
impl PlatformItem for AppConnectionX {}

/// Concrete type for iOS AppCallContext objects.
pub type AppCallContextX = Arc<AppCallContext>;
impl PlatformItem for AppCallContextX {}

/// Concrete type for iOS AppRemotePeer objects.
impl PlatformItem for AppObject {}

/// iOS implementation of platform::Platform.
pub struct IOSPlatform {
    ///
    app_interface: AppInterface,
}

unsafe impl Sync for IOSPlatform {}
unsafe impl Send for IOSPlatform {}

impl fmt::Display for IOSPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        /*        let jni_owned_pc = match self.jni_owned_pc {
                    Some(v) => format!("0x{:x}", v),
                    None    => "None".to_string(),
                };
        */
        write!(f, "[n/a]")
    }
}

impl fmt::Debug for IOSPlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Drop for IOSPlatform {
    fn drop(&mut self) {
        info!("Dropping IOSPlatform");

        // Not currently dropping anything explicitly.
    }
}

impl Platform for IOSPlatform {
    type AppIncomingMedia = IOSMediaStream;
    type AppRemotePeer = AppObject;
    type AppConnection = AppConnectionX;
    type AppCallContext = AppCallContextX;

    fn create_connection(
        &mut self,
        call: &Call<Self>,
        remote_device_id: DeviceId,
        connection_type: ConnectionType,
        signaling_version: signaling::Version,
    ) -> Result<Connection<Self>> {
        info!(
            "create_connection(): call_id: {} remote_device_id: {}, signaling_version: {:?}",
            call.call_id(),
            remote_device_id,
            signaling_version
        );

        let connection = Connection::new(call.clone(), remote_device_id, connection_type)?;

        let connection_ptr = connection.get_connection_ptr()?;

        // Get the observer because we will need it when creating the
        // PeerConnection in Swift.
        let pc_observer = PeerConnectionObserver::new(connection_ptr)?;

        let app_connection_interface = (self.app_interface.onCreateConnectionInterface)(
            self.app_interface.object,
            pc_observer.rffi_interface() as *mut c_void,
            remote_device_id,
            call.call_context()?.object,
            signaling_version.enable_dtls(),
            signaling_version.enable_rtp_data_channel(),
        );

        if app_connection_interface.object.is_null() || app_connection_interface.pc.is_null() {
            return Err(IOSError::CreateAppPeerConnection.into());
        }

        debug!("app_connection_interface: {}", app_connection_interface);

        // Finish up the connection creation...

        // Retrieve the underlying PeerConnectionInterface object from the
        // application owned RTCPeerConnection object.
        let rffi_pc_interface = app_connection_interface.pc as *const RffiPeerConnectionInterface;
        if rffi_pc_interface.is_null() {
            return Err(IOSError::ExtractNativePeerConnectionInterface.into());
        }

        let pc_interface = PeerConnection::unowned(rffi_pc_interface);

        connection.set_pc_interface(pc_interface)?;

        info!("connection: {:?}", connection);

        connection.set_app_connection(Arc::new(app_connection_interface))?;

        debug!("Done with create_connection!");

        Ok(connection)
    }

    fn on_start_call(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        direction: CallDirection,
        call_media_type: CallMediaType,
    ) -> Result<()> {
        info!("on_start_call(): id: {}, direction: {}", call_id, direction);

        (self.app_interface.onStartCall)(
            self.app_interface.object,
            remote_peer.ptr,
            u64::from(call_id) as u64,
            direction == CallDirection::OutGoing,
            call_media_type as i32,
        );

        Ok(())
    }

    fn on_event(&self, remote_peer: &Self::AppRemotePeer, event: ApplicationEvent) -> Result<()> {
        info!("on_event(): {}", event);

        (self.app_interface.onEvent)(self.app_interface.object, remote_peer.ptr, event as i32);

        Ok(())
    }

    fn on_send_offer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        offer: signaling::Offer,
    ) -> Result<()> {
        // Offer messages are always broadcast
        // TODO: Simplify Swift's onSendOffer method to assume broadcast
        let broadcast = true;
        let receiver_device_id = 0 as DeviceId;

        info!("on_send_offer(): call_id: {}", call_id);

        (self.app_interface.onSendOffer)(
            self.app_interface.object,
            u64::from(call_id) as u64,
            remote_peer.ptr,
            receiver_device_id,
            broadcast,
            app_slice_from_bytes(offer.opaque.as_ref()),
            app_slice_from_str(offer.sdp.as_ref()),
            offer.call_media_type as i32,
        );

        Ok(())
    }

    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendAnswer,
    ) -> Result<()> {
        // Answers are never broadcast
        // TODO: Simplify Swift's onSendAnswer method to assume no broadcast
        let broadcast = false;
        let receiver_device_id = send.receiver_device_id;

        info!(
            "on_send_answer(): call_id: {}, receiver_device_id: {}",
            call_id, receiver_device_id
        );

        (self.app_interface.onSendAnswer)(
            self.app_interface.object,
            u64::from(call_id) as u64,
            remote_peer.ptr,
            receiver_device_id,
            broadcast,
            app_slice_from_bytes(send.answer.opaque.as_ref()),
            app_slice_from_str(send.answer.sdp.as_ref()),
        );

        Ok(())
    }

    fn on_send_ice(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendIce,
    ) -> Result<()> {
        let (broadcast, receiver_device_id) = match send.receiver_device_id {
            // The DeviceId doesn't matter if we're broadcasting
            None => (true, 0 as DeviceId),
            Some(receiver_device_id) => (false, receiver_device_id),
        };

        info!(
            "on_send_ice(): call_id: {}, receiver_device_id: {}, broadcast: {}",
            call_id, receiver_device_id, broadcast
        );

        if send.ice.candidates_added.is_empty() {
            return Ok(());
        }

        // The format of the IceCandidate structure is not enough for iOS,
        // so we will convert to a more appropriate structure.
        let mut app_ice_candidates: Vec<AppIceCandidate> = Vec::new();

        // Take a reference here so that we don't take ownership of it
        // and cause it to be dropped prematurely.
        for candidate in &send.ice.candidates_added {
            let app_ice_candidate = AppIceCandidate {
                opaque: app_slice_from_bytes(candidate.opaque.as_ref()),
                sdp:    app_slice_from_str(candidate.sdp.as_ref()),
            };

            app_ice_candidates.push(app_ice_candidate);
        }

        let app_ice_candidates_array = AppIceCandidateArray {
            candidates: app_ice_candidates.as_ptr(),
            count:      app_ice_candidates.len(),
        };
        // The ice_candidates array is passed up by reference and must
        // be consumed by the integration layer before returning.
        (self.app_interface.onSendIceCandidates)(
            self.app_interface.object,
            u64::from(call_id) as u64,
            remote_peer.ptr,
            receiver_device_id,
            broadcast,
            &app_ice_candidates_array,
        );

        Ok(())
    }

    fn on_send_hangup(
        &self,
        remote_peer: &Self::AppRemotePeer,
        call_id: CallId,
        send: signaling::SendHangup,
    ) -> Result<()> {
        // Hangups are always broadcast
        // TODO: Simplify Swift's onSendHangup method to assume broadcast
        let broadcast = true;
        let receiver_device_id = 0 as DeviceId;

        info!("on_send_hangup(): call_id: {}", call_id);

        let (hangup_type, hangup_device_id) = send.hangup.to_type_and_device_id();
        // We set the device_id to 0 in case it is not defined. It will
        // only be used for hangup types other than Normal.
        let hangup_device_id = hangup_device_id.unwrap_or(0 as DeviceId);

        (self.app_interface.onSendHangup)(
            self.app_interface.object,
            u64::from(call_id) as u64,
            remote_peer.ptr,
            receiver_device_id,
            broadcast,
            hangup_type as i32,
            hangup_device_id,
            send.use_legacy,
        );

        Ok(())
    }

    fn on_send_busy(&self, remote_peer: &Self::AppRemotePeer, call_id: CallId) -> Result<()> {
        // Busy messages are always broadcast
        // TODO: Simplify Java's onSendBusy method to assume broadcast
        let broadcast = true;
        let receiver_device_id = 0 as DeviceId;

        info!("on_send_busy(): call_id: {}", call_id);

        (self.app_interface.onSendBusy)(
            self.app_interface.object,
            u64::from(call_id) as u64,
            remote_peer.ptr,
            receiver_device_id,
            broadcast,
        );

        Ok(())
    }

    fn create_incoming_media(
        &self,
        connection: &Connection<Self>,
        incoming_media: MediaStream,
    ) -> Result<Self::AppIncomingMedia> {
        info!("create_incoming_media():");

        let app_connection_interface = connection.app_connection()?;

        // Create application level "AppMediaStreamInterface" object from here, which is created by
        // the Swift side.
        let app_media_stream_interface = (self.app_interface.onCreateMediaStreamInterface)(
            self.app_interface.object,
            app_connection_interface.object,
        );

        if app_media_stream_interface.object.is_null() {
            return Err(IOSError::CreateAppMediaStream.into());
        }

        // Pass this object and give ownership to a new IOSMediaStream object.
        IOSMediaStream::new(app_media_stream_interface, incoming_media)
    }

    fn connect_incoming_media(
        &self,
        remote_peer: &Self::AppRemotePeer,
        app_call_context: &Self::AppCallContext,
        incoming_media: &Self::AppIncomingMedia,
    ) -> Result<()> {
        info!("connect_incoming_media():");

        let ios_media_stream = incoming_media as &IOSMediaStream;
        let app_media_stream = ios_media_stream.get_ref()?;

        (self.app_interface.onConnectMedia)(
            self.app_interface.object,
            remote_peer.ptr,
            app_call_context.object,
            app_media_stream,
        );

        Ok(())
    }

    fn compare_remotes(
        &self,
        remote_peer1: &Self::AppRemotePeer,
        remote_peer2: &Self::AppRemotePeer,
    ) -> Result<bool> {
        info!("compare_remotes():");

        let result = (self.app_interface.onCompareRemotes)(
            self.app_interface.object,
            remote_peer1.ptr,
            remote_peer2.ptr,
        );

        Ok(result)
    }

    fn on_call_concluded(&self, remote_peer: &Self::AppRemotePeer) -> Result<()> {
        info!("on_call_concluded():");

        (self.app_interface.onCallConcluded)(self.app_interface.object, remote_peer.ptr);

        Ok(())
    }
}

impl IOSPlatform {
    /// Create a new IOSPlatform object.
    pub fn new(
        app_call_manager_interface: *mut c_void,
        app_interface: AppInterface,
    ) -> Result<Self> {
        debug!(
            "IOSPlatform::new: {:?} {:?}",
            app_call_manager_interface, app_interface
        );

        Ok(Self { app_interface })
    }
}

fn app_slice_from_bytes(bytes: Option<&Vec<u8>>) -> AppByteSlice {
    match bytes {
        None => AppByteSlice {
            bytes: std::ptr::null(),
            len:   0,
        },
        Some(bytes) => AppByteSlice {
            bytes: bytes.as_ptr(),
            len:   bytes.len(),
        },
    }
}

fn app_slice_from_str(s: Option<&String>) -> AppByteSlice {
    match s {
        None => AppByteSlice {
            bytes: std::ptr::null(),
            len:   0,
        },
        Some(s) => AppByteSlice {
            bytes: s.as_ptr(),
            len:   s.len(),
        },
    }
}
