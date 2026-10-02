//! Portable Bitcoin block containers and Merkle data for the single mcw app.
//!
//! Hashes are raw SHA256d digest bytes, as carried in Bitcoin wire fields.
//! Only `BlockHash` text display reverses bytes. Transactions and SHA256d use
//! the actual first-party bitcoin_wire and bitcoin_encoding implementations.
//! Decoding does not authenticate a header or enforce consensus, proof of work,
//! chain selection, witness commitments, scripts, fees, or wallet state rules.
//! Bitcoin Core v30.0 is the format/algorithm reference; see the test provenance.
#![forbid(unsafe_code)]

use crate::{bitcoin_encoding, bitcoin_wire};
use bitcoin_wire::{DecodeMode, Transaction, TxId, TxIn, TxOut};
use std::{fmt, mem::size_of};

pub const HEADER_BYTES: usize = 80;
/// Bitcoin Core's partial-tree structural bound: 4,000,000 / (4 * 60).
/// This limited proof check is not validation of a full block's weight.
pub const MAX_PARTIAL_TRANSACTIONS: u32 = 16_666;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resource {
    BlockBytes,
    Transactions,
    DecodedBytes,
    MerkleLeaves,
    MerkleWorkBytes,
    PartialBytes,
}

/// Application bounds, separate from consensus. Decoded bytes count vector
/// elements and copied payload, not allocator metadata or caller-owned slices.
/// Merkle work counts reserved node/proof/match arrays, excluding caller input.
/// Serialization additionally holds its output and at most one transaction's
/// serialization buffer; each is independently bounded by its wire byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_block_bytes: usize,
    pub max_transactions: usize,
    pub max_decoded_bytes: usize,
    pub max_merkle_leaves: usize,
    pub max_merkle_work_bytes: usize,
    /// Applies to a partial tree alone, or the whole header + merkleblock payload.
    pub max_partial_bytes: usize,
    pub transaction: bitcoin_wire::Limits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_block_bytes: 4_000_000,
            max_transactions: 100_000,
            max_decoded_bytes: 64_000_000,
            max_merkle_leaves: 100_000,
            max_merkle_work_bytes: 16_000_000,
            max_partial_bytes: 1_000_000,
            transaction: bitcoin_wire::Limits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartialError {
    ZeroTransactions,
    TooManyTransactions,
    HashCount,
    FlagCount,
    ExhaustedBits,
    ExhaustedHashes,
    UnusedHashes,
    UnusedFlagBytes,
    NonZeroPadding,
    IdenticalBranches,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    UnexpectedEnd {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    TrailingBytes {
        offset: usize,
        remaining: usize,
    },
    LimitExceeded {
        resource: Resource,
        limit: usize,
        actual: u64,
    },
    /// `offset` is the containing block's byte offset; the source's offsets
    /// are relative to the transaction or CompactSize starting there.
    Wire {
        offset: usize,
        transaction: Option<usize>,
        source: bitcoin_wire::Error,
    },
    Encoding(bitcoin_encoding::Error),
    HashTextLength {
        actual: usize,
    },
    SizeOverflow,
    AllocationFailed,
    InvalidIndex {
        index: u32,
        transactions: u32,
    },
    ProofLength {
        expected: usize,
        actual: usize,
    },
    InvalidDuplicateLast,
    MutatedTree,
    RootMismatch,
    MatchMaskLength {
        expected: usize,
        actual: usize,
    },
    InvalidPartial(PartialError),
    NegativeTarget,
    ZeroTarget,
    TargetOverflow,
    NonCanonicalTarget,
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
            Self::TrailingBytes { offset, remaining } => {
                write!(f, "{remaining} trailing bytes at byte {offset}")
            }
            Self::LimitExceeded {
                resource,
                limit,
                actual,
            } => write!(f, "{resource:?} size {actual} exceeds limit {limit}"),
            Self::Wire {
                offset,
                transaction,
                source,
            } => write!(
                f,
                "wire data at byte {offset}, transaction {transaction:?}: {source}"
            ),
            Self::Encoding(source) => write!(f, "hash encoding: {source}"),
            Self::HashTextLength { actual } => {
                write!(f, "hash text needs 64 ASCII hex bytes, got {actual}")
            }
            Self::SizeOverflow => f.write_str("block size calculation overflowed"),
            Self::AllocationFailed => f.write_str("bounded allocation failed"),
            Self::InvalidIndex {
                index,
                transactions,
            } => write!(f, "transaction index {index} outside {transactions} leaves"),
            Self::ProofLength { expected, actual } => {
                write!(f, "Merkle branch needs {expected} siblings, got {actual}")
            }
            Self::InvalidDuplicateLast => {
                f.write_str("odd-width Merkle sibling must duplicate the last node")
            }
            Self::MutatedTree => f.write_str("identical real Merkle siblings detected"),
            Self::RootMismatch => f.write_str("Merkle root does not match supplied root"),
            Self::MatchMaskLength { expected, actual } => {
                write!(f, "match mask needs {expected} elements, got {actual}")
            }
            Self::InvalidPartial(reason) => write!(f, "invalid partial Merkle tree: {reason:?}"),
            Self::NegativeTarget => f.write_str("compact target is negative"),
            Self::ZeroTarget => f.write_str("compact target is zero"),
            Self::TargetOverflow => f.write_str("compact target exceeds 256 bits"),
            Self::NonCanonicalTarget => f.write_str("compact target is not canonically encoded"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Wire { source, .. } => Some(source),
            Self::Encoding(source) => Some(source),
            _ => None,
        }
    }
}

fn add(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b).ok_or(Error::SizeOverflow)
}
fn mul(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or(Error::SizeOverflow)
}
fn limit(resource: Resource, actual: u64, maximum: usize) -> Result<usize, Error> {
    let value = usize::try_from(actual).map_err(|_| Error::LimitExceeded {
        resource,
        limit: maximum,
        actual,
    })?;
    if value > maximum {
        Err(Error::LimitExceeded {
            resource,
            limit: maximum,
            actual,
        })
    } else {
        Ok(value)
    }
}
fn reserve<T>(count: usize) -> Result<Vec<T>, Error> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| Error::AllocationFailed)?;
    Ok(result)
}
fn wire_error(offset: usize, transaction: Option<usize>, source: bitcoin_wire::Error) -> Error {
    Error::Wire {
        offset,
        transaction,
        source,
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    maximum: usize,
    resource: Resource,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = add(self.offset, count)?;
        limit(self.resource, end as u64, self.maximum)?;
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if count > remaining {
            return Err(Error::UnexpectedEnd {
                offset: self.offset,
                needed: count,
                remaining,
            });
        }
        let result = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(result)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut value = [0; N];
        value.copy_from_slice(self.take(N)?);
        Ok(value)
    }
    fn compact(&mut self) -> Result<u64, Error> {
        let (value, count) = bitcoin_wire::decode_compact_size(&self.bytes[self.offset..])
            .map_err(|e| wire_error(self.offset, None, e))?;
        self.take(count)?;
        Ok(value)
    }
    fn remaining(&self) -> usize {
        self.bytes
            .len()
            .min(self.maximum)
            .saturating_sub(self.offset)
    }
    fn finish(&self) -> Result<(), Error> {
        if self.offset != self.bytes.len() {
            Err(Error::TrailingBytes {
                offset: self.offset,
                remaining: self.bytes.len() - self.offset,
            })
        } else {
            Ok(())
        }
    }
}

/// Raw digest/wire order. Lowercase display reverses exactly these 32 bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BlockHash(pub [u8; 32]);
impl BlockHash {
    pub fn from_display_hex(text: &str) -> Result<Self, Error> {
        if text.len() != 64 {
            return Err(Error::HashTextLength { actual: text.len() });
        }
        let decoded = bitcoin_encoding::hex_decode(text).map_err(Error::Encoding)?;
        let mut bytes = [0; 32];
        bytes.copy_from_slice(&decoded);
        bytes.reverse();
        Ok(Self(bytes))
    }
}
impl fmt::Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.iter().rev() {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockHeader {
    pub version: i32,
    pub previous_block_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub time: u32,
    pub bits: u32,
    pub nonce: u32,
}
impl BlockHeader {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let (header, consumed) = Self::decode_prefix(bytes)?;
        if consumed != bytes.len() {
            return Err(Error::TrailingBytes {
                offset: consumed,
                remaining: bytes.len() - consumed,
            });
        }
        Ok(header)
    }
    pub fn decode_prefix(bytes: &[u8]) -> Result<(Self, usize), Error> {
        let mut reader = Reader {
            bytes,
            offset: 0,
            maximum: HEADER_BYTES,
            resource: Resource::BlockBytes,
        };
        let header = Self {
            version: i32::from_le_bytes(reader.array()?),
            previous_block_hash: reader.array()?,
            merkle_root: reader.array()?,
            time: u32::from_le_bytes(reader.array()?),
            bits: u32::from_le_bytes(reader.array()?),
            nonce: u32::from_le_bytes(reader.array()?),
        };
        Ok((header, reader.offset))
    }
    pub fn encode(&self) -> [u8; HEADER_BYTES] {
        let mut bytes = [0; HEADER_BYTES];
        bytes[..4].copy_from_slice(&self.version.to_le_bytes());
        bytes[4..36].copy_from_slice(&self.previous_block_hash);
        bytes[36..68].copy_from_slice(&self.merkle_root);
        bytes[68..72].copy_from_slice(&self.time.to_le_bytes());
        bytes[72..76].copy_from_slice(&self.bits.to_le_bytes());
        bytes[76..80].copy_from_slice(&self.nonce.to_le_bytes());
        bytes
    }
    pub fn hash(&self) -> Result<BlockHash, Error> {
        bitcoin_encoding::double_sha256(&self.encode())
            .map(BlockHash)
            .map_err(Error::Encoding)
    }
    /// Checks only compact numeric representation, without a network powLimit
    /// or any proof-of-work/header/chain acceptance decision.
    pub fn target(&self, require_canonical: bool) -> Result<[u8; 32], Error> {
        CompactTarget::from_bits(self.bits).checked_positive(require_canonical)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockSizes {
    pub stripped: usize,
    pub total: usize,
    pub weight: usize,
}

/// Matches bitcoin_wire's decoded-element accounting, without implementing
/// transaction parsing/serialization/hashing again.
fn transaction_heap_bytes(tx: &Transaction) -> Result<usize, Error> {
    let mut bytes = add(
        mul(tx.inputs.len(), size_of::<TxIn>())?,
        mul(tx.outputs.len(), size_of::<TxOut>())?,
    )?;
    for input in &tx.inputs {
        bytes = add(
            bytes,
            add(
                input.script_sig.len(),
                mul(input.witness.len(), size_of::<Vec<u8>>())?,
            )?,
        )?;
        for item in &input.witness {
            bytes = add(bytes, item.len())?;
        }
    }
    for output in &tx.outputs {
        bytes = add(bytes, output.script_pubkey.len())?;
    }
    Ok(bytes)
}
impl Block {
    pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, Error> {
        Self::decode_exact(bytes, limits, DecodeMode::Witness)
    }
    /// Explicit legacy transaction interpretation, including zero-input data.
    pub fn decode_legacy(bytes: &[u8], limits: &Limits) -> Result<Self, Error> {
        Self::decode_exact(bytes, limits, DecodeMode::Legacy)
    }
    fn decode_exact(bytes: &[u8], limits: &Limits, mode: DecodeMode) -> Result<Self, Error> {
        limit(
            Resource::BlockBytes,
            bytes.len() as u64,
            limits.max_block_bytes,
        )?;
        let (block, consumed) = Self::decode_prefix(bytes, limits, mode)?;
        if consumed != bytes.len() {
            return Err(Error::TrailingBytes {
                offset: consumed,
                remaining: bytes.len() - consumed,
            });
        }
        Ok(block)
    }
    /// Byte limits apply to this block, not subsequent stream bytes.
    pub fn decode_prefix(
        bytes: &[u8],
        limits: &Limits,
        mode: DecodeMode,
    ) -> Result<(Self, usize), Error> {
        limit(
            Resource::BlockBytes,
            HEADER_BYTES as u64,
            limits.max_block_bytes,
        )?;
        let (header, _) = BlockHeader::decode_prefix(bytes)?;
        let mut reader = Reader {
            bytes,
            offset: HEADER_BYTES,
            maximum: limits.max_block_bytes,
            resource: Resource::BlockBytes,
        };
        let count = limit(
            Resource::Transactions,
            reader.compact()?,
            limits.max_transactions,
        )?;
        let minimum = mul(count, 10)?; // smallest decodable transaction, not a consensus minimum
        if minimum > reader.remaining() {
            return Err(Error::UnexpectedEnd {
                offset: reader.offset,
                needed: minimum,
                remaining: reader.remaining(),
            });
        }
        let mut decoded = add(size_of::<Self>(), mul(count, size_of::<Transaction>())?)?;
        limit(
            Resource::DecodedBytes,
            decoded as u64,
            limits.max_decoded_bytes,
        )?;
        let mut transactions = reserve(count)?;
        for index in 0..count {
            let offset = reader.offset;
            let mut tx_limits = limits.transaction;
            tx_limits.max_transaction_bytes = tx_limits
                .max_transaction_bytes
                .min(limits.max_block_bytes - offset);
            tx_limits.max_decoded_bytes = tx_limits
                .max_decoded_bytes
                .min((limits.max_decoded_bytes - decoded).saturating_add(size_of::<Transaction>()));
            let (tx, consumed) = Transaction::decode_prefix(&bytes[offset..], &tx_limits, mode)
                .map_err(|e| wire_error(offset, Some(index), e))?;
            reader.take(consumed)?;
            decoded = add(decoded, transaction_heap_bytes(&tx)?)?;
            limit(
                Resource::DecodedBytes,
                decoded as u64,
                limits.max_decoded_bytes,
            )?;
            transactions.push(tx);
        }
        Ok((
            Self {
                header,
                transactions,
            },
            reader.offset,
        ))
    }
    /// Exact container sizes; weight is reported as data, not consensus policy.
    pub fn sizes(&self, limits: &Limits) -> Result<BlockSizes, Error> {
        limit(
            Resource::Transactions,
            self.transactions.len() as u64,
            limits.max_transactions,
        )?;
        let base = add(
            HEADER_BYTES,
            bitcoin_wire::compact_size_len(self.transactions.len() as u64),
        )?;
        let (mut stripped, mut total) = (base, base);
        let mut decoded = add(
            size_of::<Self>(),
            mul(self.transactions.len(), size_of::<Transaction>())?,
        )?;
        limit(
            Resource::DecodedBytes,
            decoded as u64,
            limits.max_decoded_bytes,
        )?;
        limit(Resource::BlockBytes, total as u64, limits.max_block_bytes)?;
        for (index, tx) in self.transactions.iter().enumerate() {
            let sizes = tx
                .sizes(&limits.transaction)
                .map_err(|e| wire_error(total, Some(index), e))?;
            decoded = add(decoded, transaction_heap_bytes(tx)?)?;
            limit(
                Resource::DecodedBytes,
                decoded as u64,
                limits.max_decoded_bytes,
            )?;
            stripped = add(stripped, sizes.stripped)?;
            total = add(total, sizes.total)?;
            limit(Resource::BlockBytes, total as u64, limits.max_block_bytes)?;
        }
        Ok(BlockSizes {
            stripped,
            total,
            weight: add(mul(stripped, 3)?, total)?,
        })
    }
    pub fn serialize(&self, limits: &Limits) -> Result<Vec<u8>, Error> {
        self.serialize_mode(limits, true)
    }
    pub fn serialize_legacy(&self, limits: &Limits) -> Result<Vec<u8>, Error> {
        self.serialize_mode(limits, false)
    }
    fn serialize_mode(&self, limits: &Limits, witness: bool) -> Result<Vec<u8>, Error> {
        let sizes = self.sizes(limits)?;
        let mut bytes = reserve(if witness { sizes.total } else { sizes.stripped })?;
        bytes.extend_from_slice(&self.header.encode());
        bitcoin_wire::write_compact_size(self.transactions.len() as u64, &mut bytes)
            .map_err(|e| wire_error(HEADER_BYTES, None, e))?;
        for (index, tx) in self.transactions.iter().enumerate() {
            let encoded = if witness {
                tx.serialize(&limits.transaction)
            } else {
                tx.serialize_legacy(&limits.transaction)
            }
            .map_err(|e| wire_error(bytes.len(), Some(index), e))?;
            bytes.extend_from_slice(&encoded);
        }
        Ok(bytes)
    }
    pub fn txids(&self, limits: &Limits) -> Result<Vec<TxId>, Error> {
        self.sizes(limits)?;
        leaf_count(self.transactions.len(), limits)?;
        limit(
            Resource::MerkleWorkBytes,
            mul(self.transactions.len(), size_of::<TxId>())? as u64,
            limits.max_merkle_work_bytes,
        )?;
        let mut txids = reserve(self.transactions.len())?;
        for (index, tx) in self.transactions.iter().enumerate() {
            txids.push(
                tx.txid(&limits.transaction)
                    .map_err(|e| wire_error(0, Some(index), e))?,
            );
        }
        Ok(txids)
    }
    pub fn merkle_root(&self, limits: &Limits) -> Result<MerkleRoot, Error> {
        let txids = self.txids(limits)?;
        merkle_root(&txids, &after_txids(limits, txids.len())?)
    }
    /// Empty or mutated containers are explicitly rejected by this opt-in check.
    /// A matching root still does not authenticate the header or witness data.
    pub fn check_merkle_root(&self, limits: &Limits) -> Result<(), Error> {
        if self.transactions.is_empty() {
            return Err(Error::InvalidPartial(PartialError::ZeroTransactions));
        }
        let result = self.merkle_root(limits)?;
        if result.mutated {
            return Err(Error::MutatedTree);
        }
        if result.hash != self.header.merkle_root {
            return Err(Error::RootMismatch);
        }
        Ok(())
    }
    pub fn merkle_proof(
        &self,
        index: u32,
        limits: &Limits,
    ) -> Result<(MerkleProof, MerkleRoot), Error> {
        let txids = self.txids(limits)?;
        MerkleProof::build(&txids, index, &after_txids(limits, txids.len())?)
    }
}
fn after_txids(limits: &Limits, count: usize) -> Result<Limits, Error> {
    let bytes = mul(count, size_of::<TxId>())?;
    limit(
        Resource::MerkleWorkBytes,
        bytes as u64,
        limits.max_merkle_work_bytes,
    )?;
    Ok(Limits {
        max_merkle_work_bytes: limits.max_merkle_work_bytes - bytes,
        ..*limits
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MerkleRoot {
    pub hash: [u8; 32],
    pub mutated: bool,
}
fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> Result<[u8; 32], Error> {
    let mut pair = [0; 64];
    pair[..32].copy_from_slice(left);
    pair[32..].copy_from_slice(right);
    bitcoin_encoding::double_sha256(&pair).map_err(Error::Encoding)
}
fn leaf_count(count: usize, limits: &Limits) -> Result<u32, Error> {
    limit(
        Resource::MerkleLeaves,
        count as u64,
        limits.max_merkle_leaves.min(u32::MAX as usize),
    )?;
    Ok(count as u32)
}
fn tree_height(mut count: u32) -> usize {
    let mut height = 0;
    while count > 1 {
        count = count.div_ceil(2);
        height += 1;
    }
    height
}
fn copy_nodes(txids: &[TxId]) -> Result<Vec<[u8; 32]>, Error> {
    let mut nodes = reserve(txids.len())?;
    nodes.extend(txids.iter().map(|txid| txid.0));
    Ok(nodes)
}
fn reduce(nodes: &mut Vec<[u8; 32]>, mutated: &mut bool) -> Result<(), Error> {
    let width = nodes.len();
    for parent in 0..width.div_ceil(2) {
        let left = nodes[parent * 2];
        let right = if parent * 2 + 1 < width {
            let right = nodes[parent * 2 + 1];
            *mutated |= left == right;
            right
        } else {
            left
        };
        nodes[parent] = hash_pair(&left, &right)?;
    }
    nodes.truncate(width.div_ceil(2));
    Ok(())
}
/// Bitcoin duplicate-last tree, detecting equal real sibling pairs at every
/// level before artificial duplication. Empty input returns a zero root.
pub fn merkle_root(txids: &[TxId], limits: &Limits) -> Result<MerkleRoot, Error> {
    leaf_count(txids.len(), limits)?;
    limit(
        Resource::MerkleWorkBytes,
        mul(txids.len(), 32)? as u64,
        limits.max_merkle_work_bytes,
    )?;
    let mut nodes = copy_nodes(txids)?;
    let mut mutated = false;
    while nodes.len() > 1 {
        reduce(&mut nodes, &mut mutated)?;
    }
    Ok(MerkleRoot {
        hash: nodes.first().copied().unwrap_or([0; 32]),
        mutated,
    })
}

/// Siblings run from leaf to root, including explicit duplicate-last siblings.
/// Transaction count constrains the shape; it is caller-supplied metadata and
/// is not independently committed by a Bitcoin Merkle root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerkleProof {
    pub transaction_count: u32,
    pub transaction_index: u32,
    pub siblings: Vec<[u8; 32]>,
}
impl MerkleProof {
    /// Returns full-tree mutation evidence alongside the proof. Do not discard
    /// a true flag: verification alone sees only mutations exposed on its path.
    pub fn build(txids: &[TxId], index: u32, limits: &Limits) -> Result<(Self, MerkleRoot), Error> {
        let count = leaf_count(txids.len(), limits)?;
        if index >= count {
            return Err(Error::InvalidIndex {
                index,
                transactions: count,
            });
        }
        let height = tree_height(count);
        limit(
            Resource::MerkleWorkBytes,
            mul(add(txids.len(), height)?, 32)? as u64,
            limits.max_merkle_work_bytes,
        )?;
        let mut nodes = copy_nodes(txids)?;
        let mut siblings = reserve(height)?;
        let (mut position, mut mutated) = (index as usize, false);
        while nodes.len() > 1 {
            let sibling = (position ^ 1).min(nodes.len() - 1);
            siblings.push(nodes[sibling]);
            reduce(&mut nodes, &mut mutated)?;
            position /= 2;
        }
        let root = MerkleRoot {
            hash: nodes[0],
            mutated,
        };
        Ok((
            Self {
                transaction_count: count,
                transaction_index: index,
                siblings,
            },
            root,
        ))
    }
    /// Validates count/index/path shape and the visible real-sibling mutation
    /// rule. Hidden sibling subtrees, header provenance and transaction count
    /// authenticity cannot be established from an inclusion proof.
    pub fn root(&self, leaf: TxId, limits: &Limits) -> Result<[u8; 32], Error> {
        leaf_count(self.transaction_count as usize, limits)?;
        if self.transaction_index >= self.transaction_count {
            return Err(Error::InvalidIndex {
                index: self.transaction_index,
                transactions: self.transaction_count,
            });
        }
        let expected = tree_height(self.transaction_count);
        if self.siblings.len() != expected {
            return Err(Error::ProofLength {
                expected,
                actual: self.siblings.len(),
            });
        }
        limit(
            Resource::MerkleWorkBytes,
            mul(expected, 32)? as u64,
            limits.max_merkle_work_bytes,
        )?;
        let (mut hash, mut width, mut position) =
            (leaf.0, self.transaction_count, self.transaction_index);
        for sibling in &self.siblings {
            if (position ^ 1) >= width {
                if *sibling != hash {
                    return Err(Error::InvalidDuplicateLast);
                }
            } else if *sibling == hash {
                return Err(Error::MutatedTree);
            }
            hash = if position & 1 == 0 {
                hash_pair(&hash, sibling)?
            } else {
                hash_pair(sibling, &hash)?
            };
            width = width.div_ceil(2);
            position /= 2;
        }
        Ok(hash)
    }
    pub fn verify(
        &self,
        leaf: TxId,
        expected_root: &[u8; 32],
        limits: &Limits,
    ) -> Result<(), Error> {
        if self.root(leaf, limits)? != *expected_root {
            return Err(Error::RootMismatch);
        }
        Ok(())
    }
}

/// BIP37 partial-tree wire fields, with hashes in raw digest order and flags
/// least-significant bit first. Public mutable fields are checked on every use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialMerkleTree {
    pub transaction_count: u32,
    pub hashes: Vec<[u8; 32]>,
    pub flags: Vec<u8>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MerkleMatch {
    pub index: u32,
    pub txid: TxId,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedMatches {
    pub merkle_root: [u8; 32],
    /// Depth-first leaf order, hence increasing original transaction indices.
    pub matches: Vec<MerkleMatch>,
    pub bits_used: usize,
}

fn partial_count(count: u32, limits: &Limits) -> Result<(), Error> {
    if count == 0 {
        return Err(Error::InvalidPartial(PartialError::ZeroTransactions));
    }
    if count > MAX_PARTIAL_TRANSACTIONS {
        return Err(Error::InvalidPartial(PartialError::TooManyTransactions));
    }
    leaf_count(count as usize, limits)?;
    Ok(())
}
fn tree_width(count: u32, height: usize) -> usize {
    u64::from(count).div_ceil(1u64 << height) as usize
}
fn node_count(mut width: usize) -> Result<usize, Error> {
    let mut total = width;
    while width > 1 {
        width = width.div_ceil(2);
        total = add(total, width)?;
    }
    Ok(total)
}
#[derive(Clone, Copy)]
struct TreeNode {
    hash: [u8; 32],
    matched: bool,
}
fn traversal_counts(levels: &[Vec<TreeNode>], height: usize, position: usize) -> (usize, usize) {
    if height == 0 || !levels[height][position].matched {
        return (1, 1);
    }
    let (left_bits, left_hashes) = traversal_counts(levels, height - 1, position * 2);
    if position * 2 + 1 < levels[height - 1].len() {
        let (right_bits, right_hashes) = traversal_counts(levels, height - 1, position * 2 + 1);
        (1 + left_bits + right_bits, left_hashes + right_hashes)
    } else {
        (1 + left_bits, left_hashes)
    }
}
fn build_traversal(
    levels: &[Vec<TreeNode>],
    height: usize,
    position: usize,
    bit: &mut usize,
    tree: &mut PartialMerkleTree,
) {
    let node = levels[height][position];
    if node.matched {
        tree.flags[*bit / 8] |= 1 << (*bit % 8);
    }
    *bit += 1;
    if height == 0 || !node.matched {
        tree.hashes.push(node.hash);
    } else {
        build_traversal(levels, height - 1, position * 2, bit, tree);
        if position * 2 + 1 < levels[height - 1].len() {
            build_traversal(levels, height - 1, position * 2 + 1, bit, tree);
        }
    }
}

impl PartialMerkleTree {
    /// Builds canonical zero padding. Full input is available, so construction
    /// rejects any known full-tree mutation even if the chosen mask hides it.
    /// Extraction can only detect equality in branches exposed by the proof.
    pub fn build(txids: &[TxId], matches: &[bool], limits: &Limits) -> Result<Self, Error> {
        let count = leaf_count(txids.len(), limits)?;
        partial_count(count, limits)?;
        if matches.len() != txids.len() {
            return Err(Error::MatchMaskLength {
                expected: txids.len(),
                actual: matches.len(),
            });
        }
        let height = tree_height(count);
        let work = add(
            mul(node_count(txids.len())?, size_of::<TreeNode>())?,
            mul(height + 1, size_of::<Vec<TreeNode>>())?,
        )?;
        limit(
            Resource::MerkleWorkBytes,
            work as u64,
            limits.max_merkle_work_bytes,
        )?;
        let mut levels = reserve(height + 1)?;
        let mut leaves = reserve(txids.len())?;
        leaves.extend(txids.iter().zip(matches).map(|(txid, matched)| TreeNode {
            hash: txid.0,
            matched: *matched,
        }));
        levels.push(leaves);
        while levels.last().is_some_and(|level| level.len() > 1) {
            let previous = &levels[levels.len() - 1];
            let mut next = reserve(previous.len().div_ceil(2))?;
            for pair in previous.chunks(2) {
                let left = pair[0];
                let right = pair.get(1).copied().unwrap_or(left);
                if pair.len() == 2 && left.hash == right.hash {
                    return Err(Error::MutatedTree);
                }
                next.push(TreeNode {
                    hash: hash_pair(&left.hash, &right.hash)?,
                    matched: left.matched || right.matched,
                });
            }
            levels.push(next);
        }
        let (bits, hashes) = traversal_counts(&levels, height, 0);
        let flags = bits.div_ceil(8);
        let decoded = add(size_of::<Self>(), add(mul(hashes, 32)?, flags)?)?;
        limit(
            Resource::DecodedBytes,
            decoded as u64,
            limits.max_decoded_bytes,
        )?;
        limit(
            Resource::MerkleWorkBytes,
            add(work, add(mul(hashes, 32)?, flags)?)? as u64,
            limits.max_merkle_work_bytes,
        )?;
        let length = partial_length(hashes, flags)?;
        limit(
            Resource::PartialBytes,
            length as u64,
            limits.max_partial_bytes,
        )?;
        let mut tree = Self {
            transaction_count: count,
            hashes: reserve(hashes)?,
            flags: reserve(flags)?,
        };
        tree.flags.resize(flags, 0);
        build_traversal(&levels, height, 0, &mut 0, &mut tree);
        Ok(tree)
    }
    fn validate_shape(&self, limits: &Limits) -> Result<(), Error> {
        partial_count(self.transaction_count, limits)?;
        if self.hashes.is_empty() || self.hashes.len() > self.transaction_count as usize {
            return Err(Error::InvalidPartial(PartialError::HashCount));
        }
        let maximum_flags = node_count(self.transaction_count as usize)?.div_ceil(8);
        if self.flags.is_empty()
            || self.flags.len() > maximum_flags
            || mul(self.flags.len(), 8)? < self.hashes.len()
        {
            return Err(Error::InvalidPartial(PartialError::FlagCount));
        }
        let length = partial_length(self.hashes.len(), self.flags.len())?;
        limit(
            Resource::PartialBytes,
            length as u64,
            limits.max_partial_bytes,
        )?;
        let stored = add(
            size_of::<Self>(),
            add(mul(self.hashes.len(), 32)?, self.flags.len())?,
        )?;
        // Extraction reserves at most one output match for each supplied hash.
        let output = mul(self.hashes.len(), size_of::<MerkleMatch>())?;
        limit(
            Resource::DecodedBytes,
            add(stored, output)? as u64,
            limits.max_decoded_bytes,
        )?;
        limit(
            Resource::MerkleWorkBytes,
            output as u64,
            limits.max_merkle_work_bytes,
        )?;
        Ok(())
    }
    /// Core-compatible extraction: consumes all hashes and all flag bytes, but
    /// ignores unused bits in the final consumed byte. Those bits are retained
    /// for byte-exact round trips. No hidden-subtree mutation claim is made.
    pub fn extract(&self, limits: &Limits) -> Result<ExtractedMatches, Error> {
        self.validate_shape(limits)?;
        let mut cursor = PartialCursor {
            tree: self,
            bit: 0,
            hash: 0,
            matches: reserve(self.hashes.len())?,
        };
        let merkle_root = cursor.visit(tree_height(self.transaction_count), 0)?;
        if cursor.hash != self.hashes.len() {
            return Err(Error::InvalidPartial(PartialError::UnusedHashes));
        }
        if cursor.bit.div_ceil(8) != self.flags.len() {
            return Err(Error::InvalidPartial(PartialError::UnusedFlagBytes));
        }
        Ok(ExtractedMatches {
            merkle_root,
            matches: cursor.matches,
            bits_used: cursor.bit,
        })
    }
    /// Optional stronger format policy for callers that require zero padding.
    pub fn extract_canonical(&self, limits: &Limits) -> Result<ExtractedMatches, Error> {
        let extracted = self.extract(limits)?;
        let remainder = extracted.bits_used % 8;
        if remainder != 0 && self.flags[self.flags.len() - 1] >> remainder != 0 {
            return Err(Error::InvalidPartial(PartialError::NonZeroPadding));
        }
        Ok(extracted)
    }
    pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, Error> {
        limit(
            Resource::PartialBytes,
            bytes.len() as u64,
            limits.max_partial_bytes,
        )?;
        let (tree, offset) = Self::decode_prefix(bytes, limits)?;
        let reader = Reader {
            bytes,
            offset,
            maximum: limits.max_partial_bytes,
            resource: Resource::PartialBytes,
        };
        reader.finish()?;
        Ok(tree)
    }
    pub fn decode_prefix(bytes: &[u8], limits: &Limits) -> Result<(Self, usize), Error> {
        let mut reader = Reader {
            bytes,
            offset: 0,
            maximum: limits.max_partial_bytes,
            resource: Resource::PartialBytes,
        };
        let transaction_count = u32::from_le_bytes(reader.array()?);
        partial_count(transaction_count, limits)?;
        let count = reader.compact()?;
        if count == 0 || count > u64::from(transaction_count) {
            return Err(Error::InvalidPartial(PartialError::HashCount));
        }
        let count = count as usize;
        let hash_bytes = mul(count, 32)?;
        limit(
            Resource::DecodedBytes,
            add(size_of::<Self>(), hash_bytes)? as u64,
            limits.max_decoded_bytes,
        )?;
        let raw_hashes = reader.take(hash_bytes)?;
        let flag_count = reader.compact()?;
        let maximum_flags = node_count(transaction_count as usize)?.div_ceil(8);
        if flag_count == 0 || flag_count > maximum_flags as u64 {
            return Err(Error::InvalidPartial(PartialError::FlagCount));
        }
        let flag_count = flag_count as usize;
        limit(
            Resource::DecodedBytes,
            add(size_of::<Self>(), add(hash_bytes, flag_count)?)? as u64,
            limits.max_decoded_bytes,
        )?;
        let raw_flags = reader.take(flag_count)?;
        let mut hashes = reserve(count)?;
        hashes.extend_from_slice(raw_hashes.as_chunks::<32>().0);
        let mut flags = reserve(flag_count)?;
        flags.extend_from_slice(raw_flags);
        let tree = Self {
            transaction_count,
            hashes,
            flags,
        };
        tree.extract(limits)?;
        Ok((tree, reader.offset))
    }
    pub fn encoded_len(&self, limits: &Limits) -> Result<usize, Error> {
        self.extract(limits)?;
        partial_length(self.hashes.len(), self.flags.len())
    }
    pub fn serialize(&self, limits: &Limits) -> Result<Vec<u8>, Error> {
        let mut bytes = reserve(self.encoded_len(limits)?)?;
        self.write_fields(&mut bytes)?;
        Ok(bytes)
    }
    fn write_fields(&self, bytes: &mut Vec<u8>) -> Result<(), Error> {
        bytes.extend_from_slice(&self.transaction_count.to_le_bytes());
        bitcoin_wire::write_compact_size(self.hashes.len() as u64, bytes)
            .map_err(|e| wire_error(bytes.len(), None, e))?;
        for hash in &self.hashes {
            bytes.extend_from_slice(hash);
        }
        bitcoin_wire::write_compact_size(self.flags.len() as u64, bytes)
            .map_err(|e| wire_error(bytes.len(), None, e))?;
        bytes.extend_from_slice(&self.flags);
        Ok(())
    }
}
fn partial_length(hashes: usize, flags: usize) -> Result<usize, Error> {
    add(
        add(
            add(4, bitcoin_wire::compact_size_len(hashes as u64))?,
            mul(hashes, 32)?,
        )?,
        add(bitcoin_wire::compact_size_len(flags as u64), flags)?,
    )
}
struct PartialCursor<'a> {
    tree: &'a PartialMerkleTree,
    bit: usize,
    hash: usize,
    matches: Vec<MerkleMatch>,
}
impl PartialCursor<'_> {
    fn visit(&mut self, height: usize, position: usize) -> Result<[u8; 32], Error> {
        let byte = self
            .tree
            .flags
            .get(self.bit / 8)
            .ok_or(Error::InvalidPartial(PartialError::ExhaustedBits))?;
        let matched = byte & (1 << (self.bit % 8)) != 0;
        self.bit += 1;
        if height == 0 || !matched {
            let hash = self
                .tree
                .hashes
                .get(self.hash)
                .copied()
                .ok_or(Error::InvalidPartial(PartialError::ExhaustedHashes))?;
            self.hash += 1;
            if height == 0 && matched {
                self.matches.push(MerkleMatch {
                    index: position as u32,
                    txid: TxId(hash),
                });
            }
            Ok(hash)
        } else {
            let left = self.visit(height - 1, position * 2)?;
            let right = if position * 2 + 1 < tree_width(self.tree.transaction_count, height - 1) {
                let right = self.visit(height - 1, position * 2 + 1)?;
                if right == left {
                    return Err(Error::InvalidPartial(PartialError::IdenticalBranches));
                }
                right
            } else {
                left
            };
            hash_pair(&left, &right)
        }
    }
}

/// The Bitcoin `merkleblock` payload (no P2P message envelope or Bloom filter).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerkleBlock {
    pub header: BlockHeader,
    pub tree: PartialMerkleTree,
}
impl MerkleBlock {
    fn tree_limits(limits: &Limits) -> Result<Limits, Error> {
        let header_memory = size_of::<Self>() - size_of::<PartialMerkleTree>();
        limit(
            Resource::PartialBytes,
            HEADER_BYTES as u64,
            limits.max_partial_bytes,
        )?;
        limit(
            Resource::DecodedBytes,
            header_memory as u64,
            limits.max_decoded_bytes,
        )?;
        Ok(Limits {
            max_partial_bytes: limits.max_partial_bytes - HEADER_BYTES,
            max_decoded_bytes: limits.max_decoded_bytes - header_memory,
            ..*limits
        })
    }
    pub fn build(block: &Block, matches: &[bool], limits: &Limits) -> Result<Self, Error> {
        let txids = block.txids(limits)?;
        let inner = Self::tree_limits(&after_txids(limits, txids.len())?)?;
        let tree = PartialMerkleTree::build(&txids, matches, &inner)?;
        if tree.extract(&inner)?.merkle_root != block.header.merkle_root {
            return Err(Error::RootMismatch);
        }
        Ok(Self {
            header: block.header,
            tree,
        })
    }
    /// A valid payload commits the extracted txids to the supplied header's
    /// root; authenticity and network/chain validity of that header remain external.
    pub fn extract(&self, limits: &Limits) -> Result<ExtractedMatches, Error> {
        let extracted = self.tree.extract(&Self::tree_limits(limits)?)?;
        if extracted.merkle_root != self.header.merkle_root {
            return Err(Error::RootMismatch);
        }
        Ok(extracted)
    }
    pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, Error> {
        limit(
            Resource::PartialBytes,
            bytes.len() as u64,
            limits.max_partial_bytes,
        )?;
        let (block, offset) = Self::decode_prefix(bytes, limits)?;
        Reader {
            bytes,
            offset,
            maximum: limits.max_partial_bytes,
            resource: Resource::PartialBytes,
        }
        .finish()?;
        Ok(block)
    }
    pub fn decode_prefix(bytes: &[u8], limits: &Limits) -> Result<(Self, usize), Error> {
        let inner = Self::tree_limits(limits)?;
        let (header, consumed) = BlockHeader::decode_prefix(bytes)?;
        let (tree, count) = PartialMerkleTree::decode_prefix(&bytes[consumed..], &inner)?;
        let block = Self { header, tree };
        block.extract(limits)?;
        Ok((block, add(consumed, count)?))
    }
    pub fn serialize(&self, limits: &Limits) -> Result<Vec<u8>, Error> {
        self.extract(limits)?;
        let length = add(
            HEADER_BYTES,
            partial_length(self.tree.hashes.len(), self.tree.flags.len())?,
        )?;
        limit(
            Resource::PartialBytes,
            length as u64,
            limits.max_partial_bytes,
        )?;
        let mut bytes = reserve(length)?;
        bytes.extend_from_slice(&self.header.encode());
        self.tree.write_fields(&mut bytes)?;
        Ok(bytes)
    }
}

/// Bitcoin Core SetCompact interpretation. Magnitude is big-endian and, on
/// overflow, truncated modulo 2^256 like Core; use checked_positive before use.
/// Sign and overflow follow the mantissa after small-exponent truncation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactTarget {
    pub magnitude_be: [u8; 32],
    pub negative: bool,
    pub overflow: bool,
    pub canonical: bool,
}
impl CompactTarget {
    pub fn from_bits(bits: u32) -> Self {
        let exponent = (bits >> 24) as usize;
        let mut mantissa = bits & 0x007f_ffff;
        let mut magnitude_be = [0; 32];
        let offset = if exponent <= 3 {
            mantissa >>= 8 * (3 - exponent);
            0
        } else {
            exponent - 3
        };
        for (index, byte) in mantissa.to_le_bytes()[..3].iter().enumerate() {
            if offset + index < 32 {
                magnitude_be[31 - offset - index] = *byte;
            }
        }
        let negative = mantissa != 0 && bits & 0x0080_0000 != 0;
        let overflow = mantissa != 0
            && (exponent > 34
                || (mantissa > 0xff && exponent > 33)
                || (mantissa > 0xffff && exponent > 32));
        let canonical = !overflow && encode_compact_target(&magnitude_be, negative) == bits;
        Self {
            magnitude_be,
            negative,
            overflow,
            canonical,
        }
    }
    pub fn checked_positive(&self, require_canonical: bool) -> Result<[u8; 32], Error> {
        if self.overflow {
            return Err(Error::TargetOverflow);
        }
        if self.negative {
            return Err(Error::NegativeTarget);
        }
        if self.magnitude_be == [0; 32] {
            return Err(Error::ZeroTarget);
        }
        if require_canonical && !self.canonical {
            return Err(Error::NonCanonicalTarget);
        }
        Ok(self.magnitude_be)
    }
}
/// Core GetCompact: the top three significant bytes, truncated toward zero,
/// with sign-bit normalization. Arbitrary 256-bit inputs may lose low bits.
pub fn encode_compact_target(magnitude_be: &[u8; 32], negative: bool) -> u32 {
    let Some(first) = magnitude_be.iter().position(|byte| *byte != 0) else {
        return 0;
    };
    let mut exponent = 32 - first;
    let mut mantissa = 0u32;
    for index in 0..3 {
        mantissa =
            (mantissa << 8) | u32::from(magnitude_be.get(first + index).copied().unwrap_or(0));
    }
    if mantissa & 0x0080_0000 != 0 {
        mantissa >>= 8;
        exponent += 1;
    }
    mantissa
        | ((exponent as u32) << 24)
        | if negative && mantissa != 0 {
            0x0080_0000
        } else {
            0
        }
}
