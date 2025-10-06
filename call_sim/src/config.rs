use std::time::SystemTime;

use base64::{Engine, prelude::BASE64_STANDARD};
use hex::ToHex;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::common::{ClientProfile, Group, GroupMember};

type HmacSha256 = Hmac<Sha256>;
const GV2_AUTH_MATCH_LIMIT: usize = 10;

pub fn generate_client_profiles(
    num_profiles: usize,
    auth_key: &[u8; 32],
    now: SystemTime,
) -> Vec<ClientProfile> {
    let user_id_hex = gen_uuid();
    let user_id_base64 = BASE64_STANDARD.encode(hex::decode(&user_id_hex).unwrap());

    let member_id_hex = format!("{}{}", gen_uuid(), gen_uuid());
    let member_id_base64 = BASE64_STANDARD.encode(hex::decode(&member_id_hex).unwrap());

    let group_name = "generated_group".to_owned();
    let group_id_hex = gen_uuid();
    let group_id_base64 = BASE64_STANDARD.encode(hex::decode(&group_id_hex).unwrap());

    let timestamp = now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let membership_proof = BASE64_STANDARD.encode(generate_signed_v2_password(
        &member_id_hex,
        &group_id_hex,
        timestamp,
        ALL_PERMISSIONS,
        auth_key,
    ));
    let members = vec![GroupMember {
        user_id: user_id_base64.clone(),
        member_id: member_id_base64,
    }];
    let groups = vec![Group {
        name: group_name,
        id: group_id_base64,
        membership_proof,
        members,
    }];

    (1..=num_profiles)
        .map(|idx| ClientProfile {
            user_id: user_id_hex.clone(),
            device_id: idx.to_string(),
            groups: groups.clone(),
        })
        .collect()
}

fn gen_uuid() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")
}

const ALL_PERMISSIONS: &str = "1";

fn generate_signed_v2_password(
    user_id_hex: &str,
    group_id_hex: &str,
    timestamp: u64,
    permission: &str,
    key: &[u8; 32],
) -> String {
    let opaque_user_id = sha256_as_hexstring(&hex::decode(user_id_hex).unwrap());
    // Format the credentials string.
    let credentials = format!(
        "2:{}:{}:{}:{}",
        opaque_user_id, group_id_hex, timestamp, permission
    );

    // Get the MAC for the credentials.
    let mut hmac = HmacSha256::new_from_slice(key).unwrap();
    hmac.update(credentials.as_bytes());
    let mac = hmac.finalize().into_bytes();
    let mac = &mac[..GV2_AUTH_MATCH_LIMIT];

    // Append the MAC to the credentials.
    format!("{}:{}", credentials, mac.encode_hex::<String>())
}

fn sha256_as_hexstring(data: &[u8]) -> String {
    Sha256::digest(data).encode_hex()
}
