//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Client for the Group Calling SFU (selective forwarding unit)

use std::collections::HashMap;
use std::net::SocketAddr;

use hex::ToHex;
use serde::Deserialize;
use serde_json::json;

use crate::common::{HttpMethod, HttpResponse, Result};
use crate::core::group_call::{
    BoxedPeekInfoHandler, DemuxId, GroupMemberInfo, MembershipProof, OpaqueUserId,
    OpaqueUserIdMapping, PeekInfo, SfuInfo, UserId,
};
use crate::core::util::sha256_as_hexstring;
use crate::core::{group_call, http_client::HttpClient};
use crate::error::RingRtcError;

#[derive(Deserialize, Debug)]
struct JoinResponse {
    #[serde(rename = "demuxId")]
    demux_id: u32,
    port: u16,
    ip: String,
    #[serde(rename = "iceUfrag")]
    ice_ufrag: String,
    #[serde(rename = "icePwd")]
    ice_pwd: String,
    #[serde(rename = "dhePublicKey", with = "hex")]
    dhe_pub_key: [u8; 32],
}

#[derive(Deserialize, Debug)]
struct ParticipantsResponse {
    #[serde(rename = "conferenceId")]
    era_id: Option<String>,
    #[serde(rename = "maxDevices")]
    max_devices: Option<u32>,
    participants: Vec<SfuParticipant>,
    creator: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SfuParticipant {
    #[serde(rename = "opaqueUserId")]
    opaque_user_id: OpaqueUserId,
    #[serde(rename = "demuxId")]
    demux_id: u32,
}

pub struct SfuClient {
    url: String,
    // For use post-DHE
    hkdf_extra_info: Vec<u8>,
    http_client: Box<dyn HttpClient + Send>,
    auth_header: Option<String>,
    opaque_user_id_mappings: Vec<OpaqueUserIdMapping>,
    deferred_join: Option<(String, [u8; 32], group_call::Client)>,
}

const RESPONSE_CODE_NO_CONFERENCE: u16 = 404;
const RESPONSE_CODE_MAX_PARTICIPANTS_REACHED: u16 = 413;

pub struct Joined {
    pub sfu_info: SfuInfo,
    pub local_demux_id: DemuxId,
    pub server_dhe_pub_key: [u8; 32],
    pub hkdf_extra_info: Vec<u8>,
}

impl SfuClient {
    pub fn new(
        http_client: Box<dyn HttpClient + Send>,
        url: String,
        hkdf_extra_info: Vec<u8>,
    ) -> Self {
        let url = url.trim_end_matches('/').to_string();
        SfuClient {
            url,
            hkdf_extra_info,
            http_client,
            auth_header: None,
            opaque_user_id_mappings: vec![],
            deferred_join: None,
        }
    }

    fn process_join_response(
        response: Option<HttpResponse>,
        hkdf_extra_info: Vec<u8>,
    ) -> Result<Joined> {
        let body = match response {
            Some(r) if r.status_code >= 200 && r.status_code <= 300 => r.body,
            Some(r) if r.status_code == RESPONSE_CODE_MAX_PARTICIPANTS_REACHED => {
                error!("SfuClient: maximum number of participants reached, can't join");
                return Err(RingRtcError::MaxParticipantsReached.into());
            }
            Some(r) => {
                error!(
                    "SfuClient: unexpected join response status code {}",
                    r.status_code
                );
                return Err(RingRtcError::SfuClientReceivedUnexpectedResponseStatusCode(
                    r.status_code,
                )
                .into());
            }
            _ => {
                error!("SfuClient: join request failed (no response)");
                return Err(RingRtcError::SfuClientRequestFailed.into());
            }
        };
        let body = std::str::from_utf8(&body)?;
        debug!("SfuClient: join response: {:?}", body);

        let deserialized: JoinResponse = serde_json::from_str(body)?;
        let server_dhe_pub_key = deserialized.dhe_pub_key;
        let udp_addresses: Vec<SocketAddr> =
            vec![SocketAddr::new(deserialized.ip.parse()?, deserialized.port)];
        let ice_ufrag = deserialized.ice_ufrag;
        let ice_pwd = deserialized.ice_pwd;

        let sfu_info = group_call::SfuInfo {
            udp_addresses,
            ice_ufrag,
            ice_pwd,
        };
        let local_demux_id = deserialized.demux_id;
        debug!(
            "SfuClient: successful join, info: {:?}, demux_id: {}, server_dhe_public_key: {:?}, hkdf_extra_info: {:?}",
            sfu_info, local_demux_id, server_dhe_pub_key, hkdf_extra_info
        );
        Ok(Joined {
            sfu_info,
            local_demux_id,
            server_dhe_pub_key,
            hkdf_extra_info,
        })
    }

    fn join_with_header(
        &self,
        auth_header: &str,
        ice_ufrag: &str,
        dhe_pub_key: &[u8],
        client: group_call::Client,
    ) {
        info!("SfuClient join_with_header:");

        let join_json = json!({
            "iceUfrag" : ice_ufrag,
            "dhePublicKey": dhe_pub_key.encode_hex::<String>(),
            "hkdfExtraInfo": self.hkdf_extra_info.encode_hex::<String>(),
        });
        debug!("Sending join request: {}", join_json.to_string());

        let participants_url = format!("{}/v2/conference/participants", self.url);
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), auth_header.to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        let body = join_json.to_string().as_bytes().to_vec();
        let hkdf_extra_info = self.hkdf_extra_info.clone();
        self.http_client.make_request(
            participants_url,
            HttpMethod::Put,
            headers,
            Some(body),
            Box::new(move |resp| {
                let outcome = Self::process_join_response(resp, hkdf_extra_info);
                client.on_sfu_client_joined(outcome);
            }),
        );
    }

    fn process_remote_devices_response(
        response: Option<HttpResponse>,
        opaque_user_id_mappings: Vec<OpaqueUserIdMapping>,
    ) -> Result<PeekInfo> {
        let body = match response {
            Some(r) if r.status_code >= 200 && r.status_code <= 300 => r.body,
            Some(r) if r.status_code == RESPONSE_CODE_NO_CONFERENCE => {
                info!("SfuClient: no participants joined");
                return Ok(PeekInfo {
                    devices: vec![],
                    creator: None,
                    era_id: None,
                    max_devices: None,
                    device_count: 0,
                });
            }
            Some(r) => {
                error!(
                    "SfuClient: unexpected GetParticipants response status code {}",
                    r.status_code
                );
                return Err(RingRtcError::SfuClientReceivedUnexpectedResponseStatusCode(
                    r.status_code,
                )
                .into());
            }
            _ => {
                error!("SfuClient: GetParticipants request failed (no response)");
                return Err(RingRtcError::SfuClientRequestFailed.into());
            }
        };
        let body = std::str::from_utf8(&body)?;
        debug!("Remote Devices Response: {}", body);
        let deserialized: ParticipantsResponse = serde_json::from_str(body)?;

        let era_id = deserialized.era_id;
        let max_devices = deserialized.max_devices;
        let device_count = deserialized.participants.len() as u32;
        let creator = match deserialized.creator {
            None => None,
            Some(opaque_user_id) => {
                Self::find_user_id_by_opaque_user_id(&opaque_user_id_mappings, &opaque_user_id)
            }
        };

        let devices: Vec<group_call::PeekDeviceInfo> = deserialized
            .participants
            .into_iter()
            .map(|p| {
                let demux_id = p.demux_id;
                let user_id = Self::find_user_id_by_opaque_user_id(
                    &opaque_user_id_mappings,
                    &p.opaque_user_id,
                );
                group_call::PeekDeviceInfo { demux_id, user_id }
            })
            .collect();
        Ok(PeekInfo {
            devices,
            creator,
            era_id,
            max_devices,
            device_count,
        })
    }

    fn request_remote_devices_with_header(
        &self,
        auth_header: &str,
        handle_result: BoxedPeekInfoHandler,
    ) {
        let participants_url = format!("{}/v2/conference/participants", self.url);
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), auth_header.to_string());
        let opaque_user_id_mappings = self.opaque_user_id_mappings.clone();
        self.http_client.make_request(
            participants_url,
            HttpMethod::Get,
            headers,
            None,
            Box::new(move |resp| {
                let result = Self::process_remote_devices_response(resp, opaque_user_id_mappings);
                handle_result(result);
            }),
        );
    }

    fn find_user_id_by_opaque_user_id(
        opaque_user_id_mappings: &[OpaqueUserIdMapping],
        opaque_user_id: &str,
    ) -> Option<UserId> {
        opaque_user_id_mappings.iter().find_map(|mapping| {
            if opaque_user_id == mapping.opaque_user_id {
                Some(mapping.user_id.clone())
            } else {
                None
            }
        })
    }

    pub fn request_joined_members(
        &mut self,
        membership_proof: group_call::MembershipProof,
        group_members: Vec<group_call::GroupMemberInfo>,
        handle_response: BoxedPeekInfoHandler,
    ) {
        group_call::SfuClient::set_membership_proof(self, membership_proof);
        group_call::SfuClient::set_group_members(self, group_members);
        if let Some(header) = self.auth_header.as_ref() {
            self.request_remote_devices_with_header(header, handle_response);
        }
    }
}

impl group_call::SfuClient for SfuClient {
    fn set_membership_proof(&mut self, proof: MembershipProof) {
        // TODO: temporary until the SFU is updated to ignore the username part of the token and make it truly opaque.
        let token = match std::str::from_utf8(&proof) {
            Ok(token) => token,
            Err(_) => {
                error!("Membership token isn't valid UTF-8");
                return;
            }
        };
        let uuid = match token.split(':').next() {
            Some(uuid) => uuid,
            None => {
                error!("No UUID part in the membership token");
                return;
            }
        };

        let auth_params = format!("{}:{}", uuid, token);
        let auth_params = base64::encode(auth_params);
        let header = format!("Basic {}", auth_params);
        self.auth_header = Some(header.clone());

        // Release any tasks that were blocked on getting the token.
        if let Some((ice_ufrag, dhe_pub_key, client)) = self.deferred_join.take() {
            info!("membership token received, proceeding with deferred join");
            self.join_with_header(&header, &ice_ufrag, &dhe_pub_key[..], client);
        }
    }

    fn join(&mut self, ice_ufrag: &str, dhe_pub_key: [u8; 32], client: group_call::Client) {
        match self.auth_header.as_ref() {
            Some(h) => self.join_with_header(h, ice_ufrag, &dhe_pub_key[..], client),
            None => {
                info!("join requested without membership token - deferring");
                let ice_ufrag = ice_ufrag.to_string();
                self.deferred_join = Some((ice_ufrag, dhe_pub_key, client));
            }
        }
    }

    fn peek(&mut self, handle_remote_devices: BoxedPeekInfoHandler) {
        match self.auth_header.as_ref() {
            Some(h) => self.request_remote_devices_with_header(h, handle_remote_devices),
            None => {
                handle_remote_devices(Err(RingRtcError::SfuClientHasNotAuthToken.into()));
            }
        }
    }

    fn set_group_members(&mut self, members: Vec<GroupMemberInfo>) {
        // Transform the list of (UserId, GroupMemberId) to a list of (UserId, OpaqueUserId)
        // so we can map from OpaqueUserId to UserId.
        self.opaque_user_id_mappings = members
            .iter()
            .map(|m| OpaqueUserIdMapping {
                opaque_user_id: sha256_as_hexstring(&m.member_id),
                user_id: m.user_id.clone(),
            })
            .collect();
        info!(
            "SfuClient set_group_members: {} members",
            self.opaque_user_id_mappings.len()
        );
    }
}
