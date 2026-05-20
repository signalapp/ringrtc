//
// Copyright 2026 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use jni::{
    Env, bind_java_type,
    refs::{LoaderContext, Reference as _},
};

// RingRTC App Classes
bind_java_type! { pub CallSummary => org.signal.ringrtc.CallSummary }
bind_java_type! { pub QualityStats => org.signal.ringrtc.CallSummary::QualityStats }
bind_java_type! { pub MediaQualityStats => org.signal.ringrtc.CallSummary::MediaQualityStats }
bind_java_type! { pub CallLinkState => org.signal.ringrtc.CallLinkState }
bind_java_type! { pub CallLinkRootKey => org.signal.ringrtc.CallLinkRootKey }
bind_java_type! { pub HttpHeader => org.signal.ringrtc.HttpHeader }
bind_java_type! { pub HttpResult => org.signal.ringrtc.CallManager::HttpResult }
bind_java_type! { pub PeekInfo => org.signal.ringrtc.PeekInfo }
bind_java_type! { pub Reaction => org.signal.ringrtc.GroupCall::Reaction }
bind_java_type! { pub RemoteDeviceState => org.signal.ringrtc.GroupCall::RemoteDeviceState }
bind_java_type! { pub ReceivedAudioLevel => org.signal.ringrtc.GroupCall::ReceivedAudioLevel }

// RingRTC Enum Classes
bind_java_type! {
    pub CallEvent => org.signal.ringrtc.CallManager::CallEvent,
    methods { static fn from_native_index(value: jint) -> CallEvent }
}
bind_java_type! {
    pub CallMediaType => org.signal.ringrtc.CallManager::CallMediaType,
    methods { static fn from_native_index(value: jint) -> CallMediaType }
}
bind_java_type! {
    pub HangupType => org.signal.ringrtc.CallManager::HangupType,
    methods { static fn from_native_index(value: jint) -> HangupType }
}
bind_java_type! {
    pub HttpMethod => org.signal.ringrtc.CallManager::HttpMethod,
    methods { static fn from_native_index(value: jint) -> HttpMethod }
}
bind_java_type! {
    pub CallEndReason => org.signal.ringrtc.CallManager::CallEndReason,
    methods { static fn from_native_index(value: jint) -> CallEndReason }
}
bind_java_type! {
    pub ConnectionState => org.signal.ringrtc.GroupCall::ConnectionState,
    methods { static fn from_native_index(value: jint) -> ConnectionState }
}
bind_java_type! {
    pub JoinState => org.signal.ringrtc.GroupCall::JoinState,
    methods { static fn from_native_index(value: jint) -> JoinState }
}
bind_java_type! {
    pub SpeechEvent => org.signal.ringrtc.GroupCall::SpeechEvent,
    methods { static fn from_native_index(value: jint) -> SpeechEvent }
}

// JDK Primitives
bind_java_type! {
    pub JBoolean => java.lang.Boolean,
    constructors { fn new(value: jboolean) }
}
bind_java_type! {
    pub JFloat => java.lang.Float,
    constructors { fn new(value: jfloat) }
}
bind_java_type! {
    pub JInteger => java.lang.Integer,
    constructors { fn new(value: jint) }
}
bind_java_type! {
    pub JLong => java.lang.Long,
    constructors { fn new(value: jlong) }
}

// JDK Collections
bind_java_type! {
    pub JArrayList => java.util.ArrayList,
    constructors { fn with_capacity(initial_capacity: jint) }
}
bind_java_type! {
    pub JHashMap => java.util.HashMap,
    constructors { fn with_capacity(initial_capacity: jint) }
}

pub fn init_class_cache(env: &mut Env) -> jni::errors::Result<()> {
    let ctx = LoaderContext::default();
    CallSummary::lookup_class(env, &ctx)?;
    QualityStats::lookup_class(env, &ctx)?;
    MediaQualityStats::lookup_class(env, &ctx)?;
    CallLinkState::lookup_class(env, &ctx)?;
    CallLinkRootKey::lookup_class(env, &ctx)?;
    HttpHeader::lookup_class(env, &ctx)?;
    HttpResult::lookup_class(env, &ctx)?;
    PeekInfo::lookup_class(env, &ctx)?;
    Reaction::lookup_class(env, &ctx)?;
    RemoteDeviceState::lookup_class(env, &ctx)?;
    ReceivedAudioLevel::lookup_class(env, &ctx)?;
    CallEvent::lookup_class(env, &ctx)?;
    CallMediaType::lookup_class(env, &ctx)?;
    HangupType::lookup_class(env, &ctx)?;
    HttpMethod::lookup_class(env, &ctx)?;
    CallEndReason::lookup_class(env, &ctx)?;
    ConnectionState::lookup_class(env, &ctx)?;
    JoinState::lookup_class(env, &ctx)?;
    SpeechEvent::lookup_class(env, &ctx)?;
    JBoolean::lookup_class(env, &ctx)?;
    JFloat::lookup_class(env, &ctx)?;
    JInteger::lookup_class(env, &ctx)?;
    JLong::lookup_class(env, &ctx)?;
    JArrayList::lookup_class(env, &ctx)?;
    JHashMap::lookup_class(env, &ctx)?;
    Ok(())
}
