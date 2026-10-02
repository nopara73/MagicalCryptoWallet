//! Retained BIP21 payment requests for the mcw application.
//!
//! This is portable domain code: only std and the first-party bitcoin_encoding
//! service are used. Parsing never sends a payment or contacts an optional URI.
//! Address checks are delegated through AddressValidator; no address codec or
//! cryptography is duplicated here. All BTC amounts are exact integer satoshis.
#![forbid(unsafe_code)]

use crate::bitcoin_encoding::{self, Network};
use std::{collections::BTreeSet, fmt};

/// Portable application-service payloads. Framing, IPC and lifecycle stay in the
/// application host; the BIP21 domain APIs above and below require none of them.
#[path = "payment_uri/service.rs"]
pub mod service;

pub const SATOSHIS_PER_BTC: u64 = 100_000_000;
pub const MAX_SATOSHIS: u64 = 21_000_000 * SATOSHIS_PER_BTC;
/// Same bound as the retained managed AddressParser's String.Length limit.
pub const MAX_INPUT_UTF16_UNITS: usize = 1_000;
/// Bounds standalone percent-codec allocations; URI parsing has a tighter limit.
pub const MAX_COMPONENT_BYTES: usize = 4_000;

/// A nonnegative Bitcoin amount within consensus MoneyRange.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Amount(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmountError {
    Empty,
    InvalidDecimal,
    TooManyDecimalPlaces,
    OutOfRange,
}

impl Amount {
    pub const ZERO: Self = Self(0);

    pub fn from_satoshis(satoshis: u64) -> Result<Self, AmountError> {
        if satoshis > MAX_SATOSHIS {
            Err(AmountError::OutOfRange)
        } else {
            Ok(Self(satoshis))
        }
    }

    pub const fn satoshis(self) -> u64 {
        self.0
    }

    /// Decimal BTC only: ASCII digits, at most one '.', up to eight fractional
    /// digits. BIP21's grammar permits '.1' and '1.'; an empty string or '.'
    /// has no amount. No signs, whitespace, grouping, exponents, or rounding.
    pub fn parse_btc(text: &str) -> Result<Self, AmountError> {
        if text.is_empty() {
            return Err(AmountError::Empty);
        }
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if whole.is_empty() && fraction.is_empty() {
            return Err(AmountError::InvalidDecimal);
        }
        if !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
        {
            return Err(AmountError::InvalidDecimal);
        }
        if fraction.len() > 8 {
            return Err(AmountError::TooManyDecimalPlaces);
        }
        let mut coins = 0_u64;
        for b in whole.bytes() {
            coins = coins
                .checked_mul(10)
                .and_then(|n| n.checked_add(u64::from(b - b'0')))
                .ok_or(AmountError::OutOfRange)?;
            if coins > 21_000_000 {
                return Err(AmountError::OutOfRange);
            }
        }
        let mut sats = 0_u64;
        for b in fraction.bytes() {
            sats = sats * 10 + u64::from(b - b'0');
        }
        sats *= 10_u64.pow((8 - fraction.len()) as u32);
        Self::from_satoshis(coins * SATOSHIS_PER_BTC + sats)
    }

    /// Shortest exact decimal BTC spelling, independent of locale.
    pub fn to_btc(self) -> String {
        let whole = self.0 / SATOSHIS_PER_BTC;
        let fraction = self.0 % SATOSHIS_PER_BTC;
        if fraction == 0 {
            return whole.to_string();
        }
        let fraction = format!("{fraction:08}");
        format!("{whole}.{}", fraction.trim_end_matches('0'))
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_btc())
    }
}

impl fmt::Display for AmountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "missing BTC amount",
            Self::InvalidDecimal => "invalid decimal BTC amount",
            Self::TooManyDecimalPlaces => "BTC amount has more than eight decimal places",
            Self::OutOfRange => "BTC amount is outside MoneyRange",
        })
    }
}
impl std::error::Error for AmountError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ParsingMode {
    /// RFC3986/BIP21: case-sensitive query keys, literal '+', encoded UTF-8.
    #[default]
    Bip21,
    /// Retained managed behavior: ASCII case-insensitive query keys, '+' as a
    /// space, and raw spaces/UTF-8 in values. No malformed-percent repair,
    /// invalid UTF-8 substitution, amount rounding, or fragment acceptance.
    ManagedCompatibility,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PercentError {
    TooLong,
    /// Byte offset within the encoded component.
    InvalidEscape {
        offset: usize,
    },
    /// Byte offset within the decoded component.
    InvalidUtf8 {
        offset: usize,
    },
}

/// Encode UTF-8 bytes, leaving only RFC3986 unreserved ASCII characters.
/// Space is always '%20', never '+', and hex digits are uppercase.
pub fn percent_encode(text: &str) -> Result<String, PercentError> {
    if text.len() > MAX_COMPONENT_BYTES {
        return Err(PercentError::TooLong);
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(b >> 4)]));
            encoded.push(char::from(HEX[usize::from(b & 15)]));
        }
    }
    Ok(encoded)
}

/// Decode once with strict UTF-8 validation. This codec does not validate raw
/// URI grammar (the URI parser does); raw valid UTF-8 is retained. Form-style
/// '+' conversion occurs only in ManagedCompatibility and only for raw '+'.
pub fn percent_decode(text: &str, mode: ParsingMode) -> Result<String, PercentError> {
    if text.len() > MAX_COMPONENT_BYTES * 3 {
        return Err(PercentError::TooLong);
    }
    let hex = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let input = text.as_bytes();
    let mut decoded = Vec::with_capacity(input.len().min(MAX_COMPONENT_BYTES));
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == b'%' {
            let pair = input
                .get(i + 1..i + 3)
                .and_then(|p| Some((hex(p[0])?, hex(p[1])?)))
                .ok_or(PercentError::InvalidEscape { offset: i })?;
            decoded.push(pair.0 << 4 | pair.1);
            i += 3;
        } else {
            decoded.push(if b == b'+' && mode == ParsingMode::ManagedCompatibility {
                b' '
            } else {
                b
            });
            i += 1;
        }
        if decoded.len() > MAX_COMPONENT_BYTES {
            return Err(PercentError::TooLong);
        }
    }
    String::from_utf8(decoded).map_err(|e| PercentError::InvalidUtf8 {
        offset: e.utf8_error().valid_up_to(),
    })
}

impl fmt::Display for PercentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLong => "URI component is too long",
            Self::InvalidEscape { .. } => "invalid URI percent escape",
            Self::InvalidUtf8 { .. } => "URI component is not valid UTF-8",
        })
    }
}
impl std::error::Error for PercentError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryError {
    Percent(PercentError),
    /// Byte offset within one encoded parameter name or value.
    InvalidCharacter {
        offset: usize,
    },
    EmptyParameterName,
    MissingEquals {
        parameter: String,
    },
}

/// Codes 1-9 preserve the retained managed parser's public error mapping.
/// Messages contain no user-provided data; detailed typed context is separate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error<E> {
    InvalidUri,
    InvalidScheme { scheme: String },
    MissingAddress,
    InvalidAddressSyntax,
    InvalidAddress(E),
    InvalidQuery(QueryError),
    DuplicateParameter { parameter: String },
    MissingAmountValue,
    InvalidAmountValue(AmountError),
    UnsupportedRequiredParameter { parameter: String },
    InputTooLong,
    EmptyInput,
}

impl<E> Error<E> {
    pub const fn code(&self) -> u16 {
        match self {
            Self::InvalidUri => 1,
            Self::InvalidScheme { .. } => 2,
            Self::MissingAddress => 3,
            Self::InvalidAddressSyntax | Self::InvalidAddress(_) => 4,
            Self::InvalidQuery(_) => 5,
            Self::DuplicateParameter { .. } => 6,
            Self::MissingAmountValue => 7,
            Self::InvalidAmountValue(_) => 8,
            Self::UnsupportedRequiredParameter { .. } => 9,
            Self::InputTooLong => 10,
            Self::EmptyInput => 11,
        }
    }

    pub const fn message(&self) -> &'static str {
        match self {
            Self::InvalidUri | Self::InvalidQuery(_) => "Not a valid absolute URI.",
            Self::InvalidScheme { .. } => "Expected 'bitcoin' scheme.",
            Self::MissingAddress => "Bitcoin address is missing.",
            Self::InvalidAddressSyntax | Self::InvalidAddress(_) => "Invalid Bitcoin address.",
            Self::DuplicateParameter { .. } => "Parameter can be specified just once.",
            Self::MissingAmountValue => "Missing amount value.",
            Self::InvalidAmountValue(_) => "Invalid amount value.",
            Self::UnsupportedRequiredParameter { .. } => "Unsupported required parameter found.",
            Self::InputTooLong => "Input is too long.",
            Self::EmptyInput => "Input length is invalid.",
        }
    }
}

impl<E> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}
impl<E: std::error::Error + 'static> std::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidAddress(e) => Some(e),
            Self::InvalidAmountValue(e) => Some(e),
            Self::InvalidQuery(QueryError::Percent(e)) => Some(e),
            _ => None,
        }
    }
}

/// A validator must check the complete original address against the explicitly
/// selected network, including its checksum. Parsing does not infer a network.
pub trait AddressValidator {
    type Address;
    type Error;
    fn validate(&self, text: &str, network: Network) -> Result<Self::Address, Self::Error>;
}

/// Production adapter to the separate first-party address service.
#[derive(Clone, Copy, Debug, Default)]
pub struct BitcoinAddressValidator;

impl AddressValidator for BitcoinAddressValidator {
    type Address = bitcoin_encoding::Address;
    type Error = bitcoin_encoding::Error;

    fn validate(&self, text: &str, network: Network) -> Result<Self::Address, Self::Error> {
        bitcoin_encoding::address_decode(text, network)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaymentDetails {
    pub amount: Option<Amount>,
    pub label: Option<String>,
    pub message: Option<String>,
}

/// Unknown optional BIP21 data; it has no payment behavior. None denotes a bare
/// optional flag, while Some("") denotes a parameter with an empty value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionalParameter {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedAddress<A> {
    text: String,
    network: Network,
    address: A,
}

impl<A> ValidatedAddress<A> {
    /// Original address spelling. Uppercase witness QR payloads stay uppercase.
    pub fn text(&self) -> &str {
        &self.text
    }
    pub const fn network(&self) -> Network {
        self.network
    }
    pub fn address(&self) -> &A {
        &self.address
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentUri<A> {
    destination: ValidatedAddress<A>,
    details: PaymentDetails,
    optional_parameters: Vec<OptionalParameter>,
    original_uri: Option<String>,
}

impl<A> PaymentUri<A> {
    pub fn destination(&self) -> &ValidatedAddress<A> {
        &self.destination
    }
    pub fn details(&self) -> &PaymentDetails {
        &self.details
    }
    pub fn optional_parameters(&self) -> &[OptionalParameter] {
        &self.optional_parameters
    }
    /// Exact parsed URI spelling, including scheme case and encoded content.
    /// For parse_input this is the trimmed input; no URI normalization occurs.
    pub fn original_uri(&self) -> Option<&str> {
        self.original_uri.as_deref()
    }

    /// Retained application formatter: amount/label/message only. Opaque
    /// extensions (including lightning, pj, pjos, sp) are omitted, like the
    /// retained managed Address formatter. A zero amount is kept if present.
    pub fn to_uri(&self) -> Result<String, FormatError> {
        self.format(false)
    }

    /// Explicit metadata-preserving BIP21 serialization. Optional data remains
    /// opaque; this does not activate any extension or payment protocol.
    pub fn to_uri_with_optional_parameters(&self) -> Result<String, FormatError> {
        self.format(true)
    }

    fn format(&self, include_optional: bool) -> Result<String, FormatError> {
        let mut uri = format!("bitcoin:{}", self.destination.text);
        let mut separator = '?';
        let mut append = |name: &str, value: Option<&str>| -> Result<(), FormatError> {
            uri.push(separator);
            separator = '&';
            uri.push_str(&percent_encode(name).map_err(FormatError::Percent)?);
            if let Some(value) = value {
                uri.push('=');
                uri.push_str(&percent_encode(value).map_err(FormatError::Percent)?);
            }
            Ok(())
        };
        if let Some(amount) = self.details.amount {
            append("amount", Some(&amount.to_btc()))?;
        }
        if let Some(label) = &self.details.label {
            append("label", Some(label))?;
        }
        if let Some(message) = &self.details.message {
            append("message", Some(message))?;
        }
        if include_optional {
            for parameter in &self.optional_parameters {
                append(&parameter.name, parameter.value.as_deref())?;
            }
        }
        if input_too_long(&uri) {
            return Err(FormatError::InputTooLong);
        }
        Ok(uri)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    Percent(PercentError),
    InputTooLong,
}
impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Percent(_) => "Cannot encode URI component.",
            Self::InputTooLong => "Input is too long.",
        })
    }
}
impl std::error::Error for FormatError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParsedDestination<A> {
    Address(ValidatedAddress<A>),
    PaymentUri(PaymentUri<A>),
}

fn input_too_long(text: &str) -> bool {
    text.encode_utf16().take(MAX_INPUT_UTF16_UNITS + 1).count() > MAX_INPUT_UTF16_UNITS
}

fn validate_address<V: AddressValidator>(
    text: &str,
    network: Network,
    validator: &V,
) -> Result<ValidatedAddress<V::Address>, Error<V::Error>> {
    if text.is_empty() {
        return Err(Error::MissingAddress);
    }
    // Address alphabet syntax only; checksum/network/encoding lives in validator.
    if !text.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(Error::InvalidAddressSyntax);
    }
    let address = validator
        .validate(text, network)
        .map_err(Error::InvalidAddress)?;
    Ok(ValidatedAddress {
        text: text.to_owned(),
        network,
        address,
    })
}

/// Validate a new request for QR/receive formatting. It cannot be built around
/// an unchecked destination or an out-of-range/fractional-satoshi amount.
pub fn new_request<V: AddressValidator>(
    address: &str,
    network: Network,
    details: PaymentDetails,
    validator: &V,
) -> Result<PaymentUri<V::Address>, Error<V::Error>> {
    if input_too_long(address) {
        return Err(Error::InputTooLong);
    }
    let request = PaymentUri {
        destination: validate_address(address, network, validator)?,
        details,
        optional_parameters: Vec::new(),
        original_uri: None,
    };
    request.to_uri().map_err(|_| Error::InputTooLong)?;
    Ok(request)
}

pub fn parse<V: AddressValidator>(
    input: &str,
    network: Network,
    validator: &V,
) -> Result<PaymentUri<V::Address>, Error<V::Error>> {
    parse_with_mode(input, network, validator, ParsingMode::Bip21)
}

pub fn parse_compatible<V: AddressValidator>(
    input: &str,
    network: Network,
    validator: &V,
) -> Result<PaymentUri<V::Address>, Error<V::Error>> {
    parse_with_mode(input, network, validator, ParsingMode::ManagedCompatibility)
}

/// Parse an exact URI without trimming or changing its case. Address validation
/// precedes query validation, preserving retained managed error precedence.
pub fn parse_with_mode<V: AddressValidator>(
    input: &str,
    network: Network,
    validator: &V,
    mode: ParsingMode,
) -> Result<PaymentUri<V::Address>, Error<V::Error>> {
    if input_too_long(input) {
        return Err(Error::InputTooLong);
    }
    let (scheme, payload) = input.split_once(':').ok_or(Error::InvalidUri)?;
    if !scheme
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return Err(Error::InvalidUri);
    }
    if !scheme.eq_ignore_ascii_case("bitcoin") {
        return Err(Error::InvalidScheme {
            scheme: scheme.to_owned(),
        });
    }
    // BIP21 has neither authority nor fragment; accepting these could silently
    // discard content that the payer sees in the supplied request.
    if payload.starts_with("//") || payload.contains('#') {
        return Err(Error::InvalidUri);
    }
    let (address, query) = payload.split_once('?').unwrap_or((payload, ""));
    let destination = validate_address(address, network, validator)?;
    let mut details = PaymentDetails::default();
    let mut optional_parameters = Vec::new();
    let mut seen = BTreeSet::new();
    for field in query.split('&').filter(|field| !field.is_empty()) {
        let (raw_name, raw_value) = match field.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (field, None),
        };
        let name = decode_query_component(raw_name, mode, false).map_err(Error::InvalidQuery)?;
        if name.is_empty() {
            return Err(Error::InvalidQuery(QueryError::EmptyParameterName));
        }
        let key = if mode == ParsingMode::ManagedCompatibility {
            name.to_ascii_lowercase()
        } else {
            name.clone()
        };
        if !seen.insert(key.clone()) {
            return Err(Error::DuplicateParameter { parameter: name });
        }
        // Reject all required extension names, including req-amount/label.
        // No extension is implemented by this retained feature.
        if key.starts_with("req-") {
            return Err(Error::UnsupportedRequiredParameter { parameter: name });
        }
        let value = raw_value
            .map(|value| decode_query_component(value, mode, true))
            .transpose()
            .map_err(Error::InvalidQuery)?;
        match key.as_str() {
            "amount" => {
                let value = value.ok_or(Error::MissingAmountValue)?;
                if value.is_empty()
                    || (mode == ParsingMode::ManagedCompatibility && value.trim().is_empty())
                {
                    return Err(Error::MissingAmountValue);
                }
                details.amount =
                    Some(Amount::parse_btc(&value).map_err(Error::InvalidAmountValue)?);
            }
            "label" | "message" => {
                let value = value.ok_or_else(|| {
                    Error::InvalidQuery(QueryError::MissingEquals {
                        parameter: name.clone(),
                    })
                })?;
                if key == "label" {
                    details.label = Some(value);
                } else {
                    details.message = Some(value);
                }
            }
            _ => optional_parameters.push(OptionalParameter { name, value }),
        }
    }
    Ok(PaymentUri {
        destination,
        details,
        optional_parameters,
        original_uri: Some(input.to_owned()),
    })
}

fn decode_query_component(
    component: &str,
    mode: ParsingMode,
    is_value: bool,
) -> Result<String, QueryError> {
    for (offset, b) in component.bytes().enumerate() {
        let qchar = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'%'
                    | b'!'
                    | b'$'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b':'
                    | b'@'
                    | b'/'
                    | b'?'
            );
        let compatibility = mode == ParsingMode::ManagedCompatibility
            && is_value
            && (b == b' ' || b == b'=' || b >= 128);
        if !qchar && !compatibility {
            return Err(QueryError::InvalidCharacter { offset });
        }
    }
    percent_decode(component, mode).map_err(QueryError::Percent)
}

/// Retained AddressParser entry point: trim surrounding whitespace, enforce the
/// 1000 UTF-16-unit limit, then validate a bare address or a BIP21 request.
/// Select ManagedCompatibility when replacing the existing managed callers.
pub fn parse_input<V: AddressValidator>(
    input: &str,
    network: Network,
    validator: &V,
    mode: ParsingMode,
) -> Result<ParsedDestination<V::Address>, Error<V::Error>> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::EmptyInput);
    }
    if input_too_long(input) {
        return Err(Error::InputTooLong);
    }
    if input
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("bitcoin:"))
    {
        parse_with_mode(input, network, validator, mode).map(ParsedDestination::PaymentUri)
    } else {
        validate_address(input, network, validator).map(ParsedDestination::Address)
    }
}
