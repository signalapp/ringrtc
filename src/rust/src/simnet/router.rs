//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use crate::common::units::{DataRate, DataSize};
use crate::simnet::actor::{Actor, Stopper};
use rand::{distributions, distributions::Distribution, rngs::ThreadRng, thread_rng, Rng};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{atomic, atomic::AtomicU64, Arc},
    thread,
    time::Duration,
};

const OVERHEAD_IN_BYTES_IPV4: u64 = 20;

const OVERHEAD_IN_BYTES_IPV6: u64 = 40;
const OVERHEAD_IN_BYTES_UDP: u64 = 8;
#[allow(dead_code)]
const OVERHEAD_IN_BYTES_TCP: u64 = 20;

#[derive(Debug)]
pub struct Packet {
    pub source: SocketAddr,
    pub dest:   SocketAddr,
    pub data:   Vec<u8>,
}

// Just assume UDP for now.
impl Packet {
    fn size(&self) -> DataSize {
        DataSize::from_bytes(self.data.len() as u64)
    }

    fn overhead(&self) -> DataSize {
        let ip_overhead = match self.source.ip() {
            IpAddr::V4(_) => DataSize::from_bytes(OVERHEAD_IN_BYTES_IPV4),
            IpAddr::V6(_) => DataSize::from_bytes(OVERHEAD_IN_BYTES_IPV6),
        };
        let udp_overhead = DataSize::from_bytes(OVERHEAD_IN_BYTES_UDP);
        ip_overhead + udp_overhead
    }

    fn reliable(&self) -> bool {
        false
    }
}

// Send is needed so that we can move these between threads.
pub trait PacketReceiver: Send {
    fn receive_packet(&self, packet: Packet);
}

impl<F: Fn(Packet) + Send> PacketReceiver for F {
    fn receive_packet(&self, packet: Packet) {
        self(packet)
    }
}

#[derive(Clone)]
pub struct LinkConfig {
    // Could also be mean + std_dev
    // pub delay_mean:                Duration,
    // pub delay_std_dev:             Duration,
    pub delay_min:                 Duration,
    pub delay_max:                 Duration,
    pub loss_probabilty:           f64,
    // If a packet is lost, the probably of the next one being a loss
    pub repeated_loss_probability: f64,
    pub rate:                      DataRate,
    pub queue_size:                DataSize,
}

#[derive(Clone)]
pub struct Router {
    actor: Actor<RouterState>,
}

struct RouterState {
    send_link_by_ip:    HashMap<IpAddr, Link>,
    receive_link_by_ip: HashMap<IpAddr, Link>,
}

// Routes packets between the given interfaces with configurable links.
// The path of the packet goes like this:
// => Router.send_packet
// => Router.actor.state.send_link_by_ip[ip].send_packet
// => Link.actor.state.leaky_bucket
// => Router.receive_packet
// => Router.actor.state.receive_link_by_ip[ip].receive_packet
// => Link.actor.state.leaky_bucket
// => callback passed to Router.add_interface
// One could theoretically combine Routers for a larger, more complex graph.
impl Router {
    pub fn new(stopper: &Stopper) -> Self {
        Self {
            actor: Actor::new(stopper.clone(), move |_| RouterState {
                send_link_by_ip:    HashMap::new(),
                receive_link_by_ip: HashMap::new(),
            }),
        }
    }

    // Packets sent from the ip will behave according to send_config.
    // Packets received to the ip will behave according to the receive_config
    // and be routed to the given receiver.
    pub fn add_interface(
        &self,
        ip: IpAddr,
        send_config: LinkConfig,
        receive_config: LinkConfig,
        receiver: Box<dyn PacketReceiver>,
    ) {
        let router = Box::new(self.clone());
        let stopper = self.actor.stopper().clone();
        self.actor.send(move |state| {
            state
                .send_link_by_ip
                .insert(ip, Link::new(send_config, router, stopper.clone()));
            state
                .receive_link_by_ip
                .insert(ip, Link::new(receive_config, receiver, stopper));
        });
    }

    pub fn send_packet(&self, packet: Packet) {
        self.actor.send(move |state| {
            if let Some(send_link) = state.send_link_by_ip.get(&packet.source.ip()) {
                send_link.send_packet(packet);
            } else {
                debug!(
                    "Dropped send packet because the source IP ({:?}) was unknown.",
                    packet.source.ip()
                );
            }
        });
    }
}

impl PacketReceiver for Router {
    fn receive_packet(&self, packet: Packet) {
        self.actor.send(move |state| {
            if let Some(receive_link) = state.receive_link_by_ip.get(&packet.dest.ip()) {
                receive_link.send_packet(packet);
            } else {
                debug!(
                    "Dropped receive packet because the dest IP ({:?}) was unknown.",
                    packet.dest.ip()
                );
            }
        });
    }
}

struct Link {
    actor: Actor<LinkState>,
}

struct LinkState {
    config: LinkConfig,

    // A source of randomness for dropping and delaying.
    rng: ThreadRng,

    // State for calculating dropping and delaying
    // goes out side of the actor because it affects
    // if and when we will send to the actor.
    previous_packet_dropped: bool,

    #[allow(deprecated)]
    delay_distribution: distributions::uniform::Uniform<u64>,

    // We keep a clone of the actor in the link state
    // so we can schedule tasks based on the state.
    actor: Actor<LinkState>,

    leaky_bucket: LeakyBucket,
}

impl Link {
    pub fn new(config: LinkConfig, receiver: Box<dyn PacketReceiver>, stopper: Stopper) -> Self {
        // Could also be mean + std_dev
        // let delay_distribution = distributions::Normal::new(
        //     config.delay_mean.as_secs_f64(),
        //     config.delay_std_dev.as_secs_f64(),
        // );
        let delay_distribution = distributions::Uniform::from(
            (config.delay_min.as_millis() as u64)..(config.delay_max.as_millis() as u64),
        );
        let leaky_bucket = LeakyBucket::new(config.clone(), receiver, stopper.clone());
        Self {
            actor: Actor::new(stopper, move |actor| LinkState {
                actor,
                config,
                rng: thread_rng(),
                previous_packet_dropped: false,
                delay_distribution,
                leaky_bucket,
            }),
        }
    }

    fn send_packet(&self, packet: Packet) {
        self.actor.send(move |state| {
            let loss_probabilty = if state.previous_packet_dropped {
                state.config.repeated_loss_probability
            } else {
                state.config.loss_probabilty
            };
            if !packet.reliable() && state.rng.gen_bool(loss_probabilty) {
                println!(
                    "Dropped packet from {:?} to {:?} of size {} randomly (previous_packet_dropped={})",
                    packet.source, packet.dest, packet.size().as_bytes(), state.previous_packet_dropped
                );
                state.previous_packet_dropped = true;
                // Drop the packet
                return;
            }
            state.previous_packet_dropped = false;

            // Delay the packet
            let delay = Duration::from_millis(state.delay_distribution.sample(&mut state.rng));
            state
                .actor
                .send_delayed(delay, move |state| state.leaky_bucket.send_packet(packet));
        });
    }
}

struct LeakyBucket {
    config:      LinkConfig,
    actor:       Actor<LeakyBucketState>,
    // Shared with LeakyBucketState so we can
    // see the queued size on both sides.
    queued_size: Arc<AtomicU64>,
}

struct LeakyBucketState {
    queued_size: Arc<AtomicU64>,
    receiver:    Box<dyn PacketReceiver>,
}

impl LeakyBucket {
    pub fn new(config: LinkConfig, receiver: Box<dyn PacketReceiver>, stopper: Stopper) -> Self {
        let queued_size = Arc::new(AtomicU64::new(0));
        let queued_size_clone = queued_size.clone();
        Self {
            config,
            actor: Actor::new(stopper, move |_| LeakyBucketState {
                queued_size: queued_size_clone,
                receiver,
            }),
            queued_size,
        }
    }

    pub fn send_packet(&self, packet: Packet) {
        // Using the most strict ordering because perf doesn't matter,
        // but I'm not sure if that's the right choice.
        let ordering = atomic::Ordering::SeqCst;

        // TODO: Make more accurate overhead calculation.
        let overhead = packet.overhead();
        let packet_size_without_overhead = DataSize::from_bytes(packet.data.len() as u64);
        let packet_size_with_overhead = packet_size_without_overhead + overhead;
        let queued_size = DataSize::from_bytes(self.queued_size.load(ordering));
        let max_size = self.config.queue_size;
        let rate = self.config.rate;
        if (queued_size + packet_size_with_overhead) > max_size {
            println!(
                "Dropped packet (size: {} overhead: {}) from full queue (queued_size={}/{})",
                packet_size_without_overhead.as_bytes(),
                overhead.as_bytes(),
                queued_size.as_bytes(),
                self.config.queue_size.as_bytes()
            );
            return; // Drop the packet!
        }
        self.queued_size
            .fetch_add(packet_size_with_overhead.as_bytes(), ordering);
        self.actor.send(move |state| {
            state
                .queued_size
                .fetch_sub(packet_size_with_overhead.as_bytes(), ordering);

            // Simulates the time it takes to transmit a packet.
            // TODO: accumulate sleep amounts and only sleep when more
            // than some threshold for systems that have inprecise sleep.
            thread::sleep(packet_size_with_overhead / rate);

            state.receiver.receive_packet(packet);
        })
    }
}
