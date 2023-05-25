//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::collections::VecDeque;

use hex::FromHex;

use crate::{
    core::call_mutex::CallMutex,
    lite::sfu::{MemberResolver, OpaqueUserIdMapping, UserId},
};

use super::CallLinkRootKey;

pub struct CallLinkMemberResolver {
    zkparams: zkgroup::call_links::CallLinkSecretParams,
    cache: CallMutex<VecDeque<OpaqueUserIdMapping>>,
    #[cfg(test)]
    pub cache_hits: std::sync::atomic::AtomicU64,
}

// The proper value for this is "however large the current call is", so that refreshes of the
// current call don't require lots of work, but people who leave the call don't stay forever. But
// having the call inform the member resolver of the current call size would be a bit weird, and
// eventually the current cache implementation (a linear scan) won't scale very well anymore. 16 is
// a compromise assuming that most calls are small, and just relying on falling back to
// re-decrypting for big calls, even if it means the cache will churn unnecessarily in those big
// calls.
const MAX_CACHE_ENTRIES: usize = 16;

impl<'a> From<&'a CallLinkRootKey> for CallLinkMemberResolver {
    fn from(value: &'a CallLinkRootKey) -> Self {
        Self {
            zkparams: zkgroup::call_links::CallLinkSecretParams::derive_from_root_key(
                &value.bytes(),
            ),
            cache: CallMutex::new(VecDeque::new(), "CallLinkMemberResolver.cache"),
            #[cfg(test)]
            cache_hits: Default::default(),
        }
    }
}

impl MemberResolver for CallLinkMemberResolver {
    fn resolve(&self, opaque_user_id: &str) -> Option<UserId> {
        let mut locked_cache = self.cache.lock_or_reset(|_| {
            error!("resetting CallLinkMemberResolver cache after panic");
            VecDeque::default()
        });
        if let Some(mapping) = locked_cache
            .iter()
            .find(|mapping| mapping.opaque_user_id == opaque_user_id)
        {
            #[cfg(test)]
            self.cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(mapping.user_id.clone());
        }

        let ciphertext_bytes = Vec::from_hex(opaque_user_id).ok()?;
        let ciphertext: zkgroup::groups::UuidCiphertext =
            bincode::deserialize(&ciphertext_bytes).ok()?;
        let user_id = self.zkparams.decrypt_uuid(ciphertext).ok()?.to_vec();

        if locked_cache.len() > MAX_CACHE_ENTRIES {
            _ = locked_cache.pop_front();
        }
        locked_cache.push_back(OpaqueUserIdMapping {
            opaque_user_id: opaque_user_id.into(),
            user_id: user_id.clone(),
        });

        Some(user_id)
    }
}
