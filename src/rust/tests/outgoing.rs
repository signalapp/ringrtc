//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Tests for outgoing calls

extern crate ringrtc;

#[macro_use]
extern crate log;

use std::ptr;
use std::thread;
use std::time::SystemTime;

use ringrtc::common::{
    AnswerParameters,
    ApplicationEvent,
    CallId,
    CallMediaType,
    CallState,
    ConnectionId,
    ConnectionState,
    DeviceId,
    FeatureLevel,
    HangupParameters,
    HangupType,
    OfferParameters,
};

use ringrtc::sim::error::SimError;

use ringrtc::webrtc::ice_candidate::IceCandidate;
use ringrtc::webrtc::media_stream::MediaStream;

#[macro_use]
mod common;
use common::{test_init, TestContext, PRNG};

// Simple test that:
// -- creates a call manager
// -- shuts down the call manager
#[test]
fn create_cm() {
    test_init();

    let _ = TestContext::new();
}

// Create an outbound N-remote call session up to the IceConnecting state.
//
// - create call manager
// - create an outbound call with N remote devices
// - check start outgoing event happened
// - check active call state is Starting
// - call proceed()
// - add received answer for each remote
// - add received ice candidate for each remote
// - check underlying Connection is in IceConnecting(true) state
// - check call is in Connecting state
//
// Now in the Connecting state.
fn start_outbound_n_remote_call(n_remotes: u16, enable_forking: bool) -> TestContext {
    let context = TestContext::new();
    let mut cm = context.cm();

    // don't go nuts
    assert!(n_remotes < 20);

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    cm.call(remote_peer, CallMediaType::Audio)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Starting
    );

    let mut remote_devices = Vec::<DeviceId>::new();

    for i in 1..(n_remotes + 1) {
        remote_devices.push(i.into());
    }

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
        1 as DeviceId,
        remote_devices,
        enable_forking,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // add a received answer for each remote
    for i in 1..(n_remotes + 1) {
        let remote_id = ConnectionId::new(active_call.call_id(), i as DeviceId);
        cm.received_answer(
            remote_id,
            AnswerParameters::new(
                format!("ANSWER-{}-{}", i, PRNG.gen::<u16>()).to_owned(),
                FeatureLevel::MultiRing,
            ),
        )
        .expect(error_line!());

        // add a received ICE candidate
        let ice_candidate = IceCandidate::new(
            "0".to_owned(),
            1,
            format!("ICE-{}", PRNG.gen::<u16>()).to_owned(),
        );
        cm.received_ice_candidates(remote_id, &[ice_candidate])
            .expect(error_line!());

        cm.synchronize().expect(error_line!());
        let connection = active_call
            .get_connection(i as DeviceId)
            .expect(error_line!());
        assert_eq!(
            connection.state().expect(error_line!()),
            ConnectionState::IceConnecting(true)
        );
    }

    assert_eq!(
        context.offers_sent(),
        if enable_forking { 1 } else { n_remotes.into() }
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Connecting
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    context
}

// Create an outbound call session up to the IceConnecting state.
//
// - create call manager
// - create an outbound call
// - check start outgoing event happened
// - check active call state is Starting
// - call proceed()
// - add received answer
// - add received ice candidate
// - check underlying Connection is in IceConnecting(true) state
// - check call is in Connecting state
//
// Now in the Connecting state.
fn start_outbound_call() -> TestContext {
    let enable_forking = false;
    start_outbound_n_remote_call(1, enable_forking)
}

// Create an outbound call session up to the CallConnected state.
//
// - create an offer
// - send offer
// - receive answer
// - ice connected
// - media stream added
// - call connected
//
// Now in the CallConnected state.

fn connect_outbound_call() -> TestContext {
    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice connected");
    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::IceConnected
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Ringing
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: add media stream");
    active_connection
        .on_add_stream(MediaStream::new(ptr::null()))
        .expect(error_line!());

    info!("test: injecting call connected");
    active_connection
        .inject_remote_connected(active_call.call_id())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::CallConnected
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Connected
    );

    assert_eq!(context.event_count(ApplicationEvent::RemoteConnected), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    context
}

#[test]
fn outbound_receive_answer() {
    test_init();

    let _ = start_outbound_call();
}

#[test]
fn outbound_call_connected() {
    test_init();

    let _ = connect_outbound_call();
}

#[test]
fn outbound_local_hang_up() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    info!("test: local hangup");
    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.event_count(ApplicationEvent::EndedLocalHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 1);

    // TODO - verify that the data_channel sent a hangup message
}

#[test]
fn outbound_ice_failed() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice connection failed");
    active_connection
        .inject_ice_connection_failed()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::Closed
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedConnectionFailure),
        1
    );
}

#[test]
fn outbound_ice_disconnected_before_call_connected() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice connected");
    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::IceConnected
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Ringing
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: injecting ice disconnected");
    active_connection
        .inject_ice_connection_disconnected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE disconnects before the call is connected, the connection
    // should return to the IceConnecting state.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::IceConnecting(true)
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
}

#[test]
fn outbound_call_connected_with_stale_call_id() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice connected");
    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::IceConnected
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Ringing
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: injecting stale call connected");
    let call_id = u64::from(active_call.call_id());
    active_connection
        .inject_remote_connected(CallId::new(call_id + 1))
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // verify bogus connect is simply dropped, with no state change.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::IceConnected
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Ringing
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
}

#[test]
fn outbound_call_connected_ice_failed() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice connection failed");
    active_connection
        .inject_ice_connection_failed()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::Closed
    );
    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedConnectionFailure),
        1
    );
}

#[test]
fn outbound_call_connected_local_hangup() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    info!("test: local hangup");

    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.event_count(ApplicationEvent::EndedLocalHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 1);

    // TODO - verify that the data_channel sent a hangup message
}

#[test]
fn outbound_ice_disconnected_after_call_connected_and_reconnect() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice disconnected");
    active_connection
        .inject_ice_connection_disconnected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE disconnects after the call is connected, the system
    // should move to the IceReconnecting state.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::IceReconnecting
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Reconnecting
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: injecting ice connected");
    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE reconnects after the call is connected, the system
    // should move to the Connected state.
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Connected
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::Reconnected), 1);
}

#[test]
fn outbound_ice_disconnected_after_call_connected_and_local_hangup() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice disconnected");
    active_connection
        .inject_ice_connection_disconnected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE disconnects after the call is connected, the system
    // should move to the IceReconnecting state.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::IceReconnecting
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Reconnecting
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    // Hang up before reconnect happens
    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.event_count(ApplicationEvent::EndedLocalHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
}

#[test]
fn inject_connection_error() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection.inject_internal_error(
        SimError::TestError("fake_error".to_string()).into(),
        "testing connection error injection",
    );

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 1);
    assert_eq!(context.ended_count(), 1);
}

#[test]
fn inject_call_error() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_call = context.active_call();

    active_call.inject_internal_error(
        SimError::TestError("fake_error".to_string()).into(),
        "testing call error injection",
    );

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 1);
    assert_eq!(context.ended_count(), 1);
}

#[test]
fn inject_local_video_status() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_local_video_status(false)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    // TODO -- verify that the data channel object sent a message
}

#[test]
fn inject_local_ice_candidate() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    let ice_candidate = IceCandidate::new("fake_spd_mid".to_string(), 0, "fake_spd".to_string());
    active_connection
        .inject_local_ice_candidate(ice_candidate)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);
}

#[test]
fn receive_remote_ice_candidate() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    // add a received ICE candidate
    let remote_id = ConnectionId::new(active_call.call_id(), 1 as DeviceId);
    let ice_candidate = IceCandidate::new(
        "0".to_owned(),
        1,
        format!("ICE-{}", PRNG.gen::<u16>()).to_owned(),
    );
    cm.received_ice_candidates(remote_id, &[ice_candidate])
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);

    // TODO -- verify the ice candidate was buffered
    // TODO -- verify the ice candidate was applied to the peer_connection
}

#[test]
fn received_remote_hangup() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    let remote_id = ConnectionId::new(active_call.call_id(), 1 as DeviceId);
    cm.received_hangup(remote_id, HangupParameters::new(HangupType::Normal, None))
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 1);
}

#[test]
fn received_remote_video_status() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    let mut enable_count = 0;
    let mut disable_count = 0;
    for _i in 0..20 {
        let enable = PRNG.gen::<bool>();

        active_connection
            .inject_remote_video_status(active_call.call_id(), enable)
            .expect(error_line!());
        cm.synchronize().expect(error_line!());

        if enable {
            enable_count += 1;
        } else {
            disable_count += 1;
        }

        assert_eq!(context.error_count(), 0);
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteVideoEnable),
            enable_count
        );
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteVideoDisable),
            disable_count
        );
    }
}

#[test]
fn call_timeout_before_connect() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();
    let mut active_call = context.active_call();

    active_call.inject_call_timeout().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedTimeout), 1);
}

#[test]
fn call_timeout_after_connect() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_call = context.active_call();

    active_call.inject_call_timeout().expect(error_line!());

    cm.synchronize().expect(error_line!());

    // The call is already connected, so the timeout is ignored.
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedTimeout), 0);
}

#[test]
fn outbound_proceed_with_error() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    cm.call(remote_peer, CallMediaType::Audio)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Starting
    );

    // cause the sending of the offer to fail.
    context.force_internal_fault(true);

    let active_call = context.active_call();
    let mut remote_devices = Vec::<DeviceId>::new();
    remote_devices.push(1);
    let enable_forking = false;
    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
        1 as DeviceId,
        remote_devices,
        enable_forking,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);

    // Two errors -- one from the failed send_offer and another from
    // the failed send_hangup, sent as part of the error clean up.
    assert_eq!(context.error_count(), 2);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedInternalFailure),
        2
    );
    assert_eq!(context.offers_sent(), 0);

    context.force_internal_fault(false);
}

#[test]
fn outbound_call_connected_local_hangup_with_error() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    // cause the sending of the hangup to fail.
    context.force_internal_fault(true);

    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 1);
    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
    assert_eq!(context.ended_count(), 2);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedInternalFailure),
        1
    );
    assert_eq!(context.event_count(ApplicationEvent::EndedLocalHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 0);
}

#[test]
fn local_ice_candidate_with_error() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    // cause the sending of the ICE candidate to fail.
    context.force_internal_fault(true);

    let ice_candidate = IceCandidate::new("fake_spd_mid".to_string(), 0, "fake_spd".to_string());
    active_connection
        .inject_local_ice_candidate(ice_candidate)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // Two errors -- one from the failed send_ice_candidates and another from
    // the failed send_hangup, sent as part of the error clean up.
    assert_eq!(context.error_count(), 2);

    assert_eq!(
        context.event_count(ApplicationEvent::EndedInternalFailure),
        2
    );
    // We should see that no ICE candidates were sent
    assert_eq!(context.ice_candidates_sent(), 0);
}

// Create outbound call to multiple remote devices.
//
// One of the remote devices accepts the call, which is followed by a
// local hangup.
#[test]
fn outbound_multiple_remote_devices_without_forking() {
    outbound_multiple_remote_devices(false)
}

#[test]
fn outbound_multiple_remote_devices_with_forking() {
    outbound_multiple_remote_devices(true)
}

fn outbound_multiple_remote_devices(enable_forking: bool) {
    test_init();

    let n_remotes: u16 = 5;
    let context = start_outbound_n_remote_call(n_remotes, enable_forking);
    let mut cm = context.cm();
    let active_call = context.active_call();

    for i in 1..(n_remotes + 1) {
        let mut connection = active_call
            .get_connection(i as DeviceId)
            .expect(error_line!());

        info!("test:{}: injecting ice connected", i);
        connection.inject_ice_connected().expect(error_line!());

        cm.synchronize().expect(error_line!());

        assert_eq!(
            connection.state().expect(error_line!()),
            ConnectionState::IceConnected
        );
        assert_eq!(
            active_call.state().expect(error_line!()),
            CallState::Ringing
        );
        assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
        assert_eq!(context.error_count(), 0);
        assert_eq!(context.ended_count(), 0);

        info!("test:{}: add media stream", i);
        connection
            .on_add_stream(MediaStream::new(ptr::null()))
            .expect(error_line!());
    }

    // connect one of the remotes
    let active_remote = (PRNG.gen::<u16>() % n_remotes) + 1;
    let mut active_connection = active_call
        .get_connection(active_remote as DeviceId)
        .expect(error_line!());

    info!(
        "test:active_remote:{}: injecting call connected",
        active_remote
    );
    assert_eq!(
        false,
        active_connection
            .app_connection()
            .unwrap()
            .outgoing_audio_enabled(),
    );
    active_connection
        .inject_remote_connected(active_call.call_id())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::CallConnected
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Connected
    );
    assert_eq!(
        true,
        active_connection
            .app_connection()
            .unwrap()
            .outgoing_audio_enabled(),
    );

    assert_eq!(context.event_count(ApplicationEvent::RemoteConnected), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert_eq!(context.accepted_hangups_sent(), 1);

    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.event_count(ApplicationEvent::EndedLocalHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
}

// Create multiple call managers, each managing one outbound call.
//
// Each call is connected and then followed by a remote hangup.
//
// Each call manager is on a separate thread
#[test]
fn outbound_multiple_call_managers() {
    test_init();

    let n_call_manager = 5;
    let enable_forking = false;

    let mut thread_vec = Vec::new();
    for i in 0..n_call_manager {
        info!("test:{}: creating call manager", i);

        let child = thread::spawn(move || {
            outbound_multiple_remote_devices(enable_forking);
        });

        thread_vec.push(child);
    }

    info!("test: joinging threads");
    for child in thread_vec {
        info!("test: joinging thread...");
        // Make sure no threads panicked
        assert!(child.join().is_ok());
    }
}

// Two users call each other at the same time
#[test]
fn glare() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();

    // Create incoming call with same remote

    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    let connection_id = ConnectionId::new(CallId::new(PRNG.gen::<u64>()), 1 as DeviceId);
    cm.received_offer(
        remote_peer,
        connection_id,
        OfferParameters::new(
            format!("OFFER-{}", PRNG.gen::<u16>()).to_owned(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect(error_line!())
                .as_millis() as u64,
            CallMediaType::Audio,
            FeatureLevel::MultiRing,
            true,
        ),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // glare case should send a busy to the new caller and conclude
    // the current call.  So two conclude call events.

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedReceivedOfferWhileActive),
        1
    );
    assert_eq!(context.busys_sent(), 1);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 1);
    assert_eq!(context.call_concluded_count(), 2);
}

// Receive a busy message when trying to establish outbound call
#[test]
fn start_outbound_receive_busy() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    cm.call(remote_peer, CallMediaType::Audio)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let call_id = {
        let active_call = context.active_call();
        assert_eq!(
            active_call.state().expect(error_line!()),
            CallState::Starting
        );
        active_call.call_id()
    };

    let mut remote_devices = Vec::<DeviceId>::new();

    remote_devices.push(1);
    let enable_forking = false;
    cm.proceed(
        call_id,
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
        1 as DeviceId,
        remote_devices,
        enable_forking,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // Receive a busy message
    let remote_id = ConnectionId::new(call_id, 1 as DeviceId);
    cm.received_busy(remote_id).expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteBusy), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.call_concluded_count(), 1);
}
