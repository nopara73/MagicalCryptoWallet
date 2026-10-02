//! Legacy, BIP143 and BIP341/342 signature hash serialization.
//! Digests are raw SHA output bytes. Bitcoin txid display order never applies.
#![forbid(unsafe_code)]

use crate::bitcoin_encoding::{self, Sha256};
use crate::bitcoin_script::{self, InstructionKind, opcodes};
use crate::bitcoin_wire::{self, Limits, OutPoint, Transaction, TxOut};
use std::fmt;

pub const MAX_SIGHASH_BYTES: usize = 8_000_000;
pub const SIGHASH_DEFAULT: u8 = 0;
pub const SIGHASH_ALL: u8 = 1;
pub const SIGHASH_NONE: u8 = 2;
pub const SIGHASH_SINGLE: u8 = 3;
pub const SIGHASH_ANYONECANPAY: u8 = 0x80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Wire(bitcoin_wire::Error),
    Script(bitcoin_script::Error),
    Encoding(bitcoin_encoding::Error),
    InputIndex { index: usize, inputs: usize },
    SpentOutputCount { outputs: usize, inputs: usize },
    MissingSpentOutputs,
    SizeLimit { length: usize, maximum: usize },
    InvalidTaprootHashType { hash_type: u8 },
    MissingSingleOutput { index: usize, outputs: usize },
    InvalidAnnex,
    InvalidKeyVersion { version: u8 },
    InvalidLeafVersion { version: u8 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wire(e) => write!(f, "transaction wire: {e}"),
            Self::Script(e) => write!(f, "script: {e}"),
            Self::Encoding(e) => write!(f, "encoding: {e}"),
            Self::InputIndex { index, inputs } => {
                write!(f, "input {index} is outside {inputs} inputs")
            }
            Self::SpentOutputCount { outputs, inputs } => {
                write!(f, "{outputs} spent outputs for {inputs} inputs")
            }
            Self::MissingSpentOutputs => f.write_str("Taproot hashing requires all spent outputs"),
            Self::SizeLimit { length, maximum } => {
                write!(f, "signature hashing size {length} exceeds {maximum}")
            }
            Self::InvalidTaprootHashType { hash_type } => {
                write!(f, "invalid Taproot hash type {hash_type}")
            }
            Self::MissingSingleOutput { index, outputs } => write!(
                f,
                "SINGLE input {index} has no matching output among {outputs}"
            ),
            Self::InvalidAnnex => f.write_str("annex must start with 0x50"),
            Self::InvalidKeyVersion { version } => {
                write!(f, "unsupported tapscript key version {version}")
            }
            Self::InvalidLeafVersion { version } => {
                write!(f, "TapLeaf version {version} has its parity bit set")
            }
        }
    }
}
impl std::error::Error for Error {}
impl From<bitcoin_wire::Error> for Error {
    fn from(e: bitcoin_wire::Error) -> Self {
        Self::Wire(e)
    }
}
impl From<bitcoin_script::Error> for Error {
    fn from(e: bitcoin_script::Error) -> Self {
        Self::Script(e)
    }
}
impl From<bitcoin_encoding::Error> for Error {
    fn from(e: bitcoin_encoding::Error) -> Self {
        Self::Encoding(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapScriptExtension {
    pub tapleaf_hash: [u8; 32],
    pub key_version: u8,
    /// Opcode position of last executed separator, or u32::MAX when none executed.
    pub codesep_position: u32,
}

struct Sink {
    hash: Sha256,
    length: usize,
    maximum: usize,
}
impl Sink {
    fn new(maximum: usize) -> Self {
        Self {
            hash: Sha256::new(),
            length: 0,
            maximum: maximum.min(MAX_SIGHASH_BYTES),
        }
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let length = self.length.saturating_add(bytes.len());
        if length > self.maximum {
            return Err(Error::SizeLimit {
                length,
                maximum: self.maximum,
            });
        }
        self.hash.update(bytes)?;
        self.length = length;
        Ok(())
    }
    fn compact_size(&mut self, size: usize) -> Result<(), Error> {
        let mut bytes = Vec::with_capacity(9);
        bitcoin_wire::write_compact_size(size as u64, &mut bytes)?;
        self.write(&bytes)
    }
    fn script(&mut self, script: &[u8]) -> Result<(), Error> {
        self.compact_size(script.len())?;
        self.write(script)
    }
    fn outpoint(&mut self, outpoint: &OutPoint) -> Result<(), Error> {
        self.write(&outpoint.txid)?;
        self.write(&outpoint.vout.to_le_bytes())
    }
    fn output(&mut self, output: &TxOut) -> Result<(), Error> {
        self.write(&output.value.to_le_bytes())?;
        self.script(&output.script_pubkey)
    }
    fn single(self) -> [u8; 32] {
        self.hash.finalize()
    }
    fn double(self) -> Result<[u8; 32], Error> {
        Ok(bitcoin_encoding::sha256(&self.single())?)
    }
}

fn check_index(tx: &Transaction, index: usize) -> Result<(), Error> {
    if index >= tx.inputs.len() {
        Err(Error::InputIndex {
            index,
            inputs: tx.inputs.len(),
        })
    } else {
        Ok(())
    }
}

fn check_transaction(tx: &Transaction, limits: &Limits) -> Result<(), Error> {
    let size = tx.sizes(limits)?.total;
    if size > MAX_SIGHASH_BYTES {
        return Err(Error::SizeLimit {
            length: size,
            maximum: MAX_SIGHASH_BYTES,
        });
    }
    Ok(())
}

/// Removes only parsed OP_CODESEPARATOR instructions, never 0xab inside pushes.
/// Malformed scriptCode is rejected explicitly before any hash is returned.
pub fn without_code_separators(script_code: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(script_code.len().min(bitcoin_script::MAX_SCRIPT_BYTES));
    for item in bitcoin_script::instructions(script_code)? {
        let item = item?;
        if item.opcode() != opcodes::OP_CODESEPARATOR {
            out.extend_from_slice(item.raw_bytes());
        }
    }
    Ok(out)
}

/// Core legacy signature hash including the historical SINGLE missing-output
/// digest 01 followed by zeros. Invalid input indices are explicit API errors.
/// hash_type retains all 32 bits; defined-type policy belongs to validation.
pub fn legacy_sighash(
    tx: &Transaction,
    index: usize,
    script_code: &[u8],
    hash_type: u32,
    limits: &Limits,
) -> Result<[u8; 32], Error> {
    check_transaction(tx, limits)?;
    check_index(tx, index)?;
    let single = hash_type & 0x1f == u32::from(SIGHASH_SINGLE);
    let none = hash_type & 0x1f == u32::from(SIGHASH_NONE);
    let anyone = hash_type & u32::from(SIGHASH_ANYONECANPAY) != 0;
    if single && index >= tx.outputs.len() {
        let mut bug = [0; 32];
        bug[0] = 1;
        return Ok(bug);
    }
    let code = without_code_separators(script_code)?;
    let maximum = limits
        .max_transaction_bytes
        .saturating_add(bitcoin_script::MAX_SCRIPT_BYTES)
        .saturating_add(16)
        .min(MAX_SIGHASH_BYTES);
    let mut sink = Sink::new(maximum);
    sink.write(&tx.version.to_le_bytes())?;
    sink.compact_size(if anyone { 1 } else { tx.inputs.len() })?;
    for (position, input) in tx
        .inputs
        .iter()
        .enumerate()
        .filter(|(position, _)| !anyone || *position == index)
    {
        sink.outpoint(&input.previous_output)?;
        sink.script(if position == index { &code } else { &[] })?;
        sink.write(
            &if position != index && (single || none) {
                0_u32
            } else {
                input.sequence
            }
            .to_le_bytes(),
        )?;
    }
    sink.compact_size(if none {
        0
    } else if single {
        index + 1
    } else {
        tx.outputs.len()
    })?;
    if !none {
        for (position, output) in
            tx.outputs
                .iter()
                .enumerate()
                .take(if single { index + 1 } else { tx.outputs.len() })
        {
            if single && position < index {
                sink.write(&(-1_i64).to_le_bytes())?;
                sink.script(&[])?;
            } else {
                sink.output(output)?;
            }
        }
    }
    sink.write(&tx.lock_time.to_le_bytes())?;
    sink.write(&hash_type.to_le_bytes())?;
    sink.double()
}

/// Immutable borrowed cache binds all precomputed hashes to exactly one tx and
/// prevout order. Rust borrowing prevents transaction mutation while it is used.
pub struct SighashCache<'a> {
    tx: &'a Transaction,
    spent_outputs: Option<&'a [TxOut]>,
    limits: Limits,
    prevouts: [u8; 32],
    sequences: [u8; 32],
    outputs: [u8; 32],
    amounts: Option<[u8; 32]>,
    scripts: Option<[u8; 32]>,
}

impl<'a> SighashCache<'a> {
    pub fn new(tx: &'a Transaction, limits: &Limits) -> Result<Self, Error> {
        check_transaction(tx, limits)?;
        let mut prevouts = Sink::new(limits.max_transaction_bytes);
        let mut sequences = Sink::new(limits.max_transaction_bytes);
        let mut outputs = Sink::new(limits.max_transaction_bytes);
        for input in &tx.inputs {
            prevouts.outpoint(&input.previous_output)?;
            sequences.write(&input.sequence.to_le_bytes())?;
        }
        for output in &tx.outputs {
            outputs.output(output)?;
        }
        Ok(Self {
            tx,
            spent_outputs: None,
            limits: *limits,
            prevouts: prevouts.single(),
            sequences: sequences.single(),
            outputs: outputs.single(),
            amounts: None,
            scripts: None,
        })
    }

    pub fn with_spent_outputs(
        tx: &'a Transaction,
        spent_outputs: &'a [TxOut],
        limits: &Limits,
    ) -> Result<Self, Error> {
        if spent_outputs.len() != tx.inputs.len() {
            return Err(Error::SpentOutputCount {
                outputs: spent_outputs.len(),
                inputs: tx.inputs.len(),
            });
        }
        let mut cache = Self::new(tx, limits)?;
        let mut amounts = Sink::new(limits.max_transaction_bytes);
        let mut scripts = Sink::new(limits.max_payload_bytes);
        for output in spent_outputs {
            if output.script_pubkey.len() > limits.max_script_bytes {
                return Err(Error::SizeLimit {
                    length: output.script_pubkey.len(),
                    maximum: limits.max_script_bytes,
                });
            }
            amounts.write(&output.value.to_le_bytes())?;
            scripts.script(&output.script_pubkey)?;
        }
        cache.spent_outputs = Some(spent_outputs);
        cache.amounts = Some(amounts.single());
        cache.scripts = Some(scripts.single());
        Ok(cache)
    }

    /// scriptCode is already sliced after the last executed separator by the
    /// interpreter; subsequent separators and all data pushes remain unchanged.
    pub fn segwit_v0(
        &self,
        index: usize,
        script_code: &[u8],
        amount: i64,
        hash_type: u32,
    ) -> Result<[u8; 32], Error> {
        check_index(self.tx, index)?;
        bitcoin_script::instructions(script_code)?;
        let base = hash_type & 0x1f;
        let anyone = hash_type & u32::from(SIGHASH_ANYONECANPAY) != 0;
        let zero = [0; 32];
        let prevouts = if anyone {
            zero
        } else {
            bitcoin_encoding::sha256(&self.prevouts)?
        };
        let sequences = if anyone || matches!(base, 2 | 3) {
            zero
        } else {
            bitcoin_encoding::sha256(&self.sequences)?
        };
        let outputs = if base == 3 {
            if let Some(output) = self.tx.outputs.get(index) {
                let mut sink = Sink::new(self.limits.max_transaction_bytes);
                sink.output(output)?;
                sink.double()?
            } else {
                zero
            }
        } else if base == 2 {
            zero
        } else {
            bitcoin_encoding::sha256(&self.outputs)?
        };
        let input = &self.tx.inputs[index];
        let mut sink = Sink::new(MAX_SIGHASH_BYTES);
        sink.write(&self.tx.version.to_le_bytes())?;
        sink.write(&prevouts)?;
        sink.write(&sequences)?;
        sink.outpoint(&input.previous_output)?;
        sink.script(script_code)?;
        sink.write(&amount.to_le_bytes())?;
        sink.write(&input.sequence.to_le_bytes())?;
        sink.write(&outputs)?;
        sink.write(&self.tx.lock_time.to_le_bytes())?;
        sink.write(&hash_type.to_le_bytes())?;
        sink.double()
    }

    /// BIP341 message including epoch 0, excluding the tagged-hash prefix.
    pub fn taproot_message(
        &self,
        index: usize,
        hash_type: u8,
        annex: Option<&[u8]>,
        extension: Option<TapScriptExtension>,
    ) -> Result<Vec<u8>, Error> {
        check_index(self.tx, index)?;
        if !matches!(hash_type, 0..=3 | 0x81..=0x83) {
            return Err(Error::InvalidTaprootHashType { hash_type });
        }
        let spent = self.spent_outputs.ok_or(Error::MissingSpentOutputs)?;
        if let Some(extension) = extension
            && extension.key_version != 0
        {
            return Err(Error::InvalidKeyVersion {
                version: extension.key_version,
            });
        }
        if let Some(annex) = annex
            && (annex.first() != Some(&0x50) || annex.len() > MAX_SIGHASH_BYTES)
        {
            return Err(Error::InvalidAnnex);
        }
        let anyone = hash_type & SIGHASH_ANYONECANPAY != 0;
        let base = if hash_type == 0 { 1 } else { hash_type & 3 };
        if base == 3 && index >= self.tx.outputs.len() {
            return Err(Error::MissingSingleOutput {
                index,
                outputs: self.tx.outputs.len(),
            });
        }
        let mut message = Vec::with_capacity(256);
        message.extend_from_slice(&[0, hash_type]);
        message.extend_from_slice(&self.tx.version.to_le_bytes());
        message.extend_from_slice(&self.tx.lock_time.to_le_bytes());
        if !anyone {
            message.extend_from_slice(&self.prevouts);
            message.extend_from_slice(&self.amounts.ok_or(Error::MissingSpentOutputs)?);
            message.extend_from_slice(&self.scripts.ok_or(Error::MissingSpentOutputs)?);
            message.extend_from_slice(&self.sequences);
        }
        if base == 1 {
            message.extend_from_slice(&self.outputs);
        }
        message.push(2 * u8::from(extension.is_some()) + u8::from(annex.is_some()));
        if anyone {
            let input = &self.tx.inputs[index];
            message.extend_from_slice(&input.previous_output.txid);
            message.extend_from_slice(&input.previous_output.vout.to_le_bytes());
            // The message itself can include a caller-supplied prevout script.
            // Bound before allocating/copying; hash_type does not bypass limits.
            let output = &spent[index];
            let length = message
                .len()
                .saturating_add(8 + 9 + 4 + 69)
                .saturating_add(output.script_pubkey.len());
            if length > MAX_SIGHASH_BYTES {
                return Err(Error::SizeLimit {
                    length,
                    maximum: MAX_SIGHASH_BYTES,
                });
            }
            message.extend_from_slice(&output.value.to_le_bytes());
            bitcoin_wire::write_compact_size(output.script_pubkey.len() as u64, &mut message)?;
            message.extend_from_slice(&output.script_pubkey);
            message.extend_from_slice(&input.sequence.to_le_bytes());
        } else {
            message.extend_from_slice(&(index as u32).to_le_bytes());
        }
        if let Some(annex) = annex {
            let mut sink = Sink::new(MAX_SIGHASH_BYTES);
            sink.script(annex)?;
            message.extend_from_slice(&sink.single());
        }
        if base == 3 {
            let mut sink = Sink::new(self.limits.max_transaction_bytes);
            sink.output(&self.tx.outputs[index])?;
            message.extend_from_slice(&sink.single());
        }
        if let Some(extension) = extension {
            message.extend_from_slice(&extension.tapleaf_hash);
            message.push(extension.key_version);
            message.extend_from_slice(&extension.codesep_position.to_le_bytes());
        }
        Ok(message)
    }

    pub fn taproot(
        &self,
        index: usize,
        hash_type: u8,
        annex: Option<&[u8]>,
        extension: Option<TapScriptExtension>,
    ) -> Result<[u8; 32], Error> {
        tagged_hash(
            b"TapSighash",
            &self.taproot_message(index, hash_type, annex, extension)?,
        )
    }
}

pub fn segwit_v0_sighash(
    tx: &Transaction,
    index: usize,
    script_code: &[u8],
    amount: i64,
    hash_type: u32,
    limits: &Limits,
) -> Result<[u8; 32], Error> {
    SighashCache::new(tx, limits)?.segwit_v0(index, script_code, amount, hash_type)
}

pub fn taproot_sighash_message(
    tx: &Transaction,
    spent_outputs: &[TxOut],
    index: usize,
    hash_type: u8,
    annex: Option<&[u8]>,
    extension: Option<TapScriptExtension>,
    limits: &Limits,
) -> Result<Vec<u8>, Error> {
    SighashCache::with_spent_outputs(tx, spent_outputs, limits)?
        .taproot_message(index, hash_type, annex, extension)
}

pub fn taproot_sighash(
    tx: &Transaction,
    spent_outputs: &[TxOut],
    index: usize,
    hash_type: u8,
    annex: Option<&[u8]>,
    extension: Option<TapScriptExtension>,
    limits: &Limits,
) -> Result<[u8; 32], Error> {
    SighashCache::with_spent_outputs(tx, spent_outputs, limits)?
        .taproot(index, hash_type, annex, extension)
}

fn tagged_hash(tag: &[u8], message: &[u8]) -> Result<[u8; 32], Error> {
    let tag_hash = bitcoin_encoding::sha256(tag)?;
    let mut sink = Sink::new(MAX_SIGHASH_BYTES);
    sink.write(&tag_hash)?;
    sink.write(&tag_hash)?;
    sink.write(message)?;
    Ok(sink.single())
}

pub fn tapleaf_hash(script: &[u8], leaf_version: u8) -> Result<[u8; 32], Error> {
    if leaf_version & 1 != 0 {
        return Err(Error::InvalidLeafVersion {
            version: leaf_version,
        });
    }
    bitcoin_script::instructions(script)?;
    let mut message = Vec::with_capacity(script.len() + 10);
    message.push(leaf_version);
    bitcoin_wire::write_compact_size(script.len() as u64, &mut message)?;
    message.extend_from_slice(script);
    tagged_hash(b"TapLeaf", &message)
}

pub fn tapbranch_hash(left: &[u8; 32], right: &[u8; 32]) -> Result<[u8; 32], Error> {
    let (left, right) = if left < right {
        (left, right)
    } else {
        (right, left)
    };
    let mut message = [0; 64];
    message[..32].copy_from_slice(left);
    message[32..].copy_from_slice(right);
    tagged_hash(b"TapBranch", &message)
}

pub fn taptweak_hash(
    internal_key: &[u8; 32],
    merkle_root: Option<&[u8; 32]>,
) -> Result<[u8; 32], Error> {
    let mut message = Vec::with_capacity(64);
    message.extend_from_slice(internal_key);
    if let Some(root) = merkle_root {
        message.extend_from_slice(root);
    }
    tagged_hash(b"TapTweak", &message)
}

/// Legacy FindAndDelete: match serialized signature pushes only at instruction
/// boundaries. Does not remove signature bytes appearing inside other data pushes.
pub fn find_and_delete_signature(
    script_code: &[u8],
    signature: &[u8],
) -> Result<(Vec<u8>, usize), Error> {
    let mut pushed = bitcoin_script::ScriptBuilder::new();
    pushed.push_data_length_only(signature)?;
    let encoded = pushed.as_bytes();
    let mut result = Vec::with_capacity(script_code.len().min(bitcoin_script::MAX_SCRIPT_BYTES));
    let mut removed = 0;
    for item in bitcoin_script::instructions(script_code)? {
        let item = item?;
        if item.raw_bytes() == encoded && matches!(item.kind(), InstructionKind::Push { .. }) {
            removed += 1;
        } else {
            result.extend_from_slice(item.raw_bytes());
        }
    }
    Ok((result, removed))
}
