//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{collections::HashMap, fmt::Debug, ops::Deref};

use aes_gcm_siv::{
    Aes256GcmSiv, Key, KeyInit,
    aead::{Aead, AeadCore, AeadInPlace, generic_array::typenum::Unsigned},
};
use anyhow::{anyhow, bail};
use hkdf::Hkdf;
use rand::{CryptoRng, RngCore};
use sha2::Sha256;
use zkgroup::call_links::CallLinkSecretParams;

use super::base16;
use crate::lite::call_links::CallLinkResponse;

const CRN_LEN: usize = CallLinkSecretParams::ROOT_KEY_MAX_BYTES_FOR_SHO;
const VERSION_OFFSET: usize = CRN_LEN;

#[derive(Debug)]
pub enum CallLinkRootKeyError {
    InvalidServerResponse,
}

pub trait CallLinkRootKeyOps {
    fn is_valid(&self) -> bool;

    /// Returns the raw byte representation of the root key.
    fn as_slice(&self) -> &[u8];

    /// Constructs an AES-GCM-SIV with a 256-bit key.
    fn make_cipher(&self) -> Aes256GcmSiv;

    /// Formats the root key. The returned string is shown to the user. The format varies,
    /// depending on the root key version.
    fn to_formatted_string(&self) -> String;

    /// Invoked to prepare any additional HTTP headers before the request is dispatched
    /// to the SFU front end.
    fn prepare_http_headers(&self, _headers: &mut HashMap<String, String>) {}

    /// Invoked to process HTTP responses. Depending on key type, this method may return
    /// a modified call link root key. The default implementation always returns
    /// `Ok(None)`.
    fn process_server_response(
        &self,
        _response: &CallLinkResponse,
    ) -> Result<Option<CallLinkRootKey>, CallLinkRootKeyError> {
        Ok(None)
    }
}

/// The original call link root key. The length of the key is 16 bytes. The key
/// does not contain any server generated information.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CallLinkRootKeyV0 {
    crn: [u8; CRN_LEN],
}

impl CallLinkRootKeyV0 {
    const LEN: usize = CRN_LEN;
    const HKDF_INFO_CIPHER: &[u8] = b"20230501-Signal-CallLinkRootKey-AES";
}

impl CallLinkRootKeyOps for CallLinkRootKeyV0 {
    fn is_valid(&self) -> bool {
        true
    }

    fn as_slice(&self) -> &[u8] {
        &self.crn
    }

    fn make_cipher(&self) -> Aes256GcmSiv {
        let mut key = Key::<Aes256GcmSiv>::default();
        Hkdf::<Sha256>::new(None, &self.crn)
            .expand(Self::HKDF_INFO_CIPHER, &mut key)
            .expect("valid output length");
        Aes256GcmSiv::new(&key)
    }

    fn to_formatted_string(&self) -> String {
        format!(
            "{:-^.2}",
            base16::ConsonantBase16::from(self.crn.as_slice())
        )
    }
}

impl TryFrom<&str> for CallLinkRootKeyV0 {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        const SEPARATORS: [usize; 8] = [2, 2, 2, 2, 2, 2, 2, 2];
        let bytes = base16::ConsonantBase16::parse_with_separators(value, SEPARATORS)
            .map_err(|_| anyhow!("invalid root key string"))?;
        Self::try_from(bytes.as_slice())
    }
}

impl TryFrom<&[u8]> for CallLinkRootKeyV0 {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        let crn: [u8; CRN_LEN] = value.try_into()?;
        if has_repeated_chunk(&crn) {
            bail!("invalid root key adjacent bytes");
        }
        Ok(Self { crn })
    }
}

/// The V1 call link root key. The key is 25 bytes long and, once fully initialized,
/// contains the server generated epoch value.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CallLinkRootKeyV1 {
    bytes: [u8; Self::LEN],
}

impl CallLinkRootKeyV1 {
    const VERSION: u8 = 1;
    const LEN: usize = CRN_LEN + 1 + Self::EPOCH_LEN;
    const EPOCH_OFFSET: usize = CRN_LEN + 1;
    const EPOCH_LEN: usize = 4;
    const VERSION_MASK: u8 = Self::INVALID_BIT ^ 0xff;
    const INVALID_BIT: u8 = 0x80;
    const HTTP_EPOCH_HEADER: &str = "X-Epoch";
    const HKDF_INFO_CIPHER: &[u8] = b"20251219-Signal-CallLinkRootKey-AES";

    // Creates a `CallLinkRootKeyV1` instance without an epoch. As such, the created root key
    // is invalid and should only be used for room identifier and secret params generation in
    // preparation for call link creation on the server.
    fn without_epoch(crn: &[u8; CRN_LEN]) -> Self {
        let mut bytes = [0u8; Self::LEN];
        bytes[..CRN_LEN].copy_from_slice(crn);
        bytes[VERSION_OFFSET] = Self::VERSION | Self::INVALID_BIT;
        Self { bytes }
    }

    // Creates a valid `CallLinkRootKeyV1` instance.
    fn with_epoch(crn: &[u8; CRN_LEN], epoch: u32) -> Self {
        let mut bytes = [0u8; Self::LEN];
        bytes[..CRN_LEN].copy_from_slice(crn);
        bytes[VERSION_OFFSET] = Self::VERSION;
        bytes[Self::EPOCH_OFFSET..Self::EPOCH_OFFSET + Self::EPOCH_LEN]
            .copy_from_slice(&epoch.to_be_bytes());
        Self { bytes }
    }
}

impl CallLinkRootKeyOps for CallLinkRootKeyV1 {
    fn is_valid(&self) -> bool {
        self.bytes[VERSION_OFFSET] & Self::INVALID_BIT == 0
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    fn make_cipher(&self) -> Aes256GcmSiv {
        let mut key = Key::<Aes256GcmSiv>::default();
        Hkdf::<Sha256>::new(None, &self.bytes[..CRN_LEN])
            .expand(Self::HKDF_INFO_CIPHER, &mut key)
            .expect("valid output length");
        Aes256GcmSiv::new(&key)
    }

    fn prepare_http_headers(&self, headers: &mut HashMap<String, String>) {
        if self.is_valid() {
            let epoch_bytes = &self.bytes[Self::EPOCH_OFFSET..Self::EPOCH_OFFSET + Self::EPOCH_LEN];
            let epoch = u32::from_be_bytes(epoch_bytes.try_into().unwrap());
            headers.insert(Self::HTTP_EPOCH_HEADER.to_string(), epoch.to_string());
        }
    }

    fn process_server_response(
        &self,
        response: &CallLinkResponse,
    ) -> Result<Option<CallLinkRootKey>, CallLinkRootKeyError> {
        if self.is_valid() {
            Ok(None)
        } else {
            let crn: [u8; CRN_LEN] = self.bytes[..CRN_LEN].try_into().unwrap();
            let updated_root_key = response.epoch.map_or_else(
                || CallLinkRootKey::V0(CallLinkRootKeyV0 { crn }),
                |epoch| CallLinkRootKey::V1(Self::with_epoch(&crn, epoch)),
            );
            Ok(Some(updated_root_key))
        }
    }

    fn to_formatted_string(&self) -> String {
        format!(
            "{:-^.4}-{}-{}",
            base16::ConsonantBase16::from(&self.bytes[..CRN_LEN]),
            base16::ConsonantBase16::from(&self.bytes[VERSION_OFFSET..VERSION_OFFSET + 1]),
            base16::ConsonantBase16::from(
                &self.bytes[Self::EPOCH_OFFSET..Self::EPOCH_OFFSET + Self::EPOCH_LEN]
            )
        )
    }
}

impl TryFrom<&str> for CallLinkRootKeyV1 {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        const SEPARATORS: [usize; 6] = [4, 4, 4, 4, 1, 4];
        let bytes = base16::ConsonantBase16::parse_with_separators(value, SEPARATORS)
            .map_err(|_| anyhow!("invalid root key string"))?;
        Self::try_from(bytes.as_slice())
    }
}

impl TryFrom<&[u8]> for CallLinkRootKeyV1 {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        let bytes: [u8; Self::LEN] = value.try_into()?;
        if bytes[VERSION_OFFSET] & Self::VERSION_MASK != Self::VERSION {
            bail!("invalid root key version");
        }
        if has_repeated_chunk(&bytes[..CRN_LEN]) {
            bail!("invalid root key adjacent bytes");
        }
        Ok(Self { bytes })
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum CallLinkRootKey {
    V0(CallLinkRootKeyV0),
    V1(CallLinkRootKeyV1),
}

impl Debug for CallLinkRootKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_formatted_string())
    }
}

impl Deref for CallLinkRootKey {
    type Target = dyn CallLinkRootKeyOps;

    fn deref(&self) -> &Self::Target {
        match self {
            CallLinkRootKey::V0(v0) => v0,
            CallLinkRootKey::V1(v1) => v1,
        }
    }
}

#[derive(Debug)]
pub struct FailedToDecrypt;

impl CallLinkRootKey {
    const HKDF_INFO_ROOM_ID: &[u8] = b"20230501-Signal-CallLinkRootKey-RoomId";
    const HTTP_ROOM_ID_HEADER: &str = "X-Room-Id";
    const ENCRYPTION_PADDING_MARKER: u8 = 0x80;
    const ENCRYPTION_BLOCK_SIZE: usize = 32;

    /// Generates a call link root key. The latest version of the key is generated.
    pub fn generate(mut rng: impl RngCore + CryptoRng) -> Self {
        // Repeatedly generate CRN bytes until there are no groups of two bytes that share
        // four of the same hex digits. The chances of having to do more than three total
        // generations are 8 in 1 billion.
        let mut crn = [0u8; CRN_LEN];
        rng.fill_bytes(&mut crn[..CRN_LEN]);
        while has_repeated_chunk(&crn[..CRN_LEN]) {
            rng.fill_bytes(&mut crn[..CRN_LEN]);
        }
        Self::V1(CallLinkRootKeyV1::without_epoch(&crn))
    }

    /// Generates an admin passkey. There are no constraints on the admin passkey, other than
    /// not being unreasonably long. It's never shown to users.
    pub fn generate_admin_passkey(mut rng: impl RngCore + CryptoRng) -> Vec<u8> {
        let mut result = [0; 16];
        rng.fill_bytes(&mut result);
        result.to_vec()
    }

    /// Derives the room identifier. There are no constraints on the room ID, other than
    /// not being unreasonably long. It's never shown to users, but it does appear in
    /// HTTP requests to the calling server.
    pub fn derive_room_id(&self) -> Vec<u8> {
        let mut room_id_bytes = [0u8; 32];
        Hkdf::<Sha256>::new(None, &self.as_slice()[..CRN_LEN])
            .expand(Self::HKDF_INFO_ROOM_ID, &mut room_id_bytes)
            .expect("valid output length");
        room_id_bytes.to_vec()
    }

    pub fn encrypt(&self, plaintext: &[u8], mut rng: impl RngCore + CryptoRng) -> Vec<u8> {
        // Pad similarly to Signal messages: append 0x80, then zero or more 0x00 bytes.
        // The final buffer will be (nonce || encrypt(plaintext || 0x80 || 0x00*) || tag).
        let mut buffer = Vec::new();
        let mut nonce = [0; <Aes256GcmSiv as AeadCore>::NonceSize::USIZE];
        rng.fill_bytes(&mut nonce);
        buffer.extend_from_slice(&nonce);
        buffer.extend_from_slice(plaintext);
        buffer.push(Self::ENCRYPTION_PADDING_MARKER);
        let padding_len = Self::ENCRYPTION_BLOCK_SIZE - buffer.len() % Self::ENCRYPTION_BLOCK_SIZE;
        if padding_len != Self::ENCRYPTION_BLOCK_SIZE {
            buffer.resize(buffer.len() + padding_len, 0);
        }
        let tag = (**self)
            .make_cipher()
            .encrypt_in_place_detached(&nonce.into(), &[], &mut buffer[nonce.len()..])
            .expect("can encrypt arbitrary data");
        buffer.extend_from_slice(&tag);
        buffer
    }

    pub fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, FailedToDecrypt> {
        let nonce_len = <Aes256GcmSiv as AeadCore>::NonceSize::USIZE;
        if encrypted.len() < nonce_len {
            return Err(FailedToDecrypt);
        }
        let (nonce, ciphertext) = encrypted.split_at(nonce_len);
        let mut plaintext = (**self)
            .make_cipher()
            .decrypt(nonce.into(), ciphertext)
            .map_err(|_| FailedToDecrypt)?;
        let padding_marker_position = plaintext
            .iter()
            .rposition(|&b| b == Self::ENCRYPTION_PADDING_MARKER)
            .ok_or(FailedToDecrypt)?;
        plaintext.truncate(padding_marker_position);
        Ok(plaintext)
    }

    pub fn is_valid(&self) -> bool {
        (**self).is_valid()
    }

    pub fn as_slice(&self) -> &[u8] {
        (**self).as_slice()
    }

    pub fn prepare_http_headers(&self, headers: &mut HashMap<String, String>) {
        let room_id = self.derive_room_id();
        headers.insert(Self::HTTP_ROOM_ID_HEADER.to_string(), hex::encode(room_id));
        (**self).prepare_http_headers(headers);
    }

    pub fn process_server_response(
        &self,
        response: &CallLinkResponse,
    ) -> Result<Option<CallLinkRootKey>, CallLinkRootKeyError> {
        (**self).process_server_response(response)
    }

    // Not a Display implementation so we don't accidentally log it.
    pub fn to_formatted_string(&self) -> String {
        (**self).to_formatted_string()
    }
}

impl TryFrom<&str> for CallLinkRootKey {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        if let Ok(inner) = CallLinkRootKeyV0::try_from(value) {
            Ok(Self::V0(inner))
        } else if let Ok(inner) = CallLinkRootKeyV1::try_from(value) {
            Ok(Self::V1(inner))
        } else {
            bail!("invalid root key string")
        }
    }
}

impl TryFrom<&[u8]> for CallLinkRootKey {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        // Currently, differentiation based on link length is sufficient.
        let key = match value.len() {
            CallLinkRootKeyV0::LEN => Self::V0(CallLinkRootKeyV0::try_from(value)?),
            CallLinkRootKeyV1::LEN => Self::V1(CallLinkRootKeyV1::try_from(value)?),
            _ => {
                bail!("invalid root key");
            }
        };
        Ok(key)
    }
}

/// Checks if an even-odd adjacent pair of bytes share four of the same hex digits (e.g.
/// 0x5555).
///
/// This avoids keys that "don't look random". About 2 in 1,000 possible 16-byte strings have at
/// least one repeated chunk.
fn has_repeated_chunk(bytes: &[u8]) -> bool {
    for pair in bytes.chunks_exact(2) {
        if u16::from_le_bytes(pair.try_into().unwrap()) % 0x1111 == 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use zkgroup::call_links::CallLinkSecretParams;

    use super::*;
    use crate::lite::call_links::CallLinkRestrictions;

    #[test]
    fn test_repeated_chunks_present_in_byte_slice() {
        assert!(has_repeated_chunk(&[0x11, 0x11]));
        assert!(has_repeated_chunk(&[0x11, 0x11, 0x23, 0x45]));
        assert!(has_repeated_chunk(&[0x23, 0x45, 0x56, 0x11, 0x55, 0x55]));
        assert!(!has_repeated_chunk(&[]));
        assert!(!has_repeated_chunk(&[0x12, 0x34, 0x45, 0x46]));
        assert!(!has_repeated_chunk(&[0x11, 0x22, 0x33, 0x44, 0x45, 0x55]));
    }

    #[test]
    fn test_round_trip_random() {
        for _ in 0..100 {
            let key = CallLinkRootKey::generate(rand::thread_rng());
            let formatted = key.to_formatted_string();
            let round_trip_key = CallLinkRootKey::try_from(formatted.as_str()).unwrap();
            assert_eq!(key.as_slice(), round_trip_key.as_slice(), "{formatted}")
        }
    }

    #[test]
    fn test_correct_root_key_from_string() {
        let key_v0_parsed =
            CallLinkRootKey::try_from("crbn-mxzp-zprt-rxkr-mpqq-rtrr-cddc-bftt").unwrap();
        assert!(key_v0_parsed.is_valid());
        assert!(matches!(key_v0_parsed, CallLinkRootKey::V0(_)));

        let key_v1_valid_parsed =
            CallLinkRootKey::try_from("crbnmxzp-zprtrxkr-mpqqrtrr-cddcbftt-bc-pprrrrqz").unwrap();
        assert!(key_v1_valid_parsed.is_valid());
        assert!(matches!(key_v1_valid_parsed, CallLinkRootKey::V1(_)));

        let key_v1_invalid_parsed =
            CallLinkRootKey::try_from("crbnmxzp-zprtrxkr-mpqqrtrr-cddcbftt-nc-bbbbbbbb").unwrap();
        assert!(!key_v1_invalid_parsed.is_valid());
        assert!(matches!(key_v1_invalid_parsed, CallLinkRootKey::V1(_)));
    }

    #[test]
    fn test_key_version_downgrade() {
        let key = CallLinkRootKey::generate(rand::thread_rng());
        let room_id = key.derive_room_id();
        let auth_params = CallLinkSecretParams::derive_from_root_key(key.as_slice());

        let encrypted_name = key.encrypt("Some key".as_bytes(), rand::thread_rng());
        let response = CallLinkResponse {
            encrypted_name: &encrypted_name,
            restrictions: CallLinkRestrictions::None,
            revoked: false,
            expiration_unix_timestamp: 0,
            epoch: None,
        };

        let updated_key = key.process_server_response(&response).unwrap().unwrap();
        let updated_room_id = updated_key.derive_room_id();
        let updated_auth_params =
            CallLinkSecretParams::derive_from_root_key(updated_key.as_slice());

        assert_eq!(room_id, updated_room_id);
        assert_eq!(auth_params.as_ref().a1, updated_auth_params.as_ref().a1);
        assert_eq!(auth_params.as_ref().a2, updated_auth_params.as_ref().a2);
        assert!(auth_params.as_ref().public_key == auth_params.as_ref().public_key);
    }

    #[test]
    fn test_encrypt() {
        let key = CallLinkRootKey::generate(rand::thread_rng());
        let plaintext = b"Secret Hideout";
        let ciphertext = key.encrypt(plaintext, rand::thread_rng());
        assert_eq!(
            plaintext.as_slice(),
            key.decrypt(&ciphertext).unwrap().as_slice()
        );

        // Check that we do use a random nonce.
        let ciphertext_repeated = key.encrypt(plaintext, rand::thread_rng());
        assert_ne!(ciphertext, ciphertext_repeated, "not salted");
        assert_ne!(
            ciphertext[..plaintext.len()],
            ciphertext_repeated[..plaintext.len()],
            "not salted"
        );

        // Check that we do pad short titles.
        let different_ciphertext = key.encrypt(b"Secret Base", rand::thread_rng());
        assert_eq!(ciphertext.len(), different_ciphertext.len(),);
    }
}
