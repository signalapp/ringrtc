//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::fmt::Write;

const ALPHABET: &[u8; 16] = b"bcdfghkmnpqrstxz";

/// A lookup table from byte to string representation.
///
/// We store the strings as a pair of ASCII characters rather than `&str` to save binary size.
struct Base16Table([[u8; 2]; 256]);

impl Base16Table {
    const fn new(alphabet: &[u8; 16]) -> Self {
        let mut table = [[0; 2]; 256];
        // const fn doesn't support for-loops, since those use the Iterator trait and traits can't
        // be const yet. Use while-loops instead.
        let mut i = 0;
        while i < 16 {
            assert!(alphabet[i].is_ascii());
            let mut j = 0;
            while j < 16 {
                table[i * 16 + j] = [alphabet[i], alphabet[j]];
                j += 1;
            }
            i += 1;
        }
        Self(table)
    }
}

impl std::ops::Index<u8> for Base16Table {
    type Output = str;

    fn index(&self, index: u8) -> &Self::Output {
        let ascii = &self.0[usize::from(index)];
        debug_assert!(ascii.is_ascii());
        unsafe { std::str::from_utf8_unchecked(ascii) }
    }
}

static TABLE: Base16Table = Base16Table::new(ALPHABET);

/// Provides a Display implementation to print a buffer using "consonant base 16", a base-16
/// alphabet of only lowercase ASCII consonants.
///
/// Specifying a _precision_ in the format (`{:.2}`) inserts a punctuator character between every N
/// bytes; specifying a _fill_ (and ignored alignment, `{:-^.2}`) controls that punctuator. `-` is
/// recommended.
pub struct ConsonantBase16<'a>(&'a [u8]);

impl std::fmt::Display for ConsonantBase16<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut is_first = true;
        for chunk in self.0.chunks(f.precision().unwrap_or(usize::MAX)) {
            if is_first {
                is_first = false;
            } else {
                f.write_char(f.fill())?;
            }
            for byte in chunk {
                f.write_str(&TABLE[*byte])?;
            }
        }
        Ok(())
    }
}

impl<'a> From<&'a [u8]> for ConsonantBase16<'a> {
    fn from(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    OddLengthInput,
    MissingSeparator(usize),
    UnexpectedSeparator(usize),
    BadCharacter(usize, char),
}

const SKIP_ALPHABET: &[u8] = b"-"; // only skip "-"

impl ConsonantBase16<'_> {
    #[allow(dead_code)]
    pub fn parse(string: &str) -> Result<Vec<u8>, DecodeError> {
        Self::parse_with_separators(string, usize::MAX)
    }

    /// Attempts to decode a consonant-base-16-encoded string to a sequence of bytes.
    ///
    /// There must be a `-` every `chunk_size` bytes (note: bytes, not characters).
    pub fn parse_with_separators(string: &str, chunk_size: usize) -> Result<Vec<u8>, DecodeError> {
        let mut result = Vec::new();

        let identify_bad_char = |iter: &std::slice::Iter<'_, u8>| {
            let position = string.len() - (iter.len() + 1);
            let bad_char = string[position..]
                .chars()
                .next()
                .expect("at least one char here");
            if bad_char.is_ascii() && SKIP_ALPHABET.contains(&(bad_char as u8)) {
                DecodeError::UnexpectedSeparator(position)
            } else {
                DecodeError::BadCharacter(position, bad_char)
            }
        };

        let mut bytes = string.as_bytes().iter();
        let mut count_until_separator = chunk_size;
        while let Some(first) = bytes.next() {
            if count_until_separator == 0 {
                if !SKIP_ALPHABET.contains(first) {
                    let position = string.len() - (bytes.len() + 1);
                    return Err(DecodeError::MissingSeparator(position));
                }
                if bytes.len() == 0 {
                    return Err(DecodeError::UnexpectedSeparator(string.len() - 1));
                }
                count_until_separator = chunk_size;
                continue;
            }

            // The compiler is smart enough to turn this into a jump table.
            let upper = ALPHABET
                .iter()
                .position(|b| b == first)
                .ok_or_else(|| identify_bad_char(&bytes))? as u8;

            let second = bytes.next().ok_or(DecodeError::OddLengthInput)?;
            let lower = ALPHABET
                .iter()
                .position(|b| b == second)
                .ok_or_else(|| identify_bad_char(&bytes))? as u8;

            result.push((upper << 4) + lower);
            count_until_separator -= 1;
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        assert_eq!(
            ConsonantBase16::from([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10].as_slice())
                .to_string(),
            "zxtsrqpnmkhgfdcb"
        );
    }

    #[test]
    fn test_round_trip() {
        #[track_caller]
        fn round_trip(value: &[u8]) {
            let encoded = ConsonantBase16::from(value).to_string();
            let decoded = ConsonantBase16::parse(&encoded)
                .unwrap_or_else(|err| panic!("just encoded, but got {err:?}: {encoded:?}"));
            assert_eq!(value, &decoded, "{encoded:?}");
        }

        round_trip(&[]);
        round_trip(&[1]);
        round_trip(&[1, 2]);
        round_trip(&[1, 2, 3]);

        round_trip(b"something reasonable");
        round_trip(&Vec::from_iter(0..=u8::MAX));
    }

    #[test]
    fn test_handles_dashes() {
        #[track_caller]
        fn check(expected: &str, actual: &str, chunk_size: usize) {
            let expected_bytes = ConsonantBase16::parse(expected).expect("well-formed for test");
            let actual_bytes = ConsonantBase16::parse_with_separators(actual, chunk_size)
                .expect("well-formed for test");
            assert_eq!(expected_bytes, actual_bytes, "{expected}");
        }

        check("bcdf", "bc-df", 1);
        check("bbbbccccddddffff", "bbbb-cccc-dddd-ffff", 2);
        check("bcdfghkmnpqrstxz", "bcdfgh-kmnpqr-stxz", 3);
    }

    #[test]
    fn test_odd_length_input() {
        assert_eq!(
            ConsonantBase16::parse("b").unwrap_err(),
            DecodeError::OddLengthInput
        );
        assert_eq!(
            ConsonantBase16::parse("bcd").unwrap_err(),
            DecodeError::OddLengthInput
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("bc-c", 1).unwrap_err(),
            DecodeError::OddLengthInput
        );
    }

    #[test]
    fn test_missing_separators() {
        assert_eq!(
            ConsonantBase16::parse_with_separators("bcdf", 1).unwrap_err(),
            DecodeError::MissingSeparator(2)
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("bc-dfbc-df", 1).unwrap_err(),
            DecodeError::MissingSeparator(5)
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("bcdfbcdf", 2).unwrap_err(),
            DecodeError::MissingSeparator(4)
        );
    }

    #[test]
    fn test_unexpected_separators() {
        assert_eq!(
            ConsonantBase16::parse("b-").unwrap_err(),
            DecodeError::UnexpectedSeparator(1)
        );
        assert_eq!(
            ConsonantBase16::parse("bc-").unwrap_err(),
            DecodeError::UnexpectedSeparator(2)
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("b-", 1).unwrap_err(),
            DecodeError::UnexpectedSeparator(1)
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("bc-", 1).unwrap_err(),
            DecodeError::UnexpectedSeparator(2)
        );
        assert_eq!(
            ConsonantBase16::parse("bcd-f").unwrap_err(),
            DecodeError::UnexpectedSeparator(3)
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("bc-d-f", 1).unwrap_err(),
            DecodeError::UnexpectedSeparator(4)
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("bcd-f", 2).unwrap_err(),
            DecodeError::UnexpectedSeparator(3)
        );
        assert_eq!(
            ConsonantBase16::parse_with_separators("bc-df", 2).unwrap_err(),
            DecodeError::UnexpectedSeparator(2)
        );
    }

    #[test]
    fn test_bad_char() {
        assert_eq!(
            ConsonantBase16::parse("ab").unwrap_err(),
            DecodeError::BadCharacter(0, 'a')
        );
        assert_eq!(
            ConsonantBase16::parse("bédf").unwrap_err(),
            DecodeError::BadCharacter(1, 'é')
        );
    }

    #[test]
    fn test_fill() {
        assert_eq!(
            format!(
                "{:-^.2}",
                ConsonantBase16::from([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10].as_slice())
            ),
            "zxts-rqpn-mkhg-fdcb"
        );

        assert_eq!(
            format!(
                "{:-^.4}",
                ConsonantBase16::from([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10].as_slice())
            ),
            "zxtsrqpn-mkhgfdcb"
        );

        assert_eq!(
            format!(
                "{:-^.6}",
                ConsonantBase16::from([0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10].as_slice())
            ),
            "zxtsrqpnmkhg-fdcb"
        );
    }
}
