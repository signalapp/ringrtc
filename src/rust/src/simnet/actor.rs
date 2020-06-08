//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use std::{
    cmp::{Ord, Ordering, PartialEq, PartialOrd},
    collections::BinaryHeap,
    sync::{
        atomic,
        atomic::AtomicBool,
        mpsc::{channel, RecvError, RecvTimeoutError, Sender},
        Arc,
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub struct Actor<State> {
    sender:  Sender<Task<State>>,
    stopper: Stopper,
}

impl<State: 'static> Actor<State> {
    pub fn new(
        stopper: Stopper,
        gen_state: impl FnOnce(Actor<State>) -> State + Send + 'static,
    ) -> Self {
        let (sender, receiver) = channel::<Task<State>>();

        let stopper_to_register = stopper.clone();

        // "stopped" is signal that this Actor's thread should stop.
        // We keep one on the inside of the loop to check if we've been stopped.
        // We keep another on the outside to trigger stopping.
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_to_register = stopped.clone();

        let actor = Self { sender, stopper };
        let actor_to_register = actor.clone();
        let actor_to_return = actor.clone();
        // Moves in actor and stopped
        let join_handle = thread::spawn(move || {
            let mut state = gen_state(actor);
            let mut delayed_tasks = BinaryHeap::<Task<State>>::new();
            loop {
                // The following is basically a manual way of doing:
                // received_task = select{ delayed_tasks.pop(), receiver.recv() }
                // Maybe we should switch to using async/await and an executor to do this.
                let received_task = match delayed_tasks.peek() {
                    // Wait forever
                    None => match receiver.recv() {
                        Ok(received_task) => received_task,
                        Err(RecvError) => {
                            break;
                        }
                    },
                    Some(delayed_task) => {
                        // Wait for delayed_task
                        match receiver.recv_timeout(delayed_task.timeout()) {
                            Ok(received_task) => received_task,
                            Err(RecvTimeoutError::Disconnected) => {
                                break;
                            }
                            // It's waited long enough.
                            // Treat it like an immediate task (run it below)
                            Err(RecvTimeoutError::Timeout) => {
                                delayed_tasks.pop().unwrap().as_immediate()
                            }
                        }
                    }
                };
                if stopped.load(atomic::Ordering::Relaxed) {
                    break;
                }
                if received_task.is_delayed() {
                    delayed_tasks.push(received_task);
                } else {
                    (received_task.run)(&mut state);
                }
            }
        });
        stopper_to_register.register(
            Box::new(actor_to_register),
            stopped_to_register,
            join_handle,
        );
        actor_to_return
    }

    pub fn send(&self, run: impl FnOnce(&mut State) + Send + 'static) {
        let _ = self.sender.send(Task::immediate(Box::new(run)));
    }

    pub fn send_delayed(&self, delay: Duration, run: impl FnOnce(&mut State) + Send + 'static) {
        let _ = self.sender.send(Task::delayed(Box::new(run), delay));
    }

    pub fn stopper(&self) -> &Stopper {
        &self.stopper
    }
}

// This doesn't #[derive] for some reason.
impl<State> Clone for Actor<State> {
    fn clone(&self) -> Self {
        Self {
            sender:  self.sender.clone(),
            stopper: self.stopper.clone(),
        }
    }
}

impl<State> Stop for Actor<State> {
    fn stop(&self, stopped: &AtomicBool) {
        stopped.store(true, atomic::Ordering::Relaxed);
        // Sending an empty message kicks the message loop if it's stuck.
        let _ = self.sender.send(Task::immediate(Box::new(|_state| {})));
    }
}

type BoxedTaskFn<State> = Box<dyn FnOnce(&mut State) + Send>;

struct Task<State> {
    run:      BoxedTaskFn<State>,
    deadline: Option<Instant>, // None == Immediately
}

impl<State> Task<State> {
    fn immediate(run: BoxedTaskFn<State>) -> Self {
        Self {
            run,
            deadline: None,
        }
    }

    fn delayed(run: BoxedTaskFn<State>, delay: Duration) -> Self {
        Self {
            run,
            deadline: Some(Instant::now() + delay),
        }
    }

    fn as_immediate(self) -> Self {
        Self {
            run:      self.run,
            deadline: None,
        }
    }

    fn is_delayed(&self) -> bool {
        self.deadline.is_some()
    }

    fn timeout(&self) -> Duration {
        match self.deadline {
            None => Duration::from_secs(0),
            Some(deadline) => deadline.saturating_duration_since(Instant::now()),
        }
    }
}

impl<T> Ord for Task<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // We reverse the order because we want the earlier to go first in a BinaryHeap
        self.deadline.cmp(&other.deadline).reverse()
    }
}

impl<T> PartialOrd for Task<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> PartialEq for Task<T> {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl<T> Eq for Task<T> {}

trait Stop: Send {
    fn stop(&self, stopped: &AtomicBool);
}

// A stopper is used to stopper all the actors associated with it.
// You pass in one stopper to many actors
// (and those actors to child actors and so forth).
// Then you close them all once.
// The reason we can't just tell the actor to close itself
// is that we'd like to be able to close *and* join, but joining
// requires a JoinHandle, which is not cloneable, and we want
// actors to be cloneable.  Plus, this makes it possible for one
// actor to spawn another and allow the create to keep a Stopper for
// it without the parent actor keeping a reference to the child.
// In the normal case when you just want to shut everything down,
// this is very convenient.
#[derive(Clone)]
pub struct Stopper {
    actors: Arc<Mutex<Vec<(Box<dyn Stop>, Arc<AtomicBool>, thread::JoinHandle<()>)>>>,
}

impl Stopper {
    pub fn new() -> Self {
        Self {
            actors: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn register(
        &self,
        actor: Box<dyn Stop>,
        stopped: Arc<AtomicBool>,
        join_handle: thread::JoinHandle<()>,
    ) {
        let mut actors = self.actors.lock().expect("Couldn't get lock to add actor");
        actors.push((actor, stopped, join_handle));
    }

    // TODO: Add support for removing actors.

    // Stop all the Actors associated with this Stopper, but don't
    // join (wait for them to end)
    pub fn stop_all_without_joining(&self) -> Vec<thread::JoinHandle<()>> {
        let mut actors = self
            .actors
            .lock()
            .expect("Couldn't get lock to stop actors");
        actors
            .drain(..)
            .map(|(actor, stopped, join_handle)| {
                actor.stop(&stopped);
                join_handle
            })
            .collect()
    }

    // Stop all the Actors associated with this Stopper, and
    // join (wait for them to end).  If joining fails, it will panic,
    // but it will still stop all the Actors first.
    pub fn stop_all_and_join(&self) {
        let join_handles = self.stop_all_without_joining();
        for join_handle in join_handles {
            join_handle.join().expect("Failed to join thread.");
        }
    }
}
