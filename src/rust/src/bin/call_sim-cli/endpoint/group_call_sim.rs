//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use ringrtc::{
    common::Result,
    native::{GroupUpdate, GroupUpdateHandler},
};

use log::*;

use super::CallEndpoint;

impl GroupUpdateHandler for CallEndpoint {
    fn handle_group_update(&self, update: GroupUpdate) -> Result<()> {
        info!("Group Update {}", update);
        Ok(())
    }
}
