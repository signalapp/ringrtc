//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use log::*;
use ringrtc::{
    common::{CallConfig, CallId, Result},
    native::{CallState, CallStateHandler, NativeCallContext},
    webrtc::{peer_connection::AudioLevel, peer_connection_observer::NetworkRoute},
};

use super::CallEndpoint;

/// Metadata for 1:1 Calls
#[derive(Clone)]
pub struct DirectCall {
    call_context: NativeCallContext,
    call_config: CallConfig,
}

impl DirectCall {
    pub fn new(call_context: NativeCallContext, call_config: CallConfig) -> Self {
        Self {
            call_context,
            call_config,
        }
    }
}

impl CallStateHandler for CallEndpoint {
    fn handle_call_state(
        &self,
        remote_peer_id: &str,
        call_id: CallId,
        call_state: CallState,
    ) -> Result<()> {
        info!(
            "State change in call from {}.{} to {}: now {:?}",
            self.peer_id, self.device_id, remote_peer_id, call_state
        );

        self.actor.send(move |state| {
            if let CallState::Incoming(_call_media_type) | CallState::Outgoing(_call_media_type) =
                call_state
            {
                state
                    .call_manager
                    .proceed(
                        call_id,
                        state.direct_call.as_ref().unwrap().call_context.clone(),
                        state.direct_call.as_ref().unwrap().call_config.clone(),
                        None,
                    )
                    .expect("proceed with call");
            } else if let CallState::Ringing = call_state {
                if let Some(ringing_sender) = &state.event_sync.ringing {
                    let _ = ringing_sender.send(());
                }
            } else if let CallState::Connected = call_state {
                if let Some(connected_sender) = &state.event_sync.connected {
                    let _ = connected_sender.send(());
                }
            }
        });
        Ok(())
    }

    fn handle_network_route(
        &self,
        remote_peer_id: &str,
        network_route: NetworkRoute,
    ) -> Result<()> {
        info!(
            "Network route changed for {} => {}: {:?}",
            self.peer_id, remote_peer_id, network_route
        );
        Ok(())
    }

    fn handle_audio_levels(
        &self,
        remote_peer_id: &str,
        captured_level: AudioLevel,
        received_level: AudioLevel,
    ) -> Result<()> {
        debug!(
            "Audio Levels captured for {} => {}: captured: {}; received: {}",
            self.peer_id, remote_peer_id, captured_level, received_level
        );
        Ok(())
    }

    fn handle_low_bandwidth_for_video(&self, remote_peer_id: &str, recovered: bool) -> Result<()> {
        info!(
            "Not enough bandwidth to send video reliably {} => {}: recovered: {}",
            self.peer_id, remote_peer_id, recovered
        );
        Ok(())
    }

    fn handle_remote_audio_state(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        info!(
            "Audio State for {} => {}: {}",
            self.peer_id, remote_peer_id, enabled
        );
        Ok(())
    }

    fn handle_remote_video_state(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        info!(
            "Video State for {} => {}: {}",
            self.peer_id, remote_peer_id, enabled
        );
        Ok(())
    }

    fn handle_remote_sharing_screen(&self, remote_peer_id: &str, enabled: bool) -> Result<()> {
        info!(
            "Sharing Screen for {} => {}: {}",
            self.peer_id, remote_peer_id, enabled
        );
        Ok(())
    }
}
