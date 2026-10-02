//! Tor v3 onion name encoding and checksum validation, with no DNS access.

use super::{Error, crypto::sha3_256};
use std::fmt;

const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

#[derive(Clone, Eq, PartialEq)]
pub struct OnionAddress([u8; 32]);
impl fmt::Debug for OnionAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OnionAddress([redacted])")
    }
}
impl OnionAddress {
    pub fn from_identity_key(key: [u8; 32]) -> Self {
        Self(key)
    }
    pub fn identity_key(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn parse(host: &str) -> Result<Self, Error> {
        let bytes = host.as_bytes();
        if bytes.len() != 62 || !bytes[56..].eq_ignore_ascii_case(b".onion") {
            return Err(Error::InvalidOnionAddress);
        }
        let mut decoded = [0; 35];
        let (mut bits, mut count, mut position) = (0u32, 0usize, 0usize);
        for byte in &bytes[..56] {
            let value = match byte.to_ascii_lowercase() {
                b'a'..=b'z' => byte.to_ascii_lowercase() - b'a',
                b'2'..=b'7' => byte - b'2' + 26,
                _ => return Err(Error::InvalidOnionAddress),
            };
            bits = (bits << 5) | u32::from(value);
            count += 5;
            if count >= 8 {
                count -= 8;
                decoded[position] = (bits >> count) as u8;
                position += 1;
            }
        }
        if decoded[34] != 3 {
            return Err(Error::InvalidOnionAddress);
        }
        let key = decoded[..32].try_into().unwrap();
        let checksum = checksum(&key);
        if decoded[32..34] != checksum {
            return Err(Error::InvalidOnionAddress);
        }
        Ok(Self(key))
    }
    pub fn host(&self) -> String {
        let mut bytes = [0; 35];
        bytes[..32].copy_from_slice(&self.0);
        bytes[32..34].copy_from_slice(&checksum(&self.0));
        bytes[34] = 3;
        let (mut bits, mut count) = (0u32, 0usize);
        let mut output = String::with_capacity(62);
        for byte in bytes {
            bits = (bits << 8) | u32::from(byte);
            count += 8;
            while count >= 5 {
                count -= 5;
                output.push(ALPHABET[((bits >> count) & 31) as usize] as char);
            }
        }
        output.push_str(".onion");
        output
    }
}
fn checksum(key: &[u8; 32]) -> [u8; 2] {
    let mut material = [0; 48];
    material[..15].copy_from_slice(b".onion checksum");
    material[15..47].copy_from_slice(key);
    material[47] = 3;
    sha3_256(&material)[..2].try_into().unwrap()
}
