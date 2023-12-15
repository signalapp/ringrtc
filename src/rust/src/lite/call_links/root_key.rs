//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use aes_gcm_siv::{
    aead::{generic_array::typenum::Unsigned, Aead, AeadCore, AeadInPlace},
    Aes256GcmSiv, Key, KeyInit,
};
use anyhow::{anyhow, bail};
use hkdf::Hkdf;
use rand::{CryptoRng, RngCore};
use sha2::Sha256;

use super::base16;

#[derive(Clone)]
pub struct CallLinkRootKey {
    bytes: [u8; 16],
}

#[derive(Debug)]
pub struct FailedToDecrypt;

impl CallLinkRootKey {
    /// Checks if an even-odd adjacent pair of bytes share four of the same hex digits (e.g.
    /// 0x5555).
    ///
    /// This avoids keys that "don't look random". About 2 in 1,000 possible 16-byte strings have at
    /// least one repeated chunk.
    fn has_repeated_chunk(bytes: &[u8; 16]) -> bool {
        for pair in bytes.chunks_exact(2) {
            // Even though this is defined using a modulo operation, it gets optimized down quite well!
            // The final assembly is a shift, a subtraction, and a comparison.
            // https://play.rust-lang.org/?version=stable&mode=release&edition=2021&code=pub+fn+test%28x%3A+u16%29+-%3E+bool+%7B%0A++++x+%25+0x1111+%3D%3D+0%0A%7D
            if u16::from_le_bytes(pair.try_into().unwrap()) % 0x1111 == 0 {
                return true;
            }
        }
        false
    }

    pub fn generate(mut rng: impl RngCore + CryptoRng) -> Self {
        let mut bytes = [0u8; 16];
        rng.fill_bytes(&mut bytes);

        // Try again if any groups of two bytes share four of the same hex digits.
        // The chances of having to do more than three total generations are 8 in 1 billion.
        while Self::has_repeated_chunk(&bytes) {
            rng.fill_bytes(&mut bytes);
        }

        Self { bytes }
    }

    pub fn generate_admin_passkey(mut rng: impl RngCore + CryptoRng) -> Vec<u8> {
        // There are no constraints on the admin passkey, other than not being unreasonably long.
        // It's never shown to users.
        let mut result = [0; 16];
        rng.fill_bytes(&mut result);
        result.to_vec()
    }

    pub fn derive_room_id(&self) -> Vec<u8> {
        // There are no constraints on the room ID, other than not being unreasonably long.
        // It's never shown to users, but it does appear in HTTP requests to the calling server.
        let mut room_id_bytes = [0u8; 32];
        Hkdf::<Sha256>::new(None, &self.bytes)
            .expand(
                b"20230501-Signal-CallLinkRootKey-RoomId",
                &mut room_id_bytes,
            )
            .expect("valid output length");
        room_id_bytes.to_vec()
    }

    fn make_cipher(&self) -> Aes256GcmSiv {
        let mut key = Key::<Aes256GcmSiv>::default();
        Hkdf::<Sha256>::new(None, &self.bytes)
            .expand(b"20230501-Signal-CallLinkRootKey-AES", &mut key)
            .expect("valid output length");
        Aes256GcmSiv::new(&key)
    }

    const ENCRYPTION_PADDING_MARKER: u8 = 0x80;
    const ENCRYPTION_BLOCK_SIZE: usize = 32;

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

        let tag = self
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

        let mut plaintext = self
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

    pub fn bytes(&self) -> [u8; 16] {
        self.bytes
    }

    // Not a Display implementation so we don't accidentally log it.
    pub fn to_formatted_string(&self) -> String {
        format!(
            "{:-^.2}",
            base16::ConsonantBase16::from(self.bytes.as_slice())
        )
    }
}

impl TryFrom<&str> for CallLinkRootKey {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        let bytes = base16::ConsonantBase16::parse_with_separators(value, 2)
            .map_err(|_| anyhow!("invalid root key string"))?;
        Self::try_from(bytes.as_slice())
    }
}

impl TryFrom<&[u8]> for CallLinkRootKey {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        let bytes: [u8; 16] = value.try_into()?;

        if Self::has_repeated_chunk(&bytes) {
            bail!("invalid root key adjacent bytes");
        }

        Ok(Self { bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip_random() {
        for _ in 0..100 {
            let key = CallLinkRootKey::generate(rand::thread_rng());
            let formatted = key.to_formatted_string();
            let round_trip_key = CallLinkRootKey::try_from(formatted.as_str()).unwrap();
            assert_eq!(key.bytes(), round_trip_key.bytes(), "{formatted}")
        }
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
