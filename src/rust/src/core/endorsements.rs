//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    collections::{BTreeMap, HashMap},
    time::{Instant, SystemTime},
};

use thiserror::Error;
use zkgroup::groups::GroupSendEndorsement;

use crate::lite::sfu::UserId;

type InvalidMarker = (Instant, String);
pub type EndorsementUpdate = (zkgroup::Timestamp, HashMap<UserId, GroupSendEndorsement>);
pub type EndorsementUpdateRef<'a> = (
    zkgroup::Timestamp,
    &'a HashMap<UserId, GroupSendEndorsement>,
);
pub type EndorsementUpdateResultRef<'a> =
    std::result::Result<EndorsementUpdateRef<'a>, EndorsementUpdateError>;
pub type EndorsementUpdateResult = std::result::Result<EndorsementUpdate, EndorsementUpdateError>;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum EndorsementUpdateError {
    #[error("Missing field '{0}' in endorsements response")]
    MissingField(&'static str),
    #[error("Received expired endorsements with expiration={0:?}")]
    ExpiredEndorsements(zkgroup::Timestamp),
    #[error("Received endorsements with invalid member ciphertexts")]
    InvalidMemberCiphertexts,
    #[error("Failed to deserialize endorsement response")]
    InvalidEndorsementResponseFormat,
    #[error("Failed to validate endorsement response")]
    InvalidEndorsementResponse,
}

/// Caches GroupSendEndorsement ordered by expiration. Also tracks whether the latest endorsements
/// were successfully validated.
#[derive(Clone, Default)]
pub struct EndorsementsCache {
    /// Caches the Endorsement sets in ascending order by expiration. Allows us to grab the latest
    /// set of endorsements easily
    endorsements: BTreeMap<zkgroup::Timestamp, HashMap<UserId, GroupSendEndorsement>>,
    /// Tracks whether an endorsement set is valid. The None marker means the last received endorsement
    /// response was invalid and likely the latest endorsement set is invalid
    invalid_markers: HashMap<Option<zkgroup::Timestamp>, InvalidMarker>,
    /// Tracks the last time an endorsement set was updated
    last_updated: HashMap<zkgroup::Timestamp, Instant>,
    /// Tracks the last time `get_latest` returned valid set of endorsements
    last_shared: Option<Instant>,
}

impl EndorsementsCache {
    /// Extends Endorsement Map with replacement for each UserId for a given expiration.
    pub fn insert(
        &mut self,
        expiration: zkgroup::Timestamp,
        endorsements: HashMap<UserId, GroupSendEndorsement>,
    ) {
        let now = Instant::now();
        self.endorsements
            .entry(expiration)
            .or_default()
            .extend(endorsements);
        self.last_updated.insert(expiration, now);
        self.invalid_markers.remove(&Some(expiration));
        self.invalid_markers.remove(&None);
    }

    /// Gets the endorsements with the latest expirations if valid. The latest expiration should
    /// contain the latest endorsements, but this invariant is maintained by the server, not the client.
    pub fn get_latest(&mut self) -> Option<EndorsementUpdateRef> {
        if !self.latest_is_valid() {
            return None;
        }

        self.last_shared = Some(Instant::now());
        self.endorsements
            .iter()
            .last()
            .map(|(expiration, endorsements)| (*expiration, endorsements))
    }

    /// Checks whether the latest endorsements have a valid update since the last time `get_latest`
    /// was called.
    pub fn has_valid_update(&self) -> bool {
        let Some((&expiration, _)) = &self.endorsements.iter().last() else {
            return false;
        };

        if self.invalid_markers.contains_key(&Some(expiration)) {
            return false;
        }

        if self.invalid_markers.contains_key(&None) {
            return false;
        }

        let Some(&last_updated) = self.last_updated.get(&expiration) else {
            // should never happen
            warn!(
                "Missing last_updated for endorsement set with expiration {:?}",
                expiration
            );
            return false;
        };

        self.last_shared
            .is_none_or(|last_shared| last_updated > last_shared)
    }

    fn latest_is_valid(&self) -> bool {
        let Some((&expiration, _)) = &self.endorsements.iter().last() else {
            return false;
        };

        !self.invalid_markers.contains_key(&Some(expiration))
            && !self.invalid_markers.contains_key(&None)
    }

    pub fn set_invalid(&mut self, expiration: Option<zkgroup::Timestamp>, reason: String) {
        let marker = (Instant::now(), reason);
        self.invalid_markers.insert(expiration, marker);
    }

    /// Removes all endorsements that have expired by now
    pub fn clear_expired(&mut self, now: SystemTime) {
        let expired = self
            .endorsements
            .keys()
            .copied()
            .filter(|&expiration| now >= expiration.into())
            .collect::<Vec<_>>();

        for expiration in expired {
            self.remove(expiration);
        }
    }

    fn remove(&mut self, expiration: zkgroup::Timestamp) {
        self.endorsements.remove(&expiration);
        self.invalid_markers.remove(&Some(expiration));
        self.last_updated.remove(&expiration);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.endorsements.len()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use libsignal_core::Aci;
    use rand::{random, thread_rng};
    use zkgroup::{
        call_links::CallLinkSecretParams,
        groups::{GroupSendDerivedKeyPair, GroupSendEndorsementsResponse},
        RandomnessBytes, ServerSecretParams, Timestamp, UUID_LEN,
    };

    use super::*;
    use crate::lite::call_links::CallLinkRootKey;

    /// generates random secrets to create endorsements, simulates server secret rotation
    fn random_receive_endorsements(
        num_endorsements: usize,
        day_aligned_now: Timestamp,
    ) -> (Timestamp, HashMap<UserId, GroupSendEndorsement>) {
        let randomness: RandomnessBytes = random();
        let expiration = day_aligned_now.add_seconds(24 * 60 * 60);
        let params = ServerSecretParams::generate(randomness);
        let public_params = params.get_public_params();
        let root_key = CallLinkRootKey::generate(thread_rng());
        let call_link_key = CallLinkSecretParams::derive_from_root_key(&root_key.bytes());
        // TODO switch to call link derived keys
        let today_key = GroupSendDerivedKeyPair::for_expiration(expiration, &params);
        let member_ids: Vec<Aci> = (0..num_endorsements as u8)
            .map(|i| Aci::from_uuid_bytes([i; UUID_LEN]))
            .collect::<Vec<_>>();
        let user_ids = member_ids
            .iter()
            .map(|id| id.service_id_binary())
            .collect::<Vec<_>>();
        let member_ciphertexts = member_ids
            .iter()
            .map(|&id| call_link_key.encrypt_uid(id))
            .collect::<Vec<_>>();

        let response =
            GroupSendEndorsementsResponse::issue(member_ciphertexts.clone(), &today_key, random());

        let endorsements = response
            .receive_with_ciphertexts(member_ciphertexts, day_aligned_now, public_params)
            .unwrap();
        (
            expiration,
            user_ids
                .into_iter()
                .zip(endorsements.into_iter().map(|e| e.decompressed))
                .collect(),
        )
    }

    #[test]
    fn extends_with_replacement() {
        let mut cache = EndorsementsCache::default();
        assert_eq!(cache.get_latest(), None, "Cache should start empty");

        let now = Timestamp::from_epoch_seconds(0);
        let (expiration, received_endorsements) = random_receive_endorsements(2, now);
        cache.insert(expiration, received_endorsements.clone());
        assert_eq!(
            cache.get_latest(),
            Some((expiration, &received_endorsements)),
            "should find received endorsements"
        );

        // Add a new user, and update a previous user, but keep an old user's value
        let user_to_not_update = received_endorsements.iter().last().unwrap().0;
        let expected_endorsements = {
            let (_, mut endorsements) = random_receive_endorsements(3, now);
            endorsements.insert(
                user_to_not_update.clone(),
                *received_endorsements.get(user_to_not_update).unwrap(),
            );
            endorsements
        };
        let endorsements_update = expected_endorsements
            .clone()
            .into_iter()
            .filter(|(user, _)| user != user_to_not_update)
            .collect();

        cache.insert(expiration, endorsements_update);
        assert_eq!(
            cache.get_latest(),
            Some((expiration, &expected_endorsements)),
            "should find new endorsements, with one not updated endorsement"
        );
    }

    #[test]
    fn get_latest_gets_latest_expiration_if_valid() {
        let mut cache = EndorsementsCache::default();
        assert_eq!(cache.get_latest(), None, "Cache should start empty");

        let nows = (0..5).map(|i| Timestamp::from_epoch_seconds(i * (24 * 60 * 60)));
        let endorsement_generations = nows
            .into_iter()
            .map(|now| random_receive_endorsements(3, now))
            .collect::<Vec<_>>();
        for (expiration, endorsements) in endorsement_generations.iter() {
            cache.insert(*expiration, endorsements.clone());
            assert_eq!(
                cache.get_latest(),
                Some((*expiration, endorsements)),
                "should get endorsements with latest expiration"
            );
        }

        let expected_endorsements = endorsement_generations
            .iter()
            .last()
            .map(|(expiration, endorsements)| (*expiration, endorsements));
        for (expiration, endorsements) in endorsement_generations.iter() {
            cache.insert(*expiration, endorsements.clone());
            assert_eq!(
                cache.get_latest(),
                expected_endorsements,
                "none of the inserted endorsements have a later expiration, expected endorsements do not change"
            );
        }

        let (expiration, endorsements) = expected_endorsements.unwrap();
        cache.set_invalid(Some(expiration), "Test making latest invalid".to_string());
        assert_eq!(
            cache.get_latest(),
            None,
            "Latest is specifically marked invalid and should not return a result"
        );
        cache.insert(expiration, endorsements.clone());
        assert_eq!(
            cache.get_latest(),
            Some((expiration, endorsements)),
            "Latest should be valid after being reinserted"
        );
        cache.set_invalid(None, "Test marking cache generally invalid".to_string());
        assert_eq!(
            cache.get_latest(),
            None,
            "Cache is marked generally invalid and should not return a result"
        );
        cache.insert(expiration, endorsements.clone());
        assert_eq!(
            cache.get_latest(),
            Some((expiration, endorsements)),
            "Latest should be valid after being reinserted"
        );
    }

    #[test]
    fn has_update_tracks_validity_and_last_shared() {
        let mut cache = EndorsementsCache::default();
        assert!(!cache.has_valid_update(), "cache is empty, no update");

        let nows = (0..5).map(|i| Timestamp::from_epoch_seconds(i * (24 * 60 * 60)));
        let mut endorsement_generations = nows
            .into_iter()
            .map(|now| random_receive_endorsements(3, now));

        let (expiration, endorsements) = endorsement_generations.next().unwrap();
        cache.insert(expiration, endorsements.clone());
        assert!(
            cache.has_valid_update(),
            "true = latest endorsements were updated"
        );
        assert!(
            cache.has_valid_update(),
            "checking multiple times does not change state"
        );
        assert_eq!(cache.get_latest(), Some((expiration, &endorsements)));
        assert!(
            !cache.has_valid_update(),
            "false = update was already shared"
        );

        cache.insert(expiration, endorsements.clone());
        assert!(
            cache.has_valid_update(),
            "true = latest endorsements were updated"
        );
        cache.set_invalid(Some(expiration), "Test making latest invalid".to_string());
        assert!(
            !cache.has_valid_update(),
            "false = latest endorsements are invalid"
        );
        cache.insert(expiration, endorsements.clone());
        assert!(
            cache.has_valid_update(),
            "true = latest endorsements are now valid again"
        );

        cache.set_invalid(None, "Test making cache invalid".to_string());
        assert!(!cache.has_valid_update(), "false = cache is invalid");
        cache.insert(expiration, endorsements.clone());
        assert!(
            cache.has_valid_update(),
            "true = latest endorsements are now valid again"
        );
        assert_eq!(cache.get_latest(), Some((expiration, &endorsements)));

        assert!(!cache.has_valid_update(), "false = latest already shared");
        let (new_expiration, new_endorsements) = endorsement_generations.next().unwrap();
        cache.insert(new_expiration, new_endorsements.clone());
        assert!(
            cache.has_valid_update(),
            "true = new latest since last share"
        );
        assert_eq!(
            cache.get_latest(),
            Some((new_expiration, &new_endorsements))
        );
        assert!(!cache.has_valid_update(), "false = latest already shared");

        cache.insert(expiration, endorsements.clone());
        assert!(
            !cache.has_valid_update(),
            "false = updated old endorsement set, not latest"
        );
    }

    #[test]
    fn clear_expired() {
        let mut cache = EndorsementsCache::default();
        assert!(!cache.has_valid_update(), "cache is empty, no update");

        let nows = (0..5).map(|i| Timestamp::from_epoch_seconds(i * (24 * 60 * 60)));
        let mut expirations = vec![];
        let endorsement_generations = nows
            .into_iter()
            .map(|now| random_receive_endorsements(3, now))
            .collect::<Vec<_>>();

        for (i, (expiration, endorsements)) in endorsement_generations.iter().enumerate() {
            expirations
                .push(SystemTime::UNIX_EPOCH + Duration::from_secs(expiration.epoch_seconds()));
            cache.insert(*expiration, endorsements.clone());
            assert_eq!(cache.len(), i + 1);
        }

        let (last_expiration, last_endorsements) =
            endorsement_generations.into_iter().last().unwrap();

        let initial_size = cache.len();
        for (i, expiration) in expirations.into_iter().enumerate() {
            cache.clear_expired(expiration);
            assert_eq!(cache.len(), initial_size - i - 1);
            if cache.len() != 0 {
                assert_eq!(
                    cache.get_latest(),
                    Some((last_expiration, &last_endorsements))
                );
            }
        }
    }
}
