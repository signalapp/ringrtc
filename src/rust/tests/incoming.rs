//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Test the FSMs using the Simulation platform

extern crate ringrtc;

#[macro_use]
extern crate log;

use std::time::Duration;

use prost::Message;
use ringrtc::common::{units::DataRate, ApplicationEvent, CallId, CallState, ConnectionState};
use ringrtc::core::bandwidth_mode::BandwidthMode;
use ringrtc::core::call_manager::MAX_MESSAGE_AGE;
use ringrtc::core::group_call;
use ringrtc::core::signaling;
use ringrtc::protobuf;
use ringrtc::webrtc;
use ringrtc::webrtc::media::MediaStream;
use ringrtc::webrtc::peer_connection_observer::{
    NetworkAdapterType, NetworkRoute, TransportProtocol,
};

#[macro_use]
mod common;
use common::{random_received_ice_candidate, random_received_offer, test_init, TestContext};

// Create an inbound call session up to the ConnectingBeforeAccepted state.
//
// - create call manager
// - receive offer
// - check start incoming event happened
// - check active call exists
// - call proceed()
// - add received ice candidate
// - check underlying Connection is in ConnectingBeforeAccepted state
// - check call is in Connecting state
// - check answer sent
//
// Now in the ConnectingBeforeAccepted state.
fn start_inbound_call() -> TestContext {
    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 0);
    assert_eq!(context.start_incoming_count(), 1);

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

    let connection = active_call.get_connection(1).expect(error_line!());

    cm.received_ice(
        active_call.call_id(),
        random_received_ice_candidate(&context.prng),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(
        connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );

    assert_eq!(context.answers_sent(), 1);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());

    context
}

#[test]
fn inbound_ice_connecting() {
    test_init();

    let _ = start_inbound_call();
}

// Create an inbound call session up to the ConnectedAndAccepted state.
//
// 1. receive an offer
// 2. ice connected
// 3. local accept call
//
// Now in the ConnectedAndAccepted state.
fn connect_inbound_call() -> TestContext {
    let context = start_inbound_call();
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
    assert_eq!(context.event_count(ApplicationEvent::LocalRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(!active_connection
        .app_connection()
        .unwrap()
        .outgoing_audio_enabled(),);

    info!("test: add media stream");
    active_connection
        .handle_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
        .expect(error_line!());

    info!("test: accepting call");
    cm.accept_call(active_call.call_id()).expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::ConnectedAndAccepted
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectedAndAccepted
    );
    assert_eq!(context.event_count(ApplicationEvent::LocalAccepted), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(active_connection
        .app_connection()
        .unwrap()
        .outgoing_audio_enabled());
    assert!(cm.busy());

    context
}

#[test]
fn inbound_call_connected() {
    test_init();

    let _ = connect_inbound_call();
}

#[test]
fn inbound_call_hangup_accepted() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::AcceptedOnAnotherDevice(2),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupAccepted),
        1
    );
    assert!(!cm.busy());
}

#[test]
fn inbound_call_hangup_declined() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::DeclinedOnAnotherDevice(2),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupDeclined),
        1
    );
    assert!(!cm.busy());
}

#[test]
fn inbound_call_hangup_busy() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.received_hangup(
        active_call.call_id(),
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::BusyOnAnotherDevice(2),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupBusy),
        1
    );
    assert!(!cm.busy());
}

#[test]
fn inbound_call_drop_connecting() {
    test_init();

    let context = start_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.drop_call(active_call.call_id()).expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedAppDroppedCall),
        1
    );
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.accepted_hangups_sent(), 0);
    assert_eq!(context.busy_hangups_sent(), 0);
    assert_eq!(context.declined_hangups_sent(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.need_permission_hangups_sent(), 0);
}

#[test]
fn inbound_call_drop_connecting_hangup_first() {
    test_init();

    let context = start_inbound_call();
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
    cm.drop_call(active_call.call_id()).expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.accepted_hangups_sent(), 0);
    assert_eq!(context.busy_hangups_sent(), 0);
    assert_eq!(context.declined_hangups_sent(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.need_permission_hangups_sent(), 0);
}

#[test]
fn inbound_call_drop_connecting_hangup_second() {
    test_init();

    let context = start_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.drop_call(active_call.call_id()).expect(error_line!());
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
    assert_eq!(
        context.event_count(ApplicationEvent::EndedAppDroppedCall),
        1
    );
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.accepted_hangups_sent(), 0);
    assert_eq!(context.busy_hangups_sent(), 0);
    assert_eq!(context.declined_hangups_sent(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.need_permission_hangups_sent(), 0);
}

#[test]
fn inbound_call_drop_connecting_ice_failed_first() {
    test_init();

    let context = start_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    active_connection.inject_ice_failed().expect(error_line!());
    cm.drop_call(active_call.call_id()).expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.accepted_hangups_sent(), 0);
    assert_eq!(context.busy_hangups_sent(), 0);
    assert_eq!(context.declined_hangups_sent(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.need_permission_hangups_sent(), 0);
}

#[test]
fn inbound_call_drop_connecting_ice_failed_second() {
    test_init();

    let context = start_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    cm.drop_call(active_call.call_id()).expect(error_line!());
    active_connection.inject_ice_failed().expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedAppDroppedCall),
        1
    );
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.accepted_hangups_sent(), 0);
    assert_eq!(context.busy_hangups_sent(), 0);
    assert_eq!(context.declined_hangups_sent(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.need_permission_hangups_sent(), 0);
}

#[test]
fn inbound_call_drop_connected() {
    test_init();

    let context = start_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    info!("test: injecting ice connected");
    active_connection
        .inject_ice_connected()
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    cm.drop_call(active_call.call_id()).expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedAppDroppedCall),
        1
    );
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.accepted_hangups_sent(), 0);
    assert_eq!(context.busy_hangups_sent(), 0);
    assert_eq!(context.declined_hangups_sent(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.need_permission_hangups_sent(), 0);
}

#[test]
fn inbound_call_drop_accepted() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();

    cm.drop_call(active_call.call_id()).expect(error_line!());
    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedAppDroppedCall),
        1
    );
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.accepted_hangups_sent(), 0);
    assert_eq!(context.busy_hangups_sent(), 0);
    assert_eq!(context.declined_hangups_sent(), 0);
    assert_eq!(context.normal_hangups_sent(), 0);
    assert_eq!(context.need_permission_hangups_sent(), 0);
}

#[test]
fn update_bandwidth_mode_default() {
    test_init();

    let context = connect_inbound_call();
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

    let context = connect_inbound_call();
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

#[test]
fn update_bandwidth_when_relayed() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_ice_network_route_changed(NetworkRoute {
            local_adapter_type: NetworkAdapterType::Unknown,
            local_adapter_type_under_vpn: NetworkAdapterType::Unknown,
            local_relayed: true,
            local_relay_protocol: TransportProtocol::Unknown,
            remote_relayed: false,
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
fn start_inbound_call_with_error() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 0);
    assert_eq!(context.start_incoming_count(), 1);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::WaitingToProceed
    );

    // cause the sending of the answer to fail.
    context.force_internal_fault(true);

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    context.force_internal_fault(false);

    let connection = active_call.get_connection(1).expect(error_line!());

    assert_eq!(
        connection.state().expect(error_line!()),
        ConnectionState::Terminating
    );

    // Two errors -- one from the failed send_answer and another from
    // the failed send_hangup, sent as part of the error clean up.
    assert_eq!(context.error_count(), 2);
    assert_eq!(context.ended_count(), 2);
    assert_eq!(context.answers_sent(), 0);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Terminated
    );
    assert!(!cm.busy());
}

#[test]
fn receive_offer_while_active() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::ReceivedOfferWhileActive),
        1
    );
    assert_eq!(context.busys_sent(), 1);
    assert_eq!(context.call_concluded_count(), 1);
    assert!(cm.busy());
}

#[test]
fn receive_expired_offer() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let age = Duration::from_secs(86400); // one whole day
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, age),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.offer_expired_count(), 1);
    assert!(!cm.busy());
}

#[test]
fn receive_offer_before_age_limit() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let age = MAX_MESSAGE_AGE - Duration::from_secs(1);
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, age),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.offer_expired_count(), 0);
    assert!(cm.busy());
}

#[test]
fn receive_offer_at_age_limit() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let age = MAX_MESSAGE_AGE;
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, age),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.offer_expired_count(), 0);
    assert!(cm.busy());
}

#[test]
fn receive_expired_offer_after_age_limit() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let age = MAX_MESSAGE_AGE + Duration::from_secs(1);
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, age),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.offer_expired_count(), 1);
    assert!(!cm.busy());
}

#[test]
fn offer_after_ice() {
    // We don't actually expose a way to automatically test that the ICE candidates are handled.
    // You can check manually by running with RUST_LOG=ringrtc::core::connection/ice_candidates
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_ice(call_id, random_received_ice_candidate(&context.prng))
        .expect(error_line!());
    cm.received_ice(call_id, random_received_ice_candidate(&context.prng))
        .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 0);
    assert_eq!(context.start_incoming_count(), 1);

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

    let connection = active_call.get_connection(1).expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(
        connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );

    assert_eq!(context.answers_sent(), 1);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

#[test]
fn offer_after_unrelated_ice() {
    // We don't actually expose a way to automatically test that the ICE candidates are handled.
    // You can check manually by running with RUST_LOG=ringrtc::core::connection/ice_candidates
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let other_call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_ice(other_call_id, random_received_ice_candidate(&context.prng))
        .expect(error_line!());
    cm.received_ice(other_call_id, random_received_ice_candidate(&context.prng))
        .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 0);
    assert_eq!(context.start_incoming_count(), 1);

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

    let connection = active_call.get_connection(1).expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(
        connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );

    assert_eq!(context.answers_sent(), 1);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

#[test]
fn offer_after_hangup() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_hangup(
        call_id,
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    cm.proceed(
        call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.answers_sent(), 0);
}

#[test]
fn offer_after_unrelated_hangup() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let unrelated_call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_hangup(
        unrelated_call_id,
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    cm.proceed(
        call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(cm.busy());
    assert_eq!(context.ended_count(), 0);
    assert_eq!(context.answers_sent(), 1);
}

#[test]
fn offer_after_ice_and_hangup() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_ice(call_id, random_received_ice_candidate(&context.prng))
        .expect(error_line!());
    cm.received_hangup(
        call_id,
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    cm.proceed(
        call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.answers_sent(), 0);
}

#[test]
fn offer_after_hangup_and_ice() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_hangup(
        call_id,
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());
    cm.received_ice(call_id, random_received_ice_candidate(&context.prng))
        .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    cm.proceed(
        call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(!cm.busy());
    assert_eq!(context.ended_count(), 1);
    assert_eq!(context.answers_sent(), 0);
}

#[test]
fn offer_after_hangup_with_intervening_ice_for_other_call() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let unrelated_call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_hangup(
        call_id,
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());
    cm.received_ice(
        unrelated_call_id,
        random_received_ice_candidate(&context.prng),
    )
    .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    cm.proceed(
        call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(cm.busy());
    assert_eq!(context.ended_count(), 0);
    assert_eq!(context.answers_sent(), 1);
}

#[test]
fn offer_after_hangup_with_intervening_hangup_for_other_call() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let unrelated_call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_hangup(
        call_id,
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());
    cm.received_hangup(
        unrelated_call_id,
        signaling::ReceivedHangup {
            sender_device_id: 1,
            hangup: signaling::Hangup::Normal,
        },
    )
    .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    cm.proceed(
        call_id,
        format!("CONTEXT-{}", context.prng.gen::<u16>()),
        BandwidthMode::Normal,
        None,
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert!(cm.busy());
    assert_eq!(context.ended_count(), 0);
    assert_eq!(context.answers_sent(), 1);
}

#[test]
fn offer_after_ice_with_previous_ice_for_other_call() {
    // We don't actually expose a way to automatically test that the ICE candidates are handled.
    // You can check manually by running with RUST_LOG=ringrtc::core::connection/ice_candidates
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", context.prng.gen::<u16>());
    let call_id = CallId::new(context.prng.gen::<u64>());
    let unrelated_call_id = CallId::new(context.prng.gen::<u64>());

    cm.received_ice(
        unrelated_call_id,
        random_received_ice_candidate(&context.prng),
    )
    .expect(error_line!());
    cm.received_ice(call_id, random_received_ice_candidate(&context.prng))
        .expect(error_line!());

    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer(&context.prng, Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert!(cm.active_call().is_ok());
    assert_eq!(context.start_outgoing_count(), 0);
    assert_eq!(context.start_incoming_count(), 1);

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

    let connection = active_call.get_connection(1).expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(
        connection.state().expect(error_line!()),
        ConnectionState::ConnectingBeforeAccepted
    );

    assert_eq!(context.answers_sent(), 1);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::ConnectingBeforeAccepted
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert!(cm.busy());
}

// Two users are in an accepted call. The remote user's leg is ended and they
// call the local user who is still in the original call. The local user should
// quietly end the active call and start handling the new incoming one.
#[test]
fn recall_when_connected() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();

    // Verify that one incoming call was started so far.
    assert_eq!(context.start_incoming_count(), 1);

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
    assert_eq!(context.start_incoming_count(), 2);
}

#[test]
fn group_call_ring() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

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
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    match &ring_updates[..] {
        [update] => {
            assert_eq!(
                &ringrtc::sim::sim_platform::GroupCallRingUpdate {
                    group_id,
                    ring_id,
                    sender,
                    update: group_call::RingUpdate::Requested
                },
                update
            );
        }
        _ => panic!("unexpected ring updates: {:?}", ring_updates),
    }

    assert!(!cm.busy());
}

#[test]
fn group_call_ring_expired() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

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

    cm.received_call_message(
        sender.clone(),
        1,
        2,
        buf,
        ringrtc::core::call_manager::MAX_MESSAGE_AGE + Duration::from_millis(1),
    )
    .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    match &ring_updates[..] {
        [update] => {
            assert_eq!(
                &ringrtc::sim::sim_platform::GroupCallRingUpdate {
                    group_id,
                    ring_id,
                    sender,
                    update: group_call::RingUpdate::ExpiredRequest
                },
                update
            );
        }
        _ => panic!("unexpected ring updates: {:?}", ring_updates),
    }

    assert!(!cm.busy());
}

#[test]
fn group_call_ring_busy_in_direct_call() {
    test_init();

    let context = connect_inbound_call();
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

    cm.received_call_message(sender.clone(), 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    match &ring_updates[..] {
        [update] => {
            assert_eq!(
                &ringrtc::sim::sim_platform::GroupCallRingUpdate {
                    group_id: group_id.clone(),
                    ring_id,
                    sender,
                    update: group_call::RingUpdate::BusyLocally
                },
                update
            );
        }
        _ => panic!("unexpected ring updates: {:?}", ring_updates),
    }

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
                        group_id: Some(group_id),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Busy.into()
                        ),
                    }),
                    ..Default::default()
                },
                call_message
            );
        }
        _ => panic!("unexpected messages: {:?}", messages),
    }

    assert!(cm.busy());
}

#[test]
fn group_call_ring_busy_in_group_call() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let self_uuid = vec![1, 0, 1];
    cm.set_self_uuid(self_uuid.clone()).expect(error_line!());

    let group_id_for_existing_group_call = vec![2, 2, 2];

    let group_call_id = context
        .create_group_call(group_id_for_existing_group_call)
        .expect(error_line!());
    cm.join(group_call_id);
    cm.synchronize().expect(error_line!());
    assert!(cm.busy());

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
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    match &ring_updates[..] {
        [update] => {
            assert_eq!(
                &ringrtc::sim::sim_platform::GroupCallRingUpdate {
                    group_id: group_id.clone(),
                    ring_id,
                    sender,
                    update: group_call::RingUpdate::BusyLocally
                },
                update
            );
        }
        _ => panic!("unexpected ring updates: {:?}", ring_updates),
    }

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
                        group_id: Some(group_id),
                        ring_id: Some(ring_id.into()),
                        r#type: Some(
                            protobuf::signaling::call_message::ring_response::Type::Busy.into()
                        ),
                    }),
                    ..Default::default()
                },
                call_message
            );
        }
        _ => panic!("unexpected messages: {:?}", messages),
    }

    assert!(cm.busy());
}

#[test]
fn group_call_ring_responses() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let group_id = vec![1, 1, 1];
    let sender = vec![1, 2, 3];
    let ring_id = group_call::RingId::from(42);

    let message = protobuf::signaling::CallMessage {
        ring_response: Some(protobuf::signaling::call_message::RingResponse {
            group_id: Some(group_id.clone()),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_response::Type::Declined.into()),
        }),
        ..Default::default()
    };
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .expect("cannot fail encoding to Vec");

    cm.received_call_message(sender.clone(), 1, 2, buf.clone(), Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    // Oops, we didn't set the current user's UUID.
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::GroupCallRingUpdate],
        ring_updates
    );

    cm.set_self_uuid(sender.clone()).expect(error_line!());

    // Okay, try again.
    cm.received_call_message(sender.clone(), 1, 2, buf, Duration::ZERO)
        .expect(error_line!());
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();

    match &ring_updates[..] {
        [update] => {
            assert_eq!(
                &ringrtc::sim::sim_platform::GroupCallRingUpdate {
                    group_id: group_id.clone(),
                    ring_id,
                    sender: sender.clone(),
                    update: group_call::RingUpdate::DeclinedOnAnotherDevice
                },
                update
            );
        }
        _ => panic!("unexpected ring updates: {:?}", ring_updates),
    }

    // We should ignore "ringing" messages regardless.
    let message = protobuf::signaling::CallMessage {
        ring_response: Some(protobuf::signaling::call_message::RingResponse {
            group_id: Some(group_id),
            ring_id: Some(ring_id.into()),
            r#type: Some(protobuf::signaling::call_message::ring_response::Type::Ringing.into()),
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

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    assert_eq!(
        &[] as &[ringrtc::sim::sim_platform::GroupCallRingUpdate],
        ring_updates
    );
}

#[test]
fn group_call_ring_timeout() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

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
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    match &ring_updates[..] {
        [update] => {
            assert_eq!(
                &ringrtc::sim::sim_platform::GroupCallRingUpdate {
                    group_id: group_id.clone(),
                    ring_id,
                    sender: sender.clone(),
                    update: group_call::RingUpdate::Requested
                },
                update
            );
        }
        _ => panic!("unexpected ring updates: {:?}", ring_updates),
    }
    assert!(!cm.busy());

    // We set the timeout to 1 second for testing, so we only need to sleep a bit longer than that.
    std::thread::sleep(Duration::from_millis(1_200));
    cm.synchronize().expect(error_line!());

    let ring_updates = cm
        .platform()
        .expect(error_line!())
        .take_group_call_ring_updates();
    match &ring_updates[..] {
        [update] => {
            assert_eq!(
                &ringrtc::sim::sim_platform::GroupCallRingUpdate {
                    group_id: group_id.clone(),
                    ring_id,
                    sender,
                    update: group_call::RingUpdate::ExpiredRequest
                },
                update
            );
        }
        _ => panic!("unexpected ring updates: {:?}", ring_updates),
    }

    // If we join at this point, we should not send an "Accepted" message.
    // (Even though real time hasn't elapsed, the cancellation removes the ring from the table.)
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
fn received_status_before_accepted() {
    let context = start_inbound_call();
    let mut cm = context.cm();
    let active_call = context.active_call();
    let mut active_connection = context.active_connection();

    active_connection
        .inject_ice_connected()
        .expect(error_line!());

    active_connection
        .handle_received_incoming_media(MediaStream::new(webrtc::Arc::null()))
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

    cm.accept_call(active_call.call_id()).expect(error_line!());

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
