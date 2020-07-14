//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Peer Connection Interface
use std::ffi::CString;
use std::fmt;

use crate::common::{units::DataRate, Result};
use crate::core::signaling;
use crate::error::RingRtcError;
use crate::webrtc::data_channel::{DataChannel, RffiDataChannelInit};
use crate::webrtc::ice_gatherer::IceGatherer;
use crate::webrtc::sdp_observer::{
    CreateSessionDescriptionObserver,
    SessionDescriptionInterface,
    SetSessionDescriptionObserver,
};
use crate::webrtc::stats_observer::StatsObserver;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection as pc;
#[cfg(not(feature = "sim"))]
pub use crate::webrtc::ffi::peer_connection::{
    RffiDataChannelInterface,
    RffiPeerConnectionInterface,
};
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::ref_count;

#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection as pc;
#[cfg(feature = "sim")]
pub use crate::webrtc::sim::peer_connection::{
    RffiDataChannelInterface,
    RffiPeerConnectionInterface,
};
#[cfg(feature = "sim")]
use crate::webrtc::sim::ref_count;

/// Rust wrapper around WebRTC C++ PeerConnectionInterface object.
pub struct PeerConnection {
    /// Pointer to C++ PeerConnectionInterface.
    rffi_pc_interface: *const RffiPeerConnectionInterface,
    // If owned, release ref count when Dropped
    owned:             bool,
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

impl Drop for PeerConnection {
    fn drop(&mut self) {
        info!("PeerConnection::drop()");
        if self.owned && !self.rffi_pc_interface.is_null() {
            ref_count::release_ref(self.rffi_pc_interface as crate::core::util::CppObject);
        }
    }
}

unsafe impl Send for PeerConnection {}
unsafe impl Sync for PeerConnection {}

impl PeerConnection {
    /// Create a new Rust PeerConnection object from a WebRTC C++
    /// PeerConnectionInterface object.
    pub fn unowned(rffi_pc_interface: *const RffiPeerConnectionInterface) -> Self {
        let owned = false;
        Self {
            rffi_pc_interface,
            owned,
        }
    }

    pub fn owned(rffi_pc_interface: *const RffiPeerConnectionInterface) -> Self {
        let owned = true;
        Self {
            rffi_pc_interface,
            owned,
        }
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

        let data_channel = unsafe { DataChannel::new(rffi_data_channel) };

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

    pub fn set_incoming_rtp_enabled(&self, enabled: bool) {
        unsafe {
            pc::Rust_setIncomingRtpEnabled(self.rffi_pc_interface, enabled);
        }
    }

    /// Rust wrapper around C++ PeerConnectionInterface::AddIceCandidate().
    pub fn add_ice_candidate(&self, candidate: &signaling::IceCandidate) -> Result<()> {
        // ICE candidates are the same for V1 and V2, so this works for V1 as well.
        let v2_sdp = candidate.to_v2_sdp()?;
        let v2_sdp_c = CString::new(v2_sdp)?;
        let add_ok = unsafe { pc::Rust_addIceCandidate(self.rffi_pc_interface, v2_sdp_c.as_ptr()) };
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

    // Rust wrapper around C++ PeerConnectionInterface::GetStats().
    pub fn get_stats(&self, stats_observer: &StatsObserver) -> Result<()> {
        unsafe { pc::Rust_getStats(self.rffi_pc_interface, stats_observer.rffi_stats_observer()) };

        Ok(())
    }

    // Rust wrapper around C++ PeerConnectionInterface::SetBitrate().
    pub fn set_max_send_bitrate(&self, max_bitrate: DataRate) -> Result<()> {
        unsafe { pc::Rust_setMaxSendBitrate(self.rffi_pc_interface, max_bitrate.as_bps() as i32) };

        Ok(())
    }
}
