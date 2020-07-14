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
use std::time::Duration;

use crate::ios::logging::{init_logging, IOSLogger};

use crate::ios::api::call_manager_interface::{AppCallContext, AppInterface, AppObject};
use crate::ios::ios_platform::IOSPlatform;

use crate::common::{BandwidthMode, CallId, CallMediaType, DeviceId, FeatureLevel, Result};

use crate::core::signaling;
use crate::core::util::{ptr_as_box, ptr_as_mut};

use crate::core::call_manager::CallManager;

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
pub fn call(
    call_manager: *mut IOSCallManager,
    app_remote: *const c_void,
    call_media_type: CallMediaType,
    app_local_device: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("call():");

    call_manager.call(
        AppObject::from(app_remote),
        call_media_type,
        app_local_device,
    )
}

/// Application notification to proceed with a new call
pub fn proceed(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    app_call_context: AppCallContext,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("proceed(): {}", call_id);

    call_manager.proceed(call_id, Arc::new(app_call_context))
}

/// Application notification that the sending of the previous message was a success.
pub fn message_sent(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("message_sent(): call_id: {}", call_id);
    call_manager.message_sent(call_id)
}

/// Application notification that the sending of the previous message was a failure.
pub fn message_send_failure(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!("message_send_failure(): call_id: {}", call_id);
    call_manager.message_send_failure(call_id)
}

/// Application notification of local hangup.
pub fn hangup(call_manager: *mut IOSCallManager) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };

    info!("hangup():");
    call_manager.hangup()
}

/// Application notification of received answer message
pub fn received_answer(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    sender_device_id: DeviceId,
    opaque: Option<Vec<u8>>,
    sdp: Option<String>,
    sender_device_feature_level: FeatureLevel,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!(
        "received_answer(): call_id: {} sender_device_id: {}",
        call_id, sender_device_id
    );
    call_manager.received_answer(
        call_id,
        signaling::ReceivedAnswer {
            answer: signaling::Answer::from_opaque_or_sdp(opaque, sdp),
            sender_device_id,
            sender_device_feature_level,
        },
    )
}

/// Application notification of received offer message
pub fn received_offer(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    remote_peer: *const c_void,
    sender_device_id: DeviceId,
    opaque: Option<Vec<u8>>,
    sdp: Option<String>,
    age_sec: u64,
    call_media_type: CallMediaType,
    receiver_device_id: DeviceId,
    sender_device_feature_level: FeatureLevel,
    receiver_device_is_primary: bool,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    let remote_peer = AppObject::from(remote_peer);

    info!(
        "received_offer(): call_id: {} remote_device_id: {}",
        call_id, sender_device_id
    );

    call_manager.received_offer(
        remote_peer,
        call_id,
        signaling::ReceivedOffer {
            offer: signaling::Offer::from_opaque_or_sdp(call_media_type, opaque, sdp),
            age: Duration::from_secs(age_sec),
            sender_device_id,
            sender_device_feature_level,
            receiver_device_id,
            receiver_device_is_primary,
        },
    )
}

/// Application notification to add ICE candidates to a Connection
pub fn received_ice(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    received: signaling::ReceivedIce,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!(
        "received_ice(): call_id: {} sender_device_id: {} candidates len: {}",
        call_id,
        received.sender_device_id,
        received.ice.candidates_added.len()
    );

    call_manager.received_ice(call_id, received)
}

/// Application notification of received Hangup message
pub fn received_hangup(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    sender_device_id: DeviceId,
    hangup_type: signaling::HangupType,
    hangup_device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!(
        "received_hangup(): call_id: {} sender device_id: {}",
        call_id, sender_device_id
    );

    call_manager.received_hangup(
        call_id,
        signaling::ReceivedHangup {
            hangup: signaling::Hangup::from_type_and_device_id(hangup_type, hangup_device_id),
            sender_device_id,
        },
    )
}

/// Application notification of received Busy message
pub fn received_busy(
    call_manager: *mut IOSCallManager,
    call_id: u64,
    sender_device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    info!(
        "received_busy(): call_id: {} sender device_id: {}",
        call_id, sender_device_id
    );

    call_manager.received_busy(call_id, signaling::ReceivedBusy { sender_device_id })
}

/// Application notification to accept the incoming call
pub fn accept_call(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    let call_id = CallId::from(call_id);

    info!("accept_call(): {}", call_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.accept_call(call_id)
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
    active_connection.inject_send_sender_status_via_data_channel(enable)
}

/// CMI request to set the low bandwidth mode
pub fn set_bandwidth_mode(call_manager: *mut IOSCallManager, mode: BandwidthMode) -> Result<()> {
    info!("set_low_bandwidth_mode():");

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let mut active_connection = call_manager.active_connection()?;
    active_connection.set_bandwidth_mode(mode)
}

/// CMI request to drop the active call
pub fn drop_call(call_manager: *mut IOSCallManager, call_id: u64) -> Result<()> {
    let call_id = CallId::from(call_id);

    info!("drop_call(): {}", call_id);

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.drop_call(call_id)
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
