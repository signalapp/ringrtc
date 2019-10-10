//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Test the FSM using the Simulation platform

extern crate ringrtc;

#[macro_use]
extern crate log;

extern crate rand;

use std::ptr;

use ringrtc::common::{
    CallDirection,
    CallState,
};

use ringrtc::core::call_connection_observer::ClientEvent;

use ringrtc::sim::error::SimError;

use ringrtc::webrtc::data_channel::DataChannel;
use ringrtc::webrtc::ice_candidate::IceCandidate;
use ringrtc::webrtc::media_stream::MediaStream;

#[macro_use]
mod common;
use common::{
    test_init,
    create_context,
    PRNG,
    TestContext,
};

#[test]
// Name this test so that it runs first in cargo's (PackageID,
// TargetKind, Name) order.
fn _test_init() {
    test_init();
}

// Simple test that:
// -- creates call connection factory
// -- creates call connection
// -- verifies a few input parameters made it in
// -- shuts down call connection
// -- shuts down call connection factory

#[test]
fn create_ccf() {

    test_init();

    for _i in 0..6 {
        let call_id = PRNG.gen::<u64>();
        let direction = if PRNG.gen::<bool>() {
            CallDirection::OutGoing
        } else {
            CallDirection::InComing
        };

        let context = create_context(call_id, direction);
        let cc = context.cc();

        assert_eq!(cc.call_id(), call_id);
        assert_eq!(cc.direction(), direction);
        assert_eq!(cc.state().expect(error_line!()), CallState::Idle);
    }
}

// Create an outbound call session up to the IceConnecting state.
//
// 1. create an offer
// 2. send offer
// 3. receive answer
//
// Now in the IceConnecting state.

fn start_outbound_call() -> TestContext {

    let call_id = PRNG.gen::<u64>();
    let direction = CallDirection::OutGoing;

    let context = create_context(call_id, direction);

    let mut cc = context.cc();

    assert_eq!(cc.call_id(), call_id);
    assert_eq!(cc.direction(), direction);

    assert_eq!(cc.state().expect(error_line!()), CallState::Idle);

    cc.inject_send_offer().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::SendingOffer);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.offers_sent(), 1);

    cc.inject_handle_answer("REMOTE SDP ANSWER".to_string()).expect(error_line!());
    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnecting(true));
    assert_eq!(context.client_error_count(), 0);

    context
}

// Create an outbound call session up to the CallConnected state.
//
// 1. create an offer
// 2. send offer
// 3. receive answer
// 4. ice connected
// 5. call connected
//
// Now in the CallConnected state.

fn connect_outbound_call() -> TestContext {

    let context = start_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice connected");
    cc.inject_ice_connected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnected);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::Ringing), 1);

    info!("test: injecting call connected");
    cc.inject_remote_connected(cc.call_id()).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::CallConnected);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::RemoteConnected), 1);

    context
}

// Create an inbound call session up to the IceConnecting state.
//
// 1. receive offer
//
// Now in the IceConnecting state.

fn start_inbound_call() -> TestContext {

    let call_id = PRNG.gen::<u64>();
    let direction = CallDirection::InComing;

    let context = create_context(call_id, direction);
    let mut cc = context.cc();

    assert_eq!(cc.call_id(), call_id);
    assert_eq!(cc.direction(), direction);

    assert_eq!(cc.state().expect(error_line!()), CallState::Idle);

    cc.inject_handle_offer("REMOTE SDP OFFER".to_string()).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnecting(true));
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.answers_sent(), 1);

    context
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
    let mut cc = context.cc();

    info!("test: injecting ice connected");
    cc.inject_ice_connected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnected);
    assert_eq!(context.client_error_count(), 0);
    // For incoming calls the Ringing event occurs when the data
    // channel is available.
    assert_eq!(context.event_count(ClientEvent::Ringing), 0);

    // synthesize an onDataChannel event
    let data_channel = DataChannel::new(ptr::null());
    cc.inject_on_data_channel(data_channel).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::Ringing), 1);

    info!("test: injecting call connected");
    cc.inject_accept_call().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::CallConnected);
    assert_eq!(context.client_error_count(), 0);

    // TODO - verify that the data_channel sent a Connected message

    context
}

#[test]
fn outbound_send_offer() {

    test_init();

    let call_id = PRNG.gen::<u64>();
    let direction = CallDirection::OutGoing;

    let context = create_context(call_id, direction);
    let mut cc = context.cc();

    cc.inject_send_offer().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::SendingOffer);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.offers_sent(), 1);

}

#[test]
fn outbound_receive_answer() {

    test_init();

    let _ = start_outbound_call();
}

#[test]
fn outbound_local_hang_up() {

    test_init();

    let context = start_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting hangup");
    cc.inject_hang_up().expect(error_line!());
    assert!(cc.terminating().expect(error_line!()));

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::Terminating);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.hangups_sent(), 1);

    // TODO - verify that the data_channel sent a hangup message

}

#[test]
fn outbound_ice_failed() {

    test_init();

    let context = start_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice connection failed");
    cc.inject_ice_connection_failed().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnectionFailed);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::ConnectionFailed), 1);

}

#[test]
fn outbound_ice_connected() {

    test_init();

    let context = start_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice connected");
    cc.inject_ice_connected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnected);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::Ringing), 1);
}

#[test]
fn outbound_ice_disconnected_before_call_connected() {

    test_init();

    let context = start_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice connected");
    cc.inject_ice_connected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnected);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::Ringing), 1);

    info!("test: injecting ice disconnected");
    cc.inject_ice_connection_disconnected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    // When ICE disconnects before the call is connected, the system
    // should return to the IceConnecting state.
    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnecting(true));
    assert_eq!(context.client_error_count(), 0);
}

#[test]
fn outbound_call_connected() {

    test_init();

    let _ = connect_outbound_call();
}

#[test]
fn outbound_call_connected_with_stale_call_id() {

    test_init();

    let context = start_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice connected");
    cc.inject_ice_connected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnected);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::Ringing), 1);

    info!("test: injecting stale call connected");
    cc.inject_remote_connected(cc.call_id() + 1).expect(error_line!());

    cc.synchronize().expect(error_line!());

    // verify bogus connect is simply dropped, with no state change.
    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnected);
    assert_eq!(context.client_error_count(), 0);
}

#[test]
fn outbound_call_connected_ice_failed() {

    test_init();

    let context = connect_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice connection failed");
    cc.inject_ice_connection_failed().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnectionFailed);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::ConnectionFailed), 1);
}

#[test]
fn outbound_call_connected_local_hangup() {

    test_init();

    let context = connect_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting hangup");
    cc.inject_hang_up().expect(error_line!());
    assert!(cc.terminating().expect(error_line!()));

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::Terminating);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.hangups_sent(), 1);

    // TODO - verify that the data_channel sent a hangup message

}

#[test]
fn outbound_ice_disconnected_after_call_connected_and_reconnect() {

    test_init();

    let context = connect_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice disconnected");
    cc.inject_ice_connection_disconnected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    // When ICE disconnects after the call is connected, the system
    // should move to the IceReconnecting state.
    assert_eq!(cc.state().expect(error_line!()), CallState::IceReconnecting);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::CallReconnecting), 1);

    info!("test: injecting ice connected");
    cc.inject_ice_connected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    // When ICE reconnects after the call is connected, the system
    // should move to the CallConnected state.
    assert_eq!(cc.state().expect(error_line!()), CallState::CallConnected);
    assert_eq!(context.client_error_count(), 0);
    // Ringing event count should be two: 1 from the initial ICE
    // connected state and another from the reconnection.
    assert_eq!(context.event_count(ClientEvent::Ringing), 2);

}

#[test]
fn outbound_ice_disconnected_after_call_connected_and_local_hangup() {

    test_init();

    let context = connect_outbound_call();
    let mut cc = context.cc();

    info!("test: injecting ice disconnected");
    cc.inject_ice_connection_disconnected().expect(error_line!());

    cc.synchronize().expect(error_line!());

    // When ICE disconnects after the call is connected, the system
    // should move to the IceReconnecting state.
    assert_eq!(cc.state().expect(error_line!()), CallState::IceReconnecting);
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::CallReconnecting), 1);

    // Hang up before reconnect happens
    cc.inject_hang_up().expect(error_line!());
    assert!(cc.terminating().expect(error_line!()));

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::Terminating);
    assert_eq!(context.client_error_count(), 0);
    // Should only be one Ringing event in this case
    assert_eq!(context.event_count(ClientEvent::Ringing), 1);
    assert_eq!(context.hangups_sent(), 1);

}

#[test]
fn inbound_ice_connecting() {

    test_init();

    let _ = start_inbound_call();
}

#[test]
fn inbound_call_connected() {

    test_init();

    let _ = connect_inbound_call();
}

#[test]
fn inject_client_error() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    cc.inject_client_error(SimError::TestError("fake_error".to_string()).into()).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(context.client_error_count(), 1);

}

#[test]
fn inject_local_video_status() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    cc.inject_local_video_status(false).expect(error_line!());

    assert_eq!(context.client_error_count(), 0);
    // TODO -- verify that the data channel object sent a message
}

#[test]
fn inject_local_ice_candidate() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    let ice_candidate = IceCandidate::new("fake_spd_mid".to_string(), 0, "fake_spd".to_string());
    cc.inject_local_ice_candidate(ice_candidate).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.ice_candidates_sent(), 1);

}

#[test]
fn inject_remote_ice_candidate() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    let ice_candidate = IceCandidate::new("fake_spd_mid".to_string(), 0, "fake_spd".to_string());
    cc.inject_remote_ice_candidate(ice_candidate).expect(error_line!());

    assert_eq!(context.client_error_count(), 0);

    // TODO -- verify the ice candidate was buffered
    // TODO -- verify the ice candidate was applied to the peer_connection
}

#[test]
fn inject_remote_hangup() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    cc.inject_remote_hangup(cc.call_id()).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::RemoteHangup), 1);
}

#[test]
fn inject_remote_video_status() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    let mut enable_count = 0;
    let mut disable_count = 0;
    for _i in 0..10 {
        let enable = PRNG.gen::<bool>();

        cc.inject_remote_video_status(cc.call_id(), enable).expect(error_line!());
        cc.synchronize().expect(error_line!());

        if enable {
            enable_count += 1;
        } else {
            disable_count +=1;
        }

        assert_eq!(context.client_error_count(), 0);
        assert_eq!(context.event_count(ClientEvent::RemoteVideoEnable), enable_count);
        assert_eq!(context.event_count(ClientEvent::RemoteVideoDisable), disable_count);

    }

}

#[test]
fn inject_call_timeout_before_connect() {
    let context = start_outbound_call();
    let mut cc = context.cc();

    cc.inject_call_timeout(cc.call_id()).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::CallTimeout), 1);

}

#[test]
fn inject_call_timeout_after_connect() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    cc.inject_call_timeout(cc.call_id()).expect(error_line!());

    cc.synchronize().expect(error_line!());

    // The call is already connected, so the timeout is ignored.
    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.event_count(ClientEvent::CallTimeout), 0);

}

#[test]
fn inject_on_add_stream() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    let media_stream = MediaStream::new(ptr::null());
    cc.inject_on_add_stream(media_stream).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(context.client_error_count(), 0);
    assert_eq!(context.stream_count(), 1);

}

#[test]
fn outbound_send_offer_with_error() {

    test_init();

    let call_id = PRNG.gen::<u64>();
    let direction = CallDirection::OutGoing;

    let context = create_context(call_id, direction);
    let mut cc = context.cc();

    // cause the sending of the offer to fail.
    context.should_fail(true);

    cc.inject_send_offer().expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::SendingOffer);
    assert_eq!(context.client_error_count(), 1);
    assert_eq!(context.offers_sent(), 0);

}

#[test]
fn start_inbound_call_with_error() {

    let call_id = PRNG.gen::<u64>();
    let direction = CallDirection::InComing;

    let context = create_context(call_id, direction);
    let mut cc = context.cc();

    assert_eq!(cc.call_id(), call_id);
    assert_eq!(cc.direction(), direction);

    assert_eq!(cc.state().expect(error_line!()), CallState::Idle);

    // cause the sending of the answer to fail.
    context.should_fail(true);

    cc.inject_handle_offer("REMOTE SDP OFFER".to_string()).expect(error_line!());

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::IceConnecting(false));
    assert_eq!(context.client_error_count(), 1);
    assert_eq!(context.answers_sent(), 0);

}

#[test]
fn outbound_call_connected_local_hangup_with_error() {

    test_init();

    let context = connect_outbound_call();
    let mut cc = context.cc();

    // cause the sending of the hangup to fail.
    context.should_fail(true);

    cc.inject_hang_up().expect(error_line!());
    assert!(cc.terminating().expect(error_line!()));

    cc.synchronize().expect(error_line!());

    assert_eq!(cc.state().expect(error_line!()), CallState::Terminating);

    // There is nothing we can do about send errors during hangup, so
    // we should not see any errors transmitted to the client.
    assert_eq!(context.client_error_count(), 0);

    assert_eq!(context.hangups_sent(), 0);

}

#[test]
fn inject_local_ice_candidate_with_error() {
    let context = connect_outbound_call();
    let mut cc = context.cc();

    // cause the sending of the ICE candidate to fail.
    context.should_fail(true);

    let ice_candidate = IceCandidate::new("fake_spd_mid".to_string(), 0, "fake_spd".to_string());
    cc.inject_local_ice_candidate(ice_candidate).expect(error_line!());

    cc.synchronize().expect(error_line!());

    // We should see an error sent to the client.
    assert_eq!(context.client_error_count(), 1);

    // We should see that no ICE candidates were sent
    assert_eq!(context.ice_candidates_sent(), 0);

    // Restore the ability to send ICE.
    context.should_fail(false);

    // Clear previous errors
    context.clear_client_error_count();

    // Send another ICE candidate
    let ice_candidate = IceCandidate::new("fake_spd_mid2".to_string(), 0, "fake_spd2".to_string());
    cc.inject_local_ice_candidate(ice_candidate).expect(error_line!());

    cc.synchronize().expect(error_line!());

    // We should see no errors sent to the client.
    assert_eq!(context.client_error_count(), 0);

    // We should see that both ICE candidates were sent
    assert_eq!(context.ice_candidates_sent(), 2);

}
