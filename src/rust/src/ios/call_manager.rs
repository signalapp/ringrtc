//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! iOS Call Manager

use std::{ffi::c_void, sync::Arc, time::Duration};

use anyhow::anyhow;

use crate::{
    common::{CallConfig, CallId, CallMediaType, DataMode, DeviceId, Result},
    core::{
        call_manager,
        call_manager::CallManager,
        group_call, signaling,
        util::{ptr_as_box, ptr_as_mut},
    },
    error::RingRtcError,
    ios::{
        api::call_manager_interface::{AppCallContext, AppInterface, AppObject},
        ios_platform::IosPlatform,
    },
    lite::{
        call_links::CallLinkRootKey,
        http,
        sfu::{DemuxId, GroupMember, UserId},
    },
    protobuf, webrtc,
    webrtc::{
        media,
        peer_connection_factory::{self as pcf, PeerConnectionFactory},
    },
};

/// Public type for iOS CallManager
pub type IosCallManager = CallManager<IosPlatform>;

/// Creates a new IosCallManager object.
pub fn create(app_interface: AppInterface, http_client: http::ios::Client) -> Result<*mut c_void> {
    let platform = IosPlatform::new(app_interface)?;
    let call_manager = IosCallManager::new(platform, http_client)?;
    let call_manager_box = Box::new(call_manager);
    Ok(Box::into_raw(call_manager_box) as *mut c_void)
}

/// Updates the current user's UUID.
pub fn set_self_uuid(call_manager: *mut IosCallManager, uuid: UserId) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_self_uuid(uuid)
}

/// Application notification to start a new call.
pub fn call(
    call_manager: *mut IosCallManager,
    app_remote: *const c_void,
    call_media_type: CallMediaType,
    app_local_device: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.call(
        AppObject::from(app_remote),
        call_media_type,
        app_local_device,
    )
}

/// Application notification to proceed with a new call
pub fn proceed(
    call_manager: *mut IosCallManager,
    call_id: u64,
    app_call_context: AppCallContext,
    call_config: CallConfig,
    audio_levels_interval: Option<Duration>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    call_manager.proceed(
        call_id,
        Arc::new(app_call_context),
        call_config,
        audio_levels_interval,
    )
}

/// Application notification that the sending of the previous message was a success.
pub fn message_sent(call_manager: *mut IosCallManager, call_id: u64) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    call_manager.message_sent(call_id)
}

/// Application notification that the sending of the previous message was a failure.
pub fn message_send_failure(call_manager: *mut IosCallManager, call_id: u64) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    call_manager.message_send_failure(call_id)
}

/// Application notification of local hangup.
pub fn hangup(call_manager: *mut IosCallManager) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.hangup()
}

/// Application notification cancelling a group ring.
pub fn cancel_group_ring(
    call_manager: *mut IosCallManager,
    group_id: group_call::GroupId,
    ring_id: group_call::RingId,
    reason: Option<group_call::RingCancelReason>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.cancel_group_ring(group_id, ring_id, reason)
}

/// Application notification of received answer message
#[allow(clippy::too_many_arguments)]
pub fn received_answer(
    call_manager: *mut IosCallManager,
    call_id: u64,
    sender_device_id: DeviceId,
    opaque: Option<Vec<u8>>,
    sender_identity_key: Option<Vec<u8>>,
    receiver_identity_key: Option<Vec<u8>>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);

    let opaque = match opaque {
        Some(v) => v,
        None => {
            return Err(RingRtcError::OptionValueNotSet(
                "received_answer()".to_owned(),
                "opaque".to_owned(),
            )
            .into());
        }
    };

    let sender_identity_key = match sender_identity_key {
        Some(v) => v,
        None => {
            return Err(RingRtcError::OptionValueNotSet(
                "received_answer()".to_owned(),
                "sender_identity_key".to_owned(),
            )
            .into());
        }
    };

    let receiver_identity_key = match receiver_identity_key {
        Some(v) => v,
        None => {
            return Err(RingRtcError::OptionValueNotSet(
                "received_answer()".to_owned(),
                "receiver_identity_key".to_owned(),
            )
            .into());
        }
    };

    call_manager.received_answer(
        call_id,
        signaling::ReceivedAnswer {
            answer: signaling::Answer::new(opaque)?,
            sender_device_id,
            sender_identity_key,
            receiver_identity_key,
        },
    )
}

/// Application notification of received offer message
#[allow(clippy::too_many_arguments)]
pub fn received_offer(
    call_manager: *mut IosCallManager,
    call_id: u64,
    remote_peer: *const c_void,
    sender_device_id: DeviceId,
    opaque: Option<Vec<u8>>,
    age_sec: u64,
    call_media_type: CallMediaType,
    receiver_device_id: DeviceId,
    sender_identity_key: Option<Vec<u8>>,
    receiver_identity_key: Option<Vec<u8>>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    let remote_peer = AppObject::from(remote_peer);

    let opaque = match opaque {
        Some(v) => v,
        None => {
            return Err(RingRtcError::OptionValueNotSet(
                "received_offer()".to_owned(),
                "opaque".to_owned(),
            )
            .into());
        }
    };

    let sender_identity_key = match sender_identity_key {
        Some(v) => v,
        None => {
            return Err(RingRtcError::OptionValueNotSet(
                "received_offer()".to_owned(),
                "sender_identity_key".to_owned(),
            )
            .into());
        }
    };

    let receiver_identity_key = match receiver_identity_key {
        Some(v) => v,
        None => {
            return Err(RingRtcError::OptionValueNotSet(
                "received_offer()".to_owned(),
                "receiver_identity_key".to_owned(),
            )
            .into());
        }
    };

    call_manager.received_offer(
        remote_peer,
        call_id,
        signaling::ReceivedOffer {
            offer: signaling::Offer::new(call_media_type, opaque)?,
            age: Duration::from_secs(age_sec),
            sender_device_id,
            receiver_device_id,
            sender_identity_key,
            receiver_identity_key,
        },
    )
}

/// Application notification to add ICE candidates to a Connection
pub fn received_ice(
    call_manager: *mut IosCallManager,
    call_id: u64,
    received: signaling::ReceivedIce,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    call_manager.received_ice(call_id, received)
}

/// Application notification of received Hangup message
pub fn received_hangup(
    call_manager: *mut IosCallManager,
    call_id: u64,
    sender_device_id: DeviceId,
    hangup_type: signaling::HangupType,
    hangup_device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
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
    call_manager: *mut IosCallManager,
    call_id: u64,
    sender_device_id: DeviceId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call_id = CallId::from(call_id);
    call_manager.received_busy(call_id, signaling::ReceivedBusy { sender_device_id })
}

pub fn received_call_message(
    call_manager: *mut IosCallManager,
    sender_uuid: Vec<u8>,
    sender_device_id: DeviceId,
    local_device_id: DeviceId,
    message: Vec<u8>,
    message_age_sec: Duration,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.received_call_message(
        sender_uuid,
        sender_device_id,
        local_device_id,
        message,
        message_age_sec,
    )
}

/// Application notification to accept the incoming call
pub fn accept_call(call_manager: *mut IosCallManager, call_id: u64) -> Result<()> {
    let call_id = CallId::from(call_id);
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.accept_call(call_id)
}

/// CMI request for the active Connection object
pub fn get_active_connection(call_manager: *mut IosCallManager) -> Result<*mut c_void> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let connection = call_manager.active_connection()?;
    let app_connection = connection.app_connection()?;

    Ok(app_connection.object)
}

/// CMI request for the active CallContext object
pub fn get_active_call_context(call_manager: *mut IosCallManager) -> Result<*mut c_void> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let call = call_manager.active_call()?;
    let app_call_context = call.call_context()?;

    Ok(app_call_context.object)
}

/// CMI request to set the audio status
pub fn set_audio_enable(call_manager: *mut IosCallManager, enable: bool) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let mut active_connection = call_manager.active_connection()?;
    active_connection.update_sender_status(signaling::SenderStatus {
        audio_enabled: Some(enable),
        ..Default::default()
    })
}

/// CMI request to set the video status
pub fn set_video_enable(call_manager: *mut IosCallManager, enable: bool) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let mut active_connection = call_manager.active_connection()?;
    active_connection.update_sender_status(signaling::SenderStatus {
        video_enabled: Some(enable),
        ..Default::default()
    })
}

/// Request to update the data mode on the direct connection
pub fn update_data_mode(call_manager: *mut IosCallManager, data_mode: DataMode) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    let mut active_connection = call_manager.active_connection()?;
    active_connection.inject_update_data_mode(data_mode)
}

/// CMI request to drop the active call
pub fn drop_call(call_manager: *mut IosCallManager, call_id: u64) -> Result<()> {
    let call_id = CallId::from(call_id);
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.drop_call(call_id)
}

/// CMI request to reset the Call Manager
pub fn reset(call_manager: *mut IosCallManager) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.reset()
}

/// CMI request to close down the Call Manager.
///
/// This is a blocking call.
pub fn close(call_manager: *mut IosCallManager) -> Result<()> {
    // Convert the raw pointer back into a Box and let it go out of
    // scope when this function exits.
    let mut call_manager = unsafe { ptr_as_box(call_manager)? };
    call_manager.close()
}

// Group Calls

#[allow(clippy::too_many_arguments)]
pub fn create_group_call_client(
    call_manager: *mut IosCallManager,
    group_id: group_call::GroupId,
    sfu_url: String,
    hkdf_extra_info: Vec<u8>,
    audio_levels_interval: Option<Duration>,
    native_peer_connection_factory: webrtc::ptr::OwnedRc<pcf::RffiPeerConnectionFactoryInterface>,
    native_audio_track: webrtc::ptr::OwnedRc<media::RffiAudioTrack>,
    native_video_track: webrtc::ptr::OwnedRc<media::RffiVideoTrack>,
) -> Result<group_call::ClientId> {
    let peer_connection_factory = unsafe {
        PeerConnectionFactory::from_native_factory(webrtc::Arc::from_owned(
            native_peer_connection_factory,
        ))
    };

    let outgoing_audio_track = media::AudioTrack::new(
        webrtc::Arc::from_owned(native_audio_track),
        Some(peer_connection_factory.rffi().clone()),
    );

    let outgoing_video_track = media::VideoTrack::new(
        webrtc::Arc::from_owned(native_video_track),
        Some(peer_connection_factory.rffi().clone()),
    );

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.create_group_call_client(
        group_id,
        sfu_url,
        hkdf_extra_info,
        audio_levels_interval,
        Some(peer_connection_factory),
        outgoing_audio_track,
        outgoing_video_track,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create_call_link_call_client(
    call_manager: *mut IosCallManager,
    sfu_url: String,
    auth_credential_presentation: Vec<u8>,
    root_key: CallLinkRootKey,
    admin_passkey: Option<Vec<u8>>,
    hkdf_extra_info: Vec<u8>,
    audio_levels_interval: Option<Duration>,
    native_peer_connection_factory: webrtc::ptr::OwnedRc<pcf::RffiPeerConnectionFactoryInterface>,
    native_audio_track: webrtc::ptr::OwnedRc<media::RffiAudioTrack>,
    native_video_track: webrtc::ptr::OwnedRc<media::RffiVideoTrack>,
) -> Result<group_call::ClientId> {
    let peer_connection_factory = unsafe {
        PeerConnectionFactory::from_native_factory(webrtc::Arc::from_owned(
            native_peer_connection_factory,
        ))
    };

    let outgoing_audio_track = media::AudioTrack::new(
        webrtc::Arc::from_owned(native_audio_track),
        Some(peer_connection_factory.rffi().clone()),
    );

    let outgoing_video_track = media::VideoTrack::new(
        webrtc::Arc::from_owned(native_video_track),
        Some(peer_connection_factory.rffi().clone()),
    );

    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.create_call_link_call_client(
        sfu_url,
        &auth_credential_presentation,
        root_key,
        admin_passkey,
        hkdf_extra_info,
        audio_levels_interval,
        Some(peer_connection_factory),
        outgoing_audio_track,
        outgoing_video_track,
        None,
    )
}

pub fn delete_group_call_client(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.delete_group_call_client(client_id);
    Ok(())
}

pub fn connect(call_manager: *mut IosCallManager, client_id: group_call::ClientId) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.connect(client_id);
    Ok(())
}

pub fn join(call_manager: *mut IosCallManager, client_id: group_call::ClientId) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.join(client_id);
    Ok(())
}

pub fn leave(call_manager: *mut IosCallManager, client_id: group_call::ClientId) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.leave(client_id);
    Ok(())
}

pub fn disconnect(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.disconnect(client_id);
    Ok(())
}

pub fn set_outgoing_audio_muted(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    muted: bool,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_outgoing_audio_muted(client_id, muted);
    Ok(())
}

pub fn set_outgoing_video_muted(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    muted: bool,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_outgoing_video_muted(client_id, muted);
    Ok(())
}

pub fn group_ring(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    recipient: Option<UserId>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.group_ring(client_id, recipient);
    Ok(())
}

pub fn resend_media_keys(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.resend_media_keys(client_id);
    Ok(())
}

pub fn set_data_mode(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    data_mode: DataMode,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_data_mode(client_id, data_mode);
    Ok(())
}

pub fn request_video(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    rendered_resolutions: Vec<group_call::VideoRequest>,
    active_speaker_height: u16,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.request_video(client_id, rendered_resolutions, active_speaker_height);
    Ok(())
}

pub fn approve_user(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    other_user_id: UserId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.approve_user(client_id, other_user_id);
    Ok(())
}

pub fn deny_user(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    other_user_id: UserId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.deny_user(client_id, other_user_id);
    Ok(())
}

pub fn remove_client(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    other_client_demux_id: DemuxId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.remove_client(client_id, other_client_demux_id);
    Ok(())
}

pub fn block_client(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    other_client_demux_id: DemuxId,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.block_client(client_id, other_client_demux_id);
    Ok(())
}

pub fn set_group_members(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    members: Vec<GroupMember>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_group_members(client_id, members);
    Ok(())
}

pub fn set_membership_proof(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    proof: Vec<u8>,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.set_membership_proof(client_id, proof);
    Ok(())
}

pub fn react(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    value: String,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.react(client_id, value);
    Ok(())
}

pub fn raise_hand(
    call_manager: *mut IosCallManager,
    client_id: group_call::ClientId,
    raise: bool,
) -> Result<()> {
    let call_manager = unsafe { ptr_as_mut(call_manager)? };
    call_manager.raise_hand(client_id, raise);
    Ok(())
}

pub fn validate_offer(
    opaque: Option<Vec<u8>>,
    age_sec: u64,
    call_media_type: CallMediaType,
) -> Result<()> {
    let opaque = match opaque {
        Some(v) => v,
        None => {
            return Err(RingRtcError::OptionValueNotSet(
                "validate_offer()".to_owned(),
                "opaque".to_owned(),
            )
            .into());
        }
    };

    call_manager::validate_offer(&signaling::ReceivedOffer {
        offer: signaling::Offer::new(call_media_type, opaque)?,
        age: Duration::from_secs(age_sec),
        sender_device_id: 1,
        receiver_device_id: 1,
        sender_identity_key: vec![],
        receiver_identity_key: vec![],
    })
    .map_err(|e| anyhow!("{:?}", e))
}

pub fn validate_call_message_as_opaque_ring(
    message: &[u8],
    age: Duration,
    validate_group_ring: impl FnOnce(group_call::GroupIdRef, group_call::RingId) -> bool,
) -> Result<()> {
    let message: protobuf::signaling::CallMessage = prost::Message::decode(message)?;
    call_manager::validate_call_message_as_opaque_ring(&message, age, validate_group_ring)
        .map_err(|e| anyhow!("{:?}", e))
}
