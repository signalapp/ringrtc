//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

pub fn string_to_uuid(id: &str) -> Result<Vec<u8>, anyhow::Error> {
    if id.len() != 32 && id.len() != 36 {
        return Err(anyhow::anyhow!(
            "Expected string to be 32 or 36 characters long."
        ));
    }

    Ok(hex::decode(id.replace('-', ""))?)
}
