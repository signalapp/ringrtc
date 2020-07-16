//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Test the FSMs using the Simulation platform

extern crate ringrtc;

#[macro_use]
extern crate log;

use std::ptr;
use std::time::Duration;

use ringrtc::common::{
    ApplicationEvent,
    CallId,
    CallMediaType,
    CallState,
    ConnectionState,
    DeviceId,
    FeatureLevel,
};

use ringrtc::core::call_manager::MAX_MESSAGE_AGE_SEC;
use ringrtc::core::signaling;
use ringrtc::webrtc::media::MediaStream;

use ringrtc::webrtc::data_channel::DataChannel;

#[macro_use]
mod common;
use common::{test_init, TestContext, PRNG};

fn random_received_offer_with_age(age: Duration) -> signaling::ReceivedOffer {
    let sdp = format!("OFFER-{}", PRNG.gen::<u16>()).to_owned();
    signaling::ReceivedOffer {
        offer: signaling::Offer::from_sdp(CallMediaType::Audio, sdp),
        age,
        sender_device_id: 1 as DeviceId,
        sender_device_feature_level: FeatureLevel::MultiRing,
        receiver_device_id: 1 as DeviceId,
        receiver_device_is_primary: true,
    }
}

// Create an inbound call session up to the IceConnecting state.
//
// - create call manager
// - receive offer
// - check start incoming event happened
// - check active call exists
// - call proceed()
// - add received ice candidate
// - check underlying Connection is in IceConnecting(true) state
// - check call is in Connecting state
// - check answer sent
// Now in the Connecting state.
fn start_inbound_call() -> TestContext {
    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer_with_age(Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 0);
    assert_eq!(context.start_incoming_count(), 1);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Starting
    );

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    let connection = active_call
        .get_connection(1 as DeviceId)
        .expect(error_line!());

    // add a received ICE candidate
    let ice_candidate =
        signaling::IceCandidate::from_sdp(format!("ICE-{}", PRNG.gen::<u16>()).to_owned());
    cm.received_ice(
        active_call.call_id(),
        signaling::ReceivedIce {
            ice:              signaling::Ice {
                candidates_added: vec![ice_candidate],
            },
            sender_device_id: 1 as DeviceId,
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());
    assert_eq!(
        connection.state().expect(error_line!()),
        ConnectionState::IceConnecting(true)
    );

    assert_eq!(context.answers_sent(), 1);
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Connecting
    );
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);

    context
}

#[test]
fn inbound_ice_connecting() {
    test_init();

    let _ = start_inbound_call();
}

// Create an inbound call session up to the CallConnected state.
//
// 1. receive an offer
// 2. ice connected
// 3. on data channel
// 4. local accept call
//
// Now in the CallConnected state.

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
        active_call.state().expect(error_line!()),
        CallState::Connecting
    );

    info!("test: injecting data channel connected");
    let data_channel = DataChannel::new(ptr::null());
    active_connection
        .inject_on_data_channel(data_channel)
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
    assert_eq!(context.event_count(ApplicationEvent::LocalRinging), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert_eq!(
        false,
        active_connection
            .app_connection()
            .unwrap()
            .outgoing_audio_enabled(),
    );

    info!("test: add media stream");
    active_connection
        .on_add_stream(MediaStream::new(ptr::null()))
        .expect(error_line!());

    info!("test: accepting call");
    cm.accept_call(active_call.call_id()).expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(
        active_connection.state().expect(error_line!()),
        ConnectionState::CallConnected
    );
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Connected
    );
    assert_eq!(context.event_count(ApplicationEvent::LocalConnected), 1);
    assert_eq!(context.stream_count(), 1);
    assert_eq!(context.error_count(), 0);
    assert_eq!(context.ended_count(), 0);
    assert_eq!(
        true,
        active_connection
            .app_connection()
            .unwrap()
            .outgoing_audio_enabled(),
    );

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
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::AcceptedOnAnotherDevice(2 as DeviceId),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupAccepted),
        1
    );
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
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::DeclinedOnAnotherDevice(2 as DeviceId),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupDeclined),
        1
    );
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
            sender_device_id: 1 as DeviceId,
            hangup:           signaling::Hangup::BusyOnAnotherDevice(2 as DeviceId),
        },
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedRemoteHangupBusy),
        1
    );
}

#[test]
fn start_inbound_call_with_error() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer_with_age(Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(cm.active_call().is_ok(), true);
    assert_eq!(context.start_outgoing_count(), 0);
    assert_eq!(context.start_incoming_count(), 1);

    let active_call = context.active_call();
    assert_eq!(
        active_call.state().expect(error_line!()),
        CallState::Starting
    );

    // cause the sending of the answer to fail.
    context.force_internal_fault(true);

    cm.proceed(
        active_call.call_id(),
        format!("CONTEXT-{}", PRNG.gen::<u16>()).to_owned(),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    context.force_internal_fault(false);

    let connection = active_call
        .get_connection(1 as DeviceId)
        .expect(error_line!());

    assert_eq!(
        connection.state().expect(error_line!()),
        ConnectionState::Terminating
    );

    // Two errors -- one from the failed send_answer and another from
    // the failed send_hangup, sent as part of the error clean up.
    assert_eq!(context.error_count(), 2);
    assert_eq!(context.ended_count(), 2);
    assert_eq!(context.answers_sent(), 0);
    assert_eq!(active_call.state().expect(error_line!()), CallState::Closed);
}

#[test]
fn receive_offer_while_active() {
    test_init();

    let context = connect_inbound_call();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer_with_age(Duration::from_secs(0)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(context.call_concluded_count(), 1);
    assert_eq!(context.busys_sent(), 1);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedReceivedOfferWhileActive),
        1
    );
}

#[test]
fn receive_expired_offer() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer_with_age(Duration::from_secs(86400)), // one whole day
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedReceivedOfferExpired),
        1
    );
}

#[test]
fn receive_offer_before_age_limit() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    // create off way in the past
    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer_with_age(Duration::from_secs(MAX_MESSAGE_AGE_SEC - 1)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedReceivedOfferExpired),
        0
    );
}

#[test]
fn receive_offer_at_age_limit() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    // create off way in the past
    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer_with_age(Duration::from_secs(MAX_MESSAGE_AGE_SEC)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedReceivedOfferExpired),
        0
    );
}

#[test]
fn receive_expired_offer_after_age_limit() {
    test_init();

    let context = TestContext::new();
    let mut cm = context.cm();

    // create off way in the past
    let remote_peer = format!("REMOTE_PEER-{}", PRNG.gen::<u16>()).to_owned();
    let call_id = CallId::new(PRNG.gen::<u64>());
    cm.received_offer(
        remote_peer,
        call_id,
        random_received_offer_with_age(Duration::from_secs(MAX_MESSAGE_AGE_SEC + 1)),
    )
    .expect(error_line!());

    cm.synchronize().expect(error_line!());

    assert_eq!(context.error_count(), 0);
    assert_eq!(
        context.event_count(ApplicationEvent::EndedReceivedOfferExpired),
        1
    );
}
