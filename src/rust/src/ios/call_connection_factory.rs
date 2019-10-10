//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS CallConnectionFactory Interface.

use std::sync::{
    Arc,
    Mutex,
};

use std::ffi::c_void;

use crate::ios::call_connection_observer::IOSCallConnectionObserver;
use crate::ios::error::iOSError;
use crate::ios::ios_platform::IOSPlatform;
use crate::ios::webrtc_app_peer_connection::appCreatePeerConnection;
use crate::ios::ios_util::*;
use crate::common::{
    Result,
    CallId,
    CallDirection,
    DATA_CHANNEL_NAME,
};
use crate::core::call_connection_factory::CallConnectionFactory;
use crate::core::util::{
    CppObject,
    ptr_as_arc_mutex,
    ptr_as_arc_ptr,
    ptr_as_box,
};
use crate::error::RingRtcError;
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::peer_connection_observer::PeerConnectionObserver;
use crate::webrtc::peer_connection::{
    PeerConnection,
    RffiPeerConnectionInterface,
};

/// Public type for iOS CallConnectionFactory.
pub type IOSCallConnectionFactory = CallConnectionFactory<IOSPlatform>;

/// Creates a new IOSCallConnectionFactory object.
pub fn native_create_call_connection_factory(app_call_connection_factory: *mut AppPeerConnectionFactory) -> Result<*mut c_void> {
    let cc_factory = IOSCallConnectionFactory::new(app_call_connection_factory as CppObject)?;
    // Wrap factory in Arc<Mutex<>> to pass amongst threads
    let cc_factory = Arc::new(Mutex::new(cc_factory));
    let ptr = Arc::into_raw(cc_factory);
    Ok(ptr as *mut c_void)
}

/// Frees a IOSCallConnectionFactory object.
pub fn native_free_factory(factory: *mut IOSCallConnectionFactory) -> Result<()> {
    // Convert pointer back into Arc, then Arc will free things up as
    // it goes out of scope.
    let call_connection_factory = unsafe { ptr_as_arc_mutex(factory)? };
    if let Ok(mut cc_factory) = call_connection_factory.lock() {
        cc_factory.close()?;
    }

    Ok(())
}

/// Create a Rust CallConnectionInterface for iOS.
///
/// # Arguments
///
/// * `native_factory` - raw pointer to Rust CallConnectionFactory
/// * `app_call_connection` - raw pointer to Swift CallConnection
/// * `call_config` - reference to IOSCallConfig structure
/// * `native_observer` - raw pointer to Rust CallConnectionObserver
/// * `rtc_config` - raw pointer to RTCConfiguration
/// * `rtc_constraints` - raw pointer to RTCMediaConstraints
///
#[allow(clippy::too_many_arguments)]
pub fn native_create_call_connection(native_factory: *mut IOSCallConnectionFactory,
                                app_call_connection: *mut AppCallConnection,
                                        call_config: IOSCallConfig,
                                    native_observer: *mut IOSCallConnectionObserver,
                                         rtc_config: *mut c_void,
                                    rtc_constraints: *mut c_void) -> Result<*mut c_void> {

    let call_connection_factory = unsafe { ptr_as_arc_ptr(native_factory)? };
    let mut cc_factory = match call_connection_factory.get_arc().lock() {
        Ok(v) => v,
        Err(_) => return Err(RingRtcError::MutexPoisoned("Call Connection Factory".to_string()).into()),
    };

    // Get callId from configuration
    let call_id: CallId = call_config.callId;

    // Get recipient from configuration
    let recipient: IOSRecipient = call_config.recipient;

    // Get call direction from configuration
    let out_bound: bool = call_config.outBound;

    let direction = if out_bound {
        CallDirection::OutGoing
    } else {
        CallDirection::InComing
    };

    let platform = IOSPlatform::new(recipient);
    let call_connection = cc_factory.create_call_connection(call_id, direction, platform)?;
    info!("call_connection: {:?}", call_connection);

    let data_channel_cc = call_connection.clone();

    let cc_box = Box::new(call_connection);
    let cc_ptr = Box::into_raw(cc_box);

    let pc_observer = PeerConnectionObserver::new(cc_ptr)?;

    // construct iOS OwnedPeerConnection object
    let webrtc_owned_pc = unsafe {
        appCreatePeerConnection(
            cc_factory.get_native_peer_connection_factory() as *mut c_void,
            app_call_connection as *mut c_void,
            rtc_config,
            rtc_constraints,
            pc_observer.rffi_interface() as *mut c_void)
    };
    info!("webrtc_owned_pc: {:?}", webrtc_owned_pc);

    if webrtc_owned_pc as i64 == 0 {
        return Err(iOSError::CreateAppPeerConnection.into());
    }

    // Retrieve the underlying PeerConnectionInterface object from the
    // Objc owned peerconnection object.
    // @note This is already provided by webrtc_owned_pc.
    let rffi_pc_interface = webrtc_owned_pc as *const RffiPeerConnectionInterface;
    if rffi_pc_interface.is_null() {
        return Err(iOSError::ExtractNativePeerConnectionInterface.into());
    }

    let pc_interface = PeerConnection::new(rffi_pc_interface);

    // Convert the native observer integer back into an Boxed object
    let cc_observer = unsafe { ptr_as_box(native_observer)? };

    // Convert the raw CallConnection pointer back into a Boxed object
    let cc_box = unsafe { Box::from_raw(cc_ptr) };

    if let CallDirection::OutGoing = direction {
        // Create data channel observer and data channel
        let dc_observer = DataChannelObserver::new(data_channel_cc)?;
        let data_channel = pc_interface.create_data_channel(DATA_CHANNEL_NAME.to_string())?;
        unsafe { data_channel.register_observer(dc_observer.rffi_interface())? } ;
        cc_box.set_data_channel(data_channel)?;
        cc_box.set_data_channel_observer(dc_observer)?;
    }

    if let Ok(mut platform) = cc_box.platform() {
        platform.set_cc_observer(cc_observer);
    } else {
        return Err(RingRtcError::MutexPoisoned("CallConnection.platform".to_string()).into());
    }
    cc_box.set_pc_interface(pc_interface)?;

    info!("call_connection: {:?}", cc_box);

    Ok(Box::into_raw(cc_box) as *mut c_void)
}
