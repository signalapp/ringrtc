//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use futures_core::Stream;
use log::*;
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio::{
    signal,
    sync::{mpsc, oneshot, RwLock},
};
use tonic::{transport::Server, Request, Response, Status};

pub mod calling {
    #![allow(clippy::derive_partial_eq_without_eq, clippy::redundant_async_block)]
    protobuf::include_call_sim_proto!();
}

use calling::signaling_relay_server::{SignalingRelay, SignalingRelayServer};
use calling::{Empty, Registration, RelayMessage};

// When a new client connects, we will create a pair of mpsc channels.
// Add the clients and their related senders to some shared state.
#[derive(Debug)]
pub struct CallingServiceState {
    clients: HashMap<String, mpsc::Sender<RelayMessage>>,
}

impl CallingServiceState {
    fn new() -> Self {
        CallingServiceState {
            clients: HashMap::new(),
        }
    }

    async fn broadcast(&self, message: RelayMessage) {
        let sender_id = format!("{}:{}", message.client, message.device_id);
        for (client, tx) in &self.clients {
            // Send only to other clients, not back to the sender...
            if !client.eq(&sender_id) {
                match tx.send(message.clone()).await {
                    Ok(_) => {
                        info!("[broadcast] to {}", client)
                    }
                    Err(_) => {
                        error!("[broadcast] tx.send() error to {}", client)
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct CallingService {
    state: Arc<RwLock<CallingServiceState>>,
}

impl CallingService {
    fn new(state: Arc<RwLock<CallingServiceState>>) -> Self {
        CallingService { state }
    }
}

#[tonic::async_trait]
impl SignalingRelay for CallingService {
    type RegisterStream =
        Pin<Box<dyn Stream<Item = Result<RelayMessage, Status>> + Send + Sync + 'static>>;

    async fn register(
        &self,
        request: Request<Registration>,
    ) -> Result<Response<Self::RegisterStream>, Status> {
        let client = request.into_inner().client;

        info!("[register] from {}", client);

        let (stream_tx, stream_rx) = mpsc::channel(1);

        // When connecting, create the related sender and receiver for the state.
        let (tx, mut rx) = mpsc::channel(1);
        {
            self.state.write().await.clients.insert(client.clone(), tx);
        } // Unlock the write lock.

        let state_clone = self.state.clone();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                match stream_tx.send(Ok(message)).await {
                    Ok(_) => {}
                    Err(_) => {
                        // If sending failed, then remove the client.
                        error!("[register] stream_tx.send() error to {}", &client);
                        state_clone.write().await.clients.remove(&client);
                    }
                }
            }

            // It seems we only detect a broken connection on the `next` send?
            info!("[register] rx.recv() returned None");
        });

        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(stream_rx),
        )))
    }

    async fn send(&self, request: Request<RelayMessage>) -> Result<Response<Empty>, Status> {
        let request_data = request.into_inner();

        info!(
            "[send] from {}:{}",
            request_data.client, request_data.device_id
        );

        self.state.read().await.broadcast(request_data).await;

        Ok(Response::new(Empty {}))
    }
}

use calling::test_management_server::{TestManagement, TestManagementServer};
use calling::{CommandMessage, Event};

// When a new client connects, we will create a pair of mpsc channels.
// Add the clients and their related senders to some shared state.
#[derive(Debug, Default)]
pub struct TestingServiceState {
    clients: HashMap<String, mpsc::Sender<CommandMessage>>,
    manager: Option<mpsc::Sender<Event>>,
}

impl TestingServiceState {
    async fn send_to(&self, message: CommandMessage) {
        // Send only to the specific client...
        if let Some(tx) = self.clients.get(&message.client) {
            match tx.send(message.clone()).await {
                Ok(_) => {
                    info!("[send_to] to {}", message.client)
                }
                Err(_) => {
                    error!("[send_to] tx.send() error to {}", message.client)
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct TestingService {
    state: Arc<RwLock<TestingServiceState>>,
}

impl TestingService {
    fn new(state: Arc<RwLock<TestingServiceState>>) -> Self {
        TestingService { state }
    }
}

#[tonic::async_trait]
impl TestManagement for TestingService {
    type ReadyStream =
        Pin<Box<dyn Stream<Item = Result<CommandMessage, Status>> + Send + Sync + 'static>>;

    async fn ready(
        &self,
        request: Request<Registration>,
    ) -> Result<Response<Self::ReadyStream>, Status> {
        let client = request.into_inner().client;

        info!("[ready] from {}", client);

        let (stream_tx, stream_rx) = mpsc::channel(1);

        // When connecting, create the related sender and receiver for the state.
        let (tx, mut rx) = mpsc::channel(1);
        {
            self.state.write().await.clients.insert(client.clone(), tx);
        } // Unlock the write lock.

        {
            let state = &self.state.read().await;
            let ready_count = state.clients.len() as i32;

            if let Some(manager) = &state.manager {
                let _ = manager.send(Event { ready_count }).await;
            }
        } // Unlock the read lock.

        let state_clone = self.state.clone();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                match stream_tx.send(Ok(message)).await {
                    Ok(_) => {}
                    Err(_) => {
                        // If sending failed, then remove the client.
                        error!("[ready] stream_tx.send() error to {}", &client);
                        state_clone.write().await.clients.remove(&client);
                    }
                }
            }

            // It seems we only detect a broken connection on the `next` send?
            info!("[ready] rx.recv() returned None");
        });

        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(stream_rx),
        )))
    }

    async fn done(&self, request: Request<Registration>) -> Result<Response<Empty>, Status> {
        let client = request.into_inner().client;

        info!("[done] from {}", client);

        {
            let mut state = self.state.write().await;
            state.clients.remove(&client);

            let ready_count = state.clients.len() as i32;

            if let Some(manager) = &state.manager {
                let _ = manager.send(Event { ready_count }).await;
            }
        } // Unlock the write lock.

        Ok(Response::new(Empty {}))
    }

    type NotificationStream =
        Pin<Box<dyn Stream<Item = Result<Event, Status>> + Send + Sync + 'static>>;

    async fn notification(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::NotificationStream>, Status> {
        info!("[notification]");

        let (stream_tx, stream_rx) = mpsc::channel(1);

        // When connecting, create the related sender and receiver for the state.
        let (tx, mut rx) = mpsc::channel(1);
        {
            self.state.write().await.manager = Some(tx);
        } // Unlock the write lock.

        tokio::spawn(async move {
            while let Some(notification) = rx.recv().await {
                match stream_tx.send(Ok(notification)).await {
                    Ok(_) => {
                        info!("[notification] send");
                    }
                    Err(_) => {
                        error!("[notification] stream_tx.send() error");
                    }
                }
            }

            // It seems we only detect a broken connection on the `next` send?
            info!("[notification] rx.recv() returned None");
        });

        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(stream_rx),
        )))
    }

    async fn send_command(
        &self,
        request: Request<CommandMessage>,
    ) -> Result<Response<Empty>, Status> {
        let request_data = request.into_inner();

        info!(
            "[sendCommand] to: {}, command: {}",
            request_data.client, request_data.command
        );

        self.state.read().await.send_to(request_data).await;

        Ok(Response::new(Empty {}))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let (signal_tx, signal_rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = signal::ctrl_c().await;
        info!("SIGINT received: shutting down");
        let _ = signal_tx.send(());
    });

    let addr = "0.0.0.0:8080".parse().unwrap();
    info!("Server listening on: {}", addr);

    let calling_state = Arc::new(RwLock::new(CallingServiceState::new()));
    let calling_server = CallingService::new(calling_state.clone());
    let calling_service = SignalingRelayServer::new(calling_server);

    let testing_state = Arc::new(RwLock::new(TestingServiceState::default()));
    let testing_server = TestingService::new(testing_state.clone());
    let testing_service = TestManagementServer::new(testing_server);

    let server = Server::builder()
        .add_service(calling_service)
        .add_service(testing_service)
        .serve_with_shutdown(addr, async {
            signal_rx.await.ok();
        });

    server.await?;

    Ok(())
}
