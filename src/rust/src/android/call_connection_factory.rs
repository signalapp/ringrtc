//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Android CallConnectionFactory Interface.

use std::sync::{
    Arc,
    Mutex,
};

use jni::JNIEnv;
use jni::objects::{
    JObject,
    JClass,
    JString,
    JList,
};
use jni::sys::jlong;
use log::Level;

use crate::android::call_connection::AndroidCallConnection;
use crate::android::call_connection_observer::AndroidCallConnectionObserver;
use crate::android::error::AndroidError;
use crate::android::webrtc_peer_connection_factory::*;
use crate::android::jni_util::*;
use crate::android::logging::init_logging;
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

type AndroidCallConnectionFactory = CallConnectionFactory<AndroidCallConnection>;

/// Library initialization routine.
///
/// Sets up the logging infrastructure.
pub fn native_initialize() -> Result<()> {

    init_logging(Level::Debug)?;

    Ok(())

}

/// Creates a new AndroidCallConnectionFactory object.
pub fn native_create_call_connection_factory(peer_connection_factory: jlong) -> Result<jlong> {

    let cc_factory = AndroidCallConnectionFactory::new(peer_connection_factory as CppObject)?;
    // Wrap factory in Arc<Mutex<>> to pass amongst threads
    let cc_factory = Arc::new(Mutex::new(cc_factory));
    let ptr = Arc::into_raw(cc_factory);
    Ok(ptr as jlong)
}

/// Frees a new AndroidCallConnectionFactory object.
pub fn native_free_factory(factory: jlong) -> Result<()> {
    // Convert integer back into Arc, then Arc will free things up as
    // it goes out of scope.
    let call_connection_factory: Arc<Mutex<AndroidCallConnectionFactory>> = get_arc_from_jlong(factory)?;
    if let Ok(mut cc_factory) = call_connection_factory.lock() {
        cc_factory.close()?;
    }
    Ok(())
}

/// Rust/JNI version of a Java org.webrtc.PeerConnection.IceServer.
struct PeerConnectionIceServer<'a> {
    uri: JString<'a>,
    username: JString<'a>,
    password: JString<'a>,
}

impl<'a> PeerConnectionIceServer<'a> {
    pub fn new(uri: JString<'a>,
               username: JString<'a>,
               password: JString<'a>) -> PeerConnectionIceServer<'a> {
        PeerConnectionIceServer {
            uri,
            username,
            password,
        }
    }
}

/// Fetch the ICE/TURN servers for the service.
fn fetch_ice_servers<'a>(env: &'a JNIEnv,
                         config: JObject) -> Result<Vec<PeerConnectionIceServer<'a>>> {

    // Get account manager object out of the config
    const ACCOUNT_MANAGER_FIELD: &str = "accountManager";
    const ACCOUNT_MANAGER_SIG: &str = "Lorg/whispersystems/signalservice/api/SignalServiceAccountManager;";
    let account_manager = jni_get_field(env, config,
                                        ACCOUNT_MANAGER_FIELD,
                                        ACCOUNT_MANAGER_SIG)?.l()?;

    const GET_TURN_SERVER_METHOD: &str = "getTurnServerInfo";
    const GET_TURN_SERVER_SIG: &str = "()Lorg/whispersystems/signalservice/api/messages/calls/TurnServerInfo;";
    let turn_server_info = jni_call_method(env, account_manager,
                                           GET_TURN_SERVER_METHOD,
                                           GET_TURN_SERVER_SIG,
                                           &[])?.l()?;
    // getTurnServerInfo() can throw an IOException
    if let Ok(exception_thrown) = env.exception_check() {
        if exception_thrown {
            env.exception_clear()?;
            return Err(AndroidError::JniException("getTurnServerInfo()".to_string()).into());
        }
    }

    const TURN_SERVER_INFO_GET_USER_NAME_METHOD: &str ="getUsername";
    const SIG_VOID_RET_STRING: &str = "()Ljava/lang/String;";
    let user_name = jni_call_method(env, turn_server_info,
                                    TURN_SERVER_INFO_GET_USER_NAME_METHOD,
                                    SIG_VOID_RET_STRING,
                                    &[])?.l()?;

    const TURN_SERVER_INFO_GET_PASSWORD_METHOD: &str ="getPassword";
    let password = jni_call_method(env, turn_server_info,
                                   TURN_SERVER_INFO_GET_PASSWORD_METHOD,
                                   SIG_VOID_RET_STRING,
                                   &[])?.l()?;

    const TURN_SERVER_INFO_GET_URLS_METHOD: &str = "getUrls";
    const TURN_SERVER_INFO_GET_URLS_SIG: &str = "()Ljava/util/List;";
    let urls = jni_call_method(env, turn_server_info,
                               TURN_SERVER_INFO_GET_URLS_METHOD,
                               TURN_SERVER_INFO_GET_URLS_SIG,
                               &[])?.l()?;

    // Convert result into a collection of PeerConnectionIceServer objects
    let mut ice_servers: Vec<PeerConnectionIceServer> = Vec::new();

    // Put googles STUN server first in the list
    const STUN_SERVER: &str = "stun:stun1.l.google.com:19302";
    ice_servers.push(PeerConnectionIceServer::new(env.new_string(STUN_SERVER)?, env.new_string("")?, env.new_string("")?));

    let url_list = env.get_list(urls)?;
    let url_iter = url_list.iter()?;
    for url in url_iter {
        let url_str: String = env.get_string(url.into())?.into();
        if url_str.starts_with("turn") {
            ice_servers.push(PeerConnectionIceServer::new(env.new_string(url_str)?, user_name.into(), password.into()));
        } else {
            ice_servers.push(PeerConnectionIceServer::new(env.new_string(url_str)?, env.new_string("")?, env.new_string("")?));
        }
    }

    Ok(ice_servers)
}

/// Create a JNI list of `org/webrtc/PeerConnection$IceServer` objects.
fn create_java_ice_servers<'a>(env: &'a JNIEnv,
                               ice_servers: &'a [PeerConnectionIceServer]) -> Result<JList<'a, 'a>> {
    let list = jni_new_linked_list(env)?;
    for server in ice_servers {
        debug!("ice_server: {}", String::from(env.get_string(server.uri)?));
        const ICE_SERVER_CLASS: &str = "org/webrtc/PeerConnection$IceServer";
        const ICE_SERVER_CLASS_SIG: &str = "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V";
        let args = [ JObject::from(server.uri).into(),
                     JObject::from(server.username).into(),
                     JObject::from(server.password).into(),
        ];
        let ice_server_obj = jni_new_object(env,
                                            ICE_SERVER_CLASS,
                                            ICE_SERVER_CLASS_SIG,
                                            &args)?;
        list.add(ice_server_obj)?;
    }

    Ok(list)

}

/// Create a Rust CallConnectionInterface for Android
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
#[allow(clippy::too_many_arguments)]
pub fn native_create_call_connection(env:               &JNIEnv,
                                     class:             JClass,
                                     native_factory:    jlong,
                                     call_config:       JObject,
                                     native_observer:   jlong,
                                     rtc_config:        JObject,
                                     media_constraints: JObject,
                                     ssl_cert_verifier: JObject) -> Result<jlong> {

    let call_connection_factory: ArcPtr<Mutex<AndroidCallConnectionFactory>> = get_arc_ptr_from_jlong(native_factory)?;
    let mut cc_factory = match call_connection_factory.get_arc().lock() {
        Ok(v) => v,
        Err(_) => return Err(RingRtcError::MutexPoisoned("Call Connection Factory".to_string()).into()),
    };

    // Get callId from configuration
    const CALL_ID_FIELD: &str = "callId";
    const CALL_ID_SIG: &str = "J";
    let call_id: CallId = jni_get_field(env, call_config,
                                        CALL_ID_FIELD,
                                        CALL_ID_SIG)?.j()?;

    // Get recipient from configuration
    const RECIPIENT_FIELD: &str = "recipient";
    const RECIPIENT_SIG: &str = "Lorg/signal/ringrtc/SignalMessageRecipient;";
    let recipient: JObject = jni_get_field(env, call_config,
                                           RECIPIENT_FIELD,
                                           RECIPIENT_SIG)?.l()?;
    let jrecipient = env.new_global_ref(recipient)?;

    // Get call direction from configuration
    const OUT_BOUND_FIELD: &str = "outBound";
    const OUT_BOUND_SIG: &str = "Z";
    let out_bound: bool = jni_get_field(env, call_config,
                                        OUT_BOUND_FIELD,
                                        OUT_BOUND_SIG)?.z()?;

    let direction = if out_bound {
        CallDirection::OutGoing
    } else {
        CallDirection::InComing
    };

    let call_connection = AndroidCallConnection::new(call_id, direction, jrecipient);
    let call_connection_handle = cc_factory.create_call_connection_handle(call_connection)?;
    info!("call_connection object: debug {:?}", call_connection_handle);

    let data_channel_cc_handle = call_connection_handle.clone();

    let cc_handle = Box::new(call_connection_handle);
    let cc_ptr = Box::into_raw(cc_handle);

    let pc_observer = PeerConnectionObserver::new(cc_ptr)?;
    info!("pc_observer: {:?}", pc_observer);

    // fetch ICE servers
    let ice_servers = fetch_ice_servers(env, call_config)?;

    // Set the turn servers in the rtc_config object....
    // turn rust vector of ice servers into a java linked list
    let java_ice_servers = create_java_ice_servers(env, &ice_servers)?;
    jni_set_field(env, rtc_config, "iceServers", "Ljava/util/List;", JObject::from(java_ice_servers).into())?;

    // construct JNI OwnedPeerConnection object
    let jni_owned_pc = unsafe {
        Java_org_webrtc_PeerConnectionFactory_nativeCreatePeerConnection(
            env.clone(), class, cc_factory.get_native_peer_connection_factory() as jlong,
            rtc_config, media_constraints, pc_observer.get_rffi_interface() as jlong, ssl_cert_verifier)
    };
    info!("jni_owned_pc: {}", jni_owned_pc);

    if jni_owned_pc == 0 {
        return Err(AndroidError::CreateJniPeerConnection.into());
    }

    // Retrieve the underlying PeerConnectionInterface object from the
    // JNI owned peerconnection object.
    let rffi_pc_interface = get_peer_connection_interface(jni_owned_pc);
    if rffi_pc_interface.is_null() {
        return Err(AndroidError::ExtractNativePeerConnectionInterface.into());
    }

    let pc_interface = PeerConnection::new(rffi_pc_interface);

    // Convert the native observer integer back into an Boxed object
    let cc_observer: Box<AndroidCallConnectionObserver> = get_object_from_jlong(native_observer)?;

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
        cc.update_pc(env, jni_owned_pc, pc_interface, cc_observer)?;
    } else {
        error!("Initial mutex is poisoned");
        return Err(RingRtcError::MutexPoisoned("CallConnectionHandle in CallConnectionFactory".to_string()).into());
    }

    Ok(Box::into_raw(cc_handle) as jlong)
}


/// Return WebRTC C++ PeerConnectionInterface from the Java/JNI
/// PeerConnection object.
fn get_peer_connection_interface(jni_owned_pc: i64) -> *const RffiPeerConnectionInterface {
    unsafe { Rust_getPeerConnectionInterface(jni_owned_pc) as *const RffiPeerConnectionInterface }
}

extern {
    fn Rust_getPeerConnectionInterface(jni_owned_pc: i64) -> CppObject;
}
