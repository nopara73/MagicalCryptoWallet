//! Bounded hash responsibilities for the retained managed wallet callers.
//! No key ownership, curve operation, recovery algorithm, logging or CLI route.
#![forbid(unsafe_code)]

use crate::wallet_hashes::{HmacSha512, hmac_sha256, hmac_sha512};
use std::{fmt, hint::black_box};

pub const OWNERSHIP_IDENTIFIER: u16 = 0x0a10;
pub const SLIP21_SEED: u16 = 0x0a11;
pub const SLIP21_CHILD: u16 = 0x0a12;
/// Version-one bridge frame capacity excluding its public 16-byte header.
pub const MAX_REQUEST_BYTES: usize = 1_048_560;
pub const KEY_BYTES: usize = 32;
pub const NODE_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidPayload,
    InputLimit,
    HashFailure,
    UnknownOperation,
}
impl Error {
    pub fn code(self) -> u16 {
        match self {
            Self::InvalidPayload | Self::InputLimit => 1,
            Self::HashFailure => 2,
            Self::UnknownOperation => 3,
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidPayload => "invalid wallet hash request",
            Self::InputLimit => "wallet hash request exceeds the limit",
            Self::HashFailure => "wallet hash computation failed",
            Self::UnknownOperation => "unsupported wallet hash operation",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for Error {}

/// A response guard clears its owned copy on every exit. Exported copies remain
/// the transport/caller's responsibility; safe Rust does not guarantee erasure.
pub enum Response {
    Identifier([u8; KEY_BYTES]),
    Node([u8; NODE_BYTES]),
}
impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WalletHashResponse([REDACTED])")
    }
}
impl Response {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Identifier(bytes) => bytes,
            Self::Node(bytes) => bytes,
        }
    }
}
impl Drop for Response {
    fn drop(&mut self) {
        let bytes: &mut [u8] = match self {
            Self::Identifier(bytes) => bytes,
            Self::Node(bytes) => bytes,
        };
        bytes.fill(0);
        black_box(bytes);
    }
}

fn limit(prefix: usize, length: usize) -> Result<(), Error> {
    if prefix
        .checked_add(length)
        .is_none_or(|n| n > MAX_REQUEST_BYTES)
    {
        return Err(Error::InputLimit);
    }
    Ok(())
}
pub fn ownership_identifier(key: &[u8; KEY_BYTES], script: &[u8]) -> Result<Response, Error> {
    limit(KEY_BYTES, script.len())?;
    hmac_sha256(key, script)
        .map(Response::Identifier)
        .map_err(|_| Error::HashFailure)
}
pub fn slip21_seed(seed: &[u8]) -> Result<Response, Error> {
    limit(0, seed.len())?;
    hmac_sha512(b"Symmetric key seed", seed)
        .map(Response::Node)
        .map_err(|_| Error::HashFailure)
}
pub fn slip21_child(key: &[u8; KEY_BYTES], label: &[u8]) -> Result<Response, Error> {
    limit(KEY_BYTES, label.len())?;
    let mut mac = HmacSha512::new(key).map_err(|_| Error::HashFailure)?;
    mac.update(&[0]).map_err(|_| Error::HashFailure)?;
    mac.update(label).map_err(|_| Error::HashFailure)?;
    mac.finalize()
        .map(Response::Node)
        .map_err(|_| Error::HashFailure)
}
pub fn handles(operation: u16) -> bool {
    matches!(operation, OWNERSHIP_IDENTIFIER | SLIP21_SEED | SLIP21_CHILD)
}
/// Ownership/child requests contain exactly the 32-byte key followed by the
/// original script/label bytes. Seed requests contain the original seed bytes.
/// The SLIP21 0x00 label prefix is added here exactly once, never on the wire.
pub fn execute(operation: u16, payload: &[u8]) -> Result<Response, Error> {
    if !handles(operation) {
        return Err(Error::UnknownOperation);
    }
    limit(0, payload.len())?;
    if operation == SLIP21_SEED {
        return slip21_seed(payload);
    }
    let key: &[u8; KEY_BYTES] = payload
        .get(..KEY_BYTES)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Error::InvalidPayload)?;
    match operation {
        OWNERSHIP_IDENTIFIER => ownership_identifier(key, &payload[KEY_BYTES..]),
        SLIP21_CHILD => slip21_child(key, &payload[KEY_BYTES..]),
        _ => Err(Error::UnknownOperation),
    }
}
