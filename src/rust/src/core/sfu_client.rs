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

use crate::common::Result;
use crate::core::group_call;
use crate::core::group_call::SfuInfo;
use crate::error::RingRtcError;
use crate::lite::{
    http, sfu,
    sfu::{DemuxId, GroupMember, MembershipProof, OpaqueUserIdMapping, PeekResultCallback},
};

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

pub struct SfuClient {
    url: String,
    // For use post-DHE
    hkdf_extra_info: Vec<u8>,
    http_client: Box<dyn http::Client + Send>,
    auth_header: Option<String>,
    opaque_user_id_mappings: Vec<OpaqueUserIdMapping>,
    deferred_join: Option<(String, [u8; 32], group_call::Client)>,
}

pub struct Joined {
    pub sfu_info: SfuInfo,
    pub local_demux_id: DemuxId,
    pub server_dhe_pub_key: [u8; 32],
    pub hkdf_extra_info: Vec<u8>,
}

impl SfuClient {
    pub fn new(
        http_client: Box<dyn http::Client + Send>,
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
        response: Option<http::Response>,
        hkdf_extra_info: Vec<u8>,
    ) -> Result<Joined> {
        let body = match response {
            Some(r) if r.status_code >= 200 && r.status_code <= 300 => r.body,
            Some(r) if r.status_code == sfu::RESPONSE_CODE_MAX_PARTICIPANTS_REACHED => {
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
        self.http_client.send_request(
            http::Request {
                method: http::Method::Put,
                url: participants_url,
                headers,
                body: Some(body),
            },
            Box::new(move |resp| {
                let outcome = Self::process_join_response(resp, hkdf_extra_info);
                client.on_sfu_client_joined(outcome);
            }),
        );
    }
}

impl group_call::SfuClient for SfuClient {
    fn set_membership_proof(&mut self, proof: MembershipProof) {
        if let Some(auth_header) = sfu::auth_header_from_membership_proof(&proof) {
            self.auth_header = Some(auth_header.clone());
            // Release any tasks that were blocked on getting the token.
            if let Some((ice_ufrag, dhe_pub_key, client)) = self.deferred_join.take() {
                info!("membership token received, proceeding with deferred join");
                self.join_with_header(&auth_header, &ice_ufrag, &dhe_pub_key[..], client);
            }
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

    fn peek(&mut self, result_callback: PeekResultCallback) {
        match self.auth_header.clone() {
            Some(auth_header) => sfu::peek(
                self.http_client.as_ref(),
                &self.url,
                auth_header,
                self.opaque_user_id_mappings.clone(),
                result_callback,
            ),
            None => {
                result_callback(Err(sfu::RESPONSE_CODE_INVALID_AUTH));
            }
        }
    }

    fn set_group_members(&mut self, members: Vec<GroupMember>) {
        self.opaque_user_id_mappings = sfu::opaque_user_id_mappings_from_group_members(&members);
        info!(
            "SfuClient set_group_members: {} members",
            self.opaque_user_id_mappings.len()
        );
    }
}
