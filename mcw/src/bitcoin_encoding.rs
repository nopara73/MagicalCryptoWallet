//! Portable Bitcoin address and data codecs for the mcw application.
//!
//! Only Rust's standard library is used. No keys, wallet state, platform APIs,
//! script execution, or curve operations enter this module. Addresses represent
//! supplied payload hashes/programs; successful decoding is not proof of ownership
//! or spendability. Input is never trimmed or repaired.
#![forbid(unsafe_code)]

use std::fmt;

/// Bound allocations in the linear hexadecimal and bit-conversion codecs.
pub const MAX_DATA_BYTES: usize = 1_048_576;
/// Base58 uses quadratic radix conversion; keep its work and allocations bounded.
pub const MAX_BASE58_BYTES: usize = 4_096;
/// Conservative upper bound on the Base58 text for MAX_BASE58_BYTES bytes.
pub const MAX_BASE58_TEXT: usize = MAX_BASE58_BYTES * 138 / 100 + 1;
pub const MAX_BASE58CHECK_PAYLOAD: usize = MAX_BASE58_BYTES - 4;
pub const MAX_BECH32_LENGTH: usize = 90;
pub const MAX_SHA256_BYTES: u64 = u64::MAX / 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Codec {
    Hex,
    Base58,
    Bech32,
    BitConversion,
    Address,
}

/// Positions in string errors are UTF-8 byte offsets, not character indices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    SizeLimit {
        codec: Codec,
        max: usize,
        actual: usize,
    },
    OddHexLength,
    InvalidCharacter {
        codec: Codec,
        index: usize,
        byte: u8,
    },
    InvalidChecksum,
    ChecksumTooShort,
    MissingSeparator,
    InvalidHrp,
    MixedCase,
    /// Encoders require a lowercase HRP; decoders accept entirely uppercase text.
    UppercaseHrp,
    InvalidDataValue {
        index: usize,
        value: u8,
    },
    InvalidBitWidth,
    InvalidPadding,
    InvalidWitnessVersion(u8),
    InvalidWitnessProgramLength {
        version: u8,
        length: usize,
    },
    MissingWitnessVersion,
    WrongChecksumVariant {
        expected: ChecksumVariant,
        actual: ChecksumVariant,
    },
    WrongNetwork {
        expected: Network,
    },
    UnknownWitnessHrp,
    InvalidLegacyLength(usize),
    InvalidLegacyVersion(u8),
    Sha256MessageTooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SizeLimit { codec, max, actual } => {
                write!(f, "{codec:?} input size {actual} exceeds limit {max}")
            }
            Self::OddHexLength => f.write_str("hexadecimal input must have an even length"),
            Self::InvalidCharacter { codec, index, byte } => {
                write!(f, "invalid {codec:?} byte 0x{byte:02x} at offset {index}")
            }
            Self::InvalidChecksum => f.write_str("invalid checksum"),
            Self::ChecksumTooShort => f.write_str("checksum is missing or too short"),
            Self::MissingSeparator => f.write_str("Bech32 separator is missing"),
            Self::InvalidHrp => f.write_str("HRP must contain 1 to 83 printable ASCII bytes"),
            Self::MixedCase => f.write_str("mixed-case Bech32 input"),
            Self::UppercaseHrp => f.write_str("Bech32 encoding requires a lowercase HRP"),
            Self::InvalidDataValue { index, value } => {
                write!(f, "data value {value} is out of range at offset {index}")
            }
            Self::InvalidBitWidth => f.write_str("bit widths must be between 1 and 8"),
            Self::InvalidPadding => f.write_str("nonzero or excessive conversion padding"),
            Self::InvalidWitnessVersion(v) => write!(f, "witness version {v} is outside 0..=16"),
            Self::InvalidWitnessProgramLength { version, length } => write!(
                f,
                "invalid witness program length {length} for version {version}"
            ),
            Self::MissingWitnessVersion => f.write_str("witness version is missing"),
            Self::WrongChecksumVariant { expected, actual } => {
                write!(f, "witness checksum is {actual:?}, expected {expected:?}")
            }
            Self::WrongNetwork { expected } => write!(f, "address does not match {expected:?}"),
            Self::UnknownWitnessHrp => f.write_str("unrecognized Bitcoin witness HRP"),
            Self::InvalidLegacyLength(n) => {
                write!(f, "legacy address payload length {n} is not 21")
            }
            Self::InvalidLegacyVersion(v) => write!(f, "unsupported legacy version 0x{v:02x}"),
            Self::Sha256MessageTooLong => {
                f.write_str("SHA-256 message exceeds its 64-bit bit length")
            }
        }
    }
}

impl std::error::Error for Error {}

fn check_size(codec: Codec, actual: usize, max: usize) -> Result<(), Error> {
    if actual > max {
        Err(Error::SizeLimit { codec, max, actual })
    } else {
        Ok(())
    }
}

/// Lowercase output. Empty bytes encode to an empty string.
pub fn hex_encode(bytes: &[u8]) -> Result<String, Error> {
    check_size(Codec::Hex, bytes.len(), MAX_DATA_BYTES)?;
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 15) as usize] as char);
    }
    Ok(text)
}

/// Accepts either case of ASCII hex digits. No prefixes, spaces, or separators.
pub fn hex_decode(text: &str) -> Result<Vec<u8>, Error> {
    check_size(Codec::Hex, text.len(), MAX_DATA_BYTES * 2)?;
    if !text.len().is_multiple_of(2) {
        return Err(Error::OddHexLength);
    }
    let digit = |index: usize, byte: u8| match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(Error::InvalidCharacter {
            codec: Codec::Hex,
            index,
            byte,
        }),
    };
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for (i, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        bytes.push((digit(i * 2, pair[0])? << 4) | digit(i * 2 + 1, pair[1])?);
    }
    Ok(bytes)
}

/// Streaming FIPS 180-4 SHA-256. State can be cloned to hash a shared prefix.
/// Updating past MAX_SHA256_BYTES returns an error without modifying the state.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    used: usize,
    bytes: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            block: [0; 64],
            used: 0,
            bytes: 0,
        }
    }

    pub fn update(&mut self, mut input: &[u8]) -> Result<(), Error> {
        let length = u64::try_from(input.len()).map_err(|_| Error::Sha256MessageTooLong)?;
        let total = self
            .bytes
            .checked_add(length)
            .filter(|&n| n <= MAX_SHA256_BYTES)
            .ok_or(Error::Sha256MessageTooLong)?;
        self.bytes = total;
        if self.used != 0 {
            let count = (64 - self.used).min(input.len());
            self.block[self.used..self.used + count].copy_from_slice(&input[..count]);
            self.used += count;
            input = &input[count..];
            if self.used < 64 {
                return Ok(());
            }
            compress_sha256(&mut self.state, &self.block);
            self.used = 0;
        }
        let (blocks, tail) = input.as_chunks::<64>();
        for block in blocks {
            compress_sha256(&mut self.state, block);
        }
        self.block[..tail.len()].copy_from_slice(tail);
        self.used = tail.len();
        Ok(())
    }

    /// Consumes the state, so callers cannot accidentally update a finalized hash.
    pub fn finalize(mut self) -> [u8; 32] {
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 56 {
            self.block[self.used..].fill(0);
            compress_sha256(&mut self.state, &self.block);
            self.block.fill(0);
        } else {
            self.block[self.used..56].fill(0);
        }
        self.block[56..].copy_from_slice(&(self.bytes * 8).to_be_bytes());
        compress_sha256(&mut self.state, &self.block);
        let mut digest = [0; 32];
        for (word, out) in self.state.iter().zip(digest.as_chunks_mut::<4>().0) {
            *out = word.to_be_bytes();
        }
        digest
    }
}

fn compress_sha256(state: &mut [u32; 8], block: &[u8; 64]) {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut w = [0u32; 64];
    for (out, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *out = u32::from_be_bytes(*bytes);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choice = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choice)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (out, word) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *out = out.wrapping_add(word);
    }
}

pub fn sha256(bytes: &[u8]) -> Result<[u8; 32], Error> {
    let mut hash = Sha256::new();
    hash.update(bytes)?;
    Ok(hash.finalize())
}

pub fn double_sha256(bytes: &[u8]) -> Result<[u8; 32], Error> {
    sha256(&sha256(bytes)?)
}

const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Bitcoin's alphabet. Each leading zero byte becomes a leading '1'.
pub fn base58_encode(bytes: &[u8]) -> Result<String, Error> {
    check_size(Codec::Base58, bytes.len(), MAX_BASE58_BYTES)?;
    let zeros = bytes.iter().take_while(|&&b| b == 0).count();
    // Little-endian digits, with sufficient capacity for worst-case growth.
    let mut digits: Vec<u8> = Vec::with_capacity((bytes.len() - zeros) * 138 / 100 + 1);
    for &byte in &bytes[zeros..] {
        let mut carry = u32::from(byte);
        for digit in &mut digits {
            carry += u32::from(*digit) * 256;
            *digit = (carry % 58) as u8;
            carry /= 58;
        }
        while carry != 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut text = String::with_capacity(zeros + digits.len());
    for _ in 0..zeros {
        text.push('1');
    }
    for &digit in digits.iter().rev() {
        text.push(BASE58[digit as usize] as char);
    }
    Ok(text)
}

/// Strict ASCII input; whitespace and Bitcoin's excluded 0/O/I/l are rejected.
pub fn base58_decode(text: &str) -> Result<Vec<u8>, Error> {
    check_size(Codec::Base58, text.len(), MAX_BASE58_TEXT)?;
    // Validate everything before doing quadratic radix conversion.
    let mut values = Vec::with_capacity(text.len());
    for (index, &byte) in text.as_bytes().iter().enumerate() {
        let value = BASE58
            .iter()
            .position(|&b| b == byte)
            .ok_or(Error::InvalidCharacter {
                codec: Codec::Base58,
                index,
                byte,
            })?;
        values.push(value as u8);
    }
    let zeros = values.iter().take_while(|&&v| v == 0).count();
    check_size(Codec::Base58, zeros, MAX_BASE58_BYTES)?;
    let mut decoded: Vec<u8> = Vec::with_capacity((values.len() - zeros) * 733 / 1000 + 1);
    for &value in &values[zeros..] {
        let mut carry = u32::from(value);
        for byte in &mut decoded {
            carry += u32::from(*byte) * 58;
            *byte = carry as u8;
            carry >>= 8;
        }
        while carry != 0 {
            decoded.push(carry as u8);
            carry >>= 8;
        }
        check_size(Codec::Base58, zeros + decoded.len(), MAX_BASE58_BYTES)?;
    }
    let mut bytes = Vec::with_capacity(zeros + decoded.len());
    bytes.resize(zeros, 0);
    bytes.extend(decoded.iter().rev());
    Ok(bytes)
}

/// Appends the first four bytes of double-SHA256(payload) before Base58 encoding.
/// Payload is opaque: callers supply their version byte(s) if their format has any.
pub fn base58check_encode(payload: &[u8]) -> Result<String, Error> {
    check_size(Codec::Base58, payload.len(), MAX_BASE58CHECK_PAYLOAD)?;
    let checksum = double_sha256(payload)?;
    let mut bytes = Vec::with_capacity(payload.len() + 4);
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&checksum[..4]);
    base58_encode(&bytes)
}

pub fn base58check_decode(text: &str) -> Result<Vec<u8>, Error> {
    let mut bytes = base58_decode(text)?;
    if bytes.len() < 4 {
        return Err(Error::ChecksumTooShort);
    }
    let payload_length = bytes.len() - 4;
    let expected = double_sha256(&bytes[..payload_length])?;
    if bytes[payload_length..] != expected[..4] {
        return Err(Error::InvalidChecksum);
    }
    bytes.truncate(payload_length);
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChecksumVariant {
    Bech32,
    Bech32m,
}

impl ChecksumVariant {
    fn constant(self) -> u32 {
        match self {
            Self::Bech32 => 1,
            Self::Bech32m => 0x2bc830a3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bech32Data {
    /// Original HRP spelling is preserved, including accepted all-uppercase input.
    pub hrp: String,
    /// Five-bit symbols, with the six checksum symbols removed.
    pub data: Vec<u8>,
    pub variant: ChecksumVariant,
}

const BECH32: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

fn polymod_step(checksum: u32, value: u8) -> u32 {
    const GEN: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
    let high = checksum >> 25;
    let mut next = ((checksum & 0x1ffffff) << 5) ^ u32::from(value);
    for (i, generator) in GEN.iter().enumerate() {
        if ((high >> i) & 1) != 0 {
            next ^= generator;
        }
    }
    next
}

fn hrp_checksum(hrp: &str) -> u32 {
    // Only checksum calculation folds ASCII case, as BIP173 specifies.
    let mut checksum = 1;
    for byte in hrp.bytes() {
        checksum = polymod_step(checksum, byte.to_ascii_lowercase() >> 5);
    }
    checksum = polymod_step(checksum, 0);
    for byte in hrp.bytes() {
        checksum = polymod_step(checksum, byte.to_ascii_lowercase() & 31);
    }
    checksum
}

fn validate_hrp(hrp: &str) -> Result<(), Error> {
    if !(1..=83).contains(&hrp.len()) || hrp.bytes().any(|b| !(33..=126).contains(&b)) {
        Err(Error::InvalidHrp)
    } else {
        Ok(())
    }
}

/// Encodes five-bit symbols, not bytes. Lowercase HRP is required explicitly.
pub fn bech32_encode(hrp: &str, data: &[u8], variant: ChecksumVariant) -> Result<String, Error> {
    validate_hrp(hrp)?;
    if hrp.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(Error::UppercaseHrp);
    }
    // Check data length first so arithmetic cannot overflow for an arbitrary slice.
    check_size(Codec::Bech32, data.len(), MAX_BECH32_LENGTH)?;
    let length = hrp.len() + 1 + data.len() + 6;
    check_size(Codec::Bech32, length, MAX_BECH32_LENGTH)?;
    let mut checksum = hrp_checksum(hrp);
    let mut text = String::with_capacity(length);
    text.push_str(hrp);
    text.push('1');
    for (index, &value) in data.iter().enumerate() {
        if value > 31 {
            return Err(Error::InvalidDataValue { index, value });
        }
        checksum = polymod_step(checksum, value);
        text.push(BECH32[value as usize] as char);
    }
    for _ in 0..6 {
        checksum = polymod_step(checksum, 0);
    }
    checksum ^= variant.constant();
    for i in (0..6).rev() {
        text.push(BECH32[((checksum >> (i * 5)) & 31) as usize] as char);
    }
    Ok(text)
}

/// Accepts both checksum variants and all-lowercase or all-uppercase ASCII.
/// A caller interpreting the data must enforce the appropriate checksum variant.
pub fn bech32_decode(text: &str) -> Result<Bech32Data, Error> {
    check_size(Codec::Bech32, text.len(), MAX_BECH32_LENGTH)?;
    let mut lower = false;
    let mut upper = false;
    for (index, byte) in text.bytes().enumerate() {
        if !(33..=126).contains(&byte) {
            return Err(Error::InvalidCharacter {
                codec: Codec::Bech32,
                index,
                byte,
            });
        }
        lower |= byte.is_ascii_lowercase();
        upper |= byte.is_ascii_uppercase();
    }
    if lower && upper {
        return Err(Error::MixedCase);
    }
    let separator = text.rfind('1').ok_or(Error::MissingSeparator)?;
    let hrp = &text[..separator];
    validate_hrp(hrp)?;
    let encoded = &text.as_bytes()[separator + 1..];
    if encoded.len() < 6 {
        return Err(Error::ChecksumTooShort);
    }
    let mut data = Vec::with_capacity(encoded.len());
    let mut checksum = hrp_checksum(hrp);
    for (i, &byte) in encoded.iter().enumerate() {
        let value = BECH32
            .iter()
            .position(|&b| b == byte.to_ascii_lowercase())
            .ok_or(Error::InvalidCharacter {
                codec: Codec::Bech32,
                index: separator + 1 + i,
                byte,
            })?;
        checksum = polymod_step(checksum, value as u8);
        data.push(value as u8);
    }
    let variant = match checksum {
        1 => ChecksumVariant::Bech32,
        0x2bc830a3 => ChecksumVariant::Bech32m,
        _ => return Err(Error::InvalidChecksum),
    };
    data.truncate(data.len() - 6);
    Ok(Bech32Data {
        hrp: hrp.to_owned(),
        data,
        variant,
    })
}

/// Convert unsigned symbols of 1..=8 bits, most significant bits first.
/// Encoding bytes to five-bit groups uses pad=true; decoding uses pad=false.
pub fn convert_bits(data: &[u8], from: u8, to: u8, pad: bool) -> Result<Vec<u8>, Error> {
    if !(1..=8).contains(&from) || !(1..=8).contains(&to) {
        return Err(Error::InvalidBitWidth);
    }
    check_size(Codec::BitConversion, data.len(), MAX_DATA_BYTES)?;
    let capacity = (data.len() * usize::from(from)).div_ceil(usize::from(to));
    check_size(Codec::BitConversion, capacity, MAX_DATA_BYTES)?;
    let mask = (1u32 << to) - 1;
    let accumulator_mask = (1u32 << (from + to - 1)) - 1;
    let mut acc = 0u32;
    let mut bits = 0u8;
    let mut converted = Vec::with_capacity(capacity);
    for (index, &value) in data.iter().enumerate() {
        if (u32::from(value) >> from) != 0 {
            return Err(Error::InvalidDataValue { index, value });
        }
        acc = ((acc << from) | u32::from(value)) & accumulator_mask;
        bits += from;
        while bits >= to {
            bits -= to;
            converted.push(((acc >> bits) & mask) as u8);
        }
    }
    if pad {
        if bits > 0 {
            converted.push(((acc << (to - bits)) & mask) as u8);
        }
    } else if bits >= from || ((acc << (to - bits)) & mask) != 0 {
        return Err(Error::InvalidPadding);
    }
    Ok(converted)
}

/// Network selection is mandatory. Testnet, Testnet4, Signet and Regtest share
/// legacy prefixes; Testnet, Testnet4 and Signet also share witness HRP "tb".
/// The address text alone cannot distinguish networks that share an encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Network {
    Mainnet,
    Testnet,
    Testnet4,
    Signet,
    Regtest,
}

impl Network {
    pub const fn witness_hrp(self) -> &'static str {
        match self {
            Self::Mainnet => "bc",
            Self::Regtest => "bcrt",
            _ => "tb",
        }
    }

    pub const fn legacy_prefix(self, kind: LegacyKind) -> u8 {
        match (self, kind) {
            (Self::Mainnet, LegacyKind::P2pkh) => 0x00,
            (Self::Mainnet, LegacyKind::P2sh) => 0x05,
            (_, LegacyKind::P2pkh) => 0x6f,
            (_, LegacyKind::P2sh) => 0xc4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyKind {
    P2pkh,
    P2sh,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyAddress {
    pub network: Network,
    pub kind: LegacyKind,
    pub hash: [u8; 20],
}

pub fn legacy_address_encode(
    network: Network,
    kind: LegacyKind,
    hash: &[u8; 20],
) -> Result<String, Error> {
    let mut payload = [0; 21];
    payload[0] = network.legacy_prefix(kind);
    payload[1..].copy_from_slice(hash);
    base58check_encode(&payload)
}

pub fn legacy_address_decode(text: &str, network: Network) -> Result<LegacyAddress, Error> {
    // A standard 25-byte Base58Check address never needs more than 35 characters.
    check_size(Codec::Address, text.len(), 35)?;
    let payload = base58check_decode(text)?;
    if payload.len() != 21 {
        return Err(Error::InvalidLegacyLength(payload.len()));
    }
    let kind = match payload[0] {
        0x00 | 0x6f => LegacyKind::P2pkh,
        0x05 | 0xc4 => LegacyKind::P2sh,
        version => return Err(Error::InvalidLegacyVersion(version)),
    };
    if payload[0] != network.legacy_prefix(kind) {
        return Err(Error::WrongNetwork { expected: network });
    }
    Ok(LegacyAddress {
        network,
        kind,
        hash: payload[1..].try_into().expect("20-byte hash"),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WitnessAddress {
    pub network: Network,
    pub version: u8,
    pub program: Vec<u8>,
}

fn validate_witness(version: u8, length: usize) -> Result<(), Error> {
    if version > 16 {
        return Err(Error::InvalidWitnessVersion(version));
    }
    if !(2..=40).contains(&length) || (version == 0 && length != 20 && length != 32) {
        return Err(Error::InvalidWitnessProgramLength { version, length });
    }
    Ok(())
}

fn witness_variant(version: u8) -> ChecksumVariant {
    if version == 0 {
        ChecksumVariant::Bech32
    } else {
        ChecksumVariant::Bech32m
    }
}

pub fn witness_address_encode(
    network: Network,
    version: u8,
    program: &[u8],
) -> Result<String, Error> {
    validate_witness(version, program.len())?;
    let mut data = Vec::with_capacity(1 + (program.len() * 8).div_ceil(5));
    data.push(version);
    data.extend(convert_bits(program, 8, 5, true)?);
    bech32_encode(network.witness_hrp(), &data, witness_variant(version))
}

pub fn witness_address_decode(text: &str, network: Network) -> Result<WitnessAddress, Error> {
    let decoded = bech32_decode(text)?;
    if !["bc", "tb", "bcrt"]
        .iter()
        .any(|hrp| decoded.hrp.eq_ignore_ascii_case(hrp))
    {
        return Err(Error::UnknownWitnessHrp);
    }
    if !decoded.hrp.eq_ignore_ascii_case(network.witness_hrp()) {
        return Err(Error::WrongNetwork { expected: network });
    }
    let (&version, symbols) = decoded
        .data
        .split_first()
        .ok_or(Error::MissingWitnessVersion)?;
    if version > 16 {
        return Err(Error::InvalidWitnessVersion(version));
    }
    let program = convert_bits(symbols, 5, 8, false)?;
    validate_witness(version, program.len())?;
    let expected = witness_variant(version);
    if decoded.variant != expected {
        return Err(Error::WrongChecksumVariant {
            expected,
            actual: decoded.variant,
        });
    }
    Ok(WitnessAddress {
        network,
        version,
        program,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Address {
    Legacy(LegacyAddress),
    Witness(WitnessAddress),
}

/// Strict address validation, with an explicit expected network.
pub fn address_decode(text: &str, network: Network) -> Result<Address, Error> {
    check_size(Codec::Address, text.len(), MAX_BECH32_LENGTH)?;
    let witness_prefix = text.split_once('1').is_some_and(|(hrp, _)| {
        ["bc", "tb", "bcrt"]
            .iter()
            .any(|known| hrp.eq_ignore_ascii_case(known))
    });
    if witness_prefix {
        witness_address_decode(text, network).map(Address::Witness)
    } else {
        legacy_address_decode(text, network).map(Address::Legacy)
    }
}

/// Canonical encoding is explicit: witness output is lowercase, legacy output
/// retains Base58's case-sensitive alphabet. Mutable public fields are revalidated.
pub fn address_encode(address: &Address) -> Result<String, Error> {
    match address {
        Address::Legacy(a) => legacy_address_encode(a.network, a.kind, &a.hash),
        Address::Witness(a) => witness_address_encode(a.network, a.version, &a.program),
    }
}

#[cfg(test)]
mod sha256_length_tests {
    use super::*;

    #[test]
    fn sha256_length_limit_is_atomic() {
        let mut hash = Sha256::new();
        hash.bytes = MAX_SHA256_BYTES;
        let before = hash.clone();
        assert_eq!(hash.update(&[1]), Err(Error::Sha256MessageTooLong));
        assert_eq!(hash.state, before.state);
        assert_eq!(hash.block, before.block);
        assert_eq!(hash.used, before.used);
        assert_eq!(hash.bytes, before.bytes);
        assert!(hash.update(&[]).is_ok());
        hash.bytes = u64::MAX;
        assert_eq!(hash.update(&[1]), Err(Error::Sha256MessageTooLong));
    }
}
