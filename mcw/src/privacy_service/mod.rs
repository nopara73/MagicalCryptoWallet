//! First-party Tor domain services. No socket, process, platform handle or DNS.
//!
//! Protocol/cryptography checkpoints do not establish an anonymous connection.
//! The service remains unavailable until the transport, directory, guard, onion
//! and platform acceptance gates in the privacy handoff are satisfied.
#![forbid(unsafe_code)]

pub mod cell;
pub mod crypto;
pub mod onion;
pub mod relay;

use std::fmt;

/// Diagnostics deliberately contain no addresses, identities or key material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    LengthLimit,
    Truncated,
    InvalidCommand,
    InvalidCircuit,
    InvalidVersion,
    InvalidCertificate,
    AuthenticationFailed,
    InvalidRelay,
    InvalidStream,
    InvalidOnionAddress,
    CipherExhausted,
    Closed,
    FlowControl,
    Cancelled,
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "privacy service: {self:?}")
    }
}
impl std::error::Error for Error {}
