//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! WebRTC Data Channel Interface.

use std::ffi::{CStr, CString};
use std::fmt;
use std::fmt::Debug;
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::Arc;

use bytes::BytesMut;
use prost::Message;

use crate::common::{units::DataRate, CallId, Result};
use crate::core::call_mutex::CallMutex;
use crate::core::signaling;
use crate::core::util::CppObject;
use crate::error::RingRtcError;
use crate::protobuf::data_channel::{Accepted, Data, Hangup, ReceiverStatus, SenderStatus};
use crate::webrtc::data_channel_observer::RffiDataChannelObserverInterface;

#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::{data_channel as dc, ref_count};

#[cfg(feature = "sim")]
use crate::webrtc::sim::{data_channel as dc, ref_count};

use crate::webrtc::peer_connection::RffiDataChannelInterface;

/// Rust friendly version of WebRTC DataChannelInit.
///
/// The definition is taken from [WebRTC RTCDataChannelInit]
/// (https://www.w3.org/TR/webrtc/#idl-def-rtcdatachannelinit).
///
/// See `struct DataChannelInit1 in
/// webrtc/src/api/data_channel_interface.h
#[repr(C)]
#[allow(non_snake_case)]
pub struct RffiDataChannelInit {
    // Deprecated. Reliability is assumed, and channel will be unreliable if
    // maxRetransmitTime or MaxRetransmits is set.
    pub reliable: bool,

    // True if ordered delivery is required.
    pub ordered: bool,

    // The max period of time in milliseconds in which retransmissions will be
    // sent. After this time, no more retransmissions will be sent. -1 if unset.
    //
    // Cannot be set along with |maxRetransmits|.
    pub maxRetransmitTime: c_int,

    // The max number of retransmissions. -1 if unset.
    //
    // Cannot be set along with |maxRetransmitTime|.
    pub maxRetransmits: c_int,

    // This is set by the application and opaque to the WebRTC
    // implementation.  Default is the empty string "".
    pub protocol: *const c_char,

    // True if the channel has been externally negotiated and we do not send an
    // in-band signalling in the form of an "open" message. If this is true, |id|
    // below must be set; otherwise it should be unset and will be negotiated
    // in-band.
    pub negotiated: bool,

    // The stream id, or SID, for SCTP data channels. -1 if unset (see above).
    pub id: c_int,
}

const DEFAULT_DATA_CHANNEL_PROTOCOL: &str = "";

impl RffiDataChannelInit {
    /// Create a new RffiDataChannelInit structure.
    pub fn new(ordered: bool) -> Result<Self> {
        let config = Self {
            reliable: false,
            ordered,
            maxRetransmitTime: -1,
            maxRetransmits: -1,
            protocol: CString::new(DEFAULT_DATA_CHANNEL_PROTOCOL)?.as_ptr(),
            negotiated: false,
            id: -1,
        };
        Ok(config)
    }
}

/// Rust wrapper around WebRTC C++ DataChannel object.
pub struct DataChannel {
    dc_interface:      *const RffiDataChannelInterface,
    reliable:          bool,
    accumulated_state: Arc<CallMutex<Data>>,
}

impl Debug for DataChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.dc_interface.fmt(f)
    }
}

// Implementing Sync and Sync required to share raw *const pointer
// across threads
unsafe impl Sync for DataChannel {}
unsafe impl Send for DataChannel {}

impl Drop for DataChannel {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl DataChannel {
    /// # Safety
    ///
    /// Create a new Rust DataChannel object from a WebRTC C++ DataChannel object.
    pub unsafe fn new(dc_interface: *const RffiDataChannelInterface) -> Self {
        let reliable = dc::Rust_dataChannelIsReliable(dc_interface);
        info!("data channel is reliable: {}", reliable);
        Self {
            dc_interface,
            reliable,
            accumulated_state: Arc::new(CallMutex::new(Data::default(), "accumulated_state")),
        }
    }

    /// Free resources related to the DataChannel object.
    pub fn dispose(&mut self) {
        if !self.dc_interface.is_null() {
            ref_count::release_ref(self.dc_interface as CppObject);
            self.dc_interface = ptr::null();
        }
    }

    /// # Safety
    ///
    /// Register a DataChannelObserver to this DataChannel.
    pub unsafe fn register_observer(
        &self,
        dc_observer: *const RffiDataChannelObserverInterface,
    ) -> Result<()> {
        debug!("register_data_channel_observer():");
        if dc_observer.is_null() {
            return Err(RingRtcError::NullPointer(
                "register_data_channel_observer".to_string(),
                "dc_observer".to_string(),
            )
            .into());
        }

        dc::Rust_registerDataChannelObserver(self.dc_interface, dc_observer);

        Ok(())
    }

    /// # Safety
    ///
    /// Unregister a DataChannelObserver from this DataChannel.
    pub unsafe fn unregister_observer(&self, dc_observer: *const RffiDataChannelObserverInterface) {
        debug!("unregister_data_channel_observer():");
        if dc_observer.is_null() {
            error!("Attempting to unregister a NULL data channel observer");
        } else {
            dc::Rust_unregisterDataChannelObserver(self.dc_interface, dc_observer);
        }
    }

    /// Return the label of this DataChannel object.
    pub fn get_label(&self) -> String {
        let string_ptr = unsafe { dc::Rust_dataChannelGetLabel(self.dc_interface) };
        if string_ptr.is_null() {
            String::from("UNKNOWN")
        } else {
            let label = unsafe { CStr::from_ptr(string_ptr).to_string_lossy().into_owned() };
            unsafe { libc::free(string_ptr as *mut libc::c_void) };
            label
        }
    }

    /// Send data via the DataChannel.
    fn send_data(&self, data: &Data) -> Result<()> {
        let mut bytes = BytesMut::with_capacity(data.encoded_len());
        data.encode(&mut bytes)?;

        let buffer: *const u8 = bytes.as_ptr();

        // Setting Binary to true relies on a custom change in WebRTC.
        let result =
            unsafe { dc::Rust_dataChannelSend(self.dc_interface, buffer, bytes.len(), true) };

        if result {
            Ok(())
        } else {
            Err(RingRtcError::DataChannelSend.into())
        }
    }

    /// Populates a data channel message using the supplied closure and sends it via the DataChannel.
    fn update_and_send<F>(&self, populate: F) -> Result<()>
    where
        F: FnOnce(&mut Data),
    {
        let message = if self.reliable {
            // Just send this one message by itself
            let mut single = Data::default();
            populate(&mut single);
            single
        } else {
            // Merge this message into accumulated_state and send out the latest version.
            let mut state = self.accumulated_state.lock()?;
            populate(&mut state);
            state.sequence_number = Some(state.sequence_number.unwrap_or(0) + 1);
            state.clone()
        };

        info!("Sending data channel message: {:?}", message);
        self.send_data(&message)
    }

    /// Send `Hangup` message via the DataChannel.
    pub fn send_hangup(&self, call_id: CallId, hangup: signaling::Hangup) -> Result<()> {
        let (hangup_type, hangup_device_id) = hangup.to_type_and_device_id();

        let mut hangup = Hangup::default();
        hangup.id = Some(u64::from(call_id));
        hangup.r#type = Some(hangup_type as i32);
        hangup.device_id = hangup_device_id;

        self.update_and_send(move |data| data.hangup = Some(hangup))
    }

    /// Send `accepted` message via the DataChannel.
    pub fn send_accepted(&self, call_id: CallId) -> Result<()> {
        let mut accepted = Accepted::default();
        accepted.id = Some(u64::from(call_id));

        self.update_and_send(move |data| data.accepted = Some(accepted))
    }

    /// Send `SenderStatus` message via the DataChannel.
    pub fn send_sender_status(&self, call_id: CallId, enabled: bool) -> Result<()> {
        let mut sender_status = SenderStatus::default();
        sender_status.id = Some(u64::from(call_id));
        sender_status.video_enabled = Some(enabled);

        self.update_and_send(move |data| data.sender_status = Some(sender_status))
    }

    /// Send `ReceiverStatus` message via the DataChannel.
    pub fn send_receiver_status(&self, call_id: CallId, max_bitrate: DataRate) -> Result<()> {
        let mut receiver_status = ReceiverStatus::default();
        receiver_status.id = Some(u64::from(call_id));
        receiver_status.max_bitrate_bps = Some(max_bitrate.as_bps());

        self.update_and_send(move |data| data.receiver_status = Some(receiver_status))
    }

    /// Sends the current accumulated state of the call.
    pub fn send_latest_state(&self) -> Result<()> {
        if self.reliable {
            // Reliable data channels handle retransmissions internally.
            Ok(())
        } else {
            let data = self.accumulated_state.lock()?;
            if *data != Data::default() {
                self.send_data(&data)
            } else {
                // Don't send empty messages
                Ok(())
            }
        }
    }
}
