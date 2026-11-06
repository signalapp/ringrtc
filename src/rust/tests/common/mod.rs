//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Common test utilities

// Requires the 'sim' feature

use std::{
    cell::RefCell,
    env,
    sync::{Arc, Condvar, LazyLock, Mutex},
    time::{Duration, SystemTime},
};

use rand::{
    RngExt,
    distr::{Distribution, StandardUniform},
    rand_core::SeedableRng,
    rngs::ChaCha20Rng,
};
use ringrtc::{
    common::{
        ApplicationEvent, CallEndReason, CallMediaType, DataMode, DeviceId, EVENT_QUEUE_SIZE,
    },
    core::{
        call::Call,
        call_manager::{CallManager, CreateGroupCallParams},
        connection::{Connection, ConnectionObserverEvent},
        group_call, signaling,
    },
    lite::http,
    protobuf,
    sim::sim_platform::SimPlatform,
    webrtc,
};
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

    pub fn generate<T>(&self) -> T
    where
        StandardUniform: Distribution<T>,
    {
        self.rng.borrow_mut().random::<T>()
    }
}

static RANDOM_SEED: LazyLock<u64> = LazyLock::new(|| {
    let seed = match env::var("RANDOM_SEED") {
        Ok(v) => v.parse().unwrap(),
        Err(_) => SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect(error_line!())
            .as_millis() as u64,
    };

    println!("\n*** Using random seed: {}", seed);
    seed
});

pub fn test_init() {
    let _ = env_logger::try_init();
    // Safety: depends on having no concurrent tests running that can modify the same envp
    unsafe {
        env::set_var("INCOMING_GROUP_CALL_RING_SECS", "1");
    }
}

/// A paused FSM's resume latch, false until the test resumes it.
type FsmPause = Arc<(Mutex<bool>, Condvar)>;

fn resume_fsm(pause: &FsmPause) {
    let (mutex, condvar) = &**pause;
    let mut resumed = mutex.lock().unwrap();
    *resumed = true;
    condvar.notify_all();
}

/// How long test waiters block before declaring a hang a regression.
#[allow(dead_code)]
const FSM_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct TestContext {
    platform: SimPlatform,
    call_manager: CallManager<SimPlatform>,
    pub prng: Prng,
    call_fsm_pause: RefCell<Option<FsmPause>>,
    connection_fsm_pause: RefCell<Option<FsmPause>>,
}

impl Drop for TestContext {
    fn drop(&mut self) {
        info!("Dropping TestContext");

        // Resume any FSM a (possibly panicking) test left paused, so close()
        // does not block on a wedged FSM.
        if let Some(pause) = self.call_fsm_pause.borrow_mut().take() {
            resume_fsm(&pause);
        }
        if let Some(pause) = self.connection_fsm_pause.borrow_mut().take() {
            resume_fsm(&pause);
        }

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
            call_fsm_pause: RefCell::new(None),
            connection_fsm_pause: RefCell::new(None),
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

    pub fn pause_connection_fsm(&self) {
        assert!(
            self.connection_fsm_pause.borrow().is_none(),
            "connection FSM already paused"
        );
        let pause = FsmPause::default();
        self.active_connection()
            .inject_pause(pause.clone())
            .unwrap();
        *self.connection_fsm_pause.borrow_mut() = Some(pause);
    }

    pub fn resume_connection_fsm(&self) {
        if let Some(pause) = self.connection_fsm_pause.borrow_mut().take() {
            resume_fsm(&pause);
        }
    }

    pub fn fill_connection_fsm_queue(&self) -> usize {
        let mut connection = self.active_connection();
        for accepted in 0..2 * EVENT_QUEUE_SIZE {
            if connection
                .inject_update_data_mode(DataMode::Normal)
                .is_err()
            {
                return accepted;
            }
        }
        panic!("connection fsm queue never filled");
    }

    pub fn pause_call_fsm(&self) {
        assert!(
            self.call_fsm_pause.borrow().is_none(),
            "call FSM already paused"
        );
        let pause = FsmPause::default();
        self.active_call().inject_pause(pause.clone()).unwrap();
        *self.call_fsm_pause.borrow_mut() = Some(pause);
    }

    pub fn resume_call_fsm(&self) {
        if let Some(pause) = self.call_fsm_pause.borrow_mut().take() {
            resume_fsm(&pause);
        }
    }

    pub fn fill_call_fsm_queue(&self) -> usize {
        let mut call = self.active_call();
        let event = ConnectionObserverEvent::AudioLevels {
            captured_level: 0,
            received_level: 0,
        };
        for accepted in 0..2 * EVENT_QUEUE_SIZE {
            if call.on_connection_observer_event(1, event).is_err() {
                return accepted;
            }
        }
        panic!("call fsm queue never filled");
    }

    pub fn wait_for_teardown(&self) {
        // Flush twice: hangup and the teardown it spawns are separate tasks on
        // the same FIFO worker, so a single flush can return between them.
        let mut cm = self.cm();
        cm.sync_worker_thread().unwrap();
        cm.sync_worker_thread().unwrap();
    }

    pub fn wait_for_connection_fsm_terminated(&self, connection: &Connection<SimPlatform>) -> bool {
        connection.wait_for_fsm_terminated(FSM_WAIT_TIMEOUT)
    }

    pub fn wait_for_call_fsm_terminated(&self, call: &Call<SimPlatform>) -> bool {
        call.wait_for_fsm_terminated(FSM_WAIT_TIMEOUT)
    }

    pub fn wait_for_call_concluded(&self, count: usize) -> bool {
        self.platform
            .wait_for_call_concluded(count, FSM_WAIT_TIMEOUT)
    }

    pub fn force_internal_fault(&self, enable: bool) {
        let mut platform = self.call_manager.platform().unwrap();
        platform.force_internal_fault(enable);
    }

    pub fn force_signaling_failure(&self, enable: bool) {
        let mut platform = self.call_manager.platform().unwrap();
        platform.force_signaling_failure(enable);
    }

    pub fn force_call_ended_failure(&self, enable: bool) {
        let mut platform = self.call_manager.platform().unwrap();
        platform.force_call_ended_failure(enable);
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

    pub fn end_reason_count(&self, reason: CallEndReason) -> usize {
        let platform = self.call_manager.platform().unwrap();
        platform.end_reason_count(reason)
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
        self.cm().create_group_call_client(CreateGroupCallParams {
            group_id,
            sfu_url: "".to_owned(),
            hkdf_extra_info: vec![],
            audio_levels_interval: None,
            dred_duration: 0,
            svc_config: None,
            peer_connection_factory: None,
            outgoing_audio_track: ringrtc::webrtc::media::AudioTrack::new(
                webrtc::Arc::null(),
                None,
            ),
            outgoing_video_track: ringrtc::webrtc::media::VideoTrack::new(
                webrtc::Arc::null(),
                None,
            ),
            incoming_video_sink: None,
        })
    }
}

pub fn random_received_offer(_prng: &Prng, age: Duration) -> signaling::ReceivedOffer {
    let local_public_key = rand::rng().random::<[u8; 32]>().to_vec();
    let offer = signaling::Offer::from_v4(
        CallMediaType::Audio,
        protobuf::signaling::ConnectionParametersV4 {
            public_key: Some(local_public_key),
            ice_ufrag: None,
            ice_pwd: None,
            decode_only_video_codecs: vec![],
            encode_only_video_codecs: vec![],
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
    let local_public_key = rand::rng().random::<[u8; 32]>().to_vec();
    let answer = signaling::Answer::from_v4(protobuf::signaling::ConnectionParametersV4 {
        public_key: Some(local_public_key),
        ice_ufrag: None,
        ice_pwd: None,
        decode_only_video_codecs: vec![],
        encode_only_video_codecs: vec![],
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
    let sdp = format!("ICE-CANDIDATE-{}", prng.generate::<u16>());
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
