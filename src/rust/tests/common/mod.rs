//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Common test utilities

// Requires the 'sim' feature

use std::env;
use std::sync::{
    Arc,
    Mutex,
};

use lazy_static::lazy_static;
use log::LevelFilter;
use rand::{
    Rng,
    SeedableRng,
};
use rand::distributions::{
    Distribution,
    Standard,
};

use rand_chacha::ChaCha20Rng;
use simplelog::{
    Config,
    SimpleLogger,
};

use ringrtc::common::{
    CallDirection,
    CallId,
};

use ringrtc::core::call_connection_observer::ClientEvent;

use ringrtc::sim::call_connection_factory;
use ringrtc::sim::call_connection_factory::{
    CallConfig,
    SimCallConnectionFactory,
};
use ringrtc::sim::call_connection_observer::SimCallConnectionObserver;
use ringrtc::sim::sim_platform::SimCallConnection;

macro_rules! error_line {
    () => { concat!(module_path!(), ":", line!()) };
}

pub struct Prng {
    seed: u64,
    rng: Mutex<Option<ChaCha20Rng>>,
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
            Err(_) => 0,
        };

        println!("\n*** Using random seed: {}", rand_seed);
        Prng::new(rand_seed)

    };
}

pub fn test_init() {
    let log_level = if env::var("DEBUG_TESTS").is_ok() {
        LevelFilter::Info
    } else {
        LevelFilter::Error
    };
    let _ = SimpleLogger::init(log_level, Config::default());

    PRNG.init();
}

pub struct TestContext {
    cc_factory:  SimCallConnectionFactory,
    cc:          Box<SimCallConnection>,
    cc_observer: Arc<Mutex<SimCallConnectionObserver>>,
}

impl Drop for TestContext {
    fn drop(&mut self) {
        info!("Dropping TestContext");

        info!("test: closing cc");
        self.cc.close().unwrap();

        info!("test: closing ccf");
        call_connection_factory::free_factory(self.cc_factory.clone()).unwrap();
    }
}

impl TestContext {

    pub fn client_error_count(&self) -> usize {
        let cc_observer = self.cc_observer.lock().unwrap();
        cc_observer.get_error_count()
    }

    pub fn clear_client_error_count(&self) {
        let cc_observer = self.cc_observer.lock().unwrap();
        cc_observer.clear_error_count()
    }

    pub fn event_count(&self, event: ClientEvent) -> usize {
        let mut cc_observer = self.cc_observer.lock().unwrap();
        cc_observer.get_event_count(event)
    }

    pub fn stream_count(&self) -> usize {
        let cc_observer = self.cc_observer.lock().unwrap();
        cc_observer.get_stream_count()
    }

    pub fn cc(&self) -> SimCallConnection {
        *self.cc.clone()
    }

    pub fn should_fail(&self, enable: bool) {
        let cc = self.cc();
        let mut platform = cc.platform().unwrap();
        platform.should_fail(enable);
    }

    pub fn offers_sent(&self) -> usize {
        let cc = self.cc();
        let platform = cc.platform().unwrap();
        platform.offers_sent()
    }

    pub fn answers_sent(&self) -> usize {
        let cc = self.cc();
        let platform = cc.platform().unwrap();
        platform.answers_sent()
    }

    pub fn ice_candidates_sent(&self) -> usize {
        let cc = self.cc();
        let platform = cc.platform().unwrap();
        platform.ice_candidates_sent()
    }

    pub fn hangups_sent(&self) -> usize {
        let cc = self.cc();
        let platform = cc.platform().unwrap();
        platform.hangups_sent()
    }

}

pub fn create_context(call_id: CallId, direction: CallDirection) -> TestContext {

    info!("create_cc_ext(): call_id: {}, direction: {}", call_id, direction);

    let call_config = CallConfig {
        call_id,
        recipient: "testing_recipient".to_string(),
        direction,
    };

    info!("test: creating observer");
    let cc_observer = Arc::new(Mutex::new(SimCallConnectionObserver::new(call_config.call_id)));

    info!("test: creating ccf");
    let cc_factory = call_connection_factory::create_call_connection_factory().unwrap();

    info!("test: creating cc");
    let cc = call_connection_factory::create_call_connection(&cc_factory, call_config, cc_observer.clone()).unwrap();

    TestContext {
        cc_factory,
        cc,
        cc_observer,
    }
}
