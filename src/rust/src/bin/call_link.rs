//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

mod support {
    pub mod http_client;
}
use support::http_client;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::time::{Duration, SystemTime};

use base64::engine::general_purpose::STANDARD as base64;
use base64::Engine;
use rand::SeedableRng;
use ringrtc::lite::call_links::{
    CallLinkRestrictions, CallLinkRootKey, CallLinkState, CallLinkUpdateRequest,
};
use ringrtc::lite::http::{self, Client};
use uuid::Uuid;
use zkgroup::call_links::CallLinkSecretParams;

struct Log;

static LOG: Log = Log;

impl log::Log for Log {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!("{} - {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

// These are the zkparams used for testing in Signal-Calling-Service.
const DEFAULT_ZKPARAMS: &str = "AMJqvmQRYwEGlm0MSy6QFPIAvgOVsqRASNX1meQyCOYHJFqxO8lITPkow5kmhPrsNbu9JhVfKFwesVSKhdZaqQko3IZlJZMqP7DDw0DgTWpdnYzSt0XBWT50DM1cw1nCUXXBZUiijdaFs+JRlTKdh54M7sf43pFxyMHlS3URH50LOeR8jVQKaUHi1bDP2GR9ZXp3Ot9Fsp0pM4D/vjL5PwoOUuzNNdpIqUSFhKVrtazwuHNn9ecHMsFsN0QPzByiDA8nhKcGpdzyWUvGjEDBvpKkBtqjo8QuXWjyS3jSl2oJ/Z4Fh3o2N1YfD2aWV/K88o+TN2/j2/k+KbaIZgmiWwppLU+SYGwthxdDfZgnbaaGT/vMYX9P5JlUWSuP3xIxDzPzxBEFho67BP0Pvux+0a5nEOEVEpfRSs61MMvwNXEKZtzkO0QFbOrFYrPntyb7ToqNi66OQNyTfl/J7kqFZg2MTm3CKjHTAIvVMFAGCIamsrT9sWXOtuNeMS94xazxDA==";

const USER_ID: [u8; 16] = [0; 16]; // null UUID
const ADMIN_PASSKEY: &[u8] = &[1, 2, 3, 4, 5];

fn prompt(s: &str) {
    std::io::stdout().write_all(s.as_bytes()).unwrap();
    std::io::stdout().flush().unwrap();
}

fn root_key_from_id(id: &str) -> CallLinkRootKey {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let prng = rand_chacha::ChaCha20Rng::seed_from_u64(hasher.finish());
    CallLinkRootKey::generate(prng)
}

fn start_of_today_in_epoch_seconds() -> zkgroup::Timestamp {
    let now: Duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time moves forwards");
    let remainder = now.as_secs() % (24 * 60 * 60);
    now.as_secs() - remainder
}

fn issue_and_present_auth_credential(
    server_zkparams: &zkgroup::generic_server_params::GenericServerSecretParams,
    public_zkparams: &zkgroup::generic_server_params::GenericServerPublicParams,
    root_key: &CallLinkRootKey,
) -> zkgroup::call_links::CallLinkAuthCredentialPresentation {
    let timestamp = start_of_today_in_epoch_seconds();
    let user_id = Uuid::from_bytes(USER_ID).into();
    let auth_credential = zkgroup::call_links::CallLinkAuthCredentialResponse::issue_credential(
        user_id,
        timestamp,
        server_zkparams,
        rand::random(),
    )
    .receive(user_id, timestamp, public_zkparams)
    .unwrap();
    let call_link_zkparams = CallLinkSecretParams::derive_from_root_key(&root_key.bytes());
    auth_credential.present(
        user_id,
        timestamp,
        public_zkparams,
        &call_link_zkparams,
        rand::random(),
    )
}

fn make_testing_request(
    id: &str,
    server_zkparams: &zkgroup::generic_server_params::GenericServerSecretParams,
    public_zkparams: &zkgroup::generic_server_params::GenericServerPublicParams,
    http_client: &http_client::HttpClient,
    url: &'static str,
    method: http::Method,
    path: &str,
) {
    let root_key = root_key_from_id(id);
    let auth_credential_presentation =
        issue_and_present_auth_credential(server_zkparams, public_zkparams, &root_key);
    let http_client_inner = http_client.clone();
    http_client.send_request(
        http::Request {
            method,
            url: format!("{url}{path}"),
            headers: HashMap::from_iter([
                (
                    "Authorization".to_string(),
                    ringrtc::lite::call_links::auth_header_from_auth_credential(
                        &bincode::serialize(&auth_credential_presentation).unwrap(),
                    ),
                ),
                (
                    "X-Room-Id".to_string(),
                    hex::encode(root_key.derive_room_id()),
                ),
            ]),
            body: None,
        },
        Box::new(move |response| match response {
            Some(response) if response.status.is_success() => {
                // Do a regular read to show the update.
                // zkgroup sin: we're reusing a presentation.
                // But this is a testing client only.
                ringrtc::lite::call_links::read_call_link(
                    &http_client_inner,
                    url,
                    root_key,
                    &bincode::serialize(&auth_credential_presentation).unwrap(),
                    Box::new(show_result),
                )
            }
            Some(response) => {
                println!("failed: {}", response.status);
                prompt("\n> ");
            }
            None => {
                println!("request failed");
                prompt("\n> ");
            }
        }),
    )
}

fn show_result(result: Result<CallLinkState, http::ResponseStatus>) {
    match result {
        Ok(state) => println!("{state:#?}"),
        Err(status) => println!("failed: {status}"),
    }
    prompt("\n> ");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let url: &'static str = args
        .get(1)
        .map(|s| &*Box::leak(s.clone().into_boxed_str()))
        .unwrap_or("http://localhost:8090");
    let zkparams_base64 = args.get(2).map(String::as_str).unwrap_or(DEFAULT_ZKPARAMS);
    let server_zkparams: zkgroup::generic_server_params::GenericServerSecretParams =
        bincode::deserialize(
            &base64
                .decode(zkparams_base64)
                .expect("zkparams should be valid base64"),
        )
        .expect("zkparams should be a valid GenericServerSecretParams (not public!)");
    let public_zkparams = server_zkparams.get_public_params();
    let user_id = Uuid::from_bytes(USER_ID).into();

    log::set_logger(&LOG).expect("set logger");
    log::set_max_level(log::LevelFilter::Info);

    let http_client = http_client::HttpClient::start();

    prompt("> ");
    for line in std::io::stdin().lines() {
        let words: Vec<&str> = line.as_ref().unwrap().split_ascii_whitespace().collect();
        match &words[..] {
            [] => {
                prompt("> ");
            }
            ["help"] => {
                println!(
                    "
Available commands are:

help                         - show this message
create <id>                  - create a new link
read <id>                    - fetch the current state of a link
set-title <id> <new-title>   - change the title of a link
admin-approval <id> (on|off) - turn on/off admin approval for a link
reset-approvals <id>         - reset a link's list of approved users (if the server has this enabled)
reset-expiration <id>        - reset a link's expiration (if the server has this enabled)
root-key <id>                - print the root key for a link
exit                         - quit

<id> can be any word you want; it is hashed to produce a root key.
The admin passkey for any created links is a constant {ADMIN_PASSKEY:?}.
"
                );
                prompt("> ");
            }
            ["create", id] => {
                let root_key = root_key_from_id(id);
                let room_id = root_key.derive_room_id();
                let create_credential_request_context =
                    zkgroup::call_links::CreateCallLinkCredentialRequestContext::new(
                        &room_id,
                        rand::random(),
                    );
                let create_credential_response =
                    create_credential_request_context.get_request().issue(
                        user_id,
                        start_of_today_in_epoch_seconds(),
                        &server_zkparams,
                        rand::random(),
                    );
                let create_credential = create_credential_request_context
                    .receive(create_credential_response, user_id, &public_zkparams)
                    .unwrap();
                let call_link_zkparams =
                    CallLinkSecretParams::derive_from_root_key(&root_key.bytes());
                let create_credential_presentation = create_credential.present(
                    &room_id,
                    user_id,
                    &public_zkparams,
                    &call_link_zkparams,
                    rand::random(),
                );
                ringrtc::lite::call_links::create_call_link(
                    &http_client,
                    url,
                    root_key,
                    &bincode::serialize(&create_credential_presentation).unwrap(),
                    ADMIN_PASSKEY,
                    &bincode::serialize(&call_link_zkparams.get_public_params()).unwrap(),
                    Box::new(show_result),
                );
            }
            ["read", id] => {
                let root_key = root_key_from_id(id);
                let auth_credential_presentation = issue_and_present_auth_credential(
                    &server_zkparams,
                    &public_zkparams,
                    &root_key,
                );
                ringrtc::lite::call_links::read_call_link(
                    &http_client,
                    url,
                    root_key,
                    &bincode::serialize(&auth_credential_presentation).unwrap(),
                    Box::new(show_result),
                );
            }
            ["set-title", id, new_title] => {
                let root_key = root_key_from_id(id);
                let encrypted_name = root_key.encrypt(new_title.as_bytes(), rand::thread_rng());
                let auth_credential_presentation = issue_and_present_auth_credential(
                    &server_zkparams,
                    &public_zkparams,
                    &root_key,
                );
                ringrtc::lite::call_links::update_call_link(
                    &http_client,
                    url,
                    root_key,
                    &bincode::serialize(&auth_credential_presentation).unwrap(),
                    &CallLinkUpdateRequest {
                        admin_passkey: ADMIN_PASSKEY,
                        encrypted_name: Some(&encrypted_name),
                        ..CallLinkUpdateRequest::default()
                    },
                    Box::new(show_result),
                );
            }
            ["admin-approval", id, on_or_off @ ("on" | "off")] => {
                let root_key = root_key_from_id(id);
                let auth_credential_presentation = issue_and_present_auth_credential(
                    &server_zkparams,
                    &public_zkparams,
                    &root_key,
                );
                let restrictions = if *on_or_off == "on" {
                    CallLinkRestrictions::AdminApproval
                } else {
                    CallLinkRestrictions::None
                };
                ringrtc::lite::call_links::update_call_link(
                    &http_client,
                    url,
                    root_key,
                    &bincode::serialize(&auth_credential_presentation).unwrap(),
                    &CallLinkUpdateRequest {
                        admin_passkey: ADMIN_PASSKEY,
                        restrictions: Some(restrictions),
                        ..CallLinkUpdateRequest::default()
                    },
                    Box::new(show_result),
                );
            }
            ["reset-approvals", id] => {
                make_testing_request(
                    id,
                    &server_zkparams,
                    &public_zkparams,
                    &http_client,
                    url,
                    http::Method::Delete,
                    "/v1/call-link/approvals",
                );
            }
            ["reset-expiration", id] => {
                make_testing_request(
                    id,
                    &server_zkparams,
                    &public_zkparams,
                    &http_client,
                    url,
                    http::Method::Post,
                    "/v1/call-link/reset-expiration",
                );
            }
            ["root-key", id] => {
                let root_key = root_key_from_id(id);
                println!("{}\n", root_key.to_formatted_string());
                prompt("> ");
            }
            ["exit" | "quit"] => {
                break;
            }
            _ => {
                println!("Couldn't parse that.\n");
                prompt("> ");
            }
        }
    }
}
