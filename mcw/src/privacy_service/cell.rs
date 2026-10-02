//! Bounded Tor channel framing for link protocols 4 and 5.
//! Parsing a CERTS structure is not certificate or relay authentication.

use super::Error;
use std::fmt;

pub const BODY_LEN: usize = 509;
pub const MAX_VARIABLE_BODY: usize = 65_535;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkProtocol {
    Negotiating,
    V4,
    V5,
}
impl LinkProtocol {
    fn circuit_id_len(self) -> usize {
        match self {
            Self::Negotiating => 2,
            Self::V4 | Self::V5 => 4,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Padding = 0,
    Relay = 3,
    Destroy = 4,
    Netinfo = 8,
    RelayEarly = 9,
    Create2 = 10,
    Created2 = 11,
    PaddingNegotiate = 12,
    Versions = 7,
    Vpadding = 128,
    Certs = 129,
    AuthChallenge = 130,
    Authenticate = 131,
}
impl Command {
    pub fn from_byte(byte: u8) -> Result<Self, Error> {
        Ok(match byte {
            0 => Self::Padding,
            3 => Self::Relay,
            4 => Self::Destroy,
            7 => Self::Versions,
            8 => Self::Netinfo,
            9 => Self::RelayEarly,
            10 => Self::Create2,
            11 => Self::Created2,
            12 => Self::PaddingNegotiate,
            128 => Self::Vpadding,
            129 => Self::Certs,
            130 => Self::AuthChallenge,
            131 => Self::Authenticate,
            _ => return Err(Error::InvalidCommand),
        })
    }
    fn variable(self) -> bool {
        self == Self::Versions || self as u8 >= 128
    }
    fn circuit(self) -> bool {
        matches!(
            self,
            Self::Relay | Self::Destroy | Self::RelayEarly | Self::Create2 | Self::Created2
        )
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Cell<'a> {
    pub circuit_id: u32,
    pub command: Command,
    pub body: &'a [u8],
}
impl fmt::Debug for Cell<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cell")
            .field("command", &self.command)
            .field("data", &"[redacted]")
            .finish()
    }
}

fn validate(circuit: u32, command: Command, protocol: LinkProtocol) -> Result<(), Error> {
    if command.circuit() == (circuit == 0) {
        return Err(Error::InvalidCircuit);
    }
    if protocol == LinkProtocol::Negotiating && command != Command::Versions {
        return Err(Error::InvalidVersion);
    }
    if command == Command::PaddingNegotiate && protocol != LinkProtocol::V5 {
        return Err(Error::InvalidVersion);
    }
    Ok(())
}

/// Returns one complete cell and its exact consumption, preserving coalesced
/// application bytes. The caller changes protocol after the first VERSIONS.
pub fn decode(bytes: &[u8], protocol: LinkProtocol) -> Result<Option<(Cell<'_>, usize)>, Error> {
    let circuit_len = protocol.circuit_id_len();
    if bytes.len() < circuit_len + 1 {
        return Ok(None);
    }
    let circuit_id = if circuit_len == 2 {
        u16::from_be_bytes(bytes[..2].try_into().unwrap()) as u32
    } else {
        u32::from_be_bytes(bytes[..4].try_into().unwrap())
    };
    let command = Command::from_byte(bytes[circuit_len])?;
    validate(circuit_id, command, protocol)?;
    let (header, body_len) = if command.variable() {
        if bytes.len() < circuit_len + 3 {
            return Ok(None);
        }
        (
            circuit_len + 3,
            u16::from_be_bytes(bytes[circuit_len + 1..circuit_len + 3].try_into().unwrap())
                as usize,
        )
    } else {
        (circuit_len + 1, BODY_LEN)
    };
    let total = header + body_len;
    if bytes.len() < total {
        return Ok(None);
    }
    Ok(Some((
        Cell {
            circuit_id,
            command,
            body: &bytes[header..total],
        },
        total,
    )))
}

pub fn encode(cell: &Cell<'_>, protocol: LinkProtocol) -> Result<Vec<u8>, Error> {
    validate(cell.circuit_id, cell.command, protocol)?;
    let body_len = if cell.command.variable() {
        if cell.body.len() > MAX_VARIABLE_BODY {
            return Err(Error::LengthLimit);
        }
        cell.body.len()
    } else {
        if cell.body.len() > BODY_LEN {
            return Err(Error::LengthLimit);
        }
        if matches!(cell.command, Command::Relay | Command::RelayEarly)
            && cell.body.len() != BODY_LEN
        {
            return Err(Error::InvalidRelay);
        }
        BODY_LEN
    };
    let mut result = Vec::with_capacity(protocol.circuit_id_len() + 3 + body_len);
    if protocol == LinkProtocol::Negotiating {
        result.extend_from_slice(&(cell.circuit_id as u16).to_be_bytes());
    } else {
        result.extend_from_slice(&cell.circuit_id.to_be_bytes());
    }
    result.push(cell.command as u8);
    if cell.command.variable() {
        result.extend_from_slice(&(body_len as u16).to_be_bytes());
    }
    result.extend_from_slice(cell.body);
    result.resize(result.len() + body_len - cell.body.len(), 0);
    Ok(result)
}

pub fn versions() -> Vec<u8> {
    vec![0, 0, 7, 0, 4, 0, 4, 0, 5]
}
pub fn negotiate(bytes: &[u8]) -> Result<LinkProtocol, Error> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return Err(Error::InvalidVersion);
    }
    let mut has_four = false;
    for version in bytes.as_chunks::<2>().0 {
        match u16::from_be_bytes(*version) {
            5 => return Ok(LinkProtocol::V5),
            4 => has_four = true,
            _ => (),
        }
    }
    if has_four {
        Ok(LinkProtocol::V4)
    } else {
        Err(Error::InvalidVersion)
    }
}

#[derive(Clone, Copy)]
pub struct Certificate<'a> {
    pub kind: u8,
    pub encoded: &'a [u8],
}
impl fmt::Debug for Certificate<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Certificate([redacted])")
    }
}
pub fn certificates(body: &[u8]) -> Result<Vec<Certificate<'_>>, Error> {
    let (&count, mut rest) = body.split_first().ok_or(Error::Truncated)?;
    let mut seen = [false; 256];
    let mut result = Vec::with_capacity(count as usize);
    for _ in 0..count {
        if rest.len() < 3 {
            return Err(Error::Truncated);
        }
        let kind = rest[0];
        let len = u16::from_be_bytes(rest[1..3].try_into().unwrap()) as usize;
        rest = &rest[3..];
        if seen[kind as usize] {
            return Err(Error::InvalidCertificate);
        }
        if len > rest.len() {
            return Err(Error::Truncated);
        }
        seen[kind as usize] = true;
        result.push(Certificate {
            kind,
            encoded: &rest[..len],
        });
        rest = &rest[len..];
    }
    // Forward-compatible trailing bytes are ignored by the Tor CERTS spec.
    Ok(result)
}

pub struct Ed25519Certificate<'a> {
    pub kind: u8,
    pub expires_at: u64,
    pub key_type: u8,
    pub certified_key: [u8; 32],
    pub signing_key: Option<[u8; 32]>,
    pub signed_bytes: &'a [u8],
    pub signature: &'a [u8; 64],
}
impl fmt::Debug for Ed25519Certificate<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ed25519Certificate([redacted])")
    }
}

pub fn ed25519_certificate(bytes: &[u8]) -> Result<Ed25519Certificate<'_>, Error> {
    if bytes.len() < 40 + 64 {
        return Err(Error::Truncated);
    }
    if bytes[0] != 1 {
        return Err(Error::InvalidCertificate);
    }
    let signature_at = bytes.len() - 64;
    let expires_at = u64::from(u32::from_be_bytes(bytes[2..6].try_into().unwrap())) * 3600;
    let mut position = 40;
    let mut signing_key = None;
    for _ in 0..bytes[39] {
        if signature_at < position + 4 {
            return Err(Error::Truncated);
        }
        let length = u16::from_be_bytes(bytes[position..position + 2].try_into().unwrap()) as usize;
        let kind = bytes[position + 2];
        let flags = bytes[position + 3];
        position += 4;
        if length > signature_at - position {
            return Err(Error::Truncated);
        }
        if kind == 4 {
            if length != 32 || signing_key.is_some() {
                return Err(Error::InvalidCertificate);
            }
            signing_key = Some(bytes[position..position + length].try_into().unwrap());
        } else if flags & 1 != 0 {
            return Err(Error::InvalidCertificate);
        }
        position += length;
    }
    if position != signature_at {
        return Err(Error::InvalidCertificate);
    }
    Ok(Ed25519Certificate {
        kind: bytes[1],
        expires_at,
        key_type: bytes[6],
        certified_key: bytes[7..39].try_into().unwrap(),
        signing_key,
        signed_bytes: &bytes[..signature_at],
        signature: bytes[signature_at..].try_into().unwrap(),
    })
}

/// NETINFO sent by a client discloses neither a clock nor its own addresses.
pub fn client_netinfo(peer: std::net::IpAddr) -> [u8; BODY_LEN] {
    let mut result = [0; BODY_LEN];
    match peer {
        std::net::IpAddr::V4(ip) => {
            result[4] = 4;
            result[5] = 4;
            result[6..10].copy_from_slice(&ip.octets());
        }
        std::net::IpAddr::V6(ip) => {
            result[4] = 6;
            result[5] = 16;
            result[6..22].copy_from_slice(&ip.octets());
        }
    }
    result
}
