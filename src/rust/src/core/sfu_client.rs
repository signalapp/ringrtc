//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Client for the Group Calling SFU (selective forwarding unit)

use std::collections::HashMap;
use std::net::SocketAddr;

use serde::Deserialize;
use serde_json::json;

use crate::common::{HttpMethod, HttpResponse, Result};
use crate::core::group_call::{
    BoxedPeekInfoHandler,
    DemuxId,
    GroupMemberInfo,
    MembershipProof,
    PeekInfo,
    SfuInfo,
    UserId,
};
use crate::core::util::sha256_as_hexstring;
use crate::core::{group_call, http_client::HttpClient};
use crate::error::RingRtcError;

#[derive(Deserialize, Debug)]
struct JoinResponse {
    #[serde(rename = "endpointId")]
    endpoint_id: String,
    #[serde(rename = "ssrcPrefix")]
    ssrc_prefix: u32,
    transport:   SfuTransport,
}

#[derive(Deserialize, Debug)]
struct SfuTransport {
    ufrag:        String,
    pwd:          String,
    fingerprints: Vec<SfuFingerprint>,
    candidates:   Vec<SfuCandidate>,
}

#[derive(Deserialize, Debug)]
struct SfuCandidate {
    ip:             String,
    port:           u16,
    /// Candidate type ('host', 'srflx' etc.)
    #[serde(rename = "type")]
    candidate_type: String,
}

#[derive(Deserialize, Debug)]
struct SfuFingerprint {
    hash:        String,
    fingerprint: String,
}

#[derive(Deserialize, Debug)]
struct ParticipantsResponse {
    #[serde(rename = "conferenceId")]
    era_id: Option<String>,

    #[serde(rename = "maxConferenceSize")]
    max_devices: Option<u32>,

    participants: Vec<SfuParticipant>,

    creator: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SfuParticipant {
    #[serde(rename = "endpointId")]
    endpoint_id: String,
    #[serde(rename = "ssrcPrefix")]
    ssrc_prefix: u32,
}

// Keeps track of which SFU-assigned endpoint ID prefix corresponds to which member's UUID.
#[derive(Clone, Debug)]
struct UuidEndpointPrefix {
    uuid:   UserId,
    prefix: String,
}

pub struct SfuClient {
    url:             String,
    http_client:     Box<dyn HttpClient + Send>,
    auth_header:     Option<String>,
    member_prefixes: Vec<UuidEndpointPrefix>,
    deferred_join: Option<(
        String,
        String,
        group_call::DtlsFingerprint,
        group_call::Client,
    )>,
}

const RESPONSE_CODE_NO_CONFERENCE: u16 = 404;
const RESPONSE_CODE_MAX_PARTICIPANTS_REACHED: u16 = 413;

impl SfuClient {
    pub fn new(http_client: Box<dyn HttpClient + Send>, url: String) -> Self {
        let url = url.trim_end_matches('/').to_string();
        SfuClient {
            url,
            http_client,
            auth_header: None,
            member_prefixes: vec![],
            deferred_join: None,
        }
    }

    fn process_join_response(response: Option<HttpResponse>) -> Result<(SfuInfo, DemuxId, String)> {
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

        let deserialized: JoinResponse = serde_json::from_str(&body)?;
        let sha256_fingerprint = match deserialized
            .transport
            .fingerprints
            .iter()
            .find(|fp| fp.hash == "sha-256")
        {
            Some(fp) => fp,
            None => {
                error!("SfuClient: no SHA-256 fingerprint in the join response");
                return Err(RingRtcError::SfuClientRequestFailed.into());
            }
        };
        let dtls_fingerprint = match group_call::decode_fingerprint(&sha256_fingerprint.fingerprint)
        {
            Some(fp) => fp,
            None => {
                error!("SfuClient: Failed to parse DTLS fingerprint in join response");
                return Err(RingRtcError::SfuClientRequestFailed.into());
            }
        };

        if deserialized.transport.candidates.is_empty() {
            error!("SfuClient: no candidates provided in the join response");
            return Err(RingRtcError::SfuClientRequestFailed.into());
        }
        let udp_addresses: Vec<SocketAddr> = deserialized
            .transport
            .candidates
            .iter()
            .filter_map(|c| Some(SocketAddr::new(c.ip.parse().ok()?, c.port)))
            .collect();

        let ice_ufrag = deserialized.transport.ufrag;
        let ice_pwd = deserialized.transport.pwd;
        let endpoint_id = deserialized.endpoint_id;

        let info = group_call::SfuInfo {
            udp_addresses,
            ice_ufrag,
            ice_pwd,
            dtls_fingerprint,
        };
        let demux_id = deserialized.ssrc_prefix;
        debug!(
            "SfuClient: successful join, info: {:?}, demux_id: {}, endpoint_id: {}",
            info, demux_id, endpoint_id
        );
        Ok((info, demux_id, endpoint_id))
    }

    fn join_with_header(
        &self,
        auth_header: &str,
        ice_ufrag: &str,
        ice_pwd: &str,
        dtls_fingerprint: &group_call::DtlsFingerprint,
        client: group_call::Client,
    ) {
        info!("SfuClient join_with_header:");

        let join_json = json!({
            // The payload types, header extensions, fingerprint hash, payload formats,
            // and SSRCs need to match those configured in peer_connection.cc
            // (CreateSessionDescriptionForGroupCall) and group_call.rs
            "transport": {
                "candidates": [],
                "fingerprints": [
                    {
                        "fingerprint" : group_call::encode_fingerprint(dtls_fingerprint),
                        "hash" : "sha-256",
                        "setup" : "active",
                    },
                ],
                "ufrag" : ice_ufrag,
                "pwd": ice_pwd,
                "xmlns" : "urn:xmpp:jingle:transports:ice-udp:1",
                "rtcp-mux": true
            },
            "audioPayloadType" : {
                "id": 102,
                "name": "opus",
                "clockrate": 48000,
                "channels": 2,
                "parameters": {
                    "minptime": 10,
                    "useinbandfec": 1
                }
            },
            "videoPayloadType" : {
                "id": 108,
                "name": "VP8",
                "clockrate": 90000,
                "channels": 0,
                "parameters": {},
                "rtcp-fbs": [
                    {
                        "type": "goog-remb"
                    },
                    {
                    "type": "transport-cc"
                    },
                    {
                    "type": "ccm",
                    "subtype": "fir"
                    }, {
                    "type": "nack"
                    }, {
                        "type": "nack",
                        "subtype": "pli"
                    }
                ]
            },
            "dataPayloadType" : {
                "id": 101,
                "name": "google-data",
                "clockrate": 90000,
                "channels": 2,
                "parameters": {
                    "minptime": 10,
                    "useinbandfec": 1
                }
            },
            "audioHeaderExtensions" : [
                // The extension IDs and URIs need to match those configured in
                // peer_connection.cc (CreateSessionDescriptionForGroupCall)
                {
                    "id": 1,
                    "uri": "http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01"
                },
                {
                    "id": 5,
                    "uri": "urn:ietf:params:rtp-hdrext:ssrc-audio-level"
                },
                {
                    "id": 12,
                    "uri": "http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time"
                },
            ],
            "videoHeaderExtensions" : [
                // The extension IDs and URIs need to match those configured in
                // peer_connection.cc (CreateSessionDescriptionForGroupCall)
                {
                    "id": 1,
                    "uri": "http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01"
                },
                // The SFU doesn't know about this.  We still send it for the benefit of other clients,
                // But the SFU just passes it along.
                // {
                //     "id": 4,
                //     "uri": "urn:3gpp:video-orientation"
                // },
                {
                    "id": 12,
                    "uri": "http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time"
                },
                {
                    "id": 13,
                    "uri": "urn:ietf:params:rtp-hdrext:toffset"
                }
            ],
            "audioSsrcs" : [0],
            "audioSsrcGroups": [],
            "dataSsrcs" : [0xD],
            "dataSsrcGroups": [],
            "videoSsrcs": [2, 3, 4, 5, 6, 7],
            "videoSsrcGroups": [
                {
                    "semantics": "SIM",
                    "sources": [2, 4, 6],
                },
                {
                    "semantics": "FID",
                    "sources": [2, 3],
                },
                {
                    "semantics": "FID",
                    "sources": [4, 5],
                },
                {
                    "semantics": "FID",
                    "sources": [6, 7],
                },
            ],
        });
        debug!("Sending join request: {}", join_json.to_string());
        let participants_url = format!("{}/v1/conference/participants", self.url);
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), auth_header.to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        let body = join_json.to_string().as_bytes().to_vec();
        self.http_client.make_request(
            participants_url,
            HttpMethod::Put,
            headers,
            Some(body),
            Box::new(move |resp| {
                let outcome = Self::process_join_response(resp);
                client.on_sfu_client_joined(outcome);
            }),
        );
    }

    fn process_remote_devices_response(
        response: Option<HttpResponse>,
        member_prefixes: Vec<UuidEndpointPrefix>,
    ) -> Result<PeekInfo> {
        let body = match response {
            Some(r) if r.status_code >= 200 && r.status_code <= 300 => r.body,
            Some(r) if r.status_code == RESPONSE_CODE_NO_CONFERENCE => {
                info!("SfuClient: no participants joined");
                return Ok(PeekInfo {
                    devices:      vec![],
                    creator:      None,
                    era_id:       None,
                    max_devices:  None,
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
        let deserialized: ParticipantsResponse = serde_json::from_str(&body)?;

        let era_id = deserialized.era_id;
        let max_devices = deserialized.max_devices;
        let device_count = deserialized.participants.len() as u32;
        let creator = match deserialized.creator {
            None => None,
            Some(encoded_uid) => Self::lookup_uuid_by_endpoint_id(&member_prefixes, &encoded_uid),
        };

        let devices: Vec<group_call::PeekDeviceInfo> = deserialized
            .participants
            .into_iter()
            .filter_map(|p| {
                let demux_id = p.ssrc_prefix;
                let user_id = Self::lookup_uuid_by_endpoint_id(&member_prefixes, &p.endpoint_id);
                if let Ok(short_device_id) =
                    p.endpoint_id.split('-').nth(1).unwrap_or_default().parse()
                {
                    Some(group_call::PeekDeviceInfo {
                        demux_id,
                        user_id,
                        short_device_id,
                        long_device_id: p.endpoint_id,
                    })
                } else {
                    warn!(
                        "Ignoring device with unparsable endpoint ID: {}",
                        p.endpoint_id
                    );
                    None
                }
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
        let participants_url = format!("{}/v1/conference/participants", self.url);
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), auth_header.to_string());
        let member_prefixes = self.member_prefixes.clone();
        self.http_client.make_request(
            participants_url,
            HttpMethod::Get,
            headers,
            None,
            Box::new(move |resp| {
                let result = Self::process_remote_devices_response(resp, member_prefixes);
                handle_result(result);
            }),
        );
    }

    // Maps an endpoint ID to the corresponding user's UUID, if we have such a user in the member list.
    fn lookup_uuid_by_endpoint_id(
        member_prefixes: &[UuidEndpointPrefix],
        endpoint_id: &str,
    ) -> Option<UserId> {
        member_prefixes.iter().find_map(|entry| {
            if endpoint_id.starts_with(&entry.prefix) {
                Some(entry.uuid.clone())
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
        if let Some((ice_ufrag, ice_pwd, dtls_fingerprint, client)) = self.deferred_join.take() {
            info!("membership token received, proceeding with deferred join");
            self.join_with_header(&header, &ice_ufrag, &ice_pwd, &dtls_fingerprint, client);
        }
    }

    fn join(
        &mut self,
        ice_ufrag: &str,
        ice_pwd: &str,
        dtls_fingerprint: &group_call::DtlsFingerprint,
        client: group_call::Client,
    ) {
        match self.auth_header.as_ref() {
            Some(h) => self.join_with_header(h, ice_ufrag, ice_pwd, dtls_fingerprint, client),
            None => {
                info!("join requested without membership token - deferring");
                let ice_ufrag = ice_ufrag.to_string();
                let ice_pwd = ice_pwd.to_string();
                let dtls_fingerprint = *dtls_fingerprint;
                self.deferred_join = Some((ice_ufrag, ice_pwd, dtls_fingerprint, client));
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
        // Transform the [uuid, ciphertext] map to a [uuid, endpoint-id-prefix] map.
        // The SFU frontend assigns the endpoint ID as "prefix-<random number>", where the
        // prefix is sha256(uuid_ciphertext).
        // Our map does not include the trailing dash.
        self.member_prefixes = members
            .iter()
            .map(|m| {
                let prefix = sha256_as_hexstring(&m.user_id_ciphertext);
                UuidEndpointPrefix {
                    uuid: m.user_id.clone(),
                    prefix,
                }
            })
            .collect();
        info!(
            "SfuClient set_group_members: {} members",
            self.member_prefixes.len()
        );
    }

    fn leave(&mut self, endpoint_id: String) {
        info!("SfuClient leave");

        let auth_header = match self.auth_header.as_ref() {
            Some(h) => h,
            None => {
                // We shouldn't have been able to join without an auth header. In theory, we could
                // request a new auth token and use it, but it will likely take longer and be less
                // reliable than just letting it time out.
                warn!("Requesting to leave a conference without an auth header; ignoring");
                return;
            }
        };

        let endpoint_url = format!("{}/v1/conference/participants/{}", self.url, endpoint_id);
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), auth_header.to_string());
        self.http_client.make_request(
            endpoint_url,
            HttpMethod::Delete,
            headers,
            None,
            Box::new(move |resp| match resp {
                Some(r) if r.status_code >= 200 && r.status_code <= 300 => {
                    debug!("SfuClient: leave successful");
                }
                Some(r) => {
                    warn!("SfuClient: HTTP error while leaving ({})", r.status_code);
                }
                _ => {
                    warn!("SfuClient: HTTP error while leaving (no response)");
                }
            }),
        );
    }
}
