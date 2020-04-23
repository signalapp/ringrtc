//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Peer Connection Interface
use std::ffi::CString;
use std::fmt;

use crate::common::Result;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::{DataChannel, RffiDataChannelInit};
use crate::webrtc::ice_candidate::IceCandidate;
use crate::webrtc::ice_gatherer::IceGatherer;
use crate::webrtc::sdp_observer::{
    CreateSessionDescriptionObserver,
    SessionDescriptionInterface,
    SetSessionDescriptionObserver,
};

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection as pc;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::peer_connection::{
    RffiDataChannelInterface,
    RffiPeerConnectionInterface,
};

#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection as pc;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::peer_connection::{
    RffiDataChannelInterface,
    RffiPeerConnectionInterface,
};

/// Rust wrapper around WebRTC C++ PeerConnectionInterface object.
pub struct PeerConnection {
    /// Pointer to C++ PeerConnectionInterface.
    rffi_pc_interface: *const RffiPeerConnectionInterface,
}

impl fmt::Display for PeerConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "pc_interface: {:p}", self.rffi_pc_interface)
    }
}

impl fmt::Debug for PeerConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

unsafe impl Send for PeerConnection {}
unsafe impl Sync for PeerConnection {}

impl PeerConnection {
    /// Create a new Rust PeerConnection object from a WebRTC C++
    /// PeerConnectionInterface object.
    pub fn new(rffi_pc_interface: *const RffiPeerConnectionInterface) -> Self {
        Self { rffi_pc_interface }
    }

    /// Rust wrapper around C++ PeerConnectionInterface::CreateDataChannel().
    pub fn create_data_channel(&self, label: String) -> Result<DataChannel> {
        let data_channel_label = CString::new(label)?;
        let data_channel_config = RffiDataChannelInit::new(true)?;

        let rffi_data_channel = unsafe {
            pc::Rust_createDataChannel(
                self.rffi_pc_interface,
                data_channel_label.as_ptr(),
                &data_channel_config,
            )
        };
        if rffi_data_channel.is_null() {
            return Err(RingRtcError::CreateDataChannel(data_channel_label.into_string()?).into());
        }

        let data_channel = DataChannel::new(rffi_data_channel);

        Ok(data_channel)
    }

    /// Rust wrapper around C++ webrtc::CreateSessionDescription(kOffer).
    pub fn create_offer(&self, csd_observer: &CreateSessionDescriptionObserver) {
        unsafe { pc::Rust_createOffer(self.rffi_pc_interface, csd_observer.rffi_observer()) }
    }

    /// Rust wrapper around C++ PeerConnectionInterface::SetLocalDescription().
    pub fn set_local_description(
        &self,
        ssd_observer: &SetSessionDescriptionObserver,
        desc: &SessionDescriptionInterface,
    ) {
        unsafe {
            pc::Rust_setLocalDescription(
                self.rffi_pc_interface,
                ssd_observer.rffi_observer(),
                desc.rffi_interface(),
            )
        }
    }

    /// Rust wrapper around C++ webrtc::CreateSessionDescription(kAnswer).
    pub fn create_answer(&self, csd_observer: &CreateSessionDescriptionObserver) {
        unsafe { pc::Rust_createAnswer(self.rffi_pc_interface, csd_observer.rffi_observer()) };
    }

    /// Rust wrapper around C++ PeerConnectionInterface::SetRemoteDescription().
    pub fn set_remote_description(
        &self,
        ssd_observer: &SetSessionDescriptionObserver,
        desc: &SessionDescriptionInterface,
    ) {
        unsafe {
            pc::Rust_setRemoteDescription(
                self.rffi_pc_interface,
                ssd_observer.rffi_observer(),
                desc.rffi_interface(),
            )
        };
    }

    /// Does something like:
    /// let sender = pc.get_audio_sender();
    /// sender.set_parameters({active: enabled});
    /// Which disables/enables the sending of any audio.
    /// Must be called *after* the answer has been set via
    /// set_remote_description or set_local_description.
    pub fn set_outgoing_audio_enabled(&self, enabled: bool) {
        unsafe {
            pc::Rust_setOutgoingAudioEnabled(self.rffi_pc_interface, enabled);
        }
    }

    /// Rust wrapper around C++ PeerConnectionInterface::AddIceCandidate().
    pub fn add_ice_candidate(&self, candidate: &IceCandidate) -> Result<()> {
        let clone = candidate.clone();
        let sdp_mid = CString::new(clone.sdp_mid)?;
        let sdp = CString::new(clone.sdp)?;
        let add_ok = unsafe {
            pc::Rust_addIceCandidate(
                self.rffi_pc_interface,
                sdp_mid.as_ptr(),
                clone.sdp_mline_index,
                sdp.as_ptr(),
            )
        };
        if add_ok {
            Ok(())
        } else {
            Err(RingRtcError::AddIceCandidate.into())
        }
    }

    // Rust wrapper around C++ PeerConnectionInterface::CreateSharedIceGatherer().
    pub fn create_shared_ice_gatherer(&self) -> Result<IceGatherer> {
        let rffi_ice_gatherer = unsafe { pc::Rust_createSharedIceGatherer(self.rffi_pc_interface) };
        if rffi_ice_gatherer.is_null() {
            return Err(RingRtcError::CreateIceGatherer.into());
        }

        let ice_gatherer = IceGatherer::new(rffi_ice_gatherer);

        Ok(ice_gatherer)
    }

    // Rust wrapper around C++ PeerConnectionInterface::UseSharedIceGatherer().
    pub fn use_shared_ice_gatherer(&self, ice_gatherer: &IceGatherer) -> Result<()> {
        let ok =
            unsafe { pc::Rust_useSharedIceGatherer(self.rffi_pc_interface, ice_gatherer.rffi()) };
        if ok {
            Ok(())
        } else {
            Err(RingRtcError::UseIceGatherer.into())
        }
    }
}
