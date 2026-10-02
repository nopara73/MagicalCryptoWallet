//! Application transport adapter for bounded public Bitcoin address validation.
//! The portable bitcoin_encoding module does not depend on this adapter.
#![forbid(unsafe_code)]

use crate::{
    bitcoin_encoding::{self, Address, Error, LegacyKind, Network},
    bridge::{self, Frame},
};

pub const VALIDATE_ADDRESS: u16 = 0x020c;
pub const MAX_ADDRESS_BYTES: usize = bitcoin_encoding::MAX_BECH32_LENGTH;

/// Reply: 0 + failure:u16le, or 1 + the exact standard scriptPubKey bytes.
/// Failures do not echo any input. They describe address validity, not IPC errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ValidationFailure {
    Format = 1,
    Network = 2,
    Checksum = 3,
    MixedCase = 4,
    Size = 5,
}

fn failure(error: Error) -> ValidationFailure {
    match error {
        Error::WrongNetwork { .. } => ValidationFailure::Network,
        Error::InvalidChecksum | Error::WrongChecksumVariant { .. } => ValidationFailure::Checksum,
        Error::MixedCase => ValidationFailure::MixedCase,
        Error::SizeLimit { .. } => ValidationFailure::Size,
        _ => ValidationFailure::Format,
    }
}

fn network(value: u8) -> Option<Network> {
    Some(match value {
        0 => Network::Mainnet,
        1 => Network::Testnet,
        2 => Network::Testnet4,
        3 => Network::Signet,
        4 => Network::Regtest,
        _ => return None,
    })
}

fn script(address: Address) -> Vec<u8> {
    match address {
        Address::Legacy(address) => match address.kind {
            LegacyKind::P2pkh => {
                let mut result = Vec::with_capacity(25);
                result.extend_from_slice(&[0x76, 0xa9, 0x14]);
                result.extend_from_slice(&address.hash);
                result.extend_from_slice(&[0x88, 0xac]);
                result
            }
            LegacyKind::P2sh => {
                let mut result = Vec::with_capacity(23);
                result.extend_from_slice(&[0xa9, 0x14]);
                result.extend_from_slice(&address.hash);
                result.push(0x87);
                result
            }
        },
        Address::Witness(address) => {
            let mut result = Vec::with_capacity(address.program.len() + 2);
            result.push(if address.version == 0 {
                0
            } else {
                0x50 + address.version
            });
            result.push(address.program.len() as u8);
            result.extend_from_slice(&address.program);
            result
        }
    }
}

/// Request bytes: explicit network:u8 followed by unmodified UTF-8 address text.
/// Only this one operation is implemented. No keys, URI parsing, normalization,
/// process launch, filesystem, logging or alternate validator enter this handler.
pub fn handle(request: &Frame) -> Frame {
    if request.kind != bridge::REQUEST || request.id == 0 || request.operation != VALIDATE_ADDRESS {
        return request.error(1, "invalid address service request");
    }
    let Some((&network_value, text)) = request.payload.split_first() else {
        return request.error(1, "address network is missing");
    };
    let Some(network) = network(network_value) else {
        return request.error(1, "unsupported address network");
    };
    if text.len() > MAX_ADDRESS_BYTES {
        let mut payload = vec![0];
        payload.extend_from_slice(&(ValidationFailure::Size as u16).to_le_bytes());
        return request.reply(payload);
    }
    let Ok(text) = std::str::from_utf8(text) else {
        return request.error(1, "address text is not UTF-8");
    };
    match bitcoin_encoding::address_decode(text, network) {
        Ok(address) => {
            let mut payload = vec![1];
            payload.extend(script(address));
            request.reply(payload)
        }
        Err(error) => {
            let mut payload = vec![0];
            payload.extend_from_slice(&(failure(error) as u16).to_le_bytes());
            request.reply(payload)
        }
    }
}
