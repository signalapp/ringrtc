//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use ringrtc::{
    common::Result,
    core::group_call::{
        self, ClientId, ConnectionState, GroupId, JoinState, Reaction, VideoRequest,
    },
    lite::sfu::{DemuxId, MembershipProof, PeekInfo},
    native::{GroupUpdate, GroupUpdateHandler},
    webrtc::peer_connection_observer::NetworkRoute,
};

use log::*;

use super::CallEndpoint;

impl Default for LocalDeviceState {
    fn default() -> Self {
        Self {
            connection_state: group_call::ConnectionState::NotConnected,
            join_state: JoinState::NotJoined(None),
            demux_id: None,
            audio_level: 0,
            audio_muted: false,
            video_muted: false,
            presenting: false,
            sharing_screen: false,
            network_route: Default::default(),
        }
    }
}

pub struct GroupCall {
    pub group_id: GroupId,
    pub membership_proof: Vec<u8>,
    pub client_id: ClientId,
    pub local_device_state: LocalDeviceState,
    pub remote_device_state: Vec<group_call::RemoteDeviceState>,

    pub peek_info: Option<PeekInfo>,
    pub reaction_log: Vec<(Instant, Vec<Reaction>)>,
    pub raised_hand_log: Vec<(Instant, Vec<DemuxId>)>,
}

impl GroupCall {
    fn new(group_client_id: ClientId, group_id: GroupId, membership_proof: Vec<u8>) -> Self {
        Self {
            group_id,
            membership_proof,
            client_id: group_client_id,
            local_device_state: LocalDeviceState::default(),
            remote_device_state: vec![],
            peek_info: None,
            reaction_log: vec![],
            raised_hand_log: vec![],
        }
    }
}

pub struct LocalDeviceState {
    connection_state: ConnectionState,
    join_state: JoinState,
    demux_id: Option<DemuxId>,

    audio_level: u16,
    network_route: Option<NetworkRoute>,

    // TODO: add support for the following
    #[allow(dead_code)]
    audio_muted: bool,
    #[allow(dead_code)]
    video_muted: bool,
    #[allow(dead_code)]
    presenting: bool,
    #[allow(dead_code)]
    sharing_screen: bool,
}

/// Implement group call specific functions
impl CallEndpoint {
    pub fn join_group_call(
        &self,
        sfu_url: String,
        group_id: GroupId,
        membership_proof: MembershipProof,
    ) {
        info!("Joining group call...");
        let our_uuid = self
            .user_id
            .as_ref()
            .expect("Must have user_id for group call")
            .clone();
        let hkdf_extra_info = vec![];

        self.actor.send(move |state| {
            let client_id = state
                .call_manager
                .create_group_call_client(
                    group_id.clone(),
                    sfu_url,
                    hkdf_extra_info,
                    Some(Duration::from_millis(200)),
                    Some(state.peer_connection_factory.clone()),
                    state.outgoing_audio_track.clone(),
                    state.outgoing_video_track.clone(),
                    Some(state.incoming_video_sink.clone()),
                )
                .expect("create group call client");

            state
                .call_manager
                .set_outgoing_audio_muted(client_id, false);
            state
                .call_manager
                .set_outgoing_video_muted(client_id, false);
            let _ = state.call_manager.set_self_uuid(our_uuid.clone());
            state
                .call_manager
                .set_membership_proof(client_id, membership_proof.clone());
            state.call_manager.connect(client_id);
            state.call_manager.join(client_id);
            state
                .call_manager
                .set_rtc_stats_interval(client_id, Duration::from_secs(1));
            let group_call = GroupCall::new(client_id, group_id, membership_proof);
            state.group_call = Some(group_call);
        });
    }

    pub fn hangup_group_call(&self) {
        info!("Starting to leave group call...");
        self.actor.send(move |state| {
            if let Some(ref group_call) = state.group_call {
                state.call_manager.leave(group_call.client_id);
                state.call_manager.disconnect(group_call.client_id);
            } else {
                warn!("Did not find group call to leave...");
            }
        });
    }
}

impl GroupUpdateHandler for CallEndpoint {
    fn handle_group_update(&self, update: GroupUpdate) -> Result<()> {
        use GroupUpdate::*;

        info!("Group Update {}", update);
        let peer_id = self.peer_id().clone();
        self.actor.send(move |state| {
            let group_call = state.group_call.as_mut().unwrap();

            match update {
                RequestMembershipProof(client_id) => {
                    state
                        .call_manager
                        .set_membership_proof(client_id, group_call.membership_proof.clone());
                }
                RequestGroupMembers(client_id) => {
                    if let Some(members) = state.group_directory.get(&group_call.group_id) {
                        state
                            .call_manager
                            .set_group_members(client_id, members.clone())
                    } else {
                        error!("Could not resolve group members, group not found in directory");
                    }
                }
                ConnectionStateChanged(client_id, connection_state) => {
                    info!(
                        "New connection state {:?} for group call {}",
                        connection_state, client_id
                    );
                    group_call.local_device_state.connection_state = connection_state;
                }
                JoinStateChanged(client_id, join_state) => {
                    info!(
                        "New join state {:?} for group call {}",
                        join_state, client_id
                    );
                    if let JoinState::Joined(demux_id) = join_state {
                        if let Some(connected_sender) = &state.event_sync.connected {
                            let _ = connected_sender.send(());
                        }
                        group_call.local_device_state.demux_id = Some(demux_id);
                    }
                    group_call.local_device_state.join_state = join_state;
                }
                RemoteDeviceStatesChanged(client_id, remote_device_states) => {
                    let present: HashSet<u32> = group_call
                        .remote_device_state
                        .iter()
                        .map(|rds| rds.demux_id)
                        .collect();
                    let added: Vec<_> = remote_device_states
                        .iter()
                        .filter(|rds| !present.contains(&rds.demux_id))
                        .collect();

                    if !added.is_empty() {
                        let rendered_resolutions = added
                            .iter()
                            .map(|remote_state| VideoRequest {
                                demux_id: remote_state.demux_id,
                                width: 1280,
                                height: 720,
                                framerate: None,
                            })
                            .collect();
                        state
                            .call_manager
                            .request_video(client_id, rendered_resolutions, 720);
                    }

                    group_call.remote_device_state = remote_device_states;
                }
                PeekChanged {
                    client_id: _,
                    peek_info,
                } => {
                    info!("Peek changed: {peek_info:?}");
                    state.call_manager.resend_media_keys(group_call.client_id);
                }
                PeekResult {
                    request_id,
                    peek_result,
                } => match peek_result {
                    Ok(peek) => {
                        group_call.peek_info = Some(peek);
                    }
                    Err(e) => {
                        error!("Failed to peek call: request_id={request_id}, error={e:?}");
                    }
                },
                NetworkRouteChanged(_client_id, network_route) => {
                    group_call.local_device_state.network_route = Some(network_route);
                }
                AudioLevels(_client_id, audio_level, _received_audio_levels) => {
                    group_call.local_device_state.audio_level = audio_level;
                }
                LowBandwidthForVideo {
                    group_id,
                    recovered,
                } => {
                    info!(
                        "Not enough bandwidth to send video reliably {} => {}: recovered: {}",
                        peer_id, group_id, recovered
                    );
                }
                Reactions(_client_id, reactions) => {
                    group_call.reaction_log.push((Instant::now(), reactions));
                }
                RaisedHands(_client_id, demux_ids) => {
                    group_call.raised_hand_log.push((Instant::now(), demux_ids));
                }
                _ => {}
            };
        });
        Ok(())
    }
}
