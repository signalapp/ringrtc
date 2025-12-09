//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    io,
    iter::{Cycle, StepBy},
    net::{SocketAddr, UdpSocket},
    thread,
};

use anyhow::anyhow;
use bitvec::{
    bits,
    prelude::{BitSlice, LocalBits, Lsb0},
};
use log::*;
use ringrtc::{
    common::Result,
    webrtc::{
        injectable_network::{self, InjectableNetwork},
        network::NetworkInterfaceType,
    },
};

pub struct DeterministicLoss {
    pre_delay: u8,
    ignore_last_n: u8,
    current_loss_count: u8,
    loss_map_iter: Cycle<StepBy<bitvec::slice::Iter<'static, usize, LocalBits>>>,
}

impl DeterministicLoss {
    pub fn new(loss_rate: u8, packet_size_ms: i32, pre_delay: u8) -> Result<Self> {
        if loss_rate > 50 || !loss_rate.is_multiple_of(5) {
            return Err(anyhow!(
                "Loss rate must be less than 50% and a multiple of 5"
            ));
        }

        // This loss map represents 1,024 decisions for a loss rate of 50%. From this, we can
        // support any loss rate < 50% whilst keeping them all _somewhat_ aligned.
        let loss_map: &'static BitSlice = bits![static
            0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1,
            0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1,
            0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 0,
            0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1,
            1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 1,
            1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1,
            0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0,
            0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 1,
            0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1,
            0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1,
            1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1,
            1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1,
            1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1,
            1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1,
            0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0,
            1, 0, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1,
            0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0,
            0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0,
            1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1,
            1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1,
            0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1,
            0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1,
            1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1,
            1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 1,
            1, 1, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1,
            1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1,
            0, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 1,
            0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1,
            1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1,
            1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0,
            0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0,
            0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0,
            0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0,
            1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1,
            0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
            0, 1, 0, 1, 1, 1, 1, 1, 0,
        ];

        // For different loss rates (< 50), we will ignore one or more for every 10 loss signals,
        // at the end of every 10 losses.
        let ignore_last_n = 10 - loss_rate / 5;

        // Iterate through the loss map. Based on the packet time, skip every n losses
        // to help keep different packet times _somewhat_ aligned.
        let packet_time_step = (packet_size_ms / 20) as usize;
        let loss_map_iter = loss_map.iter().step_by(packet_time_step).cycle();

        Ok(Self {
            pre_delay,
            ignore_last_n,
            current_loss_count: 0,
            loss_map_iter,
        })
    }

    pub fn next_is_loss(&mut self) -> bool {
        if self.pre_delay > 0 {
            self.pre_delay -= 1;
            return false;
        }

        if *self.loss_map_iter.next().expect("iterator has next()") {
            // The packet should be 'lost' as per the loss map.
            let ignore_loss = (10 - self.current_loss_count) <= self.ignore_last_n;

            self.current_loss_count += 1;
            if self.current_loss_count == 10 {
                self.current_loss_count = 0;
            }

            !ignore_loss
        } else {
            false
        }
    }
}

/// Wrapper around InjectableNetwork that allows adding DeterministicLoss
pub struct DeterministicLossNetwork {
    injectable_network: InjectableNetwork,
    socket: Option<UdpSocket>,
}

impl DeterministicLossNetwork {
    pub fn new(injectable_network: InjectableNetwork) -> Self {
        Self {
            injectable_network,
            socket: None,
        }
    }

    pub fn add_deterministic_loss(&mut self, ip: String, loss_rate: u8, packet_size_ms: i32) {
        let ip = ip.parse().expect("parse IP address");
        let mut deterministic_loss = DeterministicLoss::new(loss_rate, packet_size_ms, 10)
            .expect("parameters should be valid");

        let network = self.injectable_network.clone();

        // The injectable network currently makes an assumption that socket ports will
        // start from 2001. We also make that assumption, and we are only going to give
        // one interface and no external servers for deterministic loss testing, so only
        // one ip/port should end up being used by each client for these tests.
        let local_socket_addr = SocketAddr::new(ip, 2001);
        let socket = UdpSocket::bind(local_socket_addr).expect("bind to address");
        let socket_as_sender = socket.try_clone().expect("clone the socket");
        // Connect the Injectable Network's send function to the UdpSocket.
        network.set_sender(Box::new(move |packet: injectable_network::Packet| {
            if let Err(err) = socket_as_sender.send_to(&packet.data, packet.dest) {
                error!("Error: Sending packet to {}: {}", packet.dest, err);
            }
        }));

        // Adding it to the network causes the PeerConnections to learn about it through
        // the NetworkMonitor. For our tests, we just assume "wifi" for simplicity.
        network.add_interface("wifi", NetworkInterfaceType::Wifi, ip, 1);

        let socket_as_receiver = socket.try_clone().expect("clone the socket");
        self.socket = Some(socket);

        // Spawn a thread to maintain a receive loop on the socket. This waits for incoming
        // UDP packets until the network is stopped and the socket is set to non-blocking
        // mode. Then it will exit the loop and thread on a "Would Block" error.
        thread::spawn(move || {
            // 2K should be enough for any UDP packet.
            let mut buf = [0; 2048];
            loop {
                match socket_as_receiver.recv_from(&mut buf) {
                    Ok((number_of_bytes, src_addr)) => {
                        if number_of_bytes > 1200 {
                            warn!(
                                "Warning: {} bytes received for one packet!",
                                number_of_bytes
                            );
                        }

                        if !deterministic_loss.next_is_loss() {
                            network.receive_udp(injectable_network::Packet {
                                source: src_addr,
                                dest: local_socket_addr,
                                data: buf[..number_of_bytes].to_vec(),
                            });
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(err) => panic!("recv_from failed: {err}"),
                }
            }
        });
    }

    pub fn stop_network(&self) {
        if let Some(socket) = &self.socket {
            socket
                .set_nonblocking(true)
                .expect("set socket to non-blocking");
        }
    }
}
