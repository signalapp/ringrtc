//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Call Manager

use std::ffi::c_void;
use std::panic;
use std::sync::Arc;

use crate::ios::logging::{init_logging, IOSLogger};

use crate::ios::api::call_manager_interface::{AppCallContext, AppInterface, AppObject};
use crate::ios::ios_platform::IOSPlatform;

use crate::common::{CallId, ConnectionId, DeviceId, Result};

use crate::core::util::{ptr_as_box, ptr_as_mut};

use crate::core::call_manager::CallManager;

use crate::webrtc::ice_candidate::IceCandidate;

/// Public type for iOS CallManager
pub type IOSCallManager = CallManager<IOSPlatform>;

/// Library initialization routine.
///
/// Sets up the logging infrastructure.
pub fn initialize(log_object: IOSLogger) -> Result<()> {
    init_logging(log_object)?;

    // Set a custom panic handler that uses the logger instead of
    // stderr, which is of no use on Android.
    panic::set_hook(Box::new(|panic_info| {
        error!("Critical error: {}", panic_info);
    }));

    Ok(())
}

/// Creates a new IOSCallManager object.
pub fn create(app_call_manager: *mut c_void, app_interface: AppInterface) -> Result<*mut c_void> {
    info!("create_call_manager():");
    let platform = IOSPlatform::new(app_call_manager, app_interface)?;

    let call_manager = IOSCallManager::new(platform)?;

    let call_manager_box = Box::new(call_manager);
    Ok(Box::into_raw(call_manager_box) as *mut c_void)
}

/// Application notification to start a new call.
pub fn call(call_manager: *mut IOSCallManager, app_remote: *const c_void) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("call():");

    call_manager.call(AppObject::from(app_remote))
}

/// Application notification to proceed with a new call
pub fn proceed(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    app_call_context: AppCallContext,
    app_remote_devices: Vec<u32>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("proceed():");

    if !call_manager.call_active()? {
        warn!("proceed(): skipping inactive call");
        return Ok(());
    }

    // Convert Rust Vec<u32> into a Rust Vec<DeviceId>.
    let mut remote_devices = Vec::<DeviceId>::new();
    for device in app_remote_devices {
        let device_id = device as DeviceId;
        remote_devices.push(device_id);
    }

    info!("proceed(): remote_devices size: {}", remote_devices.len());
    for device in &remote_devices {
        info!("proceed(): device id: {}", device);
    }

    call_manager.proceed(
        CallId::from(call_id),
        Arc::new(app_call_context),
        remote_devices,
    )
}

/// Application notification that the sending of the previous message was a success.
pub fn message_sent(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("message_sent():");
    call_manager.message_sent(CallId::from(call_id))
}

/// Application notification that the sending of the previous message was a failure.
pub fn message_send_failure(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("message_send_failure():");
    call_manager.message_send_failure(CallId::from(call_id))
}

/// Application notification of local hangup.
pub fn hangup(call_manager: *mut IOSCallManager) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("hangup():");
    call_manager.hangup()
}

/// Application notification of received SDP answer message
pub fn received_answer(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    remote_device: DeviceId,
    app_answer: &str,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_answer(): id: {}", connection_id);
    call_manager.received_answer(connection_id, app_answer.to_string())
}

/// Application notification of received SDP offer message
pub fn received_offer(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    app_remote: *const c_void,
    remote_device: DeviceId,
    app_offer: &str,
    timestamp: u64,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_offer(): id: {}", connection_id);

    call_manager.received_offer(
        AppObject::from(app_remote),
        connection_id,
        app_offer.to_string(),
        timestamp,
    )
}

/// Application notification to add ICE candidates to a Connection
pub fn received_ice_candidates(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    remote_device: DeviceId,
    ice_candidates: Vec<IceCandidate>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!(
        "received_ice_candidates(): id: {} len: {}",
        connection_id,
        ice_candidates.len()
    );

    if !call_manager.call_is_active(connection_id.call_id())? {
        warn!(
            "received_ice_candidates(): skipping inactive call_id: {}",
            connection_id.call_id()
        );
        return Ok(());
    }

    call_manager.received_ice_candidates(connection_id, &ice_candidates)
}

/// Application notification of received Hangup message
pub fn received_hangup(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    remote_device: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_hangup(): id: {}", connection_id);

    call_manager.received_hangup(connection_id)
}

/// Application notification of received Busy message
pub fn received_busy(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    remote_device: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection_id = ConnectionId::new(CallId::from(call_id), remote_device);

    info!("received_busy(): id: {}", connection_id);

    call_manager.received_busy(connection_id)
}

/// Application notification to accept the incoming call
pub fn accept_call(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    info!("accept_call():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.accept_call(CallId::from(call_id))
}

/// CMI request for the active Connection object
pub fn get_active_connection(call_manager: *mut IOSCallManager) -> Result<*mut c_void> {
    info!("get_active_connection():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection = call_manager.active_connection()?;
    let app_connection = connection.app_connection()?;

    Ok(app_connection.object)
}

/// CMI request for the active CallContext object
pub fn get_active_call_context(call_manager: *mut IOSCallManager) -> Result<*mut c_void> {
    info!("get_active_call_context():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call = call_manager.active_call()?;
    let app_call_context = call.call_context()?;

    Ok(app_call_context.object)
}

/// CMI request to set the video status
pub fn set_video_enable(call_manager: *mut IOSCallManager, enable: bool) -> Result<()> {
    info!("set_video_enable():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let mut active_connection = call_manager.active_connection()?;
    active_connection.inject_local_video_status(enable)
}

/// CMI request to drop the active call
pub fn drop_call(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    info!("drop_call():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.drop_call(CallId::from(call_id))
}

/// CMI request to reset the Call Manager
pub fn reset(call_manager: *mut IOSCallManager) -> Result<()> {
    info!("reset():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.reset()
}

/// CMI request to close down the Call Manager.
///
/// This is a blocking call.
pub fn close(call_manager: *mut IOSCallManager) -> Result<()> {
    info!("close():");

    // Convert the raw pointer back into a Box and let it go out of
    // scope when this function exits.
    let mut call_manager = unsafe { ptr_as_box(call_manager)? };
    call_manager.close()
}
