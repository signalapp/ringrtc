//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use log::*;
use ringrtc::webrtc::peer_connection_factory::IceServer;

pub fn string_to_uuid(id: &str) -> Result<Vec<u8>, anyhow::Error> {
    if id.len() != 32 && id.len() != 36 {
        return Err(anyhow::anyhow!(
            "Expected string to be 32 or 36 characters long."
        ));
    }

    Ok(hex::decode(id.replace('-', ""))?)
}

pub fn convert_relay_config_to_ice_servers(
    username: String,
    password: String,
    urls: Vec<String>,
    urls_with_ips: Vec<String>,
    hostname: Option<String>,
) -> Vec<IceServer> {
    let mut ice_servers = vec![];
    let mut index = 1;

    info!("Relay Servers:");
    info!("  username: {}", username);
    info!("  password: {}", password);
    if let Some(hostname) = hostname {
        info!("  hostname: {}", hostname);

        for url in &urls_with_ips {
            info!("  server {}. {}", index, url);
            index += 1;
        }

        ice_servers.push(IceServer::new(
            username.clone(),
            password.clone(),
            hostname,
            urls_with_ips,
        ));
    } else {
        error!("  No hostname provided for urls_with_ips!");
    }

    if !urls.is_empty() {
        for url in &urls {
            info!("  server {}. {}", index, url);
            index += 1;
        }

        ice_servers.push(IceServer::new(username, password, "".to_string(), urls));
    }

    ice_servers
}
