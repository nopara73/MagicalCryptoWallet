//! Bounded Bitcoin transaction wire formats for the single mcw application.
//!
//! Scripts and witness elements are opaque bytes. Successful decoding is not
//! consensus, script, signature, amount-range, ownership, or spendability validation.
//! Amounts retain all signed 64-bit wire values, including invalid consensus values.
//! Hashes use the first-party bitcoin_encoding module; no platform APIs enter here.
#![forbid(unsafe_code)]

use crate::bitcoin_encoding;
use std::{fmt, mem::size_of};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resource {
    TransactionBytes,
    Inputs,
    Outputs,
    ScriptBytes,
    WitnessItemsPerInput,
    TotalWitnessItems,
    WitnessItemBytes,
    PayloadBytes,
    DecodedBytes,
}

/// Application resource limits, not Bitcoin consensus or relay policy.
/// `max_decoded_bytes` counts struct/vector elements plus copied payload bytes;
/// allocator bookkeeping, the caller's input slice, and output buffers are excluded.
/// Output buffers are separately bounded by `max_transaction_bytes`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_transaction_bytes: usize,
    pub max_inputs: usize,
    pub max_outputs: usize,
    pub max_script_bytes: usize,
    pub max_witness_items_per_input: usize,
    pub max_total_witness_items: usize,
    pub max_witness_item_bytes: usize,
    pub max_payload_bytes: usize,
    pub max_decoded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_transaction_bytes: 4_000_000,
            max_inputs: 100_000,
            max_outputs: 100_000,
            max_script_bytes: 4_000_000,
            max_witness_items_per_input: 100_000,
            max_total_witness_items: 100_000,
            max_witness_item_bytes: 4_000_000,
            max_payload_bytes: 4_000_000,
            max_decoded_bytes: 32_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    UnexpectedEnd {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    NonCanonicalCompactSize {
        offset: usize,
    },
    UnknownWitnessFlags {
        offset: usize,
        flags: u8,
    },
    SuperfluousWitness,
    TrailingBytes {
        offset: usize,
        remaining: usize,
    },
    LimitExceeded {
        resource: Resource,
        limit: usize,
        actual: u64,
    },
    SizeOverflow,
    AllocationFailed,
    Hash(bitcoin_encoding::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd {
                offset,
                needed,
                remaining,
            } => write!(
                f,
                "truncated at byte {offset}: need {needed}, have {remaining}"
            ),
            Self::NonCanonicalCompactSize { offset } => {
                write!(f, "noncanonical CompactSize at byte {offset}")
            }
            Self::UnknownWitnessFlags { offset, flags } => write!(
                f,
                "unsupported witness flags 0x{flags:02x} at byte {offset}"
            ),
            Self::SuperfluousWitness => f.write_str("witness encoding has only empty stacks"),
            Self::TrailingBytes { offset, remaining } => {
                write!(f, "{remaining} trailing bytes at byte {offset}")
            }
            Self::LimitExceeded {
                resource,
                limit,
                actual,
            } => write!(f, "{resource:?} size {actual} exceeds limit {limit}"),
            Self::SizeOverflow => f.write_str("wire length or size calculation overflowed"),
            Self::AllocationFailed => f.write_str("bounded allocation failed"),
            Self::Hash(error) => write!(f, "transaction hash failed: {error}"),
        }
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

/// Raw SHA256d digest byte order, identical to the 32 bytes in an outpoint.
/// Display reverses these bytes, as Bitcoin transaction-id text convention requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TxId(pub [u8; 32]);

impl fmt::Display for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.iter().rev() {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutPoint {
    /// Raw digest/wire order; no reversal takes place during serialization.
    pub txid: [u8; 32],
    pub vout: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxIn {
    pub previous_output: OutPoint,
    pub script_sig: Vec<u8>,
    pub sequence: u32,
    /// A nonempty stack containing even one empty item is a present witness.
    pub witness: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxOut {
    /// Exact signed satoshis. The codec deliberately does not enforce MoneyRange.
    pub value: i64,
    pub script_pubkey: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    /// All 32 version bits round trip (including the sign bit).
    pub version: i32,
    pub inputs: Vec<TxIn>,
    pub outputs: Vec<TxOut>,
    pub lock_time: u32,
}

/// Explicit legacy mode is necessary for PSBT unsigned transactions with zero
/// inputs: in witness-aware decoding their next byte is interpreted as flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeMode {
    Witness,
    Legacy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sizes {
    pub stripped: usize,
    pub total: usize,
    pub weight: usize,
    pub virtual_size: usize,
}

fn limit(resource: Resource, actual: u64, max: usize) -> Result<usize, Error> {
    let value = usize::try_from(actual).map_err(|_| Error::LimitExceeded {
        resource,
        limit: max,
        actual,
    })?;
    if value > max {
        Err(Error::LimitExceeded {
            resource,
            limit: max,
            actual,
        })
    } else {
        Ok(value)
    }
}

fn add(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b).ok_or(Error::SizeOverflow)
}

fn reserve<T>(count: usize) -> Result<Vec<T>, Error> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(count)
        .map_err(|_| Error::AllocationFailed)?;
    Ok(items)
}

struct Budget<'a> {
    limits: &'a Limits,
    payload: usize,
    decoded: usize,
    witness_items: usize,
}

impl<'a> Budget<'a> {
    fn new(limits: &'a Limits, header: usize) -> Result<Self, Error> {
        limit(
            Resource::DecodedBytes,
            header as u64,
            limits.max_decoded_bytes,
        )?;
        Ok(Self {
            limits,
            payload: 0,
            decoded: header,
            witness_items: 0,
        })
    }

    fn elements<T>(&mut self, count: usize) -> Result<(), Error> {
        let bytes = count
            .checked_mul(size_of::<T>())
            .ok_or(Error::SizeOverflow)?;
        self.decoded = add(self.decoded, bytes)?;
        limit(
            Resource::DecodedBytes,
            self.decoded as u64,
            self.limits.max_decoded_bytes,
        )?;
        Ok(())
    }

    fn bytes(&mut self, count: usize) -> Result<(), Error> {
        self.payload = add(self.payload, count)?;
        limit(
            Resource::PayloadBytes,
            self.payload as u64,
            self.limits.max_payload_bytes,
        )?;
        self.elements::<u8>(count)
    }

    fn witness_count(&mut self, count: usize) -> Result<(), Error> {
        limit(
            Resource::WitnessItemsPerInput,
            count as u64,
            self.limits.max_witness_items_per_input,
        )?;
        self.witness_items = add(self.witness_items, count)?;
        limit(
            Resource::TotalWitnessItems,
            self.witness_items as u64,
            self.limits.max_total_witness_items,
        )?;
        self.elements::<Vec<u8>>(count)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
    max_bytes: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], max_bytes: usize) -> Self {
        Self {
            data,
            offset: 0,
            max_bytes,
        }
    }
    fn remaining(&self) -> usize {
        self.data.len() - self.offset
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        self.require(count)?;
        let end = add(self.offset, count)?;
        let bytes = &self.data[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    fn require(&self, count: usize) -> Result<(), Error> {
        if count > self.remaining() {
            Err(Error::UnexpectedEnd {
                offset: self.offset,
                needed: count,
                remaining: self.remaining(),
            })
        } else {
            // Also used to preflight vector declarations before any allocation.
            let end = add(self.offset, count)?;
            limit(Resource::TransactionBytes, end as u64, self.max_bytes)?;
            Ok(())
        }
    }

    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut result = [0; N];
        result.copy_from_slice(self.take(N)?);
        Ok(result)
    }

    fn compact(&mut self) -> Result<u64, Error> {
        let offset = self.offset;
        let (value, minimum) = match self.byte()? {
            0xfd => (u16::from_le_bytes(self.array()?) as u64, 0xfd),
            0xfe => (u32::from_le_bytes(self.array()?) as u64, 0x1_0000),
            0xff => (u64::from_le_bytes(self.array()?), 0x1_0000_0000),
            value => return Ok(value as u64),
        };
        if value < minimum {
            Err(Error::NonCanonicalCompactSize { offset })
        } else {
            Ok(value)
        }
    }

    fn count(&mut self, resource: Resource, max: usize, min_bytes: usize) -> Result<usize, Error> {
        let count = limit(resource, self.compact()?, max)?;
        // Reject impossible declarations before allocating their vector.
        self.require(count.checked_mul(min_bytes).ok_or(Error::SizeOverflow)?)?;
        Ok(count)
    }

    fn blob(
        &mut self,
        resource: Resource,
        max: usize,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<u8>, Error> {
        let count = limit(resource, self.compact()?, max)?;
        let bytes = self.take(count)?;
        budget.bytes(count)?;
        let mut result = reserve(count)?;
        result.extend_from_slice(bytes);
        Ok(result)
    }

    fn finish(&self) -> Result<(), Error> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(Error::TrailingBytes {
                offset: self.offset,
                remaining: self.remaining(),
            })
        }
    }
}

pub const fn compact_size_len(value: u64) -> usize {
    match value {
        0..=0xfc => 1,
        0xfd..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}

fn put_compact(value: u64, output: &mut Vec<u8>) {
    match value {
        0..=0xfc => output.push(value as u8),
        0xfd..=0xffff => {
            output.push(0xfd);
            output.extend_from_slice(&(value as u16).to_le_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            output.push(0xfe);
            output.extend_from_slice(&(value as u32).to_le_bytes());
        }
        _ => {
            output.push(0xff);
            output.extend_from_slice(&value.to_le_bytes());
        }
    }
}

/// Read a canonical CompactSize prefix; returns value and consumed bytes.
pub fn decode_compact_size(bytes: &[u8]) -> Result<(u64, usize), Error> {
    let mut reader = Reader::new(bytes, 9);
    let value = reader.compact()?;
    Ok((value, reader.offset))
}

/// Append the shortest canonical encoding. Allocation failure leaves output intact.
pub fn write_compact_size(value: u64, output: &mut Vec<u8>) -> Result<(), Error> {
    output
        .try_reserve(compact_size_len(value))
        .map_err(|_| Error::AllocationFailed)?;
    put_compact(value, output);
    Ok(())
}

fn read_output(reader: &mut Reader<'_>, budget: &mut Budget<'_>) -> Result<TxOut, Error> {
    let value = i64::from_le_bytes(reader.array()?);
    let script_pubkey = reader.blob(
        Resource::ScriptBytes,
        budget.limits.max_script_bytes,
        budget,
    )?;
    Ok(TxOut {
        value,
        script_pubkey,
    })
}

fn read_witness(reader: &mut Reader<'_>, budget: &mut Budget<'_>) -> Result<Vec<Vec<u8>>, Error> {
    let count = reader.count(
        Resource::WitnessItemsPerInput,
        budget.limits.max_witness_items_per_input,
        1,
    )?;
    budget.witness_count(count)?;
    let mut stack = reserve(count)?;
    for _ in 0..count {
        stack.push(reader.blob(
            Resource::WitnessItemBytes,
            budget.limits.max_witness_item_bytes,
            budget,
        )?);
    }
    Ok(stack)
}

/// Decode exactly one TxOut, including the signed amount and opaque script.
pub fn decode_output(bytes: &[u8], limits: &Limits) -> Result<TxOut, Error> {
    limit(
        Resource::TransactionBytes,
        bytes.len() as u64,
        limits.max_transaction_bytes,
    )?;
    let mut reader = Reader::new(bytes, limits.max_transaction_bytes);
    let mut budget = Budget::new(limits, size_of::<TxOut>())?;
    let output = read_output(&mut reader, &mut budget)?;
    reader.finish()?;
    Ok(output)
}

/// Decode exactly one scriptWitness stack; an empty standalone stack is allowed.
pub fn decode_witness(bytes: &[u8], limits: &Limits) -> Result<Vec<Vec<u8>>, Error> {
    limit(
        Resource::TransactionBytes,
        bytes.len() as u64,
        limits.max_transaction_bytes,
    )?;
    let mut reader = Reader::new(bytes, limits.max_transaction_bytes);
    let mut budget = Budget::new(limits, size_of::<Vec<Vec<u8>>>())?;
    let stack = read_witness(&mut reader, &mut budget)?;
    reader.finish()?;
    Ok(stack)
}

impl Transaction {
    /// Decode one exact BIP144 transaction. Unknown flags, extra bytes, and a
    /// witness marker with all stacks empty are rejected, as in Bitcoin Core.
    pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, Error> {
        Self::decode_exact(bytes, limits, DecodeMode::Witness)
    }

    /// Decode legacy bytes only (also permits zero inputs and/or zero outputs).
    /// Required for BIP174's global unsigned transaction; does not auto-detect.
    pub fn decode_legacy(bytes: &[u8], limits: &Limits) -> Result<Self, Error> {
        Self::decode_exact(bytes, limits, DecodeMode::Legacy)
    }

    fn decode_exact(bytes: &[u8], limits: &Limits, mode: DecodeMode) -> Result<Self, Error> {
        limit(
            Resource::TransactionBytes,
            bytes.len() as u64,
            limits.max_transaction_bytes,
        )?;
        let (tx, consumed) = Self::decode_prefix(bytes, limits, mode)?;
        if consumed != bytes.len() {
            return Err(Error::TrailingBytes {
                offset: consumed,
                remaining: bytes.len() - consumed,
            });
        }
        Ok(tx)
    }

    /// Decode a transaction at the start of a larger buffer. The byte limit
    /// applies to the consumed transaction, not subsequent block/stream bytes.
    /// The caller must handle remaining bytes; exact APIs reject them.
    pub fn decode_prefix(
        bytes: &[u8],
        limits: &Limits,
        mode: DecodeMode,
    ) -> Result<(Self, usize), Error> {
        let mut reader = Reader::new(bytes, limits.max_transaction_bytes);
        let mut budget = Budget::new(limits, size_of::<Self>())?;
        let version = i32::from_le_bytes(reader.array()?);
        let mut input_count = reader.count(Resource::Inputs, limits.max_inputs, 41)?;
        let mut witness_encoding = false;
        let mut empty_legacy = false;
        if input_count == 0 && mode == DecodeMode::Witness {
            let offset = reader.offset;
            let flags = reader.byte()?;
            if flags == 0 {
                empty_legacy = true;
            } else {
                if flags != 1 {
                    return Err(Error::UnknownWitnessFlags { offset, flags });
                }
                witness_encoding = true;
                input_count = reader.count(Resource::Inputs, limits.max_inputs, 41)?;
            }
        }
        budget.elements::<TxIn>(input_count)?;
        let mut inputs = reserve(input_count)?;
        for _ in 0..input_count {
            let txid = reader.array()?;
            let vout = u32::from_le_bytes(reader.array()?);
            let script_sig =
                reader.blob(Resource::ScriptBytes, limits.max_script_bytes, &mut budget)?;
            let sequence = u32::from_le_bytes(reader.array()?);
            inputs.push(TxIn {
                previous_output: OutPoint { txid, vout },
                script_sig,
                sequence,
                witness: Vec::new(),
            });
        }
        let output_count = if empty_legacy {
            0
        } else {
            reader.count(Resource::Outputs, limits.max_outputs, 9)?
        };
        budget.elements::<TxOut>(output_count)?;
        let mut outputs = reserve(output_count)?;
        for _ in 0..output_count {
            outputs.push(read_output(&mut reader, &mut budget)?);
        }
        if witness_encoding {
            let mut any_witness = false;
            for input in &mut inputs {
                input.witness = read_witness(&mut reader, &mut budget)?;
                any_witness |= !input.witness.is_empty();
            }
            if !any_witness {
                return Err(Error::SuperfluousWitness);
            }
        }
        let lock_time = u32::from_le_bytes(reader.array()?);
        Ok((
            Self {
                version,
                inputs,
                outputs,
                lock_time,
            },
            reader.offset,
        ))
    }

    pub fn has_witness(&self) -> bool {
        self.inputs.iter().any(|input| !input.witness.is_empty())
    }

    /// Check all object/resource sizes before allocation, serialization or hashing.
    /// Weight = stripped * 3 + total; virtual size rounds weight up to 4-byte units.
    /// This reports size without claiming the transaction passes block weight rules.
    pub fn sizes(&self, limits: &Limits) -> Result<Sizes, Error> {
        limit(
            Resource::Inputs,
            self.inputs.len() as u64,
            limits.max_inputs,
        )?;
        limit(
            Resource::Outputs,
            self.outputs.len() as u64,
            limits.max_outputs,
        )?;
        let mut budget = Budget::new(limits, size_of::<Self>())?;
        budget.elements::<TxIn>(self.inputs.len())?;
        budget.elements::<TxOut>(self.outputs.len())?;
        let mut stripped = add(8, compact_size_len(self.inputs.len() as u64))?;
        stripped = add(stripped, compact_size_len(self.outputs.len() as u64))?;
        let mut witness_size = 0;
        // Count every input stack even when empty; marker/flags appear only if
        // at least one stack is nonempty.
        let mut any_witness = false;
        for input in &self.inputs {
            let len = input.script_sig.len();
            limit(Resource::ScriptBytes, len as u64, limits.max_script_bytes)?;
            budget.bytes(len)?;
            stripped = add(stripped, add(40, add(compact_size_len(len as u64), len)?)?)?;
            limit(
                Resource::TransactionBytes,
                stripped as u64,
                limits.max_transaction_bytes,
            )?;
            budget.witness_count(input.witness.len())?;
            any_witness |= !input.witness.is_empty();
            witness_size = add(witness_size, compact_size_len(input.witness.len() as u64))?;
            for item in &input.witness {
                let len = item.len();
                limit(
                    Resource::WitnessItemBytes,
                    len as u64,
                    limits.max_witness_item_bytes,
                )?;
                budget.bytes(len)?;
                witness_size = add(witness_size, add(compact_size_len(len as u64), len)?)?;
                limit(
                    Resource::TransactionBytes,
                    witness_size as u64,
                    limits.max_transaction_bytes,
                )?;
            }
        }
        for output in &self.outputs {
            let len = output.script_pubkey.len();
            limit(Resource::ScriptBytes, len as u64, limits.max_script_bytes)?;
            budget.bytes(len)?;
            stripped = add(stripped, add(8, add(compact_size_len(len as u64), len)?)?)?;
            limit(
                Resource::TransactionBytes,
                stripped as u64,
                limits.max_transaction_bytes,
            )?;
        }
        let total = if any_witness {
            add(stripped, add(2, witness_size)?)?
        } else {
            stripped
        };
        limit(
            Resource::TransactionBytes,
            total as u64,
            limits.max_transaction_bytes,
        )?;
        let weight = add(stripped.checked_mul(3).ok_or(Error::SizeOverflow)?, total)?;
        Ok(Sizes {
            stripped,
            total,
            weight,
            virtual_size: weight.div_ceil(4),
        })
    }

    /// Canonical BIP144 bytes. All-empty witness stacks use legacy encoding.
    /// Zero-input transactions with outputs need explicit `decode_legacy` on read.
    pub fn serialize(&self, limits: &Limits) -> Result<Vec<u8>, Error> {
        self.serialize_mode(limits, true)
    }

    /// Canonical legacy bytes, excluding all witnesses. This is the txid preimage.
    /// The entire object must still satisfy its resource budgets.
    pub fn serialize_legacy(&self, limits: &Limits) -> Result<Vec<u8>, Error> {
        self.serialize_mode(limits, false)
    }

    fn serialize_mode(&self, limits: &Limits, include_witness: bool) -> Result<Vec<u8>, Error> {
        let sizes = self.sizes(limits)?;
        let witness = include_witness && self.has_witness();
        let mut output = reserve(if witness { sizes.total } else { sizes.stripped })?;
        output.extend_from_slice(&self.version.to_le_bytes());
        if witness {
            output.extend_from_slice(&[0, 1]);
        }
        put_compact(self.inputs.len() as u64, &mut output);
        for input in &self.inputs {
            output.extend_from_slice(&input.previous_output.txid);
            output.extend_from_slice(&input.previous_output.vout.to_le_bytes());
            put_blob(&input.script_sig, &mut output);
            output.extend_from_slice(&input.sequence.to_le_bytes());
        }
        put_compact(self.outputs.len() as u64, &mut output);
        for item in &self.outputs {
            put_output(item, &mut output);
        }
        if witness {
            for input in &self.inputs {
                put_witness(&input.witness, &mut output);
            }
        }
        output.extend_from_slice(&self.lock_time.to_le_bytes());
        Ok(output)
    }

    /// SHA256d of legacy bytes. Returned raw digest bytes match outpoint order.
    pub fn txid(&self, limits: &Limits) -> Result<TxId, Error> {
        let bytes = self.serialize_legacy(limits)?;
        bitcoin_encoding::double_sha256(&bytes)
            .map(TxId)
            .map_err(Error::Hash)
    }

    /// SHA256d of the canonical serialization, including witness when present.
    /// Equals txid for transactions without witness (also for empty transactions).
    /// No special coinbase commitment rule is applied to this transaction hash.
    pub fn wtxid(&self, limits: &Limits) -> Result<TxId, Error> {
        let bytes = self.serialize(limits)?;
        bitcoin_encoding::double_sha256(&bytes)
            .map(TxId)
            .map_err(Error::Hash)
    }
}

fn put_blob(bytes: &[u8], output: &mut Vec<u8>) {
    put_compact(bytes.len() as u64, output);
    output.extend_from_slice(bytes);
}

fn put_output(item: &TxOut, output: &mut Vec<u8>) {
    output.extend_from_slice(&item.value.to_le_bytes());
    put_blob(&item.script_pubkey, output);
}

fn put_witness(stack: &[Vec<u8>], output: &mut Vec<u8>) {
    put_compact(stack.len() as u64, output);
    for item in stack {
        put_blob(item, output);
    }
}

pub fn serialize_output(item: &TxOut, limits: &Limits) -> Result<Vec<u8>, Error> {
    limit(
        Resource::ScriptBytes,
        item.script_pubkey.len() as u64,
        limits.max_script_bytes,
    )?;
    let mut budget = Budget::new(limits, size_of::<TxOut>())?;
    budget.bytes(item.script_pubkey.len())?;
    let size = add(
        8,
        add(
            compact_size_len(item.script_pubkey.len() as u64),
            item.script_pubkey.len(),
        )?,
    )?;
    limit(
        Resource::TransactionBytes,
        size as u64,
        limits.max_transaction_bytes,
    )?;
    let mut bytes = reserve(size)?;
    put_output(item, &mut bytes);
    Ok(bytes)
}

pub fn serialize_witness(stack: &[Vec<u8>], limits: &Limits) -> Result<Vec<u8>, Error> {
    let mut budget = Budget::new(limits, size_of::<Vec<Vec<u8>>>())?;
    budget.witness_count(stack.len())?;
    let mut size = compact_size_len(stack.len() as u64);
    for item in stack {
        limit(
            Resource::WitnessItemBytes,
            item.len() as u64,
            limits.max_witness_item_bytes,
        )?;
        budget.bytes(item.len())?;
        size = add(size, add(compact_size_len(item.len() as u64), item.len())?)?;
        limit(
            Resource::TransactionBytes,
            size as u64,
            limits.max_transaction_bytes,
        )?;
    }
    limit(
        Resource::TransactionBytes,
        size as u64,
        limits.max_transaction_bytes,
    )?;
    let mut bytes = reserve(size)?;
    put_witness(stack, &mut bytes);
    Ok(bytes)
}
