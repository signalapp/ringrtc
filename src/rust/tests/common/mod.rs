//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Common test utilities

// Requires the 'sim' feature

use std::cell::RefCell;
use std::env;
use std::time::{Duration, SystemTime};

use lazy_static::lazy_static;
use rand::distributions::{Distribution, Standard};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

use ringrtc::common::{ApplicationEvent, CallMediaType, DeviceId};
use ringrtc::core::call::Call;
use ringrtc::core::call_manager::CallManager;
use ringrtc::core::connection::Connection;
use ringrtc::core::{group_call, signaling};
use ringrtc::lite::http;
use ringrtc::protobuf;
use ringrtc::sim::sim_platform::SimPlatform;
use ringrtc::webrtc;
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
    rng: RefCell<ChaCha20Rng>,
}

impl Prng {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: RefCell::new(ChaCha20Rng::seed_from_u64(seed)),
        }
    }

    pub fn gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        self.rng.borrow_mut().gen::<T>()
    }
}

lazy_static! {
    static ref RANDOM_SEED: u64 = {
        let seed = match env::var("RANDOM_SEED") {
            Ok(v) => v.parse().unwrap(),
            Err(_) => SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect(error_line!())
                .as_millis() as u64,
        };

        println!("\n*** Using random seed: {}", seed);
        seed
    };
}

pub fn test_init() {
    let _ = env_logger::try_init();
    env::set_var("INCOMING_GROUP_CALL_RING_SECS", "1");
}

pub struct TestContext {
    platform: SimPlatform,
    call_manager: CallManager<SimPlatform>,
    pub prng: Prng,
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

struct SimHttpDelegate {}

impl http::Delegate for SimHttpDelegate {
    fn send_request(&self, _request_id: u32, _request: http::Request) {
        // Do nothing
    }
}

#[allow(dead_code)]
impl TestContext {
    pub fn new() -> Self {
        info!("TestContext::new()");

        let mut platform = SimPlatform::new();
        let http_client = http::DelegatingClient::new(SimHttpDelegate {});
        let call_manager = CallManager::new(platform.clone(), http_client).unwrap();

        platform.set_call_manager(call_manager.clone());

        Self {
            platform,
            call_manager,
            prng: Prng::new(*RANDOM_SEED),
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
            Err(_) => active_call.get_connection(1).unwrap(),
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

    pub fn last_ice_sent(&self) -> Option<signaling::SendIce> {
        let platform = self.call_manager.platform().unwrap();
        platform.last_ice_sent()
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

    pub fn offer_expired_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.offer_expired_count()
    }

    pub fn call_concluded_count(&self) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.call_concluded_count()
    }

    pub fn create_group_call(
        &self,
        group_id: group_call::GroupId,
    ) -> Result<group_call::ClientId, anyhow::Error> {
        self.cm().create_group_call_client(
            group_id,
            "".to_owned(),
            vec![],
            None,
            None,
            ringrtc::webrtc::media::AudioTrack::new(webrtc::Arc::null(), None),
            ringrtc::webrtc::media::VideoTrack::new(webrtc::Arc::null(), None),
            None,
        )
    }
}

pub fn random_received_offer(_prng: &Prng, age: Duration) -> signaling::ReceivedOffer {
    let local_public_key = rand::thread_rng().gen::<[u8; 32]>().to_vec();
    let offer = signaling::Offer::from_v4(
        CallMediaType::Audio,
        protobuf::signaling::ConnectionParametersV4 {
            public_key: Some(local_public_key),
            ice_ufrag: None,
            ice_pwd: None,
            receive_video_codecs: vec![],
            max_bitrate_bps: None,
        },
    )
    .unwrap();
    let offer = signaling::Offer::new(offer.call_media_type, offer.opaque).unwrap();
    signaling::ReceivedOffer {
        offer,
        age,
        sender_device_id: 1,
        receiver_device_id: 1,
        receiver_device_is_primary: true,
        sender_identity_key: Vec::new(),
        receiver_identity_key: Vec::new(),
    }
}

// Not sure why this is needed.  It is used...
#[allow(dead_code)]
pub fn random_received_answer(
    _prng: &Prng,
    sender_device_id: DeviceId,
) -> signaling::ReceivedAnswer {
    let local_public_key = rand::thread_rng().gen::<[u8; 32]>().to_vec();
    let answer = signaling::Answer::from_v4(protobuf::signaling::ConnectionParametersV4 {
        public_key: Some(local_public_key),
        ice_ufrag: None,
        ice_pwd: None,
        receive_video_codecs: vec![],
        max_bitrate_bps: None,
    })
    .unwrap();
    signaling::ReceivedAnswer {
        answer,
        sender_device_id,
        sender_identity_key: Vec::new(),
        receiver_identity_key: Vec::new(),
    }
}

pub fn random_ice_candidate(prng: &Prng) -> signaling::IceCandidate {
    let sdp = format!("ICE-CANDIDATE-{}", prng.gen::<u16>());
    // V1 and V2 are the same for ICE candidates
    let ice_candidate = signaling::IceCandidate::from_v3_sdp(sdp).unwrap();
    signaling::IceCandidate::new(ice_candidate.opaque)
}

pub fn random_received_ice_candidate(prng: &Prng) -> signaling::ReceivedIce {
    let candidate = random_ice_candidate(prng);
    signaling::ReceivedIce {
        ice: signaling::Ice {
            candidates: vec![candidate],
        },
        sender_device_id: 1,
    }
}
