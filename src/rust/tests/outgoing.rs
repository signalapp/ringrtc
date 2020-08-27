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
use std::time::Duration;

use ringrtc::common::{
    ApplicationEvent,
    BandwidthMode,
    CallId,
    CallMediaType,
    CallState,
    ConnectionState,
    DeviceId,
};
use ringrtc::core::signaling;

use ringrtc::sim::error::SimError;

use ringrtc::webrtc::media::MediaStream;

#[macro_use]
mod common;
use common::{
    random_ice_candidate,
    random_received_answer,
    random_received_ice_candidate,
    random_received_offer,
    test_init,
    SignalingType,
    TestContext,
    PRNG,
};

// Simple test that:
// -- creates a call manager
// -- shuts down the call manager
#[test]
fn create_cm() {
    test_init();

    let _ = TestContext::new();
}

// Create an outbound call, sending offer to an unknown number of remotes.
//
// - create call manager
// - create an outbound call with N remote devices
// - check start outgoing event happened
// - check active call state is Starting
// - call proceed() with forking
//
fn start_outbound_and_proceed() -> TestContext {
    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    cm.call(remote_peer, CallMediaType::Audio, 1 as DeviceId)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::WaitingToProceed
    );

    let mut remote_devices = Vec::<DeviceId>::new();
    remote_devices.push(1);

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.offers_sent(), 1);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    context
}

// Create an outbound N-remote call session up to the ConnectingBeforeAccepted state.
//
// - create call manager
// - create an outbound call with N remote devices
// - check start outgoing event happened
// - check active call state is Starting
// - call proceed()
// - add received answer for each remote
// - add received ice candidate for each remote
// - check underlying Connection is in ConnectingBeforeAccepted state
// - check call is in Connecting state
//
// Now in the Connecting state.
fn start_outbound_n_remote_call(signaling_type: SignalingType, n_remotes: u16) -> TestContext {
    let context = TestContext::new();
    let mut cm = context.cm();

    // don't go nuts
    assert!(n_remotes < 20);

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    cm.call(remote_peer, CallMediaType::Audio, 1 as DeviceId)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::WaitingToProceed
    );

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // add a received answer for each remote
    for i in 1..(n_remotes + 1) {
        let call_id = active_call.call_id();
        cm.received_answer(
            call_id,
            random_received_answer(signaling_type, i as DeviceId),
        )
        .expect(error_line!());

        cm.received_ice(call_id, random_received_ice_candidate(signaling_type))
            .expect(error_line!());

        cm.synchronize().expect(error_line!());
        let connection = active_call
            .get_connection(i as DeviceId)
            .expect(error_line!());
        assert_eq!(
            connection.state().expect(error_line!()),
            ConnectionState::ConnectingBeforeAccepted
        );
    }

    assert_eq!(context.offers_sent(), 1,);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    context
}

// Create an outbound call session up to the ConnectingBeforeAccepted state.
//
// - create call manager
// - create an outbound call
// - check start outgoing event happened
// - check active call state is Starting
// - call proceed()
// - add received answer
// - add received ice candidate
// - check underlying Connection is in ConnectingBeforeAccepted state
// - check call is in Connecting state
//
// Now in the Connecting state.
fn start_outbound_call(signaling_type: SignalingType) -> TestContext {
    start_outbound_n_remote_call(signaling_type, 1)
}

// Create an outbound call session up to the Accepted state.
//
// - create an offer
// - send offer
// - receive answer
// - ice connected
// - media stream added
// - call accepted
//
// Now in the Accepted state.

fn connect_outbound_call() -> TestContext {
    let context = start_outbound_call(SignalingType::Legacy);
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
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedWithDataChannelBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: inject incoming stream");
    active_connection
        .inject_received_incoming_media(MediaStream::new(ptr::null()))
        .expect(error_line!());

    info!("test: injecting accepted");
    active_connection
        .inject_received_accepted_via_data_channel(active_call.call_id())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectedAndAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedAndAccepted
    );

    assert_eq!(context.event_count(ApplicationEvent::RemoteAccepted), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    context
}

#[test]
fn outbound_receive_answer() {
    test_init();

    let _ = start_outbound_call(SignalingType::Legacy);
    let _ = start_outbound_call(SignalingType::BackwardsCompatible);
    let _ = start_outbound_call(SignalingType::LegacyFree);
}

#[test]
fn outbound_call_connected() {
    test_init();

    let _ = connect_outbound_call();
}

#[test]
fn outbound_local_hang_up() {
    test_init();

    let context = start_outbound_call(SignalingType::Legacy);
    let mut cm = context.cm();
    let active_call = context.active_call();

    info!("test: local hangup");
    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.event_count(ApplicationEvent::EndedLocalHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 1);

    // TODO - verify that the data_channel sent a hangup message
}

#[test]
fn outbound_ice_failed() {
    test_init();

    let context = start_outbound_call(SignalingType::Legacy);
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice failed");
    active_connection.inject_ice_failed().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::Terminated
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedConnectionFailure),
        1
    );
}

#[test]
fn outbound_ice_disconnected_before_call_accepted() {
    test_init();

    let context = start_outbound_call(SignalingType::Legacy);
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
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedWithDataChannelBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: injecting ice disconnected");
    active_connection
        .inject_ice_disconnected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE disconnects before the call is connected, the connection
    // should return to the ConnectingBeforeAccepted state.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
}

#[test]
fn outbound_call_accepted_with_stale_call_id() {
    test_init();

    let context = start_outbound_call(SignalingType::Legacy);
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
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedWithDataChannelBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: injecting stale accepted message");
    let call_id = u64::from(active_call.call_id());
    active_connection
        .inject_received_accepted_via_data_channel(CallId::new(call_id + 1))
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // verify bogus connect is simply dropped, with no state change.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedWithDataChannelBeforeAccepted
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
    active_connection.inject_ice_failed().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::Terminated
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
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

    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
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
        .inject_ice_disconnected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE disconnects after the call is connected, the system
    // should move to the ReconnectingAfterAccepted state.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ReconnectingAfterAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ReconnectingAfterAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: injecting ice connected");
    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE reconnects after the call is accepted, the system
    // should move to the ConnectedAndAccepted state.
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedAndAccepted
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
        .inject_ice_disconnected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // When ICE disconnects after the call is connected, the system
    // should move to the ReconnectingAfterAccepted state.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ReconnectingAfterAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ReconnectingAfterAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    // Hang up before reconnect happens
    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
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
fn inject_send_sender_status_via_data_channel() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_send_sender_status_via_data_channel(false)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    // TODO -- verify that the data channel object sent a message
}

#[test]
fn set_bandwidth_mode_normal() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection
        .set_bandwidth_mode(BandwidthMode::Normal)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    // TODO -- verify that the bitrate was changed
    // TODO -- verify that the data channel object sent a message
}

#[test]
fn set_bandwidth_mode_low() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection
        .set_bandwidth_mode(BandwidthMode::Low)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    // TODO -- verify that the bitrate was changed
    // TODO -- verify that the data channel object sent a message
}

#[test]
fn inject_local_ice_candidate() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    let ice_candidate = random_ice_candidate(SignalingType::Legacy);
    let force_send = true;
    active_connection
        .inject_local_ice_candidate(ice_candidate, force_send)
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
    let call_id = active_call.call_id();
    cm.received_ice(
        call_id,
        random_received_ice_candidate(SignalingType::Legacy),
    )
    .expect("receive_ice");
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);

    // TODO -- verify the ice candidate was buffered
    // TODO -- verify the ice candidate was applied to the peer_connection
}

#[test]
fn received_remote_hangup_before_connection() {
    test_init();

    let context = start_outbound_and_proceed();
    let mut cm = context.cm();
    let active_call = context.active_call();

    // Receiving a Hangup/Normal before connection implies the callee is declining.
    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 0);
    // Other callees should get Hangup/Declined.
    assert_eq!(context.declined_hangups_sent(), 1);
}

#[test]
fn received_remote_hangup_before_connection_with_message_in_flight() {
    test_init();

    let context = start_outbound_and_proceed();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let parent_connection = active_call.get_parent_connection();

    // Simulate sending of an ICE candidate message, and leaving it 'in-flight' so
    // the subsequent Hangup message is queued until message_sent() is called.
    context.no_auto_message_sent_for_ice(true);

    let ice_candidate = random_ice_candidate(SignalingType::Legacy);
    let force_send = true;
    parent_connection
        .unwrap()
        .inject_local_ice_candidate(ice_candidate, force_send)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);

    // Receiving a Normal hangup before connection implies the callee is declining.
    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 1);

    // Now free the message queue so that the next message can fly.
    cm.message_sent(active_call.call_id()).expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);

    // We expect that a Hangup/Declined still goes out via Signal messaging.
    assert_eq!(context.declined_hangups_sent(), 1);
}

#[test]
fn received_remote_hangup_before_connection_for_permission() {
    test_init();

    let context = start_outbound_and_proceed();
    let mut cm = context.cm();
    let active_call = context.active_call();

    // Receiving a Hangup/NeedPermission before connection implies the callee is indicating
    // that they need to obtain permission to handle the message.
    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::NeedPermission(Some(1 as DeviceId)),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupNeedPermission),
        1
    );
    // Other callees should get Hangup/Normal.
    assert_eq!(context.need_permission_hangups_sent(), 1);
    assert_eq!(context.declined_hangups_sent(), 0);
}

#[test]
fn received_remote_hangup_before_connection_for_permission_with_message_in_flight() {
    test_init();

    let context = start_outbound_and_proceed();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let parent_connection = active_call.get_parent_connection();

    // Simulate sending of an ICE candidate message, and leaving it 'in-flight' so
    // the subsequent Hangup message is queued until message_sent() is called.
    context.no_auto_message_sent_for_ice(true);

    let ice_candidate = random_ice_candidate(SignalingType::Legacy);
    let force_send = true;
    parent_connection
        .unwrap()
        .inject_local_ice_candidate(ice_candidate, force_send)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);

    // Receiving a Hangup/NeedPermission before connection implies the callee is indicating
    // that they need to obtain permission to handle the message.
    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::NeedPermission(Some(1 as DeviceId)),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupNeedPermission),
        1
    );

    // Now free the message queue so that the next message can fly.
    cm.message_sent(active_call.call_id()).expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);

    // Other callees should get Hangup/Normal.
    assert_eq!(context.need_permission_hangups_sent(), 1);
    assert_eq!(context.declined_hangups_sent(), 0);
}

#[test]
fn received_remote_hangup_after_connection() {
    test_init();

    let context = connect_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 0);
}

#[test]
fn received_remote_needs_permission() {
    test_init();

    let context = start_outbound_call(SignalingType::Legacy);
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::NeedPermission(None),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupNeedPermission),
        1
    );
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
    for i in 0..20 {
        let enable = PRNG.gen::<bool>();

        active_connection
            .inject_received_sender_status_via_data_channel(active_call.call_id(), enable, Some(i))
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

    // Ignore old ones
    active_connection
        .inject_received_sender_status_via_data_channel(active_call.call_id(), true, Some(1))
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    active_connection
        .inject_received_sender_status_via_data_channel(active_call.call_id(), false, Some(2))
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(
        context.event_count(ApplicationEvent::RemoteVideoEnable),
        enable_count
    );
    assert_eq!(
        context.event_count(ApplicationEvent::RemoteVideoDisable),
        disable_count
    );
}

#[test]
fn call_timeout_before_connect() {
    test_init();

    let context = start_outbound_call(SignalingType::Legacy);
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
    cm.call(remote_peer, CallMediaType::Audio, 1 as DeviceId)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::WaitingToProceed
    );

    // cause the sending of the offer to fail.
    context.force_internal_fault(true);

    let active_call = context.active_call();
    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );

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
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
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

    let ice_candidate = random_ice_candidate(SignalingType::Legacy);
    let force_send = true;
    active_connection
        .inject_local_ice_candidate(ice_candidate, force_send)
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

fn outbound_multiple_remote_devices(signaling_type: SignalingType) {
    test_init();

    // With 5, we hit "too many files open" on Linux.
    let n_remotes: u16 = 3;
    let context = start_outbound_n_remote_call(signaling_type, n_remotes);
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
            ConnectionState::ConnectedBeforeAccepted
        );
        assert_eq!(
            active_call.state().expect(error_line!()),
            CallState::ConnectedWithDataChannelBeforeAccepted
        );
        assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
        assert_eq!(context.error_count(), 0);
        assert_eq!(context.ended_count(), 0);

        info!("test:{}: add media stream", i);
        connection
            .handle_received_incoming_media(MediaStream::new(ptr::null()))
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
        .inject_received_accepted_via_data_channel(active_call.call_id())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectedAndAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedAndAccepted
    );
    assert_eq!(
        true,
        active_connection
            .app_connection()
            .unwrap()
            .outgoing_audio_enabled(),
    );

    assert_eq!(context.event_count(ApplicationEvent::RemoteAccepted), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert_eq!(context.accepted_hangups_sent(), 1);

    cm.hangup().expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
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

    let mut thread_vec = Vec::new();
    for (i, signaling_type) in [
        SignalingType::Legacy,
        SignalingType::BackwardsCompatible,
        SignalingType::LegacyFree,
    ]
    .iter()
    .enumerate()
    {
        info!("test:{}: creating call manager", i);

        let child = thread::spawn(move || {
            outbound_multiple_remote_devices(*signaling_type);
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
fn glare_before_connect() {
    test_init();

    let context = start_outbound_call(SignalingType::Legacy);
    let mut cm = context.cm();

    // Create incoming call with same remote

    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(SignalingType::Legacy, Duration::from_secs(0)),
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

// Two users call each other at the same time
#[test]
fn glare_after_connect() {
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

    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(SignalingType::Legacy, Duration::from_secs(0)),
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
    cm.call(remote_peer, CallMediaType::Audio, 1 as DeviceId)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let call_id = {
        let active_call = context.active_call();
        assert_eq!(
            active_call.state().expect(error_line!()),
            CallState::WaitingToProceed
        );
        active_call.call_id()
    };

    cm.proceed(call_id, format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // Receive a busy message
    cm.received_busy(
        call_id,
        signaling::ReceivedBusy {
            sender_device_id: 1 as DeviceId,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteBusy), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.call_concluded_count(), 1);
}
