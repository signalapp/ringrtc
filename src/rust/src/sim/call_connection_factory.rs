//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Simulation CallConnectionFactory Interface.

use std::ptr;
use std::sync::{
    Arc,
    Mutex,
};

use crate::sim::sim_platform::{
    SimPlatform,
    SimCallConnection,
};
use crate::sim::call_connection_observer::SimCallConnectionObserver;
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
use crate::webrtc::peer_connection::PeerConnection;

pub struct CallConfig {
    pub call_id:   CallId,
    pub recipient: String,
    pub direction: CallDirection,
}

/// Public type for Sim CallConnectionFactory.
pub type SimCallConnectionFactory = Arc<Mutex<CallConnectionFactory<SimPlatform>>>;

/// Creates a new SimCallConnectionFactory object.
pub fn create_call_connection_factory() -> Result<SimCallConnectionFactory> {

    info!("create_call_connection_factory");
    let cc_factory = CallConnectionFactory::<SimPlatform>::new(ptr::null() as CppObject)?;

    // Wrap factory in Arc<Mutex<>> to pass amongst threads
    let ccf = Arc::new(Mutex::new(cc_factory));

    // Use some unsafe functions to quiet some unused function warnings
    let ccf_ptr  = Arc::into_raw(ccf) as u64;
    let ccf_ptr2 = ccf_ptr;

    let ccf_ref = unsafe { ptr_as_arc_ptr(ccf_ptr    as *mut CallConnectionFactory<SimPlatform>).unwrap() };
    info!("create_call_connection_factory(): {:?}", ccf_ref.get_arc().lock().unwrap());

    let ccf = unsafe { ptr_as_arc_mutex(ccf_ptr2 as *mut CallConnectionFactory<SimPlatform>).unwrap() };

    Ok(ccf)
}

/// Frees a new SimCallConnectionFactory object.
pub fn free_factory(factory: SimCallConnectionFactory) -> Result<()> {
    info!("free_factory");
    if let Ok(mut cc_factory) = factory.lock() {
        cc_factory.close()?;
    }
    Ok(())
}

/// Create a Rust CallConnectionInterface for Sim
///
/// # Arguments
///
/// * `env` - JNI environemnt
/// * `class` - org.signal.ringrtc.CallConnectionFactory
/// * `native_factory` - raw pointer to Rust CallConnectionFactory
/// * `call_config` - org.signal.ringrtc.CallConnection$Configuration
/// * `native_observer` - raw pointer to Rust CallConnectionObserver
/// * `rtc_config` - org.webrtc.PeerConnection.RTCConfiguration
/// * `media_constraints` - org.webrtc.MediaConstraints
/// * `ssl_cert_verifier` - org.webrtc.SSLCertificateVerifier
///
pub fn create_call_connection(native_factory:    &SimCallConnectionFactory,
                              call_config:       CallConfig,
                              native_observer:   Arc<Mutex<SimCallConnectionObserver>>) -> Result<Box<SimCallConnection>> {

    let mut cc_factory = match native_factory.lock() {
        Ok(v) => v,
        Err(_) => return Err(RingRtcError::MutexPoisoned("Call Connection Factory".to_string()).into()),
    };

    // Get callId from configuration
    let call_id   = call_config.call_id;
    let recipient = call_config.recipient;
    let direction = call_config.direction;

    let platform = SimPlatform::new(recipient, native_observer);
    let call_connection = cc_factory.create_call_connection(call_id, direction, platform)?;
    info!("call_connection: {:?}", call_connection);

    let data_channel_cc = call_connection.clone();

    let cc_box = Box::new(call_connection);
    let cc_ptr = Box::into_raw(cc_box);

    let fake_pc_interface: u32 = 1;
    let pc_interface = PeerConnection::new(&fake_pc_interface);

    // Convert the raw CallConnection pointer back into a Boxed object
    let cc_box = unsafe { ptr_as_box(cc_ptr)? };

    if let CallDirection::OutGoing = direction {
        // Create data channel observer and data channel
        let dc_observer = DataChannelObserver::new(data_channel_cc)?;
        let data_channel = pc_interface.create_data_channel(DATA_CHANNEL_NAME.to_string())?;
        unsafe { data_channel.register_observer(dc_observer.rffi_interface())? } ;
        cc_box.set_data_channel(data_channel)?;
        cc_box.set_data_channel_observer(dc_observer)?;
    }

    cc_box.set_pc_interface(pc_interface)?;

    info!("call_connection: {:?}", cc_box);
    Ok(cc_box)
}
