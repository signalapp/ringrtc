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

use libc::size_t;

use crate::common::{
    ApplicationEvent,
    CallDirection,
    CallId,
    CallMediaType,
    ConnectionId,
    DeviceId,
    HangupParameters,
    Result,
    DATA_CHANNEL_NAME,
};
use crate::core::call::Call;
use crate::core::connection::{Connection, ConnectionType};
use crate::core::platform::{Platform, PlatformItem};
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

use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::media::MediaStream;
use crate::webrtc::peer_connection::{PeerConnection, RffiPeerConnectionInterface};
use crate::webrtc::peer_connection_observer::PeerConnectionObserver;

/// Concrete type for iOS AppMediaStream objects.
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
    type AppMediaStream = IOSMediaStream;
    type AppRemotePeer = AppObject;
    type AppConnection = AppConnectionX;
    type AppCallContext = AppCallContextX;

    fn create_connection(
        &mut self,
        call: &Call<Self>,
        remote_device: DeviceId,
        connection_type: ConnectionType,
    ) -> Result<Connection<Self>> {
        let connection_id = ConnectionId::new(call.call_id(), remote_device);

        info!("create_connection(): {}", connection_id);

        let connection = Connection::new(call.clone(), remote_device, connection_type)?;

        let connection_ptr = connection.get_connection_ptr()?;

        // Get the observer because we will need it when creating the
        // PeerConnection in Swift.
        let pc_observer = PeerConnectionObserver::new(connection_ptr)?;

        let app_connection_interface = (self.app_interface.onCreateConnectionInterface)(
            self.app_interface.object,
            pc_observer.rffi_interface() as *mut c_void,
            remote_device,
            call.call_context()?.object,
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

        if let CallDirection::OutGoing = connection.direction() {
            // Create data channel observer and data channel.
            let dc_observer = DataChannelObserver::new(connection.clone())?;
            let data_channel = pc_interface.create_data_channel(DATA_CHANNEL_NAME.to_string())?;
            unsafe { data_channel.register_observer(dc_observer.rffi_interface())? };
            connection.set_data_channel(data_channel)?;
            connection.set_data_channel_observer(dc_observer)?;
        }

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
        connection_id: ConnectionId,
        broadcast: bool,
        description: &str,
        call_media_type: CallMediaType,
    ) -> Result<()> {
        info!(
            "on_send_offer(): id: {}, broadcast: {}",
            connection_id, broadcast
        );

        let string_slice = AppByteSlice {
            bytes: description.as_ptr(),
            len:   description.len(),
        };

        (self.app_interface.onSendOffer)(
            self.app_interface.object,
            u64::from(connection_id.call_id()) as u64,
            remote_peer.ptr,
            connection_id.remote_device(),
            broadcast,
            string_slice,
            call_media_type as i32,
        );

        Ok(())
    }

    fn on_send_answer(
        &self,
        remote_peer: &Self::AppRemotePeer,
        connection_id: ConnectionId,
        broadcast: bool,
        description: &str,
    ) -> Result<()> {
        info!(
            "on_send_answer(): id: {}, broadcast: {}",
            connection_id, broadcast
        );

        let string_slice = AppByteSlice {
            bytes: description.as_ptr(),
            len:   description.len(),
        };

        (self.app_interface.onSendAnswer)(
            self.app_interface.object,
            u64::from(connection_id.call_id()) as u64,
            remote_peer.ptr,
            connection_id.remote_device(),
            broadcast,
            string_slice,
        );

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
            "on_send_ice_candidates(): id: {}, broadcast: {}",
            connection_id, broadcast
        );

        if ice_candidates.is_empty() {
            return Ok(());
        }

        // The format of the IceCandidate structure is not enough for iOS,
        // so we will convert to a more appropriate structure.
        let mut v: Vec<AppIceCandidate> = Vec::new();

        for candidate in ice_candidates {
            let sdp_mid_slice = AppByteSlice {
                bytes: candidate.sdp_mid.as_ptr(),
                len:   candidate.sdp_mid.len() as size_t,
            };

            let sdp_slice = AppByteSlice {
                bytes: candidate.sdp.as_ptr(),
                len:   candidate.sdp.len() as size_t,
            };

            let ice_candidate = AppIceCandidate {
                sdpMid:        sdp_mid_slice,
                sdpMLineIndex: candidate.sdp_mline_index,
                sdp:           sdp_slice,
            };

            v.push(ice_candidate);
        }

        let ice_candidates = AppIceCandidateArray {
            candidates: v.as_ptr(),
            count:      v.len(),
        };

        // The ice_candidates array is passed up by reference and must
        // be consumed by the integration layer before returning.
        (self.app_interface.onSendIceCandidates)(
            self.app_interface.object,
            u64::from(connection_id.call_id()) as u64,
            remote_peer.ptr,
            connection_id.remote_device(),
            broadcast,
            &ice_candidates,
        );

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
            "on_send_hangup(): id: {}, broadcast: {}",
            connection_id, broadcast
        );

        let device_id = match hangup_parameters.device_id() {
            Some(d) => d,
            // We set the device_id to 0 in case it is not defined. It will
            // only be used for hangup types other than Normal.
            None => 0,
        };

        (self.app_interface.onSendHangup)(
            self.app_interface.object,
            u64::from(connection_id.call_id()) as u64,
            remote_peer.ptr,
            connection_id.remote_device(),
            broadcast,
            hangup_parameters.hangup_type() as i32,
            device_id,
            use_legacy_hangup_message,
        );

        Ok(())
    }

    fn on_send_busy(
        &self,
        remote_peer: &Self::AppRemotePeer,
        connection_id: ConnectionId,
        broadcast: bool,
    ) -> Result<()> {
        info!(
            "on_send_busy(): id: {}, broadcast: {}",
            connection_id, broadcast
        );

        (self.app_interface.onSendBusy)(
            self.app_interface.object,
            u64::from(connection_id.call_id()) as u64,
            remote_peer.ptr,
            connection_id.remote_device(),
            broadcast,
        );

        Ok(())
    }

    fn create_media_stream(
        &self,
        connection: &Connection<Self>,
        stream: MediaStream,
    ) -> Result<Self::AppMediaStream> {
        info!("create_media_stream():");

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
        IOSMediaStream::new(app_media_stream_interface, stream)
    }

    fn on_connect_media(
        &self,
        remote_peer: &Self::AppRemotePeer,
        app_call_context: &Self::AppCallContext,
        media_stream: &Self::AppMediaStream,
    ) -> Result<()> {
        info!("on_connect_media():");

        let ios_media_stream = media_stream as &IOSMediaStream;
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
