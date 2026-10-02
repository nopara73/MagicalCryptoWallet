//! Bounded retained-Tor SOCKS readiness leaf, not a Tor bootstrap check.
//! The host owns dispatch and cancellation. No credentials or destinations
//! enter this operation, and no remote endpoint or local DNS lookup is allowed.
#![forbid(unsafe_code)]

use super::transport::{self, Cancellation, ConnectOptions, ErrorKind};
use super::wire::{Authentication, ProtocolError};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

pub const PROBE: u16 = 0x0805;
pub const VERSION: u8 = 1;
pub const PROXY_CONNECT_TIMEOUT: Duration = Duration::from_millis(125);
pub const TOTAL_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidPayload,
    UnknownOperation,
}

impl Error {
    pub fn code(self) -> u16 {
        match self {
            Self::InvalidPayload => 1,
            Self::UnknownOperation => 3,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidPayload => "invalid SOCKS readiness request",
            Self::UnknownOperation => "unsupported SOCKS readiness operation",
        }
    }
}

/// v1, address type (1 or 4), exact numeric proxy bytes, big-endian port.
/// Only literal loopback proxies are valid. A domain or scoped/mapped IPv6
/// endpoint cannot be converted into a locally-resolved address by this codec.
pub fn decode_request(payload: &[u8]) -> Result<SocketAddr, Error> {
    if payload.first() != Some(&VERSION) {
        return Err(Error::InvalidPayload);
    }
    let (address, port_offset) = match payload.get(1) {
        Some(1) if payload.len() == 8 => (
            IpAddr::V4(Ipv4Addr::new(
                payload[2], payload[3], payload[4], payload[5],
            )),
            6,
        ),
        Some(4) if payload.len() == 20 => {
            let mut octets = [0; 16];
            octets.copy_from_slice(&payload[2..18]);
            (IpAddr::V6(Ipv6Addr::from(octets)), 18)
        }
        _ => return Err(Error::InvalidPayload),
    };
    let port = u16::from_be_bytes([payload[port_offset], payload[port_offset + 1]]);
    if !address.is_loopback() || port == 0 {
        return Err(Error::InvalidPayload);
    }
    Ok(SocketAddr::new(address, port))
}

/// Safe stable result categories, mirrored by the typed transitional adapter.
/// Response is [version, ready:0/1, category]. No endpoint or native message.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    None = 0,
    Io = 1,
    TimedOut = 2,
    InvalidVersion = 3,
    MethodRejected = 4,
    UnexpectedEof = 5,
    Closed = 6,
    Cancelled = 7,
    Protocol = 8,
}

fn classify(kind: ErrorKind) -> Failure {
    match kind {
        ErrorKind::TimedOut => Failure::TimedOut,
        ErrorKind::Cancelled => Failure::Cancelled,
        ErrorKind::Closed => Failure::Closed,
        ErrorKind::UnexpectedEof => Failure::UnexpectedEof,
        ErrorKind::Protocol(ProtocolError::InvalidVersion(_)) => Failure::InvalidVersion,
        ErrorKind::Protocol(
            ProtocolError::NoAcceptableMethod | ProtocolError::UnofferedMethod(_),
        ) => Failure::MethodRejected,
        ErrorKind::Io(_) | ErrorKind::WriteZero => Failure::Io,
        _ => Failure::Protocol,
    }
}

pub fn execute(
    operation: u16,
    payload: &[u8],
    cancellation: &Cancellation,
) -> Result<[u8; 3], Error> {
    if operation != PROBE {
        return Err(Error::UnknownOperation);
    }
    let proxy = decode_request(payload)?;
    let options = ConnectOptions {
        proxy_connect_timeout: PROXY_CONNECT_TIMEOUT,
        total_timeout: TOTAL_TIMEOUT,
        poll_interval: Duration::from_millis(10),
    };
    match transport::probe(proxy, Authentication::None, options, cancellation) {
        Ok(()) => Ok([VERSION, 1, Failure::None as u8]),
        Err(cause) => Ok([VERSION, 0, classify(cause.kind) as u8]),
    }
}
