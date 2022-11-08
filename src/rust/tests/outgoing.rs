//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Tests for outgoing calls

extern crate ringrtc;

#[macro_use]
extern crate log;

use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

use prost::Message;
use ringrtc::common::{
    units::DataRate, ApplicationEvent, CallId, CallMediaType, CallState, ConnectionState, DeviceId,
};
use ringrtc::core::bandwidth_mode::BandwidthMode;
use ringrtc::core::{group_call, signaling};
use ringrtc::protobuf;
use ringrtc::sim::error::SimError;
use ringrtc::webrtc;
use ringrtc::webrtc::media::MediaStream;
use ringrtc::webrtc::peer_connection_observer::{
    NetworkAdapterType, NetworkRoute, TransportProtocol,
};

#[macro_use]
mod common;
use common::{
    random_ice_candidate, random_received_answer, random_received_ice_candidate,
    random_received_offer, test_init, TestContext,
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

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    cm.call(remote_peer, CallMediaType::Audio, 1)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::WaitingToProceed
    );

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
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
    assert!(cm.busy());

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
fn start_outbound_n_remote_call(n_remotes: u16) -> TestContext {
    let context = TestContext::new();
    let mut cm = context.cm();

    // don't go nuts
    assert!(n_remotes < 20);

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    cm.call(remote_peer, CallMediaType::Audio, 1)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::WaitingToProceed
    );

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // add a received answer for each remote
    for i in 1..(n_remotes + 1) {
        let call_id = active_call.call_id();
        cm.received_answer(
            call_id,
            random_received_answer(&context.prng, i as DeviceId),
        )
        .expect(error_line!());

        cm.received_ice(call_id, random_received_ice_candidate(&context.prng))
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
    assert!(cm.busy());

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
// Now in the ConnectingBeforeAccepted state.
fn start_outbound_call() -> TestContext {
    start_outbound_n_remote_call(1)
}

// Create an outbound call session up to the Connected state.
//
// - create an offer
// - send offer
// - receive answer
// - ice connected
//
// Now in the ConnectedBeforeAccepted state.
fn connected_outbound_call() -> TestContext {
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
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    assert!(cm.busy());

    context
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
// Now in the ConnectedAndAccepted state.
fn connected_and_accepted_outbound_call() -> TestContext {
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
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: inject incoming stream");
    active_connection
        .inject_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    info!("test: injecting accepted");
    active_connection
        .inject_received_accepted_via_rtp_data(active_call.call_id())
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
    assert!(cm.busy());

    context
}

// Create an outbound call session up to the Accepted state,
// but with the remote accept happening before ICE connected.
//
// - create an offer
// - send offer
// - receive answer
// - media stream added
// - call accepted
// - ice connected
//
// Now in the ConnectedAndAccepted state.
#[test]
fn accepted_and_connected_outbound_call_one_callee() {
    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 0);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: inject incoming stream");
    active_connection
        .inject_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    info!("test: injecting accepted");
    active_connection
        .inject_received_accepted_via_rtp_data(active_call.call_id())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectingAfterAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingAfterAccepted
    );

    info!("test: injecting ice connected");
    active_connection
        .inject_ice_connected()
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

    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    assert_eq!(context.event_count(ApplicationEvent::RemoteAccepted), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

#[test]
fn accepted_and_connected_outbound_call_two_callees() {
    let context = start_outbound_n_remote_call(2);
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = active_call.get_connection(1).unwrap();
    let mut inactive_connection = active_call.get_connection(2).unwrap();

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );
    assert_eq!(
        inactive_connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 0);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: inject incoming stream");
    active_connection
        .inject_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
        .expect(error_line!());
    inactive_connection
        .inject_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    info!("test: injecting ice connected for inactive connection");
    inactive_connection
        .inject_ice_connected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );
    assert_eq!(
        inactive_connection.state().expect(error_line!()),
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedBeforeAccepted
    );

    cm.synchronize().expect(error_line!());

    info!("test: injecting accepted for active connection");
    active_connection
        .inject_received_accepted_via_rtp_data(active_call.call_id())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectingAfterAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingAfterAccepted
    );

    info!("test: injecting ice connected for active connection");
    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectedAndAccepted
    );
    assert_eq!(
        inactive_connection.state().expect(error_line!()),
        ConnectionState::Terminated
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedAndAccepted
    );

    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    assert_eq!(context.event_count(ApplicationEvent::RemoteAccepted), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

#[test]
fn outbound_receive_answer() {
    test_init();

    let _ = start_outbound_call();
}

#[test]
fn outbound_call_connected() {
    test_init();

    let _ = connected_and_accepted_outbound_call();
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

    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.event_count(ApplicationEvent::EndedLocalHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
    assert!(!cm.busy());

    // TODO - verify that a hangup message was sent via RTP data
}

#[test]
fn outbound_ice_failed() {
    test_init();

    let context = start_outbound_call();
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
    assert!(!cm.busy());
}

#[test]
fn outbound_ice_disconnected_before_call_accepted() {
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
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedBeforeAccepted
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
    assert!(cm.busy());
}

#[test]
fn outbound_call_accepted_with_stale_call_id() {
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
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    info!("test: injecting stale accepted message");
    let call_id = u64::from(active_call.call_id());
    active_connection
        .inject_received_accepted_via_rtp_data(CallId::new(call_id + 1))
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // verify bogus connect is simply dropped, with no state change.
    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectedBeforeAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedBeforeAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

#[test]
fn outbound_call_connected_ice_failed() {
    test_init();

    let context = connected_and_accepted_outbound_call();
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
    assert!(!cm.busy());
}

#[test]
fn outbound_call_connected_local_hangup() {
    test_init();

    let context = connected_and_accepted_outbound_call();
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
    assert!(!cm.busy());

    // TODO - verify that a hangup message was sent via RTP data
}

#[test]
fn outbound_ice_disconnected_after_call_connected_and_reconnect() {
    test_init();

    let context = connected_and_accepted_outbound_call();
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
    assert!(cm.busy());
}

#[test]
fn outbound_ice_disconnected_after_call_connected_and_local_hangup() {
    test_init();

    let context = connected_and_accepted_outbound_call();
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
    assert!(!cm.busy());
}

#[test]
fn inject_connection_error() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection.inject_internal_error(
        SimError::TestError("fake_error".to_string()).into(),
        "testing connection error injection",
    );

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 1);
    assert_eq!(context.ended_count(), 1);
    assert!(!cm.busy());
}

#[test]
fn inject_call_error() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_call = context.active_call();

    active_call.inject_internal_error(
        SimError::TestError("fake_error".to_string()).into(),
        "testing call error injection",
    );

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 1);
    assert_eq!(context.ended_count(), 1);
    assert!(!cm.busy());
}

#[test]
fn update_sender_status() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    assert_eq!(None, active_connection.last_sent_sender_status());

    active_connection
        .update_sender_status(signaling::SenderStatus {
            video_enabled: Some(false),
            sharing_screen: None,
        })
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(protobuf::rtp_data::SenderStatus {
            id: Some(active_connection.call_id().into()),
            video_enabled: Some(false),
            sharing_screen: None,
        }),
        active_connection.last_sent_sender_status()
    );

    active_connection
        .update_sender_status(signaling::SenderStatus {
            video_enabled: Some(true),
            sharing_screen: None,
        })
        .expect(error_line!());

    active_connection
        .update_sender_status(signaling::SenderStatus {
            video_enabled: None,
            sharing_screen: Some(true),
        })
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(protobuf::rtp_data::SenderStatus {
            id: Some(active_connection.call_id().into()),
            video_enabled: Some(true),
            sharing_screen: Some(true),
        }),
        active_connection.last_sent_sender_status()
    );

    active_connection
        .update_sender_status(signaling::SenderStatus {
            video_enabled: None,
            sharing_screen: Some(false),
        })
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(protobuf::rtp_data::SenderStatus {
            id: Some(active_connection.call_id().into()),
            video_enabled: Some(true),
            sharing_screen: Some(false),
        }),
        active_connection.last_sent_sender_status()
    );
}

#[test]
fn update_bandwidth_mode_default() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_connection = context.active_connection();

    assert_eq!(
        Some(2_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    active_connection
        .update_bandwidth_mode(BandwidthMode::Normal)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(2_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    // It's not sent because that's what it starts as.
    assert_eq!(
        None,
        active_connection
            .app_connection()
            .unwrap()
            .last_sent_max_bitrate_bps()
    )
}

#[test]
fn update_bandwidth_mode_low() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_connection = context.active_connection();

    active_connection
        .update_bandwidth_mode(BandwidthMode::Low)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(300_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    assert_eq!(
        Some(300_000),
        active_connection
            .app_connection()
            .unwrap()
            .last_sent_max_bitrate_bps()
    )
}

fn update_bandwidth_when_relayed(local: bool) {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_ice_network_route_changed(NetworkRoute {
            local_adapter_type: NetworkAdapterType::Unknown,
            local_adapter_type_under_vpn: NetworkAdapterType::Unknown,
            local_relayed: local,
            local_relay_protocol: TransportProtocol::Unknown,
            remote_relayed: !local,
        })
        .unwrap();
    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(1_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    assert_eq!(
        None,
        active_connection
            .app_connection()
            .unwrap()
            .last_sent_max_bitrate_bps()
    );

    active_connection
        .update_bandwidth_mode(BandwidthMode::Low)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(300_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    assert_eq!(
        Some(300_000),
        active_connection
            .app_connection()
            .unwrap()
            .last_sent_max_bitrate_bps()
    );

    active_connection
        .update_bandwidth_mode(BandwidthMode::Normal)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(1_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    // Even though we limit what we *send* when using TURN, we don't
    // limit what we *request to be sent to us*.
    assert_eq!(
        Some(2_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .last_sent_max_bitrate_bps()
    );

    active_connection
        .inject_ice_network_route_changed(NetworkRoute {
            local_adapter_type: NetworkAdapterType::Unknown,
            local_adapter_type_under_vpn: NetworkAdapterType::Unknown,
            local_relayed: false,
            local_relay_protocol: TransportProtocol::Unknown,
            remote_relayed: false,
        })
        .unwrap();
    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);

    assert_eq!(
        Some(2_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    assert_eq!(
        Some(2_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .last_sent_max_bitrate_bps()
    );
}

#[test]
fn update_bandwidth_when_relayed_local() {
    update_bandwidth_when_relayed(true);
}

#[test]
fn update_bandwidth_when_relayed_remote() {
    update_bandwidth_when_relayed(false);
}

#[test]
fn inject_local_ice_candidate() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    let ice_candidate = random_ice_candidate(&context.prng);
    let force_send = true;
    active_connection
        .inject_local_ice_candidate(ice_candidate, force_send, "", None)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);
}

#[test]
fn receive_remote_ice_candidate() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    // add a received ICE candidate
    let call_id = active_call.call_id();
    cm.received_ice(call_id, random_received_ice_candidate(&context.prng))
        .expect("receive_ice");
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);

    // TODO -- verify the ice candidate was buffered
    // TODO -- verify the ice candidate was applied to the peer_connection
}

#[test]
fn ice_candidate_removal() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    let removed_addresses: Vec<SocketAddr> =
        vec!["1.2.3.4:5".parse().unwrap(), "6.7.8.9:0".parse().unwrap()];
    let force_send = true;
    active_connection
        .inject_local_ice_candidates_removed(removed_addresses.clone(), force_send)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 2);
    let last_sent = context
        .last_ice_sent()
        .expect("ICE candidate removal was sent");

    let active_call = context.active_call();
    let call_id = active_call.call_id();
    cm.received_ice(
        call_id,
        signaling::ReceivedIce {
            ice: last_sent.ice,
            sender_device_id: 1,
        },
    )
    .expect("receive_ice");
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);

    assert_eq!(
        removed_addresses,
        active_connection
            .app_connection()
            .unwrap()
            .removed_ice_candidates()
    );
}

#[test]
fn ice_send_failures_cause_error_before_connection() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();
    let active_call = context.active_call();

    // The active call should be in the connecting before accepted state.
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );

    // Simulate sending of an ICE candidate message and getting it 'in-flight'.
    context.no_auto_message_sent_for_ice(true);

    let ice_candidate = random_ice_candidate(&context.prng);
    let force_send = true;
    active_connection
        .inject_local_ice_candidate(ice_candidate, force_send, "", None)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);

    // Now indicate that the signaling mechanism failed to send the message.
    cm.message_send_failure(active_call.call_id())
        .expect(error_line!());

    // There should be a signaling failure.
    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedSignalingFailure),
        1
    );

    // The active call should be terminated.
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
    assert!(!cm.busy());
}

#[test]
fn ignore_ice_send_failures_after_connection() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();
    let active_call = context.active_call();

    // The active call should be in the connected and accepted state.
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedAndAccepted
    );

    // Simulate sending of an ICE candidate message and getting it 'in-flight'.
    context.no_auto_message_sent_for_ice(true);

    let ice_candidate = random_ice_candidate(&context.prng);
    let force_send = true;
    active_connection
        .inject_local_ice_candidate(ice_candidate, force_send, "", None)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);

    // Now indicate that the signaling mechanism failed to send the message.
    cm.message_send_failure(active_call.call_id())
        .expect(error_line!());

    // There should not be any failures due to ICE messages not being sent
    // after we are connected.
    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedSignalingFailure),
        0
    );

    // Check that there is still an active call.
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedAndAccepted
    );
    assert!(cm.busy());
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
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 0);
    // Other callees should get Hangup/Declined.
    assert_eq!(context.declined_hangups_sent(), 1);
    assert!(!cm.busy());
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

    let ice_candidate = random_ice_candidate(&context.prng);
    let force_send = true;
    parent_connection
        .unwrap()
        .inject_local_ice_candidate(ice_candidate, force_send, "", None)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);

    // Receiving a Normal hangup before connection implies the callee is declining.
    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
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
    assert!(!cm.busy());
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
            sender_device_id: 1,
            hangup: signaling::Hangup::NeedPermission(Some(1)),
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
    assert!(!cm.busy());
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

    let ice_candidate = random_ice_candidate(&context.prng);
    let force_send = true;
    parent_connection
        .unwrap()
        .inject_local_ice_candidate(ice_candidate, force_send, "", None)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);

    // Receiving a Hangup/NeedPermission before connection implies the callee is indicating
    // that they need to obtain permission to handle the message.
    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::NeedPermission(Some(1)),
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
    assert!(!cm.busy());
}

#[test]
fn received_remote_hangup_after_connection() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteHangup), 1);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert!(!cm.busy());
}

#[test]
fn received_remote_needs_permission() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::NeedPermission(None),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupNeedPermission),
        1
    );
    assert!(!cm.busy());
}

#[test]
fn received_remote_video_status() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    let mut enable_count = 0;
    let mut disable_count = 0;
    let mut old_enable = None;
    for i in 0..20 {
        let enable = context.prng.gen::<bool>();
        match (old_enable, enable) {
            (None | Some(false), true) => {
                enable_count += 1;
            }
            (None | Some(true), false) => {
                disable_count += 1;
            }
            (Some(true), true) | (Some(false), false) => {
                // Nothing changed
            }
        }
        old_enable = Some(enable);

        active_connection
            .inject_received_sender_status_via_rtp_data(
                active_call.call_id(),
                signaling::SenderStatus {
                    video_enabled: Some(enable),
                    sharing_screen: None,
                },
                i,
            )
            .expect(error_line!());
        cm.synchronize().expect(error_line!());

        assert_eq!(context.error_count(), 0);
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteVideoEnable),
            enable_count
        );
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteVideoDisable),
            disable_count
        );
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteSharingScreenEnable),
            0
        );
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteSharingScreenDisable),
            0
        );
    }

    // Ignore old ones
    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(true),
                sharing_screen: None,
            },
            1,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(false),
                sharing_screen: None,
            },
            2,
        )
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
    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenEnable),
        0
    );
    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenDisable),
        0
    );
}

#[test]
fn ignore_duplicate_remote_video_status() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(true),
                sharing_screen: None,
            },
            0,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(true),
                sharing_screen: None,
            },
            1,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(false),
                sharing_screen: None,
            },
            2,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(false),
                sharing_screen: None,
            },
            3,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoEnable), 1);
    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoDisable), 1);
}

#[test]
fn received_remote_sharing_screen_status() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    let mut enable_count = 0;
    let mut disable_count = 0;
    let mut old_enable = None;
    for i in 0..20 {
        let enable = context.prng.gen::<bool>();
        match (old_enable, enable) {
            (None | Some(false), true) => {
                enable_count += 1;
            }
            (None | Some(true), false) => {
                disable_count += 1;
            }
            (Some(true), true) | (Some(false), false) => {
                // Nothing changed
            }
        }
        old_enable = Some(enable);

        active_connection
            .inject_received_sender_status_via_rtp_data(
                active_call.call_id(),
                signaling::SenderStatus {
                    video_enabled: None,
                    sharing_screen: Some(enable),
                },
                i,
            )
            .expect(error_line!());
        cm.synchronize().expect(error_line!());

        assert_eq!(context.error_count(), 0);
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteSharingScreenEnable),
            enable_count
        );
        assert_eq!(
            context.event_count(ApplicationEvent::RemoteSharingScreenDisable),
            disable_count
        );
        assert_eq!(context.event_count(ApplicationEvent::RemoteVideoEnable), 0);
        assert_eq!(context.event_count(ApplicationEvent::RemoteVideoDisable), 0);
    }

    // Ignore old ones
    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: None,
                sharing_screen: Some(true),
            },
            1,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: None,
                sharing_screen: Some(false),
            },
            2,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenEnable),
        enable_count
    );
    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenDisable),
        disable_count
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoEnable), 0);
    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoDisable), 0);
}

#[test]
fn ignore_duplicate_remote_screen_sharing_status() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: None,
                sharing_screen: Some(true),
            },
            0,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: None,
                sharing_screen: Some(true),
            },
            1,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: None,
                sharing_screen: Some(false),
            },
            2,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: None,
                sharing_screen: Some(false),
            },
            3,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenEnable),
        1
    );
    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenDisable),
        1
    );
}

#[test]
fn received_remote_multiple_status() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    // Ignore old ones
    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(false),
                sharing_screen: Some(true),
            },
            1,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(true),
                sharing_screen: Some(false),
            },
            2,
        )
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenEnable),
        1
    );
    assert_eq!(
        context.event_count(ApplicationEvent::RemoteSharingScreenDisable),
        1
    );
    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoEnable), 1);
    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoDisable), 1);
}

#[test]
fn received_status_before_accepted() {
    let context = start_outbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    active_connection
        .inject_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
        .expect(error_line!());

    active_connection
        .inject_received_sender_status_via_rtp_data(
            active_call.call_id(),
            signaling::SenderStatus {
                video_enabled: Some(true),
                sharing_screen: None,
            },
            1,
        )
        .expect(error_line!());

    active_connection
        .inject_received_receiver_status_via_rtp_data(
            active_call.call_id(),
            DataRate::from_bps(50_000),
            1,
        )
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoEnable), 0);

    assert_eq!(
        Some(2_000_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );

    active_connection
        .inject_received_accepted_via_rtp_data(active_call.call_id())
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.event_count(ApplicationEvent::RemoteVideoEnable), 1);

    assert_eq!(
        Some(50_000),
        active_connection
            .app_connection()
            .unwrap()
            .max_bitrate_bps()
    );
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

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_call = context.active_call();

    active_call.inject_call_timeout().expect(error_line!());

    cm.synchronize().expect(error_line!());

    // The call is already connected, so the timeout is ignored.
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedTimeout), 0);
    assert!(cm.busy());
}

#[test]
fn outbound_proceed_with_error() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    cm.call(remote_peer, CallMediaType::Audio, 1)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
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
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
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
    assert!(!cm.busy());

    context.force_internal_fault(false);
}

#[test]
fn outbound_call_connected_local_hangup_with_error() {
    test_init();

    let context = connected_and_accepted_outbound_call();
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
    assert!(!cm.busy());
}

#[test]
fn local_ice_candidate_with_error() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    // cause the sending of the ICE candidate to fail.
    context.force_internal_fault(true);

    let ice_candidate = random_ice_candidate(&context.prng);
    let force_send = true;
    active_connection
        .inject_local_ice_candidate(ice_candidate, force_send, "", None)
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
    assert!(!cm.busy());
}

fn outbound_multiple_remote_devices() {
    test_init();

    // With 5, we hit "too many files open" on Linux.
    let n_remotes: u16 = 3;
    let context = start_outbound_n_remote_call(n_remotes);
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
            CallState::ConnectedBeforeAccepted
        );
        assert_eq!(context.event_count(ApplicationEvent::RemoteRinging), 1);
        assert_eq!(context.error_count(), 0);
        assert_eq!(context.ended_count(), 0);

        info!("test:{}: add media stream", i);
        connection
            .handle_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
            .expect(error_line!());
    }

    // connect one of the remotes
    let active_remote = (context.prng.gen::<u16>() % n_remotes) + 1;
    let mut active_connection = active_call
        .get_connection(active_remote as DeviceId)
        .expect(error_line!());

    info!(
        "test:active_remote:{}: injecting call connected",
        active_remote
    );
    assert!(!active_connection
        .app_connection()
        .unwrap()
        .outgoing_audio_enabled(),);
    active_connection
        .inject_received_accepted_via_rtp_data(active_call.call_id())
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
    assert!(active_connection
        .app_connection()
        .unwrap()
        .outgoing_audio_enabled(),);

    assert_eq!(context.event_count(ApplicationEvent::RemoteAccepted), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert_eq!(context.accepted_hangups_sent(), 1);
    assert!(cm.busy());

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
    assert!(!cm.busy());
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

    let mut thread_vec = Vec::new();
    for i in 0..n_call_manager {
        info!("test:{}: creating call manager", i);

        let child = thread::spawn(move || {
            outbound_multiple_remote_devices();
        });

        thread_vec.push(child);
    }

    info!("test: joining threads");
    for child in thread_vec {
        info!("test: joining thread...");
        // Make sure no threads panicked
        assert!(child.join().is_ok());
    }
}

// Two users call each other at the same time, offer received before the
// outgoing call gets an ICE connection. The winning side will continue
// with the outgoing call and end the incoming call.
#[test]
fn glare_before_connect_winner() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();

    // Create incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be less than the active call_id.
    assert!(
        context.active_call().call_id().as_u64() > 0,
        "Test case not valid if incoming call-id can't be smaller than the active call-id."
    );

    let call_id = CallId::new(context.active_call().call_id().as_u64() - 1);
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        1
    );
    assert_eq!(context.busys_sent(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 0);
    assert_eq!(context.call_concluded_count(), 1);
}

// Two users call each other at the same time, offer received before the
// outgoing call gets an ICE connection. The losing side will end the
// outgoing call and start handling the incoming call.
#[test]
fn glare_before_connect_loser() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();

    // Create incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be greater than the active call_id.
    assert!(
        context.active_call().call_id().as_u64() < std::u64::MAX,
        "Test case not valid if incoming call-id can't be greater than the active call-id."
    );

    let call_id = CallId::new(context.active_call().call_id().as_u64() + 1);
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        0
    );
    assert_eq!(context.busys_sent(), 0);
    assert_eq!(context.call_concluded_count(), 1);
}

// Two users call each other at the same time, offer received before the
// outgoing call gets an ICE connection. Call-ids are unexpectedly equal.
#[test]
fn glare_before_connect_equal() {
    test_init();

    let context = start_outbound_call();
    let mut cm = context.cm();

    // Create incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be equal to the active call_id.
    let call_id = CallId::new(context.active_call().call_id().as_u64());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        0
    );
    assert_eq!(
        context.event_count(ApplicationEvent::EndedGlareHandlingFailure),
        1
    );
    assert_eq!(context.busys_sent(), 1);
    assert_eq!(context.call_concluded_count(), 2);
}

#[test]
fn glare_before_connect_loser_with_incoming_ice_candidates_before_start() {
    // We don't actually expose a way to automatically test that the ICE candidates are handled.
    // You can check manually by running with RUST_LOG=ringrtc::core::connection/ice_candidates
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let incoming_call_id = CallId::new(u64::MAX);
    cm.received_ice(
        incoming_call_id,
        random_received_ice_candidate(&context.prng),
    )
    .expect(error_line!());

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    cm.call(remote_peer, CallMediaType::Audio, 1)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 1);
    assert_eq!(context.start_incoming_count(), 0);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::WaitingToProceed
    );

    let outgoing_call_id = active_call.call_id();
    cm.proceed(
        outgoing_call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    cm.received_answer(
        outgoing_call_id,
        random_received_answer(&context.prng, 1 as DeviceId),
    )
    .expect(error_line!());
    cm.received_ice(
        outgoing_call_id,
        random_received_ice_candidate(&context.prng),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.offers_sent(), 1,);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());

    // Create incoming call with same remote
    let remote_peer = {
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be greater than the active call_id.
    assert!(
        context.active_call().call_id().as_u64() < std::u64::MAX,
        "Test case not valid if incoming call-id can't be greater than the active call-id."
    );

    // Make sure we don't interfere with the lifetime of the call after this point.
    drop(active_call);

    cm.received_offer(
        remote_peer,
        incoming_call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        0
    );
    assert_eq!(context.busys_sent(), 0);
    assert_eq!(context.call_concluded_count(), 1);

    cm.proceed(
        incoming_call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.answers_sent(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

#[test]
fn glare_before_connect_loser_with_incoming_ice_candidates_after_start() {
    // We don't actually expose a way to automatically test that the ICE candidates are handled.
    // You can check manually by running with RUST_LOG=ringrtc::core::connection/ice_candidates
    let context = start_outbound_call();
    let mut cm = context.cm();

    let incoming_call_id = CallId::new(u64::MAX);
    cm.received_ice(
        incoming_call_id,
        random_received_ice_candidate(&context.prng),
    )
    .expect(error_line!());

    // Create incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be greater than the active call_id.
    assert!(
        context.active_call().call_id().as_u64() < std::u64::MAX,
        "Test case not valid if incoming call-id can't be greater than the active call-id."
    );

    cm.received_offer(
        remote_peer,
        incoming_call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        0
    );
    assert_eq!(context.busys_sent(), 0);
    assert_eq!(context.call_concluded_count(), 1);

    cm.proceed(
        incoming_call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.answers_sent(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

// Two users call each other at the same time, offer received after the
// outgoing call gets an ICE connection. The winning side will continue
// with the outgoing call and end the incoming call.
#[test]
fn glare_after_connect_winner() {
    test_init();

    let context = connected_outbound_call();
    let mut cm = context.cm();

    // Create incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be less than the active call_id.
    assert!(
        context.active_call().call_id().as_u64() > 0,
        "Test case not valid if incoming call-id can't be smaller than the active call-id."
    );

    let call_id = CallId::new(context.active_call().call_id().as_u64() - 1);
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        1
    );
    assert_eq!(context.busys_sent(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 0);
    assert_eq!(context.call_concluded_count(), 1);
}

// Two users call each other at the same time, offer received after the
// outgoing call gets an ICE connection. The losing side will end the
// outgoing call and start handling the incoming call.
#[test]
fn glare_after_connect_loser() {
    test_init();

    let context = connected_outbound_call();
    let mut cm = context.cm();

    // Create incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be greater than the active call_id.
    assert!(
        context.active_call().call_id().as_u64() < std::u64::MAX,
        "Test case not valid if incoming call-id can't be greater than the active call-id."
    );

    let call_id = CallId::new(context.active_call().call_id().as_u64() + 1);
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        0
    );
    assert_eq!(context.busys_sent(), 0);
    assert_eq!(context.call_concluded_count(), 1);
}

// Two users call each other at the same time, offer received before the
// outgoing call gets an ICE connection. Call-ids are unexpectedly equal.
#[test]
fn glare_after_connect_equal() {
    test_init();

    let context = connected_outbound_call();
    let mut cm = context.cm();

    // Create incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    // The incoming offer's call_id will be equal to the active call_id.
    let call_id = CallId::new(context.active_call().call_id().as_u64());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteGlare), 1);
    assert_eq!(context.normal_hangups_sent(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWithGlare),
        0
    );
    assert_eq!(
        context.event_count(ApplicationEvent::EndedGlareHandlingFailure),
        1
    );
    assert_eq!(context.busys_sent(), 1);
    assert_eq!(context.call_concluded_count(), 2);
}

// Two users are in an accepted call. The remote user's leg is ended and they
// call the local user who is still in the original call. The local user should
// quietly end the active call and start handling the new incoming one.
#[test]
fn recall_when_connected() {
    test_init();

    let context = connected_and_accepted_outbound_call();
    let mut cm = context.cm();

    // Verify that no incoming call was started yet.
    assert_eq!(context.start_incoming_count(), 0);

    // Create a new incoming call with same remote
    let remote_peer = {
        let active_call = context.active_call();
        let remote_peer = active_call.remote_peer().expect(error_line!());
        remote_peer.to_owned()
    };
    info!("active remote_peer: {}", remote_peer);

    let call_id = CallId::new(context.prng.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteReCall), 1);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.busys_sent(), 0);

    // Previous call should be concluded.
    assert_eq!(context.call_concluded_count(), 1);

    // The newly incoming call should have been started (not yet proceeded).
    assert_eq!(context.start_incoming_count(), 1);
}

// Receive a busy message when trying to establish outbound call
#[test]
fn start_outbound_receive_busy() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    cm.call(remote_peer, CallMediaType::Audio, 1)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
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

    cm.proceed(
        call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    // Receive a busy message
    cm.received_busy(
        call_id,
        signaling::ReceivedBusy {
            sender_device_id: 1,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.event_count(ApplicationEvent::EndedRemoteBusy), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.call_concluded_count(), 1);
    assert!(!cm.busy());
}

#[test]
fn cancel_group_ring() {
    use group_call::{RingCancelReason, RingId, SignalingMessageUrgency};

    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let group_id = [1, 2, 3];
    let ring_id = RingId::from(42);
    cm.cancel_group_ring(
        group_id.to_vec(),
        ring_id,
        Some(RingCancelReason::DeclinedByUser),
    )
    .expect(error_line!());
    cm.synchronize().expect(error_line!());
    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();

    // Oops, we forgot to set the current user's UUID.
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );

    let self_uuid = [1, 1, 1];
    cm.set_self_uuid(self_uuid.to_vec()).expect(error_line!());

    // Okay, try again.
    cm.cancel_group_ring(
        group_id.to_vec(),
        ring_id,
        Some(RingCancelReason::DeclinedByUser),
    )
    .expect(error_line!());
    cm.synchronize().expect(error_line!());
    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();

    match &messages[..] {
        [message] => {
            assert_eq!(&self_uuid[..], &message.recipient[..]);
            assert_eq!(SignalingMessageUrgency::HandleImmediately, message.urgency);
            let call_message = protobuf::signaling::CallMessage::decode(&message.message[..])
                .expect(error_line!());
            assert_eq!(
                protobuf::signaling::CallMessage {
                    ring_response: Some(protobuf::signaling::call_message::RingResponse {
                        group_id: Some(group_id.to_vec()),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Declined.into()
                        ),
                    }),
                    ..Default::default()
                },
                call_message
            );
        }
        _ => panic!("unexpected messages: {:?}", messages),
    }

    // If we cancel without a reason, though, nothing should get sent.
    cm.cancel_group_ring(group_id.to_vec(), ring_id, None)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );
}

#[test]
fn group_call_ring_accepted() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid.clone()).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let group_call_id = context
        .create_group_call(group_id.clone())
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );

    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    match &messages[..] {
        [message] => {
            assert_eq!(&self_uuid[..], &message.recipient[..]);
            assert_eq!(
                group_call::SignalingMessageUrgency::HandleImmediately,
                message.urgency
            );
            let call_message = protobuf::signaling::CallMessage::decode(&message.message[..])
                .expect(error_line!());
            assert_eq!(
                protobuf::signaling::CallMessage {
                    ring_response: Some(protobuf::signaling::call_message::RingResponse {
                        group_id: Some(group_id.to_vec()),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Accepted.into()
                        ),
                    }),
                    ..Default::default()
                },
                call_message
            );
        }
        _ => panic!("unexpected messages: {:?}", messages),
    }
}

#[test]
fn group_call_ring_accepted_with_existing_call() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid.clone()).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let group_call_id = context
        .create_group_call(group_id.clone())
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );

    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    match &messages[..] {
        [message] => {
            assert_eq!(&self_uuid[..], &message.recipient[..]);
            assert_eq!(
                group_call::SignalingMessageUrgency::HandleImmediately,
                message.urgency
            );
            let call_message = protobuf::signaling::CallMessage::decode(&message.message[..])
                .expect(error_line!());
            assert_eq!(
                protobuf::signaling::CallMessage {
                    ring_response: Some(protobuf::signaling::call_message::RingResponse {
                        group_id: Some(group_id.to_vec()),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Accepted.into()
                        ),
                    }),
                    ..Default::default()
                },
                call_message
            );
        }
        _ => panic!("unexpected messages: {:?}", messages),
    }
}

#[test]
fn group_call_ring_too_old() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    cm.age_all_outstanding_group_rings(Duration::from_secs(600));

    let group_call_id = context.create_group_call(group_id).expect(error_line!());
    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );
}

#[test]
fn group_call_ring_message_age_does_not_affect_ring_expiration() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid.clone()).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    // 45 seconds means the ring isn't expired yet...
    cm.received_call_message(sender, 1, 2, buf, Duration::from_secs(45))
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    // ...and adding another 45 won't make it expire, since the ages don't stack.
    cm.age_all_outstanding_group_rings(Duration::from_secs(45));

    let group_call_id = context
        .create_group_call(group_id.clone())
        .expect(error_line!());
    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    match &messages[..] {
        [message] => {
            assert_eq!(&self_uuid[..], &message.recipient[..]);
            assert_eq!(
                group_call::SignalingMessageUrgency::HandleImmediately,
                message.urgency
            );
            let call_message = protobuf::signaling::CallMessage::decode(&message.message[..])
                .expect(error_line!());
            assert_eq!(
                protobuf::signaling::CallMessage {
                    ring_response: Some(protobuf::signaling::call_message::RingResponse {
                        group_id: Some(group_id.to_vec()),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Accepted.into()
                        ),
                    }),
                    ..Default::default()
                },
                call_message
            );
        }
        _ => panic!("unexpected messages: {:?}", messages),
    }
}

#[test]
fn group_call_ring_last_ring_wins() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid.clone()).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let first_ring_id = group_call::RingId::from(42);
    let second_ring_id = group_call::RingId::from(525_600);

    let first_message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(first_ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    first_message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender.clone(), 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let second_message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(second_ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    second_message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let group_call_id = context
        .create_group_call(group_id.clone())
        .expect(error_line!());
    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    match &messages[..] {
        [message] => {
            assert_eq!(&self_uuid[..], &message.recipient[..]);
            assert_eq!(
                group_call::SignalingMessageUrgency::HandleImmediately,
                message.urgency
            );
            let call_message = protobuf::signaling::CallMessage::decode(&message.message[..])
                .expect(error_line!());
            assert_eq!(
                protobuf::signaling::CallMessage {
                    ring_response: Some(protobuf::signaling::call_message::RingResponse {
                        group_id: Some(group_id.to_vec()),
                        ring_id: Some(second_ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Accepted.into()
                        ),
                    }),
                    ..Default::default()
                },
                call_message
            );
        }
        _ => panic!("unexpected messages: {:?}", messages),
    }
}

#[test]
fn group_call_ring_cancelled_locally_before_join() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());
    cm.cancel_group_ring(group_id.clone(), ring_id, None)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let group_call_id = context.create_group_call(group_id).expect(error_line!());
    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );
}

#[test]
fn group_call_ring_cancelled_by_ringer_before_join() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender.clone(), 1, 2, buf, Duration::ZERO)
        .expect(error_line!());

    let cancel_message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Cancelled.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    cancel_message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());

    cm.synchronize().expect(error_line!());

    let group_call_id = context.create_group_call(group_id).expect(error_line!());
    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );
}

#[test]
fn group_call_ring_cancelled_by_another_device_before_join() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid.clone()).expect(error_line!());

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let message = protobuf::signaling::CallMessage {
        ring_intention: Some(protobuf::signaling::call_message::RingIntention {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_intention::Type::Ring.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());

    let cancel_message = protobuf::signaling::CallMessage {
        ring_response: Some(protobuf::signaling::call_message::RingResponse {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_response::Type::Declined.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    cancel_message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(self_uuid, 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let group_call_id = context.create_group_call(group_id).expect(error_line!());
    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());

    let messages = cm
        .platform()
        .expect(error_line!())
        .take_outgoing_call_messages();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::OutgoingCallMessage],
        &messages[..]
    );
}
