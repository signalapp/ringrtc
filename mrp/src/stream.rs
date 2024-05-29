//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! MRP - Modest Reliable Protocol
//! A simple protocol modeled on TCP that is transport agnostic. Can be used
//! on top of unreliable connections like UDP. It is modest because it
//! does not establish connections, negotiate buffer sizes, perform
//! congestion control, QOS, support streaming data, wraparound sequences, etc.
//! It focuses on being generically applicable, reliable, and aggressive on
//! retransmitting. Meant for low volumes of packets. Generic means you can
//! change how data is sent on every attempt

use super::window::{BufferWindow, WindowError};
use std::{fmt::Debug, time::Instant};

#[derive(PartialEq, Debug, Default, Clone)]
pub struct MrpHeader {
    /// SENDER -> RECEIVER
    /// sequence number in window
    pub seqnum: Option<u64>,
    /// RECEIVER -> SENDER
    /// The next expected SEQ_NUM
    pub ack_num: Option<u64>,
}

impl MrpHeader {
    pub fn new(seqnum: Option<u64>, ack_num: Option<u64>) -> Self {
        Self { seqnum, ack_num }
    }
}

/// Convenience struct for associating a Header with arbitrary data
#[derive(PartialEq, Debug, Clone)]
pub struct PacketWrapper<Data: Clone + Debug>(pub MrpHeader, pub Data);

/// Tracks timeout, attempts, and whether to transmit packet at next chance
/// [MrpStream] exposes it in Buffer type
#[derive(Debug)]
pub struct PendingPacket<Data: Clone> {
    pub packet: Data,
    next_send_at: Instant,
    try_count: u16,
    transmit: bool,
}

impl<Data> PendingPacket<Data>
where
    Data: Clone,
{
    fn should_transmit(&self, now: Instant) -> bool {
        self.transmit || now >= self.next_send_at
    }
}

/// Implements the sender and receiver state machine.
/// Buffers the sender and receiver windows.
#[derive(Debug)]
pub struct MrpStream<SendData, ReceiveData>
where
    SendData: Clone + Debug,
    ReceiveData: Clone + Debug,
{
    /// Tracks whether need to send an ACK
    should_ack: bool,
    /// Packets that been sent but not yet acked or dropped.
    send_buffer: BufferWindow<PendingPacket<SendData>>,
    /// Packets that have been received out of order
    receive_buffer: BufferWindow<ReceiveData>,
}

#[derive(thiserror::Error, PartialEq, Eq, Debug, Clone)]
pub enum MrpReceiveError {
    #[error("Receive Window is full, cannot accept packet with seqnum")]
    ReceiveWindowFull(u64),
}

#[derive(thiserror::Error, Debug)]
pub enum MrpSendError {
    #[error("Send Window is full")]
    SendWindowFull,
    #[error("Inner send failed: {0:?}")]
    InnerSendFailed(anyhow::Error),
}

impl<SendData, ReceiveData> MrpStream<SendData, ReceiveData>
where
    SendData: Clone + Debug,
    ReceiveData: Clone + Debug,
{
    const INITIAL_SEQNUM: u64 = 1;
    const INITIAL_ACKNUM: u64 = 1;

    pub fn new(max_window_size: usize) -> Self {
        Self {
            should_ack: false,
            send_buffer: BufferWindow::<PendingPacket<SendData>>::new(
                max_window_size,
                Self::INITIAL_SEQNUM,
            ),
            receive_buffer: BufferWindow::<ReceiveData>::new(max_window_size, Self::INITIAL_ACKNUM),
        }
    }

    pub fn ack_seqnum(&self) -> u64 {
        self.receive_buffer.left_bounds()
    }

    /// seqnum for the next send packet
    fn next_seqnum(&self) -> u64 {
        self.send_buffer.max_seen_seqnum() + 1
    }

    /// Preps a packet and sends using provided function. If the send window is full, returns error.
    /// If send_data fails, then does not buffer the packet and caller must try again.
    /// We do not piggy_back ACKs on these packets. See [try_send_ack].
    ///
    /// # Arguments
    /// * `packet` - this function does not check if header is empty, so outside mutation affects packet
    /// * `send_data` - sends packet and returns the timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// # use mrp::*;
    /// # use std::time::{Duration, Instant};
    /// type Packet = PacketWrapper<i32>;
    /// let mut stream = MrpStream::<Packet, Packet>::new(8);
    /// let mut inbox = Vec::with_capacity(8);
    ///
    /// for i in 1..=9 {
    ///     let mut pkt = PacketWrapper(MrpHeader::default(), i);
    ///     let result = stream.try_send(|header| {
    ///         pkt.0 = header;
    ///         inbox.push(pkt.clone());
    ///         let timeout = Instant::now() + Duration::from_millis(5_000);
    ///         Ok((pkt, timeout))
    ///     });
    ///
    ///     if let Err(err) = result {
    ///         assert_eq!(9, i);
    ///         assert!(matches!(err, MrpSendError::SendWindowFull));
    ///     }
    /// }
    ///
    /// for packet in inbox {
    ///     println!("{:?}", packet);
    /// }
    /// ```
    pub fn try_send(
        &mut self,
        send_data: impl FnOnce(MrpHeader) -> anyhow::Result<(SendData, Instant)>,
    ) -> std::result::Result<(), MrpSendError> {
        if self.send_buffer.is_full() {
            return Err(MrpSendError::SendWindowFull);
        }

        let header = MrpHeader {
            seqnum: Some(self.next_seqnum()),
            ..Default::default()
        };
        match send_data(header) {
            Ok((packet, timeout)) => {
                self.send_buffer
                    .put(
                        self.next_seqnum(),
                        PendingPacket {
                            packet,
                            next_send_at: timeout,
                            try_count: 1,
                            transmit: false,
                        },
                    )
                    .expect("buffer should not have been full");
                Ok(())
            }
            Err(e) => Err(MrpSendError::InnerSendFailed(e)),
        }
    }

    /// Method meant to be polled. Sends ACK. Caller is responsible for providing ACK.
    ///
    /// # Arguments
    /// * `send_ack` - function with lambda arg that sets the header, then sends ack. Called at most one time.
    ///
    /// # Examples
    ///
    /// ```
    /// # use mrp::*;
    /// # use std::sync::mpsc::{self, Sender, Receiver};
    /// # use std::thread;
    /// # use std::time::{Duration, Instant};
    /// type Packet = PacketWrapper<String>;
    /// let ack = || PacketWrapper(MrpHeader::default(), "".to_string());
    /// let (to_alice, alice_inbox) : (Sender<Packet>, Receiver<Packet>) = mpsc::channel();
    /// let (to_bob, bob_inbox) : (Sender<Packet>, Receiver<Packet>) = mpsc::channel();
    /// let mut alice = MrpStream::<Packet, Packet>::new(8);
    /// let mut bob = MrpStream::<Packet, Packet>::new(8);
    /// let tick = Duration::from_millis(10);
    ///
    /// thread::spawn(move ||  {
    ///     let mut recv_count = 0;
    ///     while recv_count < 10 {
    ///         if let Ok(pkt) = alice_inbox.recv() {
    ///             let ready = alice.receive(&pkt.0.clone(), pkt).unwrap();
    ///             recv_count += ready.len();
    ///
    ///             alice.try_send_ack(
    ///                 |header| {
    ///                     let mut a = ack();
    ///                     a.0 = header;
    ///                     to_bob.send(a)?;
    ///                     Ok(())
    ///                 },
    ///             ).expect("ack succeeds");
    ///         } else {
    ///            break;
    ///         }
    ///     }
    /// });
    ///
    /// ```
    pub fn try_send_ack(
        &mut self,
        mut send_ack: impl FnMut(MrpHeader) -> anyhow::Result<()>,
    ) -> std::result::Result<Option<u64>, MrpSendError> {
        if self.should_ack {
            let mut header = MrpHeader::default();
            if self.should_ack {
                header.ack_num = Some(self.ack_seqnum());
            }

            match send_ack(header) {
                Ok(_) => {
                    self.should_ack = false;
                    Ok(Some(self.ack_seqnum()))
                }
                Err(e) => Err(MrpSendError::InnerSendFailed(e)),
            }
        } else {
            Ok(None)
        }
    }

    /// Checks the send window and retransmits pending packets that have timed out
    /// * `send_data` - sends packet and returns the timeout. may be called multiple times
    ///
    pub fn try_resend(
        &mut self,
        now: Instant,
        mut send_data: impl FnMut(&SendData) -> anyhow::Result<Instant>,
    ) -> std::result::Result<(), MrpSendError> {
        for seqnum in self.send_buffer.left_bounds()..=self.send_buffer.max_seen_seqnum() {
            if let Some(ppkt) = self.send_buffer.get_mut(seqnum) {
                if ppkt.should_transmit(now) {
                    match send_data(&ppkt.packet) {
                        Ok(next_send_at) => {
                            ppkt.next_send_at = next_send_at;
                            ppkt.try_count += 1;
                        }
                        Err(e) => {
                            return Err(MrpSendError::InnerSendFailed(e));
                        }
                    };
                }
            }
        }

        Ok(())
    }

    /// Receives a packet. Treats it as either an ACK or Data Packet.
    /// We prevent piggybacking both in one packet for now.
    ///
    /// returns packets ready for processing
    pub fn receive(
        &mut self,
        header: &MrpHeader,
        packet: ReceiveData,
    ) -> std::result::Result<Vec<ReceiveData>, MrpReceiveError> {
        if let Some(ack_num) = header.ack_num {
            self.update_send_window(ack_num)?;
            Ok(vec![])
        } else if header.seqnum.is_some() {
            self.update_receiver_window(header, packet)
        } else {
            // Not a valid MRP header! Ignore, immediately passback for processing
            Ok(vec![packet])
        }
    }

    fn update_send_window(
        &mut self,
        received_ack_num: u64,
    ) -> std::result::Result<(), MrpReceiveError> {
        // Peer sent impossible ACK, which in TCP would cause a reset
        // Currently we do not support resets, so we ignore this case
        if received_ack_num > self.next_seqnum() {
            log::warn!(
                "Received invalid acknum `{}`, would cause reset to seqnum {}",
                received_ack_num,
                self.send_buffer.left_bounds()
            );
            return Ok(());
        }
        // Assuming no wrapping, this must be an old ACK since we only ever increase
        // seqnums. So we ignore
        if received_ack_num < self.send_buffer.left_bounds() {
            return Ok(());
        }
        if received_ack_num >= self.send_buffer.left_bounds() {
            let old = received_ack_num - self.send_buffer.left_bounds();
            self.send_buffer.drop_front(old as usize);
        }

        Ok(())
    }

    fn update_receiver_window(
        &mut self,
        header: &MrpHeader,
        packet: ReceiveData,
    ) -> std::result::Result<Vec<ReceiveData>, MrpReceiveError> {
        if let Some(seqnum) = header.seqnum {
            return match self.receive_buffer.put(seqnum, packet) {
                // we already received packet previously, so ack again
                Err(WindowError::BeforeWindow) => {
                    self.should_ack = true;
                    Ok(vec![])
                }
                Err(WindowError::AfterWindow) => Err(MrpReceiveError::ReceiveWindowFull(seqnum)),
                Ok(_) => {
                    if let Some((_, ready_packets)) = self.receive_buffer.drain_front() {
                        self.should_ack = true;
                        Ok(ready_packets)
                    } else {
                        Ok(vec![])
                    }
                }
            };
        }

        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use rand::{
        distributions::{DistIter, Uniform},
        rngs::StdRng,
        Rng, SeedableRng,
    };
    use std::sync::OnceLock;

    use super::*;
    use std::{
        cell::RefCell,
        collections::BinaryHeap,
        rc::Rc,
        sync::mpsc::{self, Receiver, Sender, TryRecvError},
        thread,
        time::{Duration, Instant},
    };

    type Packet = PacketWrapper<u64>;

    fn packet(data: u64) -> Packet {
        PacketWrapper(
            MrpHeader {
                ..MrpHeader::default()
            },
            data,
        )
    }

    fn ack(data: u64) -> Packet {
        PacketWrapper(MrpHeader::default(), data)
    }

    type PacketSchedule = Vec<Event>;

    static BASE_TIME: OnceLock<Instant> = OnceLock::new();

    fn base_time() -> Instant {
        *BASE_TIME.get_or_init(|| Instant::now())
    }

    fn instant_of(offset: u64) -> Instant {
        base_time() + Duration::from_millis(offset)
    }

    #[derive(Clone, Debug)]
    struct Event {
        send_at: Instant,
        recv_at: Instant,
        pkt: Packet,
    }

    impl Event {
        fn schedule_of(timestamps: Vec<(u64, u64)>) -> PacketSchedule {
            let mut seqnum = 0;
            let mut next = || {
                seqnum += 1;
                packet(seqnum)
            };
            timestamps
                .iter()
                .map(|tsp| Event {
                    send_at: instant_of(tsp.0),
                    recv_at: instant_of(tsp.1),
                    pkt: next(),
                })
                .collect()
        }
    }

    struct TestCase {
        alice: MrpStream<Packet, Packet>,
        bob: MrpStream<Packet, Packet>,
        alice_schedule: PacketSchedule,
        bob_schedule: PacketSchedule,
        alice_inbox: RefCell<Vec<Event>>,
        bob_inbox: RefCell<Vec<Event>>,
        current_time: Instant,
        last_alice_sent: Instant,
        last_bob_sent: Instant,
    }

    impl TestCase {
        fn new(
            buffer_size: usize,
            alice_schedule: PacketSchedule,
            bob_schedule: PacketSchedule,
        ) -> Self {
            TestCase {
                alice: MrpStream::new(buffer_size),
                bob: MrpStream::new(buffer_size),
                alice_inbox: RefCell::new(vec![]),
                bob_inbox: RefCell::new(vec![]),
                alice_schedule,
                bob_schedule,
                current_time: base_time(),
                last_alice_sent: base_time(),
                last_bob_sent: base_time(),
            }
        }

        fn run_to(&mut self, now: u64) -> &mut Self {
            self.current_time = instant_of(now);
            self
        }

        fn send_from_alice(&mut self, timeout: u64) -> Vec<std::result::Result<(), MrpSendError>> {
            let results = Self::send_inner(
                self.current_time,
                self.last_alice_sent,
                instant_of(timeout),
                &mut self.alice_schedule,
                &mut self.alice,
                &mut self.bob_inbox,
            );
            self.last_alice_sent = self.current_time;
            results
        }

        fn send_from_bob(&mut self, timeout: u64) -> Vec<std::result::Result<(), MrpSendError>> {
            let results = Self::send_inner(
                self.current_time,
                self.last_bob_sent,
                instant_of(timeout),
                &mut self.bob_schedule,
                &mut self.bob,
                &mut self.alice_inbox,
            );
            self.last_bob_sent = self.current_time;
            results
        }

        fn send_inner(
            now: Instant,
            last_sent: Instant,
            timeout: Instant,
            schedule: &mut PacketSchedule,
            sender: &mut MrpStream<Packet, Packet>,
            inbox: &mut RefCell<Vec<Event>>,
        ) -> Vec<std::result::Result<(), MrpSendError>> {
            schedule
                .iter_mut()
                .filter(|event| event.send_at <= now && event.send_at > last_sent)
                .map(|event| {
                    sender.try_send(|header| {
                        // update event's packet to capture updated header
                        event.pkt.0 = header;
                        inbox.get_mut().push(event.clone());
                        Ok((event.pkt.clone(), timeout))
                    })
                })
                .collect()
        }

        fn recv_for_alice(&mut self) -> Vec<u64> {
            Self::recv_inner(self.current_time, &mut self.alice_inbox, &mut self.alice)
        }

        fn recv_for_bob(&mut self) -> Vec<u64> {
            Self::recv_inner(self.current_time, &mut self.bob_inbox, &mut self.bob)
        }

        fn recv_inner(
            now: Instant,
            inbox: &mut RefCell<Vec<Event>>,
            receiver: &mut MrpStream<Packet, Packet>,
        ) -> Vec<u64> {
            let mut received = vec![];
            inbox.get_mut().retain(|event| {
                if event.recv_at > now {
                    return true;
                }
                if let Ok(v) = receiver.receive(&event.pkt.0, event.pkt.to_owned()) {
                    v.iter().for_each(|p| received.push(p.0.seqnum.unwrap()))
                }
                false
            });
            received
        }

        fn updates_from_alice(&mut self) -> (Option<u64>, Vec<u64>) {
            Self::update_inner(self.current_time, &mut self.alice, &mut self.bob_inbox)
        }

        fn updates_from_bob(&mut self) -> (Option<u64>, Vec<u64>) {
            Self::update_inner(self.current_time, &mut self.bob, &mut self.alice_inbox)
        }

        fn update_inner(
            now: Instant,
            sender: &mut MrpStream<Packet, Packet>,
            receiver_inbox: &mut RefCell<Vec<Event>>,
        ) -> (Option<u64>, Vec<u64>) {
            let ack_result = sender.try_send_ack(|header| {
                let mut a = ack(0);
                a.0 = header;
                receiver_inbox.borrow_mut().push(Event {
                    send_at: base_time(),
                    pkt: a,
                    recv_at: now,
                });
                Ok(())
            });

            let mut resent = vec![];
            let _ = sender.try_resend(now, |pkt| {
                resent.push(pkt.0.seqnum.unwrap());
                receiver_inbox.borrow_mut().push(Event {
                    send_at: base_time(),
                    pkt: pkt.clone(),
                    recv_at: now,
                });
                Ok(instant_of(NEVER_TIMEOUT))
            });

            (ack_result.unwrap(), resent)
        }
    }

    macro_rules! resent {
        ($($x:expr),* $(,)?) => {
            (None, vec![$($x),*])
        }
    }

    fn assert_sent(send_result: Vec<Result<(), MrpSendError>>, num_sent: usize) {
        assert_eq!(send_result.len(), num_sent);
        assert!(send_result.iter().all(|e| e.is_ok()));
    }

    fn acked(ack_num: u64) -> (Option<u64>, Vec<u64>) {
        (Some(ack_num), vec![])
    }

    static NO_UPDATES: (Option<u64>, Vec<u64>) = (None, vec![]);
    static NEVER_TIMEOUT: u64 = 10000000;
    static NO_RECEIVES: Vec<u64> = vec![];

    #[test]
    fn test_ping_pong_one_direction() {
        // Every tick, Alice sends a packet, Bob receives it and acks it
        // and Alice receives the ack
        let mut tc = TestCase::new(
            16,
            Event::schedule_of((1..50).map(|i| (i, i)).collect()),
            Event::schedule_of(vec![]),
        );

        for ts in 1..50 {
            tc.run_to(ts);
            assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 1);
            assert_sent(tc.send_from_bob(NEVER_TIMEOUT), 0);
            assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
            assert_eq!(tc.recv_for_bob(), &[ts]);
            assert_eq!(tc.updates_from_alice(), NO_UPDATES);
            assert_eq!(tc.updates_from_bob(), acked(ts + 1));
        }
    }

    #[test]
    fn test_ping_pong_two_directions() {
        // Both Bob and Alice send, receive, ack, and receive ack in the same tick
        let mut tc = TestCase::new(
            16,
            Event::schedule_of((1..50).map(|i| (i, i)).collect()),
            Event::schedule_of((1..50).map(|i| (i, i)).collect()),
        );

        for ts in 1..50 {
            tc.run_to(ts);
            assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 1);
            assert_sent(tc.send_from_bob(NEVER_TIMEOUT), 1);
            assert_eq!(tc.recv_for_alice(), &[ts]);
            assert_eq!(tc.recv_for_bob(), &[ts]);
            assert_eq!(tc.updates_from_alice(), acked(ts + 1));
            assert_eq!(tc.updates_from_bob(), acked(ts + 1));
        }
    }

    #[test]
    fn test_out_of_order_buffering() {
        // Alice and Bob send a packet every tick. The packets are organized into
        // sets of 10. Set X's Packet 1 is always delayed 10 ticks. Packets
        // 2-10 are delayed such that they arrive at or before packet 1 arrives.
        // Therefore, every 10 ticks, both Alice and Bob should produce a set of 10
        // packets in sequence on receive
        let rng = Rc::new(RefCell::new(rand::thread_rng()));
        let event = |ts| {
            let delay = if ts % 10 == 0 {
                10
            } else {
                rng.borrow_mut().gen_range(0..(10 - (ts % 10)))
            };
            (ts, ts + delay)
        };
        let mut tc = TestCase::new(
            16,
            Event::schedule_of((10..=60).map(event.clone()).collect()),
            Event::schedule_of((10..=60).map(event).collect()),
        );

        let mut pending_seqnum = 1;
        for ts in 10..=60 {
            tc.run_to(ts);
            assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 1);
            assert_sent(tc.send_from_bob(NEVER_TIMEOUT), 1);

            if ts > 10 && ts % 10 == 0 {
                let expected: Vec<_> = (pending_seqnum..(pending_seqnum + 10)).collect();
                pending_seqnum += 10;
                assert_eq!(tc.recv_for_alice(), expected);
                assert_eq!(tc.recv_for_bob(), expected);
                assert_eq!(tc.updates_from_alice(), acked(pending_seqnum));
                assert_eq!(tc.updates_from_bob(), acked(pending_seqnum));
            } else {
                assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
                assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
                assert_eq!(tc.updates_from_alice(), NO_UPDATES);
                assert_eq!(tc.updates_from_bob(), NO_UPDATES);
            }
        }
    }

    #[test]
    fn test_varied_buffering() {
        // Alice sends packets that have various delay patterns.
        // Bob sends packets with similar pattern to
        // [test_out_of_order_buffering], receiving 9 every 10th tick
        let mut tc = TestCase::new(
            16,
            Event::schedule_of(vec![
                (1, 1),
                (2, 2),
                (3, 3),
                (4, 4),
                (5, 5), // Packets 1 - 5, no blocks
                (5, 7),
                (5, 9),
                (6, 6),
                (6, 6),
                (7, 8), // Packet 6 is returned at ts=7, Packets 7-11 arrive at ts=9
                (9, 9),
                (10, 10),
                (10, 10), // Packets 12 - 13, no blocks
                (10, 12),
                (11, 11),
                (11, 13),
                (11, 12), // Packets 14-15 at ts=12, Packets 16-17 at ts = 13
            ]),
            Event::schedule_of((1..=50).map(|i| (i, i + (10 - (i % 10)))).collect()),
        );

        for ts in 1..=5 {
            tc.run_to(ts);
            if ts == 5 {
                assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 3);
            } else {
                assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 1);
            }
            assert_sent(tc.send_from_bob(NEVER_TIMEOUT), 1);
            assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
            assert_eq!(tc.recv_for_bob(), &[ts]);
            assert_eq!(tc.updates_from_alice(), NO_UPDATES);
            assert_eq!(tc.updates_from_bob(), acked(ts + 1));
        }

        for ts in 6..=9 {
            tc.run_to(ts);
            match ts {
                6 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 2);
                }
                7 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 1);
                    assert_eq!(tc.recv_for_bob(), &[6]);
                    assert_eq!(tc.updates_from_bob(), acked(7));
                }
                8 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
                    assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
                    assert_eq!(tc.updates_from_bob(), NO_UPDATES);
                }
                9 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 1);
                    assert_eq!(tc.recv_for_bob(), &[7, 8, 9, 10, 11]);
                    assert_eq!(tc.updates_from_bob(), acked(12));
                }
                _ => {
                    panic!("Rust should infer this is not possible -_-");
                }
            };

            assert_sent(tc.send_from_bob(NEVER_TIMEOUT), 1);
            assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
            assert_eq!(tc.updates_from_alice(), NO_UPDATES);
        }

        for ts in 10..=13 {
            tc.run_to(ts);
            assert_sent(tc.send_from_bob(NEVER_TIMEOUT), 1);

            match ts {
                10 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 3);
                    let received: Vec<u64> = (1..10).collect();
                    assert_eq!(tc.recv_for_alice(), received);
                    assert_eq!(tc.recv_for_bob(), &[12, 13]);
                    assert_eq!(tc.updates_from_alice(), acked(10));
                    assert_eq!(tc.updates_from_bob(), acked(14));
                }
                11 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 3);
                    assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
                    assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
                    assert_eq!(tc.updates_from_alice(), NO_UPDATES);
                    assert_eq!(tc.updates_from_bob(), NO_UPDATES);
                }
                12 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
                    assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
                    assert_eq!(tc.recv_for_bob(), &[14, 15]);
                    assert_eq!(tc.updates_from_alice(), NO_UPDATES);
                    assert_eq!(tc.updates_from_bob(), acked(16));
                }
                13 => {
                    assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
                    assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
                    assert_eq!(tc.recv_for_bob(), &[16, 17]);
                    assert_eq!(tc.updates_from_alice(), NO_UPDATES);
                    assert_eq!(tc.updates_from_bob(), acked(18));
                }
                _ => {
                    panic!("Rust should infer this is not possible -_-");
                }
            }
        }

        for ts in 14..50 {
            tc.run_to(ts);
            assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
            assert_sent(tc.send_from_bob(NEVER_TIMEOUT), 1);
            if ts % 10 == 0 {
                let received: Vec<_> = ((ts - 10)..ts).collect();
                assert_eq!(tc.recv_for_alice(), received);
                assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
                assert_eq!(tc.updates_from_alice(), acked(ts));
                assert_eq!(tc.updates_from_bob(), NO_UPDATES);
            } else {
                assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
                assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
                assert_eq!(tc.updates_from_alice(), NO_UPDATES);
                assert_eq!(tc.updates_from_bob(), NO_UPDATES);
            }
        }
    }

    #[test]
    fn test_timeouts() {
        // Alice sends packets with timeouts that cause retransmissions.
        // Retransmissions will instantly succeed (same tick).
        let mut tc = TestCase::new(
            16,
            Event::schedule_of(vec![
                // Packets 1-7: Test head of line blocking. Packet 4 is resent at t=10,
                // so Packets 4-6 are returned at t=10 resulting in ack(7) at t=10, ack(8) at t=11
                (1, 2),
                (2, 3),
                (3, 3),
                (4, u64::MAX),
                (5, 5),
                (5, 6),
                (8, 11),
                // Packet 8: Test duplicate sends do not result in duplicate receives, and are reacked
                // Timeout Packet 8 at t=13, resulting in a retransmission + ack.
                // Original Packet 8 arrives at t=15 and is acked again
                (12, 15),
                // Packets 9-11: Test that duplicate sends get latest acknum
                // Timeout packet 9 at t=19. Retransmission results in ack(12) for
                // packets 8-10. Original packet 8 arrives at t=21, resulting in
                // resending ack(12)
                (16, 21),
                (17, 17),
                (18, 19),
            ]),
            Event::schedule_of(vec![]),
        );

        // Test head of line blocking
        // Packets 1-3 sent and received
        tc.run_to(3);
        assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 3);
        assert_eq!(tc.recv_for_bob(), &[1, 2, 3]);
        assert_eq!(tc.updates_from_bob(), acked(4));
        // Packet 4 sent and not received
        tc.run_to(4);
        let timeout = 10;
        assert_sent(tc.send_from_alice(timeout), 1);
        assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
        assert_eq!(tc.updates_from_alice(), NO_UPDATES);
        assert_eq!(tc.updates_from_bob(), NO_UPDATES);
        // Packets 5-7 sent, no receives
        tc.run_to(9);
        assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 3);
        assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
        assert_eq!(tc.updates_from_alice(), NO_UPDATES);
        assert_eq!(tc.updates_from_bob(), NO_UPDATES);
        // Resend packet 4, bob receives 4-6, alice gets ack=7
        tc.run_to(10);
        assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
        assert_eq!(tc.updates_from_alice(), resent![4]);
        assert_eq!(tc.recv_for_bob(), &[4, 5, 6]);
        assert_eq!(tc.updates_from_bob(), acked(7));
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
        // Bob receives packet 7
        tc.run_to(11);
        assert_eq!(tc.recv_for_bob(), &[7]);
        assert_eq!(tc.updates_from_bob(), acked(8));
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
        assert_eq!(tc.updates_from_alice(), NO_UPDATES);

        // Test duplicate sends do not result in duplicate receives, and are reacked
        // Send Packet 8
        tc.run_to(12);
        let timeout = 14;
        assert_sent(tc.send_from_alice(timeout), 1);
        assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
        assert_eq!(tc.updates_from_bob(), NO_UPDATES);
        // Resend packet 8, bob receives 8, alice gets ack=8
        tc.run_to(14);
        assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
        assert_eq!(tc.updates_from_alice(), resent![8]);
        assert_eq!(tc.recv_for_bob(), &[8]);
        assert_eq!(tc.updates_from_bob(), acked(9));
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
        // Bob receives delayed packet 8, sends same ack
        tc.run_to(15);
        assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
        assert_eq!(tc.updates_from_alice(), NO_UPDATES);
        assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
        assert_eq!(tc.updates_from_bob(), acked(9));
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);

        // Test duplicate sends get latest ack
        // Send Packet 9
        tc.run_to(16);
        let timeout = 20;
        assert_sent(tc.send_from_alice(timeout), 1);
        assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
        assert_eq!(tc.updates_from_alice(), NO_UPDATES);
        assert_eq!(tc.updates_from_bob(), NO_UPDATES);
        // Send Packet 10 & 11, 9 blocks
        tc.run_to(19);
        assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 2);
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
        assert_eq!(tc.updates_from_bob(), NO_UPDATES);
        // Resend Packet 9, bob receives 9-11
        tc.run_to(20);
        assert_sent(tc.send_from_alice(NEVER_TIMEOUT), 0);
        assert_eq!(tc.updates_from_alice(), resent![9]);
        assert_eq!(tc.recv_for_bob(), &[9, 10, 11]);
        assert_eq!(tc.updates_from_bob(), acked(12));
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
        // Original Packet 9 arrives, no duplicate receives, sends latest acknum
        tc.run_to(21);
        assert_eq!(tc.recv_for_bob(), NO_RECEIVES);
        assert_eq!(tc.updates_from_bob(), acked(12));
        assert_eq!(tc.recv_for_alice(), NO_RECEIVES);
    }

    #[test]
    fn transfer_inorder() {
        transfer_test(
            1_000,
            Duration::from_millis(2),
            Duration::from_millis(u64::MAX),
            Duration::from_millis(0),
            Duration::from_millis(1),
        );
    }

    #[test]
    fn transfer_outoforder_notimeouts() {
        transfer_test(
            1_000,
            Duration::from_millis(2),
            Duration::from_millis(u64::MAX),
            Duration::from_millis(5),
            Duration::from_millis(35),
        );
    }

    #[test]
    fn transfer_outoforder_timeouts_noresets() {
        transfer_test(
            100,
            Duration::from_millis(2),
            Duration::from_millis(20),
            Duration::from_millis(0),
            Duration::from_millis(21),
        );
    }

    fn transfer_test(
        num_packets: usize,
        send_pace: Duration,
        timeout: Duration,
        delay_min: Duration,
        delay_max: Duration,
    ) {
        let alice = MrpStream::new(64);
        let bob = MrpStream::new(64);
        let (to_alice, alice_inbox) = mpsc::channel();
        let (to_bob, bob_inbox) = mpsc::channel();
        let alice_receiver = DelayReceiver::new(
            alice_inbox,
            delay_min.as_millis() as u64,
            delay_max.as_millis() as u64,
        );
        let bob_receiver = DelayReceiver::new(bob_inbox, 10, 11);

        let alice_endpoint = spawn_endpoint(
            "alice",
            num_packets,
            alice,
            to_bob,
            alice_receiver,
            send_pace.clone(),
            timeout,
        );
        let bob_endpoint = spawn_endpoint(
            "bob",
            num_packets,
            bob,
            to_alice,
            bob_receiver,
            send_pace.clone(),
            timeout,
        );

        let alice_results: Vec<_> = alice_endpoint
            .join()
            .unwrap()
            .iter()
            .map(|pkt| pkt.1)
            .collect();
        let bob_results: Vec<_> = bob_endpoint
            .join()
            .unwrap()
            .iter()
            .map(|pkt| pkt.1)
            .collect();

        assert_eq!(alice_results, bob_results);
    }

    fn spawn_endpoint(
        tag: &str,
        num_packets: usize,
        mut stream: MrpStream<Packet, Packet>,
        sender: Sender<Packet>,
        mut receiver: DelayReceiver<Packet>,
        pace: Duration,
        timeout: Duration,
    ) -> std::thread::JoinHandle<Vec<Packet>> {
        let tag = tag.to_string();
        thread::spawn(move || {
            let goal = stream.ack_seqnum() + num_packets as u64;
            let mut sent = 0;
            let mut resend_count = 0;
            let mut received = Vec::with_capacity(num_packets);
            let mut last_sent = Instant::now() - pace;
            let tick = Duration::from_millis(1);

            while received.len() < num_packets || stream.ack_seqnum() < goal {
                let now = Instant::now();
                if now >= last_sent + pace && sent < num_packets {
                    let mut pkt = packet(stream.next_seqnum());
                    if let Ok(_) = stream.try_send(|header| {
                        pkt.0 = header;
                        if let Err(err) = sender.send(pkt.clone()) {
                            Err(anyhow::anyhow!(err))
                        } else {
                            Ok((pkt, now + timeout))
                        }
                    }) {
                        last_sent = now;
                        sent += 1;
                    }
                }

                if let Ok(new_pkt) = receiver.try_recv(now) {
                    received.append(&mut stream.receive(&new_pkt.0.clone(), new_pkt).unwrap());
                }

                let _ = stream.try_send_ack(|header| {
                    let mut a = ack(0);
                    a.0 = header;
                    if let Err(err) = sender.send(a) {
                        Err(anyhow::anyhow!(err))
                    } else {
                        Ok(())
                    }
                });

                let _ = stream.try_resend(now, |pkt| {
                    resend_count += 1;
                    if let Err(err) = sender.send(pkt.clone()) {
                        Err(anyhow::anyhow!(err))
                    } else {
                        Ok(now + timeout)
                    }
                });

                thread::sleep(tick);
            }

            println!(
                "'{}' exited loop: sent: {}, received: {}, resent: {}",
                tag,
                sent,
                received.len(),
                resend_count
            );
            received
        })
    }

    #[derive(Debug)]
    struct DelayReceiver<T: Debug + PartialEq> {
        delay_iter: DistIter<Uniform<u64>, StdRng, u64>,
        buffer: BinaryHeap<Delayed<T>>,
        recv_channel: Receiver<T>,
    }

    impl<T: Debug + PartialEq> DelayReceiver<T> {
        fn new(recv_channel: Receiver<T>, low: u64, high: u64) -> Self {
            // unfortunately no poisson distribution
            let rng: StdRng = SeedableRng::from_entropy();
            let delay_iter = rng.sample_iter(Uniform::new(low, high));
            DelayReceiver {
                delay_iter,
                buffer: BinaryHeap::with_capacity(1024),
                recv_channel,
            }
        }

        fn try_recv(&mut self, now: Instant) -> std::result::Result<T, TryRecvError> {
            if let Ok(v) = self.recv_channel.try_recv() {
                let delay = self.delay_iter.next().unwrap();
                let recv_at = now + Duration::from_millis(delay);
                self.buffer
                    .push(Delayed(v, recv_at, Duration::from_millis(delay)));
            }

            let is_ready = self
                .buffer
                .peek()
                .map_or(false, |Delayed(_, recv_at, _)| recv_at <= &now);
            if is_ready {
                let Delayed(v, _, _) = self.buffer.pop().unwrap();
                Ok(v)
            } else {
                Err(TryRecvError::Empty)
            }
        }
    }

    #[derive(Debug, PartialEq)]
    struct Delayed<T: Debug + PartialEq>(T, Instant, Duration);

    impl<T: Debug + PartialEq> PartialOrd for Delayed<T> {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            std::cmp::Reverse(self.1).partial_cmp(&std::cmp::Reverse(other.1))
        }
    }

    impl<T: Debug + PartialEq> Eq for Delayed<T> {}

    impl<T: Debug + PartialEq> Ord for Delayed<T> {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.partial_cmp(other).unwrap()
        }
    }
}
