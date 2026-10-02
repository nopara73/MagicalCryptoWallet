//! Bounded payment URI application-service payloads, independent of transport.
//!
//! This module knows only operation IDs and byte buffers. It imports no bridge,
//! frame, IPC, process, filesystem, platform, managed runtime, or wallet state.
//! The host owns routing and lifecycle. Every operation invokes the real sibling
//! domain/address implementation with no fallback, copied codec, or shadow check.
#![forbid(unsafe_code)]

use super::{
    Amount, AmountError, BitcoinAddressValidator, Error as DomainError, FormatError,
    MAX_COMPONENT_BYTES, ParsedDestination, ParsingMode, PaymentDetails, new_request, parse_input,
    percent_decode, percent_encode,
};
use crate::bitcoin_encoding::{self, Address, LegacyKind, Network};
use std::fmt;

pub const VERSION: u8 = 1;
pub const MAX_PAYLOAD_BYTES: usize = 32_000;
pub const PARSE_DESTINATION: u16 = 0x0400;
pub const FORMAT_REQUEST: u16 = 0x0401;
pub const PARSE_AMOUNT: u16 = 0x0402;
pub const FORMAT_AMOUNT: u16 = 0x0403;
pub const ENCODE_METADATA: u16 = 0x0404;
pub const DECODE_METADATA: u16 = 0x0405;

/// Only static categories cross the service boundary. The domain's supplied
/// address/query diagnostic strings are dropped rather than retained or logged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error {
    pub code: u16,
    pub message: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}
impl std::error::Error for Error {}

const UNKNOWN_OPERATION: Error = Error {
    code: 100,
    message: "Unsupported payment URI operation.",
};
const MALFORMED: Error = Error {
    code: 101,
    message: "Malformed payment URI payload.",
};
const WRONG_VERSION: Error = Error {
    code: 102,
    message: "Unsupported payment URI payload version.",
};
const WRONG_NETWORK: Error = Error {
    code: 103,
    message: "Invalid payment URI network.",
};
const WRONG_MODE: Error = Error {
    code: 104,
    message: "Invalid payment URI parsing mode.",
};
const INVALID_UTF8: Error = Error {
    code: 105,
    message: "Payment URI payload is not valid UTF-8.",
};
const WRONG_FLAGS: Error = Error {
    code: 106,
    message: "Invalid payment URI field flags.",
};
const TOO_LARGE: Error = Error {
    code: 107,
    message: "Payment URI payload exceeds its size limit.",
};

pub const fn handles(operation: u16) -> bool {
    matches!(
        operation,
        PARSE_DESTINATION
            | FORMAT_REQUEST
            | PARSE_AMOUNT
            | FORMAT_AMOUNT
            | ENCODE_METADATA
            | DECODE_METADATA
    )
}

/// Stable wire IDs: mainnet=0, testnet=1, testnet4=2, signet=3, regtest=4.
fn network_id(network: Network) -> u8 {
    match network {
        Network::Mainnet => 0,
        Network::Testnet => 1,
        Network::Testnet4 => 2,
        Network::Signet => 3,
        Network::Regtest => 4,
    }
}

fn network(id: u8) -> Result<Network, Error> {
    match id {
        0 => Ok(Network::Mainnet),
        1 => Ok(Network::Testnet),
        2 => Ok(Network::Testnet4),
        3 => Ok(Network::Signet),
        4 => Ok(Network::Regtest),
        _ => Err(WRONG_NETWORK),
    }
}

fn mode(id: u8) -> Result<ParsingMode, Error> {
    match id {
        0 => Ok(ParsingMode::Bip21),
        1 => Ok(ParsingMode::ManagedCompatibility),
        _ => Err(WRONG_MODE),
    }
}

fn domain<E>(error: DomainError<E>) -> Error {
    Error {
        code: error.code(),
        message: error.message(),
    }
}

fn amount(error: AmountError) -> Error {
    match error {
        AmountError::Empty => Error {
            code: 7,
            message: "Missing amount value.",
        },
        _ => Error {
            code: 8,
            message: "Invalid amount value.",
        },
    }
}

fn format_error(error: FormatError) -> Error {
    match error {
        FormatError::InputTooLong | FormatError::Percent(_) => Error {
            code: 10,
            message: "Input is too long.",
        },
    }
}

struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, length: usize) -> Result<&'a [u8], Error> {
        let Some((value, tail)) = self.remaining.split_at_checked(length) else {
            return Err(MALFORMED);
        };
        self.remaining = tail;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }
    fn number(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?.try_into().map_err(|_| MALFORMED)?,
        ))
    }
    fn text(&mut self, max: usize) -> Result<&'a str, Error> {
        let length = u32::from_le_bytes(self.bytes(4)?.try_into().map_err(|_| MALFORMED)?);
        let length = usize::try_from(length).map_err(|_| TOO_LARGE)?;
        if length > max {
            return Err(TOO_LARGE);
        }
        std::str::from_utf8(self.bytes(length)?).map_err(|_| INVALID_UTF8)
    }
    fn finish(self) -> Result<(), Error> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(MALFORMED)
        }
    }
}

struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self {
            bytes: vec![VERSION],
        }
    }
    fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }
    fn number(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn data(&mut self, data: &[u8]) -> Result<(), Error> {
        let length = u32::try_from(data.len()).map_err(|_| TOO_LARGE)?;
        self.bytes.extend_from_slice(&length.to_le_bytes());
        self.bytes.extend_from_slice(data);
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<(), Error> {
        self.data(text.as_bytes())
    }
    fn finish(self) -> Result<Vec<u8>, Error> {
        if self.bytes.len() > MAX_PAYLOAD_BYTES {
            Err(TOO_LARGE)
        } else {
            Ok(self.bytes)
        }
    }
}

fn flags(details: &PaymentDetails) -> u8 {
    u8::from(details.amount.is_some())
        | (u8::from(details.label.is_some()) << 1)
        | (u8::from(details.message.is_some()) << 2)
}

fn write_details(output: &mut Writer, details: &PaymentDetails) -> Result<(), Error> {
    output.byte(flags(details));
    if let Some(amount) = details.amount {
        output.number(amount.satoshis());
    }
    if let Some(label) = &details.label {
        output.text(label)?;
    }
    if let Some(message) = &details.message {
        output.text(message)?;
    }
    Ok(())
}

fn read_details(input: &mut Reader<'_>) -> Result<PaymentDetails, Error> {
    let flags = input.byte()?;
    if flags & !7 != 0 {
        return Err(WRONG_FLAGS);
    }
    let amount = if flags & 1 != 0 {
        Some(Amount::from_satoshis(input.number()?).map_err(amount)?)
    } else {
        None
    };
    let label = if flags & 2 != 0 {
        Some(input.text(MAX_COMPONENT_BYTES)?.to_owned())
    } else {
        None
    };
    let message = if flags & 4 != 0 {
        Some(input.text(MAX_COMPONENT_BYTES)?.to_owned())
    } else {
        None
    };
    Ok(PaymentDetails {
        amount,
        label,
        message,
    })
}

fn write_address(
    output: &mut Writer,
    text: &str,
    selected: Network,
    address: &Address,
) -> Result<(), Error> {
    output.byte(network_id(selected));
    let (kind, version, bytes): (u8, u8, &[u8]) = match address {
        Address::Legacy(address) => (
            if address.kind == LegacyKind::P2pkh {
                0
            } else {
                1
            },
            255,
            &address.hash,
        ),
        Address::Witness(address) => (2, address.version, &address.program),
    };
    output.byte(kind);
    output.byte(version);
    output.data(bytes)?;
    output.text(text)?;
    output.text(
        &bitcoin_encoding::address_encode(address)
            .map_err(DomainError::InvalidAddress)
            .map_err(domain)?,
    )?;
    Ok(())
}

/// Versioned little-endian payloads with length-prefixed UTF-8 strings. Rejects
/// unsupported IDs, fields, invalid UTF-8, truncation and trailing bytes before
/// executing a domain operation. See the owned handoff for the exact schema.
pub fn handle(operation: u16, payload: &[u8]) -> Result<Vec<u8>, Error> {
    if !handles(operation) {
        return Err(UNKNOWN_OPERATION);
    }
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(TOO_LARGE);
    }
    let mut input = Reader { remaining: payload };
    if input.byte()? != VERSION {
        return Err(WRONG_VERSION);
    }
    let mut output = Writer::new();
    match operation {
        PARSE_DESTINATION => {
            let selected = network(input.byte()?)?;
            let mode = mode(input.byte()?)?;
            let text = input.text(MAX_COMPONENT_BYTES)?;
            input.finish()?;
            let parsed =
                parse_input(text, selected, &BitcoinAddressValidator, mode).map_err(domain)?;
            match parsed {
                ParsedDestination::Address(address) => {
                    output.byte(0);
                    write_address(&mut output, address.text(), selected, address.address())?;
                    write_details(&mut output, &PaymentDetails::default())?;
                    output.byte(0); // No original URI.
                    output.data(&[])?; // Optional-parameter count = 0.
                }
                ParsedDestination::PaymentUri(request) => {
                    output.byte(1);
                    let address = request.destination();
                    write_address(&mut output, address.text(), selected, address.address())?;
                    write_details(&mut output, request.details())?;
                    output.byte(1);
                    output.text(request.original_uri().ok_or(MALFORMED)?)?;
                    let count = u32::try_from(request.optional_parameters().len())
                        .map_err(|_| TOO_LARGE)?;
                    output.bytes.extend_from_slice(&count.to_le_bytes());
                    for parameter in request.optional_parameters() {
                        output.text(&parameter.name)?;
                        output.byte(u8::from(parameter.value.is_some()));
                        if let Some(value) = &parameter.value {
                            output.text(value)?;
                        }
                    }
                }
            }
        }
        FORMAT_REQUEST => {
            let selected = network(input.byte()?)?;
            let address = input.text(MAX_COMPONENT_BYTES)?;
            let details = read_details(&mut input)?;
            input.finish()?;
            let request = new_request(address, selected, details, &BitcoinAddressValidator)
                .map_err(domain)?;
            output.text(&request.to_uri().map_err(format_error)?)?;
        }
        PARSE_AMOUNT => {
            let text = input.text(1_000)?;
            input.finish()?;
            output.number(Amount::parse_btc(text).map_err(amount)?.satoshis());
        }
        FORMAT_AMOUNT => {
            let value = input.number()?;
            input.finish()?;
            output.text(&Amount::from_satoshis(value).map_err(amount)?.to_btc())?;
        }
        ENCODE_METADATA => {
            let text = input.text(MAX_COMPONENT_BYTES)?;
            input.finish()?;
            output.text(&percent_encode(text).map_err(|_| TOO_LARGE)?)?;
        }
        DECODE_METADATA => {
            let mode = mode(input.byte()?)?;
            let text = input.text(MAX_COMPONENT_BYTES * 3)?;
            input.finish()?;
            output.text(&percent_decode(text, mode).map_err(|_| Error {
                code: 5,
                message: "Not a valid absolute URI.",
            })?)?;
        }
        _ => return Err(UNKNOWN_OPERATION),
    }
    output.finish()
}
