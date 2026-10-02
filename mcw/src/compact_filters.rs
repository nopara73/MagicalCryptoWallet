//! First-party, portable BIP158 Golomb-coded sets and basic block filters.
//!
//! Block hashes, filter hashes, and filter headers use their raw 32-byte internal
//! / wire representation, **not** the reversed hexadecimal display order.
//! Basic construction takes scripts, not transactions or wallet state. The
//! caller must supply all output scripts and the spent scripts for non-coinbase
//! inputs. Parsing validates the whole encoding before exposing membership.
//! Matches are probabilistic; they do not prove a transaction is relevant.
#![forbid(unsafe_code)]

use crate::bitcoin_encoding;
use std::fmt;

pub const BASIC_FILTER_TYPE: u8 = 0;
pub const BASIC_P: u8 = 19;
pub const BASIC_M: u32 = 784_931;

/// Parameters are kept outside the serialized GCS, as specified by BIP158.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Params {
    p: u8,
    m: u32,
}

impl Params {
    pub const BASIC: Self = Self {
        p: BASIC_P,
        m: BASIC_M,
    };

    /// P is in 0..=63. M is nonzero and, by its type, less than 2^32.
    pub fn new(p: u8, m: u32) -> Result<Self, Error> {
        if p > 63 || m == 0 {
            return Err(Error::InvalidParameters);
        }
        Ok(Self { p, m })
    }

    pub const fn p(self) -> u8 {
        self.p
    }
    pub const fn m(self) -> u32 {
        self.m
    }
}

/// Local resource policy, not Bitcoin consensus or P2P message-size limits.
/// Input limits apply before exclusions / deduplication, including empty items.
/// Query byte limits apply separately to each matching call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_filter_bytes: usize,
    pub max_elements: u32,
    pub max_queries: usize,
    pub max_input_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_filter_bytes: 4_000_000,
            max_elements: 1_000_000,
            max_queries: 1_000_000,
            max_input_bytes: 32_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidParameters,
    FilterTooLarge,
    TooManyElements,
    TooManyQueries,
    InputTooLarge,
    CountOverflow,
    Truncated,
    NonCanonicalCompactSize,
    TrailingBytes,
    NonZeroPadding,
    ValueOutOfRange,
    ValuesNotSorted,
    AllocationFailed,
    Hash(bitcoin_encoding::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidParameters => "GCS requires P <= 63 and 0 < M < 2^32",
            Self::FilterTooLarge => "serialized filter exceeds the byte limit",
            Self::TooManyElements => "filter element count exceeds the local limit",
            Self::TooManyQueries => "query count exceeds the local limit",
            Self::InputTooLarge => "script or query bytes exceed the local limit",
            Self::CountOverflow => "filter element count must be less than 2^32",
            Self::Truncated => "truncated compact filter",
            Self::NonCanonicalCompactSize => "nonminimal CompactSize element count",
            Self::TrailingBytes => "compact filter contains trailing bytes",
            Self::NonZeroPadding => "compact filter contains nonzero padding bits",
            Self::ValueOutOfRange => "decoded value is outside [0, N * M)",
            Self::ValuesNotSorted => "mapped values must be in nondecreasing order",
            Self::AllocationFailed => "compact filter allocation failed",
            Self::Hash(error) => return write!(f, "compact filter hash: {error}"),
        };
        f.write_str(message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Hash(error) => Some(error),
            _ => None,
        }
    }
}

/// First 16 bytes of an authenticated block hash in internal / wire order.
pub fn basic_key(block_hash: &[u8; 32]) -> [u8; 16] {
    let mut key = [0; 16];
    key.copy_from_slice(&block_hash[..16]);
    key
}

/// SipHash-2-4, with little-endian key words and little-endian message words.
/// This is BIP158's keyed mapping primitive, not a wallet signing primitive.
pub fn siphash24(key: &[u8; 16], message: &[u8]) -> u64 {
    let k0 = u64::from_le_bytes(key[..8].try_into().unwrap());
    let k1 = u64::from_le_bytes(key[8..].try_into().unwrap());
    let mut state = [
        0x736f6d6570736575 ^ k0,
        0x646f72616e646f6d ^ k1,
        0x6c7967656e657261 ^ k0,
        0x7465646279746573 ^ k1,
    ];
    let mut words = message.chunks_exact(8);
    for word in &mut words {
        let word = u64::from_le_bytes(word.try_into().unwrap());
        state[3] ^= word;
        sip_round(&mut state);
        sip_round(&mut state);
        state[0] ^= word;
    }
    // SipHash encodes the message length modulo 256 in the high byte.
    let mut last = (message.len() as u64) << 56;
    for (i, &byte) in words.remainder().iter().enumerate() {
        last |= u64::from(byte) << (8 * i);
    }
    state[3] ^= last;
    sip_round(&mut state);
    sip_round(&mut state);
    state[0] ^= last;
    state[2] ^= 0xff;
    for _ in 0..4 {
        sip_round(&mut state);
    }
    state[0] ^ state[1] ^ state[2] ^ state[3]
}

fn sip_round(v: &mut [u64; 4]) {
    v[0] = v[0].wrapping_add(v[1]);
    v[1] = v[1].rotate_left(13);
    v[1] ^= v[0];
    v[0] = v[0].rotate_left(32);
    v[2] = v[2].wrapping_add(v[3]);
    v[3] = v[3].rotate_left(16);
    v[3] ^= v[2];
    v[0] = v[0].wrapping_add(v[3]);
    v[3] = v[3].rotate_left(21);
    v[3] ^= v[0];
    v[2] = v[2].wrapping_add(v[1]);
    v[1] = v[1].rotate_left(17);
    v[1] ^= v[2];
    v[2] = v[2].rotate_left(32);
}

/// High 64 bits of a 128-bit product. Range zero yields zero, but an empty
/// filter never attempts a membership comparison in this zero-sized range.
pub fn map_into_range(hash: u64, range: u64) -> u64 {
    ((u128::from(hash) * u128::from(range)) >> 64) as u64
}

fn mapped(key: &[u8; 16], element: &[u8], range: u64) -> u64 {
    map_into_range(siphash24(key, element), range)
}

fn check_items(
    items: &[&[u8]],
    max_count: usize,
    limits: Limits,
    query: bool,
) -> Result<(), Error> {
    if items.len() > max_count {
        return Err(if query {
            Error::TooManyQueries
        } else {
            Error::TooManyElements
        });
    }
    let mut bytes = 0usize;
    for item in items {
        bytes = bytes.checked_add(item.len()).ok_or(Error::InputTooLarge)?;
        if bytes > limits.max_input_bytes {
            return Err(Error::InputTooLarge);
        }
    }
    Ok(())
}

fn reserved<T>(count: usize) -> Result<Vec<T>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| Error::AllocationFailed)?;
    Ok(values)
}

/// Construct a GCS from a set of byte strings (including empty strings).
/// Identical input strings are deduplicated before N and the hash range are
/// calculated. Distinct strings whose mapped hashes collide remain separate
/// encoded values, with zero deltas. No script rules apply to this generic API.
pub fn encode_gcs(
    key: &[u8; 16],
    params: Params,
    elements: &[&[u8]],
    limits: Limits,
) -> Result<Vec<u8>, Error> {
    check_items(elements, limits.max_elements as usize, limits, false)?;
    let mut items = reserved(elements.len())?;
    items.extend_from_slice(elements);
    encode_items(key, params, items, limits)
}

/// Construct a basic filter. Empty scripts are omitted. Output scripts starting
/// with OP_RETURN (0x6a) are omitted; spent scripts are not subject to that rule.
/// Script bytes are never parsed or rewritten. Coinbase inputs must be excluded
/// by the caller. This API does not fetch prevouts or validate block contents.
pub fn encode_basic(
    block_hash: &[u8; 32],
    output_scripts: &[&[u8]],
    spent_scripts: &[&[u8]],
    limits: Limits,
) -> Result<Vec<u8>, Error> {
    let count = output_scripts
        .len()
        .checked_add(spent_scripts.len())
        .ok_or(Error::TooManyElements)?;
    if count > limits.max_elements as usize {
        return Err(Error::TooManyElements);
    }
    check_items(output_scripts, limits.max_elements as usize, limits, false)?;
    let mut bytes = 0usize;
    for script in output_scripts.iter().chain(spent_scripts) {
        bytes = bytes
            .checked_add(script.len())
            .ok_or(Error::InputTooLarge)?;
        if bytes > limits.max_input_bytes {
            return Err(Error::InputTooLarge);
        }
    }
    let mut items = reserved(count)?;
    items.extend(
        output_scripts
            .iter()
            .copied()
            .filter(|s| !s.is_empty() && s[0] != 0x6a),
    );
    items.extend(spent_scripts.iter().copied().filter(|s| !s.is_empty()));
    encode_items(&basic_key(block_hash), Params::BASIC, items, limits)
}

fn encode_items(
    key: &[u8; 16],
    params: Params,
    mut items: Vec<&[u8]>,
    limits: Limits,
) -> Result<Vec<u8>, Error> {
    items.sort_unstable();
    items.dedup();
    let count = u32::try_from(items.len()).map_err(|_| Error::CountOverflow)?;
    let range = u64::from(count) * u64::from(params.m);
    let mut values = reserved(items.len())?;
    values.extend(items.into_iter().map(|item| mapped(key, item, range)));
    values.sort_unstable();
    encode_mapped_values(params, &values, limits)
}

/// Encode already mapped, nondecreasing values with CompactSize N and canonical
/// zero padding. Useful for codec inspection and conformance tests. Every value
/// must be below N*M. Equal values are retained, not deduplicated.
pub fn encode_mapped_values(
    params: Params,
    values: &[u64],
    limits: Limits,
) -> Result<Vec<u8>, Error> {
    let count = u32::try_from(values.len()).map_err(|_| Error::CountOverflow)?;
    if count > limits.max_elements {
        return Err(Error::TooManyElements);
    }
    let range = u64::from(count) * u64::from(params.m);
    let mut bits = u128::from(count) * (u128::from(params.p) + 1);
    let mut previous = 0;
    for &value in values {
        if value >= range {
            return Err(Error::ValueOutOfRange);
        }
        let delta = value.checked_sub(previous).ok_or(Error::ValuesNotSorted)?;
        bits += u128::from(delta >> params.p);
        previous = value;
    }
    let prefix = compact_size_len(count);
    let byte_count = u128::from(prefix as u64) + bits.div_ceil(8);
    if byte_count > limits.max_filter_bytes as u128 {
        return Err(Error::FilterTooLarge);
    }
    let byte_count = usize::try_from(byte_count).map_err(|_| Error::FilterTooLarge)?;
    let mut encoded = reserved(byte_count)?;
    write_count(&mut encoded, count);
    let mut writer = BitWriter {
        encoded: &mut encoded,
        used: 0,
    };
    previous = 0;
    for &value in values {
        let delta = value - previous;
        writer.ones(delta >> params.p);
        writer.bit(false);
        for shift in (0..params.p).rev() {
            writer.bit(((delta >> shift) & 1) != 0);
        }
        previous = value;
    }
    Ok(encoded)
}

fn compact_size_len(count: u32) -> usize {
    if count < 253 {
        1
    } else if count <= u16::MAX as u32 {
        3
    } else {
        5
    }
}

fn write_count(output: &mut Vec<u8>, count: u32) {
    if count < 253 {
        output.push(count as u8);
    } else if count <= u16::MAX as u32 {
        output.push(253);
        output.extend_from_slice(&(count as u16).to_le_bytes());
    } else {
        output.push(254);
        output.extend_from_slice(&count.to_le_bytes());
    }
}

fn read_count(encoded: &[u8]) -> Result<(u32, usize), Error> {
    let first = *encoded.first().ok_or(Error::Truncated)?;
    let (value, length, minimum) = match first {
        0..=252 => return Ok((u32::from(first), 1)),
        253 => (2usize, 3usize, 253u64),
        254 => (4, 5, 65_536),
        255 => (8, 9, 4_294_967_296),
    };
    let bytes = encoded.get(1..length).ok_or(Error::Truncated)?;
    let mut count = 0u64;
    for (i, &byte) in bytes.iter().enumerate().take(value) {
        count |= u64::from(byte) << (8 * i);
    }
    if count < minimum {
        return Err(Error::NonCanonicalCompactSize);
    }
    Ok((
        u32::try_from(count).map_err(|_| Error::CountOverflow)?,
        length,
    ))
}

struct BitWriter<'a> {
    encoded: &'a mut Vec<u8>,
    used: u8,
}

impl BitWriter<'_> {
    fn bit(&mut self, one: bool) {
        if self.used == 0 {
            self.encoded.push(0);
        }
        if one {
            *self.encoded.last_mut().unwrap() |= 1 << (7 - self.used);
        }
        self.used = (self.used + 1) % 8;
    }

    fn ones(&mut self, mut count: u64) {
        while self.used != 0 && count > 0 {
            self.bit(true);
            count -= 1;
        }
        while count >= 8 {
            self.encoded.push(0xff);
            count -= 8;
        }
        for _ in 0..count {
            self.bit(true);
        }
    }
}

struct ValueDecoder<'a> {
    bytes: &'a [u8],
    bit_position: usize,
    remaining: u32,
    previous: u64,
    range: u64,
    p: u8,
}

impl<'a> ValueDecoder<'a> {
    fn new(bytes: &'a [u8], count: u32, params: Params) -> Self {
        Self {
            bytes,
            bit_position: 0,
            remaining: count,
            previous: 0,
            range: u64::from(count) * u64::from(params.m),
            p: params.p,
        }
    }

    fn bit(&mut self) -> Result<bool, Error> {
        let byte = self
            .bytes
            .get(self.bit_position / 8)
            .ok_or(Error::Truncated)?;
        let one = (byte >> (7 - self.bit_position % 8)) & 1 != 0;
        self.bit_position = self
            .bit_position
            .checked_add(1)
            .ok_or(Error::FilterTooLarge)?;
        Ok(one)
    }

    fn next_value(&mut self) -> Result<Option<u64>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let max_delta = self.range - 1 - self.previous;
        let max_quotient = max_delta >> self.p;
        let mut quotient = 0u64;
        loop {
            // Skip full unary bytes, with an explicit range bound before shift.
            if self.bit_position.is_multiple_of(8)
                && self.bytes.get(self.bit_position / 8) == Some(&0xff)
            {
                quotient = quotient.checked_add(8).ok_or(Error::ValueOutOfRange)?;
                if quotient > max_quotient {
                    return Err(Error::ValueOutOfRange);
                }
                self.bit_position = self
                    .bit_position
                    .checked_add(8)
                    .ok_or(Error::FilterTooLarge)?;
            } else if self.bit()? {
                quotient = quotient.checked_add(1).ok_or(Error::ValueOutOfRange)?;
                if quotient > max_quotient {
                    return Err(Error::ValueOutOfRange);
                }
            } else {
                break;
            }
        }
        let mut remainder = 0u64;
        for _ in 0..self.p {
            remainder = (remainder << 1) | u64::from(self.bit()?);
        }
        let delta = (quotient << self.p) | remainder;
        if delta > max_delta {
            return Err(Error::ValueOutOfRange);
        }
        self.previous += delta;
        self.remaining -= 1;
        Ok(Some(self.previous))
    }

    fn finish(&self) -> Result<(), Error> {
        if self.remaining != 0 {
            return Err(Error::Truncated);
        }
        let used = self.bit_position.div_ceil(8);
        if used != self.bytes.len() {
            return Err(Error::TrailingBytes);
        }
        let offset = self.bit_position % 8;
        if offset != 0 && self.bytes[used - 1] & ((1u8 << (8 - offset)) - 1) != 0 {
            return Err(Error::NonZeroPadding);
        }
        Ok(())
    }
}

/// A borrowed, fully validated canonical filter. Safe Rust's immutable borrow
/// prevents callers from changing encoded bytes after validation. No wallet or
/// script data is retained. This is a codec object, not an authenticated chain.
#[derive(Clone, Debug)]
pub struct GcsFilter<'a> {
    encoded: &'a [u8],
    body: &'a [u8],
    count: u32,
    params: Params,
    key: [u8; 16],
    limits: Limits,
}

impl<'a> GcsFilter<'a> {
    pub fn parse(
        encoded: &'a [u8],
        key: [u8; 16],
        params: Params,
        limits: Limits,
    ) -> Result<Self, Error> {
        if encoded.len() > limits.max_filter_bytes {
            return Err(Error::FilterTooLarge);
        }
        let (count, prefix) = read_count(encoded)?;
        if count > limits.max_elements {
            return Err(Error::TooManyElements);
        }
        let body = &encoded[prefix..];
        // Reject counts impossible even without unary bits, before walking N.
        if u128::from(count) * (u128::from(params.p) + 1) > (body.len() as u128) * 8 {
            return Err(Error::Truncated);
        }
        let mut decoder = ValueDecoder::new(body, count, params);
        while decoder.next_value()?.is_some() {}
        decoder.finish()?;
        Ok(Self {
            encoded,
            body,
            count,
            params,
            key,
            limits,
        })
    }

    pub fn parse_basic(
        encoded: &'a [u8],
        block_hash: &[u8; 32],
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::parse(encoded, basic_key(block_hash), Params::BASIC, limits)
    }

    pub const fn count(&self) -> u32 {
        self.count
    }
    pub const fn params(&self) -> Params {
        self.params
    }
    pub const fn key(&self) -> &[u8; 16] {
        &self.key
    }
    pub const fn encoded(&self) -> &'a [u8] {
        self.encoded
    }

    fn decoder(&self) -> ValueDecoder<'a> {
        ValueDecoder::new(self.body, self.count, self.params)
    }
    fn range(&self) -> u64 {
        u64::from(self.count) * u64::from(self.params.m)
    }

    /// Decode for inspection. Matching itself never allocates the filter values.
    pub fn mapped_values(&self) -> Result<Vec<u64>, Error> {
        let mut values = reserved(self.count as usize)?;
        let mut decoder = self.decoder();
        while let Some(value) = decoder.next_value()? {
            values.push(value);
        }
        Ok(values)
    }

    pub fn matches(&self, element: &[u8]) -> Result<bool, Error> {
        check_items(&[element], self.limits.max_queries, self.limits, true)?;
        if self.count == 0 {
            return Ok(false);
        }
        let target = mapped(&self.key, element, self.range());
        let mut decoder = self.decoder();
        while let Some(value) = decoder.next_value()? {
            if value == target {
                return Ok(true);
            }
            if value > target {
                break;
            }
        }
        Ok(false)
    }

    /// Sort query hashes once, then merge against decoded filter values. Empty
    /// queries and empty filters return false. Duplicate queries are harmless.
    pub fn match_any(&self, elements: &[&[u8]]) -> Result<bool, Error> {
        check_items(elements, self.limits.max_queries, self.limits, true)?;
        if self.count == 0 || elements.is_empty() {
            return Ok(false);
        }
        let mut queries = reserved(elements.len())?;
        queries.extend(
            elements
                .iter()
                .map(|item| mapped(&self.key, item, self.range())),
        );
        queries.sort_unstable();
        queries.dedup();
        let mut query = 0;
        let mut decoder = self.decoder();
        while let Some(value) = decoder.next_value()? {
            while queries[query] < value {
                query += 1;
                if query == queries.len() {
                    return Ok(false);
                }
            }
            if queries[query] == value {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Return one membership result per query in the original order. Equal
    /// query hashes all receive the same result, including false positives.
    pub fn match_queries(&self, elements: &[&[u8]]) -> Result<Vec<bool>, Error> {
        check_items(elements, self.limits.max_queries, self.limits, true)?;
        let mut result = reserved(elements.len())?;
        result.resize(elements.len(), false);
        if self.count == 0 || elements.is_empty() {
            return Ok(result);
        }
        let mut queries = reserved(elements.len())?;
        queries.extend(
            elements
                .iter()
                .enumerate()
                .map(|(i, item)| (mapped(&self.key, item, self.range()), i)),
        );
        queries.sort_unstable();
        let mut query = 0;
        let mut decoder = self.decoder();
        while let Some(value) = decoder.next_value()? {
            while query < queries.len() && queries[query].0 < value {
                query += 1;
            }
            while query < queries.len() && queries[query].0 == value {
                result[queries[query].1] = true;
                query += 1;
            }
            if query == queries.len() {
                break;
            }
        }
        Ok(result)
    }

    /// BIP157 hashes the full serialization, including the CompactSize prefix.
    pub fn filter_hash(&self) -> Result<[u8; 32], Error> {
        bitcoin_encoding::double_sha256(self.encoded).map_err(Error::Hash)
    }

    /// Genesis uses an all-zero previous header. Raw digests are concatenated
    /// without reversing them; reversal belongs only to text display adapters.
    pub fn filter_header(&self, previous_header: &[u8; 32]) -> Result<[u8; 32], Error> {
        filter_header_from_hash(&self.filter_hash()?, previous_header)
    }
}

/// Calculate the next BIP157 filter-header commitment from raw hashes. This
/// does not authenticate the supplied filter hash or previous header.
pub fn filter_header_from_hash(
    filter_hash: &[u8; 32],
    previous_header: &[u8; 32],
) -> Result<[u8; 32], Error> {
    let mut input = [0; 64];
    input[..32].copy_from_slice(filter_hash);
    input[32..].copy_from_slice(previous_header);
    bitcoin_encoding::double_sha256(&input).map_err(Error::Hash)
}

/// Bounded, ordered header calculation with no chain storage, heights, peer
/// policy, block-header validation, checkpoint handling, or reorg state.
pub fn chain_filter_headers(
    filter_hashes: &[[u8; 32]],
    previous_header: &[u8; 32],
    max_headers: usize,
) -> Result<Vec<[u8; 32]>, Error> {
    if filter_hashes.len() > max_headers {
        return Err(Error::TooManyElements);
    }
    let mut headers = reserved(filter_hashes.len())?;
    let mut previous = *previous_header;
    for hash in filter_hashes {
        previous = filter_header_from_hash(hash, &previous)?;
        headers.push(previous);
    }
    Ok(headers)
}
