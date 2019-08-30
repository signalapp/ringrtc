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

use crate::ios::call_connection::iOSCallConnection;
use crate::ios::call_connection_observer::iOSCallConnectionObserver;
use crate::ios::error::iOSError;
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
    ArcPtr,
};
use crate::error::RingRtcError;
use crate::webrtc::data_channel_observer::DataChannelObserver;
use crate::webrtc::peer_connection_observer::PeerConnectionObserver;
use crate::webrtc::peer_connection::{
    PeerConnection,
    RffiPeerConnectionInterface,
};

#[allow(non_camel_case_types)]
type iOSCallConnectionFactory = CallConnectionFactory<iOSCallConnection>;

/// Creates a new iOSCallConnectionFactory object.
pub fn native_create_call_connection_factory(app_call_connection_factory: jlong) -> Result<jlong> {
    // @todo Is CppObject correctly compatible with jlong/i64/*mut c_void?
    let cc_factory = iOSCallConnectionFactory::new(app_call_connection_factory as CppObject)?;
    // Wrap factory in Arc<Mutex<>> to pass amongst threads
    let cc_factory = Arc::new(Mutex::new(cc_factory));
    let ptr = Arc::into_raw(cc_factory);
    Ok(ptr as jlong)
}

/// Frees a new iOSCallConnectionFactory object.
pub fn native_free_factory(factory: jlong) -> Result<()> {
    // Convert integer back into Arc, then Arc will free things up as
    // it goes out of scope.
    let call_connection_factory: Arc<Mutex<iOSCallConnectionFactory>> = get_arc_from_jlong(factory)?;
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
pub fn native_create_call_connection(native_factory: jlong,
                                app_call_connection: jlong,
                                        call_config: IOSCallConfig,
                                    native_observer: jlong,
                                         rtc_config: jlong,
                                    rtc_constraints: jlong) -> Result<jlong> {

    let call_connection_factory: ArcPtr<Mutex<iOSCallConnectionFactory>> = get_arc_ptr_from_jlong(native_factory)?;
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

    let call_connection = iOSCallConnection::new(call_id, direction, recipient);
    let call_connection_handle = cc_factory.create_call_connection_handle(call_connection)?;
    info!("call_connection object: debug {:?}", call_connection_handle);

    let data_channel_cc_handle = call_connection_handle.clone();

    let cc_handle = Box::new(call_connection_handle);
    let cc_ptr = Box::into_raw(cc_handle);

    let pc_observer = PeerConnectionObserver::new(cc_ptr)?;
    info!("pc_observer: {:?}", pc_observer);

    // construct iOS OwnedPeerConnection object
    let webrtc_owned_pc = unsafe {
        appCreatePeerConnection(
            cc_factory.get_native_peer_connection_factory() as *mut c_void,
            app_call_connection as *mut c_void,
            rtc_config as *mut c_void,
            rtc_constraints as *mut c_void,
            pc_observer.get_rffi_interface() as *mut c_void)
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
    let cc_observer: Box<iOSCallConnectionObserver> = get_object_from_jlong(native_observer)?;

    // Convert the raw CallConnection pointer back into a Boxed object
    let cc_handle = unsafe { Box::from_raw(cc_ptr) };

    if let Ok(mut cc) = cc_handle.lock() {
        if let CallDirection::OutGoing = direction {
            // Create data channel observer and data channel
            let dc_observer = DataChannelObserver::new(data_channel_cc_handle)?;
            let data_channel = pc_interface.create_data_channel(DATA_CHANNEL_NAME.to_string())?;
            data_channel.register_observer(dc_observer.get_rffi_interface())?;
            cc.set_data_channel(data_channel);
            cc.set_data_channel_observer(dc_observer);
        }
        cc.update_pc(pc_interface, cc_observer)?;
    } else {
        error!("Initial mutex is poisoned");
        return Err(RingRtcError::MutexPoisoned("CallConnectionHandle in CallConnectionFactory".to_string()).into());
    }

    Ok(Box::into_raw(cc_handle) as jlong)
}
