//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Common test utilities

// Requires the 'sim' feature

use std::env;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use lazy_static::lazy_static;
use log::LevelFilter;
use rand::distributions::{Distribution, Standard};
use rand::{Rng, SeedableRng};

use rand_chacha::ChaCha20Rng;
use simplelog::{Config, ConfigBuilder, SimpleLogger};

use ringrtc::common::{ApplicationEvent, CallMediaType, DeviceId, FeatureLevel};
use ringrtc::core::call::Call;
use ringrtc::core::call_manager::CallManager;
use ringrtc::core::connection::Connection;
use ringrtc::core::signaling;
use ringrtc::sim::sim_platform::SimPlatform;

/*
use ringrtc::common::{CallDirection, CallId};

use ringrtc::core::call_connection_observer::ClientEvent;

use ringrtc::sim::call_connection_factory;
use ringrtc::sim::call_connection_factory::{CallConfig, SimCallConnectionFactory};
use ringrtc::sim::call_connection_observer::SimCallConnectionObserver;
use ringrtc::sim::sim_platform::SimCallConnection;
*/

macro_rules! error_line {
    () => {
        concat!(module_path!(), ":", line!())
    };
}

pub struct Prng {
    seed: u64,
    rng:  Mutex<Option<ChaCha20Rng>>,
}

impl Prng {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            rng: Mutex::new(None),
        }
    }

    // Use a freshly seeded PRNG for each test
    pub fn init(&self) {
        let mut opt = self.rng.lock().unwrap();
        let _ = opt.replace(ChaCha20Rng::seed_from_u64(self.seed));
    }

    pub fn gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        self.rng.lock().unwrap().as_mut().unwrap().gen::<T>()
    }
}

lazy_static! {
    pub static ref PRNG: Prng = {
        let rand_seed = match env::var("RANDOM_SEED") {
            Ok(v) => v.parse().unwrap(),
            Err(_) => SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect(error_line!())
                .as_millis() as u64,
        };

        println!("\n*** Using random seed: {}", rand_seed);
        Prng::new(rand_seed)
    };
}

pub fn test_init() {
    let (log_level, config) = if env::var("DEBUG_TESTS").is_ok() {
        (
            LevelFilter::Info,
            ConfigBuilder::new()
                .set_thread_level(LevelFilter::Info)
                .set_target_level(LevelFilter::Info)
                .set_location_level(LevelFilter::Info)
                .build(),
        )
    } else {
        (LevelFilter::Error, Config::default())
    };

    let _ = SimpleLogger::init(log_level, config);

    PRNG.init();
}

pub struct TestContext {
    platform:     SimPlatform,
    call_manager: CallManager<SimPlatform>,
}

impl Drop for TestContext {
    fn drop(&mut self) {
        info!("Dropping TestContext");

        info!("test: closing call manager");
        self.call_manager.close().unwrap();

        info!("test: closing platform");
        self.platform.close();
    }
}

#[allow(dead_code)]
impl TestContext {
    pub fn new() -> Self {
        info!("TestContext::new()");

        let mut platform = SimPlatform::new();
        let call_manager = CallManager::new(platform.clone()).unwrap();

        platform.set_call_manager(call_manager.clone());

        Self {
            platform,
            call_manager,
        }
    }

    pub fn cm(&self) -> CallManager<SimPlatform> {
        self.call_manager.clone()
    }

    pub fn active_call(&self) -> Call<SimPlatform> {
        self.call_manager.active_call().unwrap()
    }

    pub fn active_connection(&self) -> Connection<SimPlatform> {
        let active_call = self.call_manager.active_call().unwrap();
        match active_call.active_connection() {
            Ok(v) => v,
            Err(_) => active_call.get_connection(1 as DeviceId).unwrap(),
        }
    }

    pub fn force_internal_fault(&self, enable: bool) {
        let mut platform = self.call_manager.platform().unwrap();
        platform.force_internal_fault(enable);
    }

    pub fn force_signaling_fault(&self, enable: bool) {
        let mut platform = self.call_manager.platform().unwrap();
        platform.force_signaling_fault(enable);
    }

    pub fn no_auto_message_sent_for_ice(&self, enable: bool) {
        let mut platform = self.call_manager.platform().unwrap();
        platform.no_auto_message_sent_for_ice(enable);
    }

    pub fn offers_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.offers_sent()
    }

    pub fn answers_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.answers_sent()
    }

    pub fn ice_candidates_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.ice_candidates_sent()
    }

    pub fn normal_hangups_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.normal_hangups_sent()
    }

    pub fn need_permission_hangups_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.need_permission_hangups_sent()
    }

    pub fn accepted_hangups_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.accepted_hangups_sent()
    }

    pub fn declined_hangups_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.declined_hangups_sent()
    }

    pub fn busy_hangups_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.busy_hangups_sent()
    }

    pub fn error_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.error_count()
    }

    pub fn clear_error_count(&self) {
        let platform = self.call_manager.platform().unwrap();
        platform.clear_error_count()
    }

    pub fn ended_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.ended_count()
    }

    pub fn event_count(&self, event: ApplicationEvent) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.event_count(event)
    }

    pub fn busys_sent(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.busys_sent()
    }

    pub fn stream_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.stream_count()
    }

    pub fn start_outgoing_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.start_outgoing_count()
    }

    pub fn start_incoming_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.start_incoming_count()
    }

    pub fn call_concluded_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.call_concluded_count()
    }
}

#[derive(Clone, Copy)]
pub enum SignalingType {
    Legacy,              // SDP only
    BackwardsCompatible, // SDP and Opaque
    LegacyFree,          // Only Opaque
}

pub fn random_received_offer(
    signaling_type: SignalingType,
    age: Duration,
) -> signaling::ReceivedOffer {
    let sdp = format!("OFFER-{}", PRNG.gen::<u16>()).to_owned();
    let offer =
        signaling::Offer::from_v2_and_v1_sdp(CallMediaType::Audio, sdp.clone(), sdp).unwrap();
    let (opaque, sdp) = match signaling_type {
        SignalingType::Legacy => (None, offer.sdp),
        SignalingType::BackwardsCompatible => (offer.opaque, offer.sdp),
        SignalingType::LegacyFree => (offer.opaque, None),
    };
    let offer = signaling::Offer::from_opaque_or_sdp(offer.call_media_type, opaque, sdp);
    signaling::ReceivedOffer {
        offer,
        age,
        sender_device_id: 1 as DeviceId,
        sender_device_feature_level: FeatureLevel::MultiRing,
        receiver_device_id: 1 as DeviceId,
        receiver_device_is_primary: true,
    }
}

// Not sure why this is needed.  It is used...
#[allow(dead_code)]
pub fn random_received_answer(
    signaling_type: SignalingType,
    sender_device_id: DeviceId,
) -> signaling::ReceivedAnswer {
    let sdp = format!("ANSWER-{}", PRNG.gen::<u16>()).to_owned();
    let answer = match signaling_type {
        SignalingType::Legacy => signaling::Answer::from_v1_sdp(sdp),
        SignalingType::BackwardsCompatible => signaling::Answer::from_v2_sdp(sdp).unwrap(),
        SignalingType::LegacyFree => signaling::Answer::from_v1_sdp(sdp),
    };
    signaling::ReceivedAnswer {
        answer,
        sender_device_id,
        sender_device_feature_level: FeatureLevel::MultiRing,
    }
}

pub fn random_ice_candidate(signaling_type: SignalingType) -> signaling::IceCandidate {
    let sdp = format!("ICE-CANDIDATE-{}", PRNG.gen::<u16>()).to_owned();
    // V1 and V2 are the same for ICE candidates
    let ice_candidate = signaling::IceCandidate::from_v2_sdp(sdp).unwrap();
    let (opaque, sdp) = match signaling_type {
        SignalingType::Legacy => (None, ice_candidate.sdp),
        SignalingType::BackwardsCompatible => (ice_candidate.opaque, ice_candidate.sdp),
        SignalingType::LegacyFree => (ice_candidate.opaque, None),
    };
    signaling::IceCandidate::from_opaque_or_sdp(opaque, sdp)
}

pub fn random_received_ice_candidate(signaling_type: SignalingType) -> signaling::ReceivedIce {
    let candidate = random_ice_candidate(signaling_type);
    signaling::ReceivedIce {
        ice:              signaling::Ice {
            candidates_added: vec![candidate],
        },
        sender_device_id: 1 as DeviceId,
    }
}
