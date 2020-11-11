//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Call Manager Interface

use std::ffi::c_void;
use std::{fmt, ptr, slice, str};

use libc::size_t;

use crate::ios::call_manager;
use crate::ios::call_manager::IOSCallManager;
use crate::ios::logging::IOSLogger;

use crate::common::{BandwidthMode, CallMediaType, DeviceId, FeatureLevel, HttpResponse};
use crate::core::group_call;
use crate::core::signaling;

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
    pub len:   size_t,
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

/// Structure for passing Ice Candidates to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppIceCandidate {
    pub opaque: AppByteSlice,
    pub sdp:    AppByteSlice,
}

/// Structure for passing multiple Ice Candidates to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppIceCandidateArray {
    pub candidates: *const AppIceCandidate,
    pub count:      size_t,
}

/// Structure for passing name/value strings to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppHeader {
    pub name:  AppByteSlice,
    pub value: AppByteSlice,
}

/// Structure for passing multiple name/value headers to/from Swift.
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppHeaderArray {
    pub headers: *const AppHeader,
    pub count:   size_t,
}

/// Structure for passing connection details from the application.
#[repr(C)]
#[derive(Clone, Debug)]
#[allow(non_snake_case)]
pub struct AppConnectionInterface {
    pub object:  *mut c_void,
    pub pc:      *mut c_void,
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
    pub object:  *mut c_void,
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
#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppMediaStreamInterface {
    pub object:            *mut c_void,
    /// Swift object clean up method.
    pub destroy:           extern "C" fn(object: *mut c_void),
    /// Returns a pointer to a RTCMediaStream object.
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
    pub demuxId:           group_call::DemuxId,
    pub user_id:           AppByteSlice,
    pub mediaKeysReceived: bool,
    pub audioMuted:        AppOptionalBool,
    pub videoMuted:        AppOptionalBool,
    pub addedTime:         u64, // unix millis
    pub speakerTime:       u64, // unix millis; 0 if never was a speaker
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppRemoteDeviceStateArray {
    pub states: *const AppRemoteDeviceState,
    pub count:  size_t,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppGroupMemberInfo {
    pub userId:           AppByteSlice,
    pub userIdCipherText: AppByteSlice,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppGroupMemberInfoArray {
    pub members: *const AppGroupMemberInfo,
    pub count:   size_t,
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
    pub demux_id:  group_call::DemuxId,
    pub width:     u16,
    pub height:    u16,
    pub framerate: AppOptionalUInt16,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
pub struct AppVideoRequestArray {
    pub resolutions: *const AppVideoRequest,
    pub count:       size_t,
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_snake_case)]
/// iOS Interface for communicating with the Swift application.
pub struct AppInterface {
    /// Raw Swift object pointer.
    pub object:                       *mut c_void,
    /// Swift object clean up method.
    pub destroy:                      extern "C" fn(object: *mut c_void),
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
    pub onSendOffer: extern "C" fn(
        object: *mut c_void,
        callId: u64,
        remote: *const c_void,
        destinationDeviceId: u32,
        broadcast: bool,
        opaque: AppByteSlice,
        sdp: AppByteSlice,
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
        sdp: AppByteSlice,
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
        useLegacyHangupMessage: bool,
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
    pub sendCallMessage:
        extern "C" fn(object: *mut c_void, recipientUuid: AppByteSlice, message: AppByteSlice),
    ///
    pub sendHttpRequest: extern "C" fn(
        object: *mut c_void,
        requestId: u32,
        url: AppByteSlice,
        method: i32,
        headerArray: AppHeaderArray,
        body: AppByteSlice,
    ),
    ///
    pub onCreateConnectionInterface: extern "C" fn(
        object: *mut c_void,
        observer: *mut c_void,
        deviceId: u32,
        context: *mut c_void,
        enable_dtls: bool,
        enable_rtp_data_channel: bool,
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
    pub onCallConcluded:              extern "C" fn(object: *mut c_void, remote: *const c_void),

    // Group Calls
    ///
    pub handlePeekResponse: extern "C" fn(
        object: *mut c_void,
        requestId: u32,
        joinedMembers: AppUuidArray,
        creator: AppByteSlice,
        eraId: AppByteSlice,
        maxDevices: AppOptionalUInt32,
        deviceCount: u32,
    ),
    ///
    pub requestMembershipProof: extern "C" fn(object: *mut c_void, clientId: group_call::ClientId),
    ///
    pub requestGroupMembers: extern "C" fn(object: *mut c_void, clientId: group_call::ClientId),
    ///
    pub handleConnectionStateChanged:
        extern "C" fn(object: *mut c_void, clientId: group_call::ClientId, connectionState: i32),
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
        remoteDemuxId: group_call::DemuxId,
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
    if app_slice.bytes.is_null() {
        return None;
    }
    let slice = unsafe { slice::from_raw_parts(app_slice.bytes, app_slice.len as usize) };
    Some(slice.to_vec())
}

pub fn string_from_app_slice(app_slice: &AppByteSlice) -> Option<String> {
    if app_slice.bytes.is_null() {
        return None;
    }
    let slice = unsafe { slice::from_raw_parts(app_slice.bytes, app_slice.len as usize) };
    match str::from_utf8(slice) {
        Ok(s) => Some(s.to_string()),
        Err(_) => None,
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcInitialize(logObject: IOSLogger) -> *mut c_void {
    match call_manager::initialize(logObject) {
        Ok(_v) => {
            // Return non-null pointer to indicate success.
            1 as *mut c_void
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcCreate(
    appCallManager: *mut c_void,
    appInterface: AppInterface,
) -> *mut c_void {
    match call_manager::create(appCallManager, appInterface) {
        Ok(v) => v,
        Err(_e) => ptr::null_mut(),
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
        callManager as *mut IOSCallManager,
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
) -> *mut c_void {
    match call_manager::proceed(callManager as *mut IOSCallManager, callId, appCallContext) {
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
    match call_manager::message_sent(callManager as *mut IOSCallManager, callId) {
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
    match call_manager::message_send_failure(callManager as *mut IOSCallManager, callId) {
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
    match call_manager::hangup(callManager as *mut IOSCallManager) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedAnswer(
    callManager: *mut c_void,
    callId: u64,
    senderDeviceId: u32,
    opaque: AppByteSlice,
    sdp: AppByteSlice,
    senderSupportsMultiRing: bool,
    senderIdentityKey: AppByteSlice,
    receiverIdentityKey: AppByteSlice,
) -> *mut c_void {
    let sender_device_feature_level = if senderSupportsMultiRing {
        FeatureLevel::MultiRing
    } else {
        FeatureLevel::Unspecified
    };

    match call_manager::received_answer(
        callManager as *mut IOSCallManager,
        callId,
        senderDeviceId as DeviceId,
        byte_vec_from_app_slice(&opaque),
        string_from_app_slice(&sdp),
        sender_device_feature_level,
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
    sdp: AppByteSlice,
    messageAgeSec: u64,
    callMediaType: i32,
    receiverDeviceId: u32,
    senderSupportsMultiRing: bool,
    receiverDeviceIsPrimary: bool,
    senderIdentityKey: AppByteSlice,
    receiverIdentityKey: AppByteSlice,
) -> *mut c_void {
    let sender_device_feature_level = if senderSupportsMultiRing {
        FeatureLevel::MultiRing
    } else {
        FeatureLevel::Unspecified
    };

    match call_manager::received_offer(
        callManager as *mut IOSCallManager,
        callId,
        remotePeer,
        senderDeviceId as DeviceId,
        byte_vec_from_app_slice(&opaque),
        string_from_app_slice(&sdp),
        messageAgeSec,
        CallMediaType::from_i32(callMediaType),
        receiverDeviceId as DeviceId,
        sender_device_feature_level,
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
    let count = unsafe { (*appIceCandidateArray).count };
    let candidates = unsafe { (*appIceCandidateArray).candidates };

    let app_ice_candidates = unsafe { slice::from_raw_parts(candidates, count) };
    let mut ice_candidates = Vec::new();

    for app_ice_candidate in app_ice_candidates {
        ice_candidates.push(signaling::IceCandidate::from_opaque_or_sdp(
            byte_vec_from_app_slice(&app_ice_candidate.opaque),
            string_from_app_slice(&app_ice_candidate.sdp),
        ));
    }

    match call_manager::received_ice(
        callManager as *mut IOSCallManager,
        callId,
        signaling::ReceivedIce {
            ice:              signaling::Ice {
                candidates_added: ice_candidates,
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
        callManager as *mut IOSCallManager,
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
        callManager as *mut IOSCallManager,
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
        callManager as *mut IOSCallManager,
        sender_uuid.unwrap(),
        senderDeviceId as DeviceId,
        localDeviceId as DeviceId,
        message.unwrap(),
        messageAgeSec,
    ) {
        Ok(_v) => {}
        Err(_e) => {}
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcReceivedHttpResponse(
    callManager: *mut c_void,
    requestId: u32,
    statusCode: u16,
    body: AppByteSlice,
) {
    info!("ringrtcReceivedHttpResponse():");

    let body = byte_vec_from_app_slice(&body);
    if body.is_none() {
        error!("Invalid body");
        return;
    }

    let response = HttpResponse {
        status_code: statusCode,
        body:        body.unwrap(),
    };

    let result = call_manager::received_http_response(
        callManager as *mut IOSCallManager,
        requestId,
        Some(response),
    );
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcHttpRequestFailed(callManager: *mut c_void, requestId: u32) {
    info!("ringrtcHttpRequestFailed():");

    let result =
        call_manager::received_http_response(callManager as *mut IOSCallManager, requestId, None);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcAccept(callManager: *mut c_void, callId: u64) -> *mut c_void {
    match call_manager::accept_call(callManager as *mut IOSCallManager, callId) {
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
    match call_manager::get_active_connection(callManager as *mut IOSCallManager) {
        Ok(v) => v,
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcGetActiveCallContext(callManager: *mut c_void) -> *mut c_void {
    match call_manager::get_active_call_context(callManager as *mut IOSCallManager) {
        Ok(v) => v,
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetVideoEnable(callManager: *mut c_void, enable: bool) -> *mut c_void {
    match call_manager::set_video_enable(callManager as *mut IOSCallManager, enable) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcSetLowBandwidthMode(
    callManager: *mut c_void,
    enabled: bool,
) -> *mut c_void {
    let mode = if enabled {
        BandwidthMode::Low
    } else {
        BandwidthMode::Normal
    };
    match call_manager::set_direct_bandwidth_mode(callManager as *mut IOSCallManager, mode) {
        Ok(_v) => {
            // Return the object reference back as indication of success.
            callManager
        }
        Err(_e) => ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcDrop(callManager: *mut c_void, callId: u64) -> *mut c_void {
    match call_manager::drop_call(callManager as *mut IOSCallManager, callId) {
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
    match call_manager::reset(callManager as *mut IOSCallManager) {
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
    match call_manager::close(callManager as *mut IOSCallManager) {
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
pub extern "C" fn ringrtcPeekGroupCall(
    callManager: *mut c_void,
    requestId: u32,
    sfuUrl: AppByteSlice,
    proof: AppByteSlice,
    appGroupMemberInfoArray: *const AppGroupMemberInfoArray,
) {
    info!("ringrtcPeekGroupCall():");

    let sfu_url = string_from_app_slice(&sfuUrl);
    if sfu_url.is_none() {
        error!("Invalid sfuUrl");
        return;
    }

    let proof = byte_vec_from_app_slice(&proof);
    if proof.is_none() {
        error!("Invalid proof");
        return;
    }

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

        let user_id_ciphertext = byte_vec_from_app_slice(&member.userIdCipherText);
        if user_id_ciphertext.is_none() {
            error!("Invalid userIdCipherText");
            continue;
        }

        group_members.push(group_call::GroupMemberInfo {
            user_id:            user_id.unwrap(),
            user_id_ciphertext: user_id_ciphertext.unwrap(),
        })
    }

    let result = call_manager::peek_group_call(
        callManager as *mut IOSCallManager,
        requestId,
        sfu_url.unwrap(),
        proof.unwrap(),
        group_members,
    );
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcCreateGroupCallClient(
    callManager: *mut c_void,
    groupId: AppByteSlice,
    sfuUrl: AppByteSlice,
    nativeAudioTrack: *const c_void,
    nativeVideoTrack: *const c_void,
) -> group_call::ClientId {
    info!("ringrtcCreateGroupCallClient():");

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

    match call_manager::create_group_call_client(
        callManager as *mut IOSCallManager,
        group_id.unwrap(),
        sfu_url.unwrap(),
        nativeAudioTrack,
        nativeVideoTrack,
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
        call_manager::delete_group_call_client(callManager as *mut IOSCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcConnect(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcConnect():");

    let result = call_manager::connect(callManager as *mut IOSCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcJoin(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcJoin():");

    let result = call_manager::join(callManager as *mut IOSCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcLeave(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcLeave():");

    let result = call_manager::leave(callManager as *mut IOSCallManager, clientId);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcDisconnect(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcDisconnect():");

    let result = call_manager::disconnect(callManager as *mut IOSCallManager, clientId);
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
        call_manager::set_outgoing_audio_muted(callManager as *mut IOSCallManager, clientId, muted);
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
        call_manager::set_outgoing_video_muted(callManager as *mut IOSCallManager, clientId, muted);
    if result.is_err() {
        error!("{:?}", result.err());
    }
}

#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn ringrtcResendMediaKeys(callManager: *mut c_void, clientId: group_call::ClientId) {
    info!("ringrtcResendMediaKeys():");

    let result = call_manager::resend_media_keys(callManager as *mut IOSCallManager, clientId);
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

    // Translate from the app's mode to the internal bitrate version.
    let bandwidth_mode = if bandwidthMode == 0 {
        BandwidthMode::Low
    } else if bandwidthMode == 1 {
        BandwidthMode::Normal
    } else {
        warn!("Invalid bandwidthMode: {}", bandwidthMode);
        return;
    };

    let result = call_manager::set_bandwidth_mode(
        callManager as *mut IOSCallManager,
        clientId,
        bandwidth_mode,
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
            demux_id:  resolution.demux_id as group_call::DemuxId,
            width:     resolution.width,
            height:    resolution.height,
            framerate: optional_framerate,
        });
    }

    let result = call_manager::request_video(
        callManager as *mut IOSCallManager,
        clientId,
        rendered_resolutions,
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

        let user_id_ciphertext = byte_vec_from_app_slice(&member.userIdCipherText);
        if user_id_ciphertext.is_none() {
            error!("Invalid userIdCipherText");
            continue;
        }

        group_members.push(group_call::GroupMemberInfo {
            user_id:            user_id.unwrap(),
            user_id_ciphertext: user_id_ciphertext.unwrap(),
        })
    }

    let result = call_manager::set_group_members(
        callManager as *mut IOSCallManager,
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
        callManager as *mut IOSCallManager,
        clientId,
        proof.unwrap(),
    );
    if result.is_err() {
        error!("{:?}", result.err());
    }
}
