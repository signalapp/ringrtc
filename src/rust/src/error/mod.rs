//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Common error codes.

use thiserror::Error;

use crate::common::{CallId, DeviceId};

/// Platform independent error conditions.
#[derive(Error, Debug)]
pub enum RingRtcError {
    // Project wide common error codes
    #[error("Mutex poisoned: {0}")]
    MutexPoisoned(String),
    #[error("Null pointer in: {0}, var: {1}")]
    NullPointer(String, String),
    #[error("Expecting non-none option value in: {0}, var: {1}")]
    OptionValueNotSet(String, String),
    #[error("Couldn't register an actor")]
    RegisterActor,

    // Call Manager error codes
    #[error("Active call already in progress, id: {0}")]
    CallAlreadyInProgress(CallId),
    #[error("Call Manager is busy")]
    CallManagerIsBusy,
    #[error("No active call found")]
    NoActiveCall,
    #[error("CallID not found in call_map: {0}")]
    CallIdNotFound(CallId),
    #[error("Connection not found in connection_map: {0}")]
    ConnectionNotFound(DeviceId),
    #[error("Active device ID is already set, remote_device: {0}")]
    ActiveDeviceIdAlreadySet(DeviceId),
    #[error("Active Media Stream is already set, remote_device: {0}")]
    ActiveMediaStreamAlreadySet(DeviceId),
    #[error("Pending incoming call is already set, remote_device: {0}")]
    PendingCallAlreadySet(DeviceId),
    #[error("Application Connection is already set, remote_device: {0}")]
    AppConnectionAlreadySet(DeviceId),
    #[error("Application Call Context is already set, call_id: {0}")]
    AppCallContextAlreadySet(CallId),

    // WebRTC / C++ error codes
    #[error("Unable to create C++ PeerConnectionObserver")]
    CreatePeerConnectionObserver,
    #[error("Unable to create C++ PeerConnectionFactory")]
    CreatePeerConnectionFactory,
    #[error("Unable to create C++ PeerConnection")]
    CreatePeerConnection,
    #[error("Unable to create C++ VideoSource")]
    CreateVideoSource,
    #[error("Unable to create C++ VideoTrack")]
    CreateVideoTrack,
    #[error("Unable to create C++ AudioTrack")]
    CreateAudioTrack,
    #[error("Unable to query Audio Devices")]
    #[allow(dead_code)]
    QueryAudioDevices,
    #[allow(dead_code)]
    #[error("Unable to set Audio Device")]
    SetAudioDevice,

    // WebRTC / C++ session description error codes
    #[error("CreateSessionDescriptionObserver failure. error msg: {0}, type: {1}")]
    CreateSessionDescriptionObserver(String, i32),
    #[error("CreateSessionDescriptionObserver get result failure. error msg: {0}")]
    CreateSessionDescriptionObserverResult(String),
    #[error("SetSessionDescriptionObserver failure. error msg: {0}, type: {1}")]
    SetSessionDescriptionObserver(String, i32),
    #[error("SetSessionDescriptionObserver get result failure. error msg: {0}")]
    SetSessionDescriptionObserverResult(String),
    #[error("AddIceCandidate failure")]
    AddIceCandidate,

    // WebRTC / C++ offer / answer error codes
    #[error("Unable to convert offer or answer to SDP")]
    ToSdp,
    #[error("Unable to convert sdp to answer")]
    ConvertSdpAnswer,
    #[error("Unable to convert sdp to offer")]
    ConvertSdpOffer,
    #[error("Unable to munge SDP")]
    MungeSdp,
    #[error("Unknown signaled protocol version")]
    UnknownSignaledProtocolVersion,

    // RTP Data error codes
    #[error("RTP data protocol error: {0}")]
    RtpDataProtocol(String),
    #[error("Unable to send RTP data")]
    SendRtp,
    #[error("Unable to receive RTP data")]
    ReceiveRtp,

    // IceGatherer error codes
    #[error("UseSharedIceGatherer failure")]
    UseIceGatherer,
    #[error("CreateIceGatherer failure")]
    CreateIceGatherer,

    // SFU client error codes
    #[error("SfuClient received unexpected response status code {0}")]
    UnexpectedResponseCodeFromSFu(u16),
    #[error("SfuClient request failed")]
    SfuClientRequestFailed,
    #[error("The maximum number of participants has been reached")]
    GroupCallFull,

    // Frame encryption error codes
    #[error("Frame Counter too big")]
    FrameCounterTooBig,
    #[error("Failed to encrypt")]
    FailedToEncrypt,
    #[error("Failed to decrypt")]
    FailedToDecrypt,

    // Misc error codes
    #[error("Failed to negotiate SRTP keys")]
    SrtpKeyNegotiationFailure,
    #[error("Buffer too small")]
    BufferTooSmall,
}
