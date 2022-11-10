//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! iOS Call Manager Interface

use std::convert::TryFrom;
use std::ffi::c_void;
use std::time::Duration;
use std::{fmt, ptr, slice};

use libc::size_t;

use crate::ios::call_manager;
use crate::ios::call_manager::IosCallManager;

use crate::common::{CallMediaType, DeviceId};
use crate::core::bandwidth_mode::BandwidthMode;
use crate::core::group_call;
use crate::core::signaling;
use crate::lite::{http, sfu, sfu::DemuxId};
use crate::webrtc::peer_connection::AudioLevel;
use crate::webrtc::{self, media, peer_connection_factory as pcf};

///
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)]
pub struct AppObject {
    pub ptr: *mut c_void,
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for AppObject {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for AppObject {}

impl From<AppObject> for *mut c_void {
    fn from(item: AppObject) -> Self {
        item.ptr
    }
}

impl From<AppObject> for *const c_void {
    fn from(item: AppObject) -> Self {
        item.ptr
    }
}

impl From<&AppObject> for *mut c_void {
    fn from(item: &AppObject) -> Self {
        item.ptr
    }
}

impl AppObject {
    pub fn new(ptr: *mut c_void) -> Self {
        Self { ptr }
    }
}

impl From<*mut c_void> for AppObject {
    fn from(item: *mut c_void) -> Self {
        AppObject::new(item)
    }
}

impl From<*const c_void> for AppObject {
    fn from(item: *const c_void) -> Self {
        AppObject::new(item as *mut c_void)
    }
}

/// Structure for passing buffers (strings/bytes) to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppByteSlice {
    pub bytes: *const u8,
    pub len: size_t,
}

impl AppByteSlice {
    fn as_slice(&self) -> Option<&[u8]> {
        if self.bytes.is_null() {
            return None;
        }
        Some(unsafe { slice::from_raw_parts(self.bytes, self.len) })
    }
}

/// Structure for passing optional u16 values to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppOptionalUInt16 {
    pub value: u16,
    pub valid: bool,
}

/// Structure for passing optional u32 values to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppOptionalUInt32 {
    pub value: u32,
    pub valid: bool,
}

/// Structure for passing optional bool values to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppOptionalBool {
    pub value: bool,
    pub valid: bool,
}

/// Structure for passing multiple Ice Candidates to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppIceCandidateArray {
    pub candidates: *const AppByteSlice,
    pub count: size_t,
}

/// Structure for passing connection details from the application.
#[repr(C)]
#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct AppConnectionInterface {
    pub object: *mut c_void,
    pub pc: *mut c_void,
    /// Swift object clean up method.
    pub destroy: extern "C" fn(object: *mut c_void),
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for AppConnectionInterface {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for AppConnectionInterface {}

impl fmt::Display for AppConnectionInterface {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// Rust owns the connection details object from Swift. Drop it when it
// goes out of scope.
impl Drop for AppConnectionInterface {
    fn drop(&mut self) {
        (self.destroy)(self.object);
    }
}

/// Structure for holding call context details on behalf of the application.
#[repr(C)]
#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct AppCallContext {
    pub object: *mut c_void,
    /// Swift object clean up method.
    pub destroy: extern "C" fn(object: *mut c_void),
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for AppCallContext {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for AppCallContext {}

impl fmt::Display for AppCallContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// Rust owns the connection details object from Swift. Drop it when it
// goes out of scope.
impl Drop for AppCallContext {
    fn drop(&mut self) {
        (self.destroy)(self.object);
    }
}

/// Structure for passing media stream instances from the application.
// See comment of IosMediaStream to understand
// where this fits in the many layers of wrappers.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppMediaStreamInterface {
    // Really a *mut ConnectionMediaStream.
    pub object: *mut c_void,
    /// Swift object clean up method.
    // Really connectionMediaStreamDestroy
    pub destroy: extern "C" fn(object: *mut c_void),
    /// Returns a pointer to a RTCMediaStream object.
    // Really connectionMediaStreamCreateMediaStream
    pub createMediaStream:
        extern "C" fn(object: *mut c_void, nativeStream: *mut c_void) -> *mut c_void,
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for AppMediaStreamInterface {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for AppMediaStreamInterface {}

impl fmt::Display for AppMediaStreamInterface {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// Rust owns the connection details object from Swift. Drop it when it
// goes out of scope.
impl Drop for AppMediaStreamInterface {
    fn drop(&mut self) {
        (self.destroy)(self.object);
    }
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppRemoteDeviceState {
    pub demuxId: DemuxId,
    pub user_id: AppByteSlice,
    pub mediaKeysReceived: bool,
    pub audioMuted: AppOptionalBool,
    pub videoMuted: AppOptionalBool,
    pub presenting: AppOptionalBool,
    pub sharingScreen: AppOptionalBool,
    pub addedTime: u64,   // unix millis
    pub speakerTime: u64, // unix millis; 0 if never was a speaker
    pub forwardingVideo: AppOptionalBool,
    pub isHigherResolutionPending: bool,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppRemoteDeviceStateArray {
    pub states: *const AppRemoteDeviceState,
    pub count: size_t,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppReceivedAudioLevel {
    pub demuxId: DemuxId,
    pub level: AudioLevel,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppReceivedAudioLevelArray {
    pub levels: *const AppReceivedAudioLevel,
    pub count: size_t,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppGroupMemberInfo {
    pub userId: AppByteSlice,
    pub memberId: AppByteSlice,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppGroupMemberInfoArray {
    pub members: *const AppGroupMemberInfo,
    pub count: size_t,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppUuidArray {
    pub uuids: *const AppByteSlice,
    pub count: size_t,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppVideoRequest {
    pub demux_id: DemuxId,
    pub width: u16,
    pub height: u16,
    pub framerate: AppOptionalUInt16,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppVideoRequestArray {
    pub resolutions: *const AppVideoRequest,
    pub count: size_t,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
/// iOS Interface for communicating with the Swift application.
pub struct AppInterface {
    /// Raw Swift object pointer.
    pub object: *mut c_void,
    /// Swift object clean up method.
    pub destroy: extern "C" fn(object: *mut c_void),
    ///
    pub onStartCall: extern "C" fn(
        object: *mut c_void,
        remote: *const c_void,
        callId: u64,
        isOutgoing: bool,
        callMediaType: i32,
    ),
    /// Swift event callback method.
    pub onEvent: extern "C" fn(object: *mut c_void, remote: *const c_void, event: i32),
    ///
    pub onNetworkRouteChanged:
        extern "C" fn(object: *mut c_void, remote: *const c_void, localNetworkAdapterType: i32),
    ///
    pub onAudioLevels: extern "C" fn(
        object: *mut c_void,
        remote: *const c_void,
        capturedLevel: u16,
        receivedLevel: u16,
    ),
    ///
    pub onSendOffer: extern "C" fn(
        object: *mut c_void,
        callId: u64,
        remote: *const c_void,
        destinationDeviceId: u32,
        broadcast: bool,
        opaque: AppByteSlice,
        callMediaType: i32,
    ),
    ///
    pub onSendAnswer: extern "C" fn(
        object: *mut c_void,
        callId: u64,
        remote: *const c_void,
        destinationDeviceId: u32,
        broadcast: bool,
        opaque: AppByteSlice,
    ),
    ///
    pub onSendIceCandidates: extern "C" fn(
        object: *mut c_void,
        callId: u64,
        remote: *const c_void,
        destinationDeviceId: u32,
        broadcast: bool,
        candidates: *const AppIceCandidateArray,
    ),
    ///
    pub onSendHangup: extern "C" fn(
        object: *mut c_void,
        callId: u64,
        remote: *const c_void,
        destinationDeviceId: u32,
        broadcast: bool,
        hangupType: i32,
        deviceId: u32,
    ),
    ///
    pub onSendBusy: extern "C" fn(
        object: *mut c_void,
        callId: u64,
        remote: *const c_void,
        destinationDeviceId: u32,
        broadcast: bool,
    ),
    ///
    pub sendCallMessage: extern "C" fn(
        object: *mut c_void,
        recipientUuid: AppByteSlice,
        message: AppByteSlice,
        urgency: i32,
    ),
    ///
    pub sendCallMessageToGroup: extern "C" fn(
        object: *mut c_void,
        groupId: AppByteSlice,
        message: AppByteSlice,
        urgency: i32,
    ),
    pub onCreateConnectionInterface: extern "C" fn(
        object: *mut c_void,
        observer: *mut c_void,
        deviceId: u32,
        context: *mut c_void,
    ) -> AppConnectionInterface,
    /// Request that the application create an application Media Stream object
    /// associated with the given application Connection object.
    pub onCreateMediaStreamInterface:
        extern "C" fn(object: *mut c_void, connection: *mut c_void) -> AppMediaStreamInterface,
    ///
    pub onConnectMedia: extern "C" fn(
        object: *mut c_void,
        remote: *const c_void,
        context: *mut c_void,
        stream: *const c_void,
    ),
    ///
    pub onCompareRemotes:
        extern "C" fn(object: *mut c_void, remote1: *const c_void, remote2: *const c_void) -> bool,
    ///
    pub onCallConcluded: extern "C" fn(object: *mut c_void, remote: *const c_void),

    // Group Calls
    ///
    pub groupCallRingUpdate: extern "C" fn(
        object: *mut c_void,
        groupId: AppByteSlice,
        ringId: i64,
        senderUuid: AppByteSlice,
        ringUpdate: i32,
    ),
    ///
    pub requestMembershipProof: extern "C" fn(object: *mut c_void, clientId: group_call::ClientId),
    ///
    pub requestGroupMembers: extern "C" fn(object: *mut c_void, clientId: group_call::ClientId),
    ///
    pub handleConnectionStateChanged:
        extern "C" fn(object: *mut c_void, clientId: group_call::ClientId, connectionState: i32),
    pub handleNetworkRouteChanged: extern "C" fn(
        object: *mut c_void,
        clientId: group_call::ClientId,
        localNetworkAdapterType: i32,
    ),
    pub handleAudioLevels: extern "C" fn(
        object: *mut c_void,
        clientId: group_call::ClientId,
        capturedLevel: u16,
        receivedAudioLevels: AppReceivedAudioLevelArray,
    ),
    ///
    pub handleJoinStateChanged:
        extern "C" fn(object: *mut c_void, clientId: group_call::ClientId, joinState: i32),
    ///
    pub handleRemoteDevicesChanged: extern "C" fn(
        object: *mut c_void,
        clientId: group_call::ClientId,
        remoteDeviceStates: AppRemoteDeviceStateArray,
    ),
    ///
    pub handleIncomingVideoTrack: extern "C" fn(
        object: *mut c_void,
        clientId: group_call::ClientId,
        remoteDemuxId: DemuxId,
        nativeVideoTrack: *mut c_void,
    ),
    ///
    pub handlePeekChanged: extern "C" fn(
        object: *mut c_void,
        clientId: group_call::ClientId,
        joinedMembers: AppUuidArray,
        creator: AppByteSlice,
        eraId: AppByteSlice,
        maxDevices: AppOptionalUInt32,
        deviceCount: u32,
    ),
    ///
    pub handleEnded:
        extern "C" fn(object: *mut c_void, clientId: group_call::ClientId, reason: i32),
}

// Add an empty Send trait to allow transfer of ownership between threads.
unsafe impl Send for AppInterface {}

// Add an empty Sync trait to allow access from multiple threads.
unsafe impl Sync for AppInterface {}

// Rust owns the interface object from Swift. Drop it when it goes out
// of scope.
impl Drop for AppInterface {
    fn drop(&mut self) {
        (self.destroy)(self.object);
    }
}

pub fn byte_vec_from_app_slice(app_slice: &AppByteSlice) -> Option<Vec<u8>> {
    Some(app_slice.as_slice()?.to_vec())
}

pub fn string_from_app_slice(app_slice: &AppByteSlice) -> Option<String> {
    Some(std::str::from_utf8(app_slice.as_slice()?).ok()?.to_string())
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn ringrtcCreateCallManager(
    appInterface: AppInterface,
    httpClient: *const http::ios::Client,
) -> *mut c_void {
    if let Some(http_client) = httpClient.as_ref() {
        call_manager::create(appInterface, http_client.clone()).unwrap_or(std::ptr::null_mut())
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetSelfUuid(callManager: *mut c_void, uuid: AppByteSlice) -> *mut c_void {
    let uuid = match byte_vec_from_app_slice(&uuid) {
        Some(uuid) => uuid,
        None => {
            error!("Missing UUID");
            return ptr::null_mut();
        }
    };
    match call_manager::set_self_uuid(callManager as *mut IosCallManager, uuid) {
        Ok(_) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcCall(
    callManager: *mut c_void,
    appRemote: *const c_void,
    callMediaType: i32,
    appLocalDevice: u32,
) -> *mut c_void {
    match call_manager::call(
        callManager as *mut IosCallManager,
        appRemote,
        CallMediaType::from_i32(callMediaType),
        appLocalDevice as DeviceId,
    ) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcProceed(
    callManager: *mut c_void,
    callId: u64,
    appCallContext: AppCallContext,
    bandwidthMode: i32,
    audioLevelsIntervalMillis: u64,
) -> *mut c_void {
    let audio_levels_interval = if audioLevelsIntervalMillis == 0 {
        None
    } else {
        Some(Duration::from_millis(audioLevelsIntervalMillis))
    };
    match call_manager::proceed(
        callManager as *mut IosCallManager,
        callId,
        appCallContext,
        BandwidthMode::from_i32(bandwidthMode),
        audio_levels_interval,
    ) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcMessageSent(callManager: *mut c_void, callId: u64) -> *mut c_void {
    match call_manager::message_sent(callManager as *mut IosCallManager, callId) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcMessageSendFailure(callManager: *mut c_void, callId: u64) -> *mut c_void {
    match call_manager::message_send_failure(callManager as *mut IosCallManager, callId) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcHangup(callManager: *mut c_void) -> *mut c_void {
    match call_manager::hangup(callManager as *mut IosCallManager) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcCancelGroupRing(
    callManager: *mut c_void,
    groupId: AppByteSlice,
    ringId: i64,
    reason: i32,
) -> *mut c_void {
    info!("ringrtcCancelGroupRing():");

    let groupId = match byte_vec_from_app_slice(&groupId) {
        Some(groupId) => groupId,
        None => {
            error!("Missing groupId");
            return ptr::null_mut();
        }
    };

    let reason = if reason == -1 {
        None
    } else {
        match group_call::RingCancelReason::try_from(reason) {
            Ok(reason) => Some(reason),
            Err(e) => {
                error!("Invalid reason: {}", e);
                return ptr::null_mut();
            }
        }
    };

    match call_manager::cancel_group_ring(
        callManager as *mut IosCallManager,
        groupId,
        ringId.into(),
        reason,
    ) {
        Ok(_) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedAnswer(
    callManager: *mut c_void,
    callId: u64,
    senderDeviceId: u32,
    opaque: AppByteSlice,
    senderIdentityKey: AppByteSlice,
    receiverIdentityKey: AppByteSlice,
) -> *mut c_void {
    match call_manager::received_answer(
        callManager as *mut IosCallManager,
        callId,
        senderDeviceId as DeviceId,
        byte_vec_from_app_slice(&opaque),
        byte_vec_from_app_slice(&senderIdentityKey),
        byte_vec_from_app_slice(&receiverIdentityKey),
    ) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(e) => {
            error!("{}", e);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedOffer(
    callManager: *mut c_void,
    callId: u64,
    remotePeer: *const c_void,
    senderDeviceId: u32,
    opaque: AppByteSlice,
    messageAgeSec: u64,
    callMediaType: i32,
    receiverDeviceId: u32,
    receiverDeviceIsPrimary: bool,
    senderIdentityKey: AppByteSlice,
    receiverIdentityKey: AppByteSlice,
) -> *mut c_void {
    match call_manager::received_offer(
        callManager as *mut IosCallManager,
        callId,
        remotePeer,
        senderDeviceId as DeviceId,
        byte_vec_from_app_slice(&opaque),
        messageAgeSec,
        CallMediaType::from_i32(callMediaType),
        receiverDeviceId as DeviceId,
        receiverDeviceIsPrimary,
        byte_vec_from_app_slice(&senderIdentityKey),
        byte_vec_from_app_slice(&receiverIdentityKey),
    ) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(e) => {
            error!("{}", e);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedIceCandidates(
    callManager: *mut c_void,
    callId: u64,
    senderDeviceId: u32,
    appIceCandidateArray: *const AppIceCandidateArray,
) -> *mut c_void {
    info!("ringrtcReceivedIceCandidates():");

    let count = unsafe { (*appIceCandidateArray).count };
    let candidates = unsafe { (*appIceCandidateArray).candidates };

    let app_ice_candidates = unsafe { slice::from_raw_parts(candidates, count) };
    let mut ice_candidates = Vec::new();

    for app_ice_candidate in app_ice_candidates {
        let opaque = byte_vec_from_app_slice(app_ice_candidate);
        match opaque {
            Some(v) => {
                ice_candidates.push(signaling::IceCandidate::new(v));
            }
            None => {
                warn!("Skipping empty opaque value");
            }
        }
    }

    match call_manager::received_ice(
        callManager as *mut IosCallManager,
        callId,
        signaling::ReceivedIce {
            ice: signaling::Ice {
                candidates: ice_candidates,
            },
            sender_device_id: senderDeviceId as DeviceId,
        },
    ) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedHangup(
    callManager: *mut c_void,
    callId: u64,
    remoteDevice: u32,
    hangupType: i32,
    deviceId: u32,
) -> *mut c_void {
    match call_manager::received_hangup(
        callManager as *mut IosCallManager,
        callId,
        remoteDevice as DeviceId,
        signaling::HangupType::from_i32(hangupType).unwrap_or(signaling::HangupType::Normal),
        deviceId as DeviceId,
    ) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedBusy(
    callManager: *mut c_void,
    callId: u64,
    remoteDevice: u32,
) -> *mut c_void {
    match call_manager::received_busy(
        callManager as *mut IosCallManager,
        callId,
        remoteDevice as DeviceId,
    ) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedCallMessage(
    callManager: *mut c_void,
    senderUuid: AppByteSlice,
    senderDeviceId: u32,
    localDeviceId: u32,
    message: AppByteSlice,
    messageAgeSec: u64,
) {
    info!("ringrtcReceivedCallMessage():");

    let sender_uuid = byte_vec_from_app_slice(&senderUuid);
    if sender_uuid.is_none() {
        error!("Invalid senderUuid");
        return;
    }

    let message = byte_vec_from_app_slice(&message);
    if message.is_none() {
        error!("Invalid message");
        return;
    }

    match call_manager::received_call_message(
        callManager as *mut IosCallManager,
        sender_uuid.unwrap(),
        senderDeviceId as DeviceId,
        localDeviceId as DeviceId,
        message.unwrap(),
        Duration::from_secs(messageAgeSec),
    ) {
        Ok(_v) => {}
        Err(_e) => {}
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcAccept(callManager: *mut c_void, callId: u64) -> *mut c_void {
    match call_manager::accept_call(callManager as *mut IosCallManager, callId) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcGetActiveConnection(callManager: *mut c_void) -> *mut c_void {
    match call_manager::get_active_connection(callManager as *mut IosCallManager) {
        Ok(v) => v,
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcGetActiveCallContext(callManager: *mut c_void) -> *mut c_void {
    match call_manager::get_active_call_context(callManager as *mut IosCallManager) {
        Ok(v) => v,
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetVideoEnable(callManager: *mut c_void, enable: bool) -> *mut c_void {
    match call_manager::set_video_enable(callManager as *mut IosCallManager, enable) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcUpdateBandwidthMode(callManager: *mut c_void, bandwidthMode: i32) {
    let result = call_manager::update_bandwidth_mode(
        callManager as *mut IosCallManager,
        BandwidthMode::from_i32(bandwidthMode),
    );
    if result.is_err() {
        error!("ringrtcUpdateBandwidthMode(): {:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcDrop(callManager: *mut c_void, callId: u64) -> *mut c_void {
    match call_manager::drop_call(callManager as *mut IosCallManager, callId) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReset(callManager: *mut c_void) -> *mut c_void {
    match call_manager::reset(callManager as *mut IosCallManager) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcClose(callManager: *mut c_void) -> *mut c_void {
    match call_manager::close(callManager as *mut IosCallManager) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

// Group Calls

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcCreateGroupCallClient(
    callManager: *mut c_void,
    groupId: AppByteSlice,
    sfuUrl: AppByteSlice,
    hkdfExtraInfo: AppByteSlice,
    audio_levels_interval_millis: u64,
    nativePeerConnectionFactoryOwnedRc: *const c_void,
    nativeAudioTrackOwnedRc: *const c_void,
    nativeVideoTrackOwnedRc: *const c_void,
) -> group_call::ClientId {
    info!("ringrtcCreateGroupCallClient():");

    // Note that failing these checks will result in the native objects being leaked.
    // So...don't do that!

    let group_id = byte_vec_from_app_slice(&groupId);
    if group_id.is_none() {
        error!("Invalid groupId");
        return group_call::INVALID_CLIENT_ID;
    }
    let sfu_url = string_from_app_slice(&sfuUrl);
    if sfu_url.is_none() {
        error!("Invalid sfuUrl");
        return group_call::INVALID_CLIENT_ID;
    }
    let hkdf_extra_info = byte_vec_from_app_slice(&hkdfExtraInfo);
    if hkdf_extra_info.is_none() {
        error!("Invalid HKDF extra info");
        return group_call::INVALID_CLIENT_ID;
    }

    let audio_levels_interval = if audio_levels_interval_millis == 0 {
        None
    } else {
        Some(Duration::from_millis(audio_levels_interval_millis))
    };

    match call_manager::create_group_call_client(
        callManager as *mut IosCallManager,
        group_id.unwrap(),
        sfu_url.unwrap(),
        hkdf_extra_info.unwrap(),
        audio_levels_interval,
        unsafe {
            webrtc::ptr::OwnedRc::from_ptr(
                nativePeerConnectionFactoryOwnedRc
                    as *const pcf::RffiPeerConnectionFactoryInterface,
            )
        },
        unsafe {
            webrtc::ptr::OwnedRc::from_ptr(nativeAudioTrackOwnedRc as *const media::RffiAudioTrack)
        },
        unsafe {
            webrtc::ptr::OwnedRc::from_ptr(nativeVideoTrackOwnedRc as *const media::RffiVideoTrack)
        },
    ) {
        Ok(client_id) => client_id,
        Err(_e) => 0,
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcDeleteGroupCallClient(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
) {
    info!("ringrtcDeleteGroupCallClient():");

    let result =
        call_manager::delete_group_call_client(callManager as *mut IosCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcConnect(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcConnect():");

    let result = call_manager::connect(callManager as *mut IosCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcJoin(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcJoin():");

    let result = call_manager::join(callManager as *mut IosCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcLeave(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcLeave():");

    let result = call_manager::leave(callManager as *mut IosCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcDisconnect(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcDisconnect():");

    let result = call_manager::disconnect(callManager as *mut IosCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcGroupRing(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
    recipient: AppByteSlice,
) {
    info!("ringrtcGroupRing():");

    let recipient = byte_vec_from_app_slice(&recipient);
    let result = call_manager::group_ring(callManager as *mut IosCallManager, clientId, recipient);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetOutgoingAudioMuted(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
    muted: bool,
) {
    info!("ringrtcSetOutgoingAudioMuted():");

    let result =
        call_manager::set_outgoing_audio_muted(callManager as *mut IosCallManager, clientId, muted);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetOutgoingVideoMuted(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
    muted: bool,
) {
    info!("ringrtcSetOutgoingVideoMuted():");

    let result =
        call_manager::set_outgoing_video_muted(callManager as *mut IosCallManager, clientId, muted);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcResendMediaKeys(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcResendMediaKeys():");

    let result = call_manager::resend_media_keys(callManager as *mut IosCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetBandwidthMode(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
    bandwidthMode: i32,
) {
    info!("ringrtcSetBandwidthMode():");

    let result = call_manager::set_bandwidth_mode(
        callManager as *mut IosCallManager,
        clientId,
        BandwidthMode::from_i32(bandwidthMode),
    );
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcRequestVideo(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
    appVideoRequestArray: *const AppVideoRequestArray,
    activeSpeakerHeight: u16,
) {
    info!("ringrtcRequestVideo():");

    let count = unsafe { (*appVideoRequestArray).count };
    let resolutions = unsafe { (*appVideoRequestArray).resolutions };

    let app_resolutions = unsafe { slice::from_raw_parts(resolutions, count) };
    let mut rendered_resolutions = Vec::new();

    for resolution in app_resolutions {
        let optional_framerate = if resolution.framerate.valid {
            Some(resolution.framerate.value)
        } else {
            None
        };

        rendered_resolutions.push(group_call::VideoRequest {
            demux_id: resolution.demux_id as DemuxId,
            width: resolution.width,
            height: resolution.height,
            framerate: optional_framerate,
        });
    }

    let result = call_manager::request_video(
        callManager as *mut IosCallManager,
        clientId,
        rendered_resolutions,
        activeSpeakerHeight,
    );
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetGroupMembers(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
    appGroupMemberInfoArray: *const AppGroupMemberInfoArray,
) {
    info!("ringrtcSetGroupMembers():");

    let count = unsafe { (*appGroupMemberInfoArray).count };
    let app_group_members = unsafe { (*appGroupMemberInfoArray).members };

    let app_members = unsafe { slice::from_raw_parts(app_group_members, count) };
    let mut group_members = Vec::new();

    for member in app_members {
        let user_id = byte_vec_from_app_slice(&member.userId);
        if user_id.is_none() {
            error!("Invalid userId");
            continue;
        }

        let member_id = byte_vec_from_app_slice(&member.memberId);
        if member_id.is_none() {
            error!("Invalid userIdCipherText");
            continue;
        }

        group_members.push(sfu::GroupMember {
            user_id: user_id.unwrap(),
            member_id: member_id.unwrap(),
        })
    }

    let result = call_manager::set_group_members(
        callManager as *mut IosCallManager,
        clientId,
        group_members,
    );
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetMembershipProof(
    callManager: *mut c_void,
    clientId: group_call::ClientId,
    proof: AppByteSlice,
) {
    info!("ringrtcSetMembershipProof():");

    let proof = byte_vec_from_app_slice(&proof);
    if proof.is_none() {
        error!("Invalid proof");
        return;
    }

    let result = call_manager::set_membership_proof(
        callManager as *mut IosCallManager,
        clientId,
        proof.unwrap(),
    );
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcIsValidOffer(
    opaque: AppByteSlice,
    messageAgeSec: u64,
    callMediaType: i32,
) -> bool {
    match call_manager::validate_offer(
        byte_vec_from_app_slice(&opaque),
        messageAgeSec,
        CallMediaType::from_i32(callMediaType),
    ) {
        Ok(()) => true,
        Err(e) => {
            error!("{:?}", e);
            false
        }
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcIsCallMessageValidOpaqueRing(
    message: AppByteSlice,
    messageAgeSec: u64,
    callbackContext: *mut c_void,
    validateGroupIdAndRing: extern "C" fn(AppByteSlice, i64, *mut c_void) -> bool,
) -> bool {
    let message = message.as_slice();
    if message.is_none() {
        error!("Invalid message");
        return false;
    }

    match call_manager::validate_call_message_as_opaque_ring(
        message.unwrap(),
        Duration::from_secs(messageAgeSec),
        |group_id, ring_id| {
            validateGroupIdAndRing(
                AppByteSlice {
                    bytes: group_id.as_ptr(),
                    len: group_id.len(),
                },
                ring_id.into(),
                callbackContext,
            )
        },
    ) {
        Ok(()) => true,
        Err(e) => {
            error!("{:?}", e);
            false
        }
    }
}
