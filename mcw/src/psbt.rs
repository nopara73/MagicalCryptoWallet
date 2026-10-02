//! Lossless BIP174 (v0) and BIP370 (v2) containers inside the mcw application.
//!
//! This is a format layer, not a signer or consensus validator. Key/value bytes,
//! unknown extensions, proprietary records, and map order are preserved exactly.
//! Accepted CompactSize encodings are canonical, so serialization reproduces the
//! original binary bytes. Embedded transactions use mcw's first-party wire codec.
//! Public keys and signatures are checked for encoding shape, not cryptographic
//! validity; hash preimages, UTXO commitments, and script semantics are not checked.
#![forbid(unsafe_code)]

use crate::bitcoin_wire;
use std::collections::BTreeSet;
use std::fmt;

pub const MAGIC: &[u8; 5] = b"psbt\xff";
pub const LOCKTIME_THRESHOLD: u32 = 500_000_000;

/// Limits apply before allocating from untrusted lengths or counts.
/// The global map counts toward `max_maps`; all records count toward `max_records`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_maps: usize,
    pub max_records: usize,
    pub max_records_per_map: usize,
    pub max_key_bytes: usize,
    pub max_value_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 16 * 1024 * 1024,
            max_maps: 20_001,
            max_records: 100_000,
            max_records_per_map: 4_096,
            max_key_bytes: 16_384,
            max_value_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Input(usize),
    Output(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Bytes,
    Maps,
    Records,
    RecordsPerMap,
    KeyBytes,
    ValueBytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidMagic,
    UnexpectedEnd,
    NonCanonicalCompactSize,
    LengthOverflow,
    LimitExceeded(Limit),
    DuplicateKey,
    InvalidField { key_type: u64, reason: &'static str },
    MissingField(u64),
    ForbiddenField(u64),
    UnsupportedVersion(u32),
    MapCountMismatch,
    TrailingData,
    InvalidBase64,
    InvalidHex,
    InvalidTransaction,
    IncompatibleLocktimes,
    AllocationFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub scope: Scope,
    /// Absolute byte offset for container framing errors, when known.
    pub offset: Option<usize>,
}

impl Error {
    fn new(kind: ErrorKind, scope: Scope) -> Self {
        Self {
            kind,
            scope,
            offset: None,
        }
    }

    fn at(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PSBT {:?} in {:?}", self.kind, self.scope)?;
        if let Some(offset) = self.offset {
            write!(f, " at byte {offset}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// A canonical, immutable key and its opaque value. Duplicate identity is the
/// entire key, not just the type: multiple different public keys are permitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    key: Vec<u8>,
    value: Vec<u8>,
    key_type: u64,
    type_len: usize,
}

impl Record {
    /// Construct a canonical key. Scope-specific validation happens in `Psbt`.
    pub fn new(key_type: u64, key_data: &[u8], value: &[u8]) -> Result<Self> {
        let key_len = compact_size_len(key_type)
            .checked_add(key_data.len())
            .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, Scope::Global))?;
        let mut key = allocate(key_len, Scope::Global)?;
        write_compact_size(key_type, &mut key);
        let type_len = key.len();
        key.extend_from_slice(key_data);
        Ok(Self {
            key,
            value: copy_bytes(value, Scope::Global)?,
            key_type,
            type_len,
        })
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }
    pub fn key_type(&self) -> u64 {
        self.key_type
    }
    pub fn key_data(&self) -> &[u8] {
        &self.key[self.type_len..]
    }
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Interpret known BIP174/BIP370 field shapes without discarding raw bytes.
    /// Unrecognized extensions (including BIP371) remain `Field::Unknown`.
    pub fn field(&self, scope: Scope) -> Result<Field<'_>> {
        interpret(self, scope)
    }
}

/// Records remain in source order. Building a map rejects repeated full keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Map {
    records: Vec<Record>,
}

impl Map {
    pub fn new(records: Vec<Record>) -> Result<Self> {
        check_duplicates(&records, Scope::Global)?;
        Ok(Self { records })
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    pub fn get(&self, key_type: u64, key_data: &[u8]) -> Option<&Record> {
        self.records
            .iter()
            .find(|record| record.key_type == key_type && record.key_data() == key_data)
    }

    pub fn singleton(&self, key_type: u64) -> Option<&[u8]> {
        self.get(key_type, &[]).map(Record::value)
    }

    /// An immutable edit: replacement keeps the original position; a new key
    /// is appended. Rebuild through `Psbt::from_maps` to validate the new scope.
    pub fn with_record(&self, record: Record) -> Self {
        let mut next = self.clone();
        if let Some(existing) = next
            .records
            .iter_mut()
            .find(|entry| entry.key == record.key)
        {
            *existing = record;
        } else {
            next.records.push(record);
        }
        next
    }

    pub fn without_record(&self, key_type: u64, key_data: &[u8]) -> Self {
        Self {
            records: self
                .records
                .iter()
                .filter(|record| !(record.key_type == key_type && record.key_data() == key_data))
                .cloned()
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    V0,
    V2,
}

/// Borrowed BIP32 key origin: the fingerprint and each little-endian path index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyOrigin<'a> {
    pub fingerprint: &'a [u8; 4],
    path: &'a [u8],
}

impl KeyOrigin<'_> {
    pub fn path(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.path
            .as_chunks::<4>()
            .0
            .iter()
            .map(|index| u32::from_le_bytes(*index))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proprietary<'a> {
    pub identifier: &'a [u8],
    pub subtype: u64,
    pub key_data: &'a [u8],
    pub value: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreimageHash {
    Ripemd160,
    Sha256,
    Hash160,
    Hash256,
}

/// Shape-checked views. Amounts are signed wire values; format parsing does not
/// establish monetary, signature, hash, script, or transaction validity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field<'a> {
    UnsignedTransaction(&'a [u8]),
    ExtendedPublicKey {
        xpub: &'a [u8; 78],
        origin: KeyOrigin<'a>,
    },
    PsbtVersion(u32),
    TransactionVersion(i32),
    FallbackLocktime(u32),
    InputCount(u64),
    OutputCount(u64),
    TransactionModifiable(u8),
    NonWitnessUtxo(&'a [u8]),
    WitnessUtxo {
        amount: i64,
        script: &'a [u8],
    },
    PartialSignature {
        public_key: &'a [u8],
        signature: &'a [u8],
    },
    SighashType(u32),
    RedeemScript(&'a [u8]),
    WitnessScript(&'a [u8]),
    Bip32Derivation {
        public_key: &'a [u8],
        origin: KeyOrigin<'a>,
    },
    FinalScriptSig(&'a [u8]),
    FinalScriptWitness(&'a [u8]),
    Preimage {
        algorithm: PreimageHash,
        hash: &'a [u8],
        preimage: &'a [u8],
    },
    PreviousTxid(&'a [u8; 32]),
    OutputIndex(u32),
    Sequence(u32),
    RequiredTimeLocktime(u32),
    RequiredHeightLocktime(u32),
    OutputAmount(i64),
    OutputScript(&'a [u8]),
    Proprietary(Proprietary<'a>),
    Unknown {
        key_type: u64,
        key_data: &'a [u8],
        value: &'a [u8],
    },
}

/// A fully framed, immutable format container. There is no mutable access that
/// can invalidate a previously checked map; edits are revalidated via from_maps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Psbt {
    version: Version,
    global: Map,
    inputs: Vec<Map>,
    outputs: Vec<Map>,
    limits: Limits,
    serialized_len: usize,
    v0_locktime: Option<u32>,
}

impl Psbt {
    pub fn parse(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.max_bytes {
            return Err(Error::new(
                ErrorKind::LimitExceeded(Limit::Bytes),
                Scope::Global,
            ));
        }
        if !bytes.starts_with(MAGIC) {
            return Err(Error::new(ErrorKind::InvalidMagic, Scope::Global).at(0));
        }
        if limits.max_maps == 0 {
            return Err(Error::new(
                ErrorKind::LimitExceeded(Limit::Maps),
                Scope::Global,
            ));
        }
        let mut reader = Reader {
            bytes,
            pos: MAGIC.len(),
            scope: Scope::Global,
        };
        let mut records = 0;
        let global = read_map(&mut reader, limits, &mut records)?;
        let (version, input_count, output_count, v0_locktime) = global_info(&global, limits)?;
        check_map_count(input_count, output_count, limits)?;
        let remaining_maps = input_count
            .checked_add(output_count)
            .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, Scope::Global))?;
        // Even an empty map consumes a terminator. Check before reserving vectors.
        if remaining_maps > reader.remaining() {
            return Err(Error::new(ErrorKind::UnexpectedEnd, Scope::Global).at(reader.pos));
        }
        let mut inputs = reserve_items(input_count, Scope::Global)?;
        let mut outputs = reserve_items(output_count, Scope::Global)?;
        for index in 0..input_count {
            reader.scope = Scope::Input(index);
            let map = read_map(&mut reader, limits, &mut records)?;
            validate_map(&map, reader.scope, version, limits)?;
            inputs.push(map);
        }
        for index in 0..output_count {
            reader.scope = Scope::Output(index);
            let map = read_map(&mut reader, limits, &mut records)?;
            validate_map(&map, reader.scope, version, limits)?;
            outputs.push(map);
        }
        if reader.remaining() != 0 {
            return Err(Error::new(ErrorKind::TrailingData, reader.scope).at(reader.pos));
        }
        Ok(Self {
            version,
            global,
            inputs,
            outputs,
            limits,
            serialized_len: bytes.len(),
            v0_locktime,
        })
    }

    /// Construct or edit a PSBT while enforcing the same constraints as parse.
    pub fn from_maps(
        global: Map,
        inputs: Vec<Map>,
        outputs: Vec<Map>,
        limits: Limits,
    ) -> Result<Self> {
        check_map_count(inputs.len(), outputs.len(), limits)?;
        let mut records = 0;
        let mut serialized_len = MAGIC.len();
        for (scope, map) in std::iter::once((Scope::Global, &global))
            .chain(
                inputs
                    .iter()
                    .enumerate()
                    .map(|(i, map)| (Scope::Input(i), map)),
            )
            .chain(
                outputs
                    .iter()
                    .enumerate()
                    .map(|(i, map)| (Scope::Output(i), map)),
            )
        {
            check_duplicates(&map.records, scope)?;
            measure_map(map, scope, limits, &mut records, &mut serialized_len)?;
        }
        let (version, input_count, output_count, v0_locktime) = global_info(&global, limits)?;
        if inputs.len() != input_count || outputs.len() != output_count {
            return Err(Error::new(ErrorKind::MapCountMismatch, Scope::Global));
        }
        for (index, map) in inputs.iter().enumerate() {
            validate_map(map, Scope::Input(index), version, limits)?;
        }
        for (index, map) in outputs.iter().enumerate() {
            validate_map(map, Scope::Output(index), version, limits)?;
        }
        Ok(Self {
            version,
            global,
            inputs,
            outputs,
            limits,
            serialized_len,
            v0_locktime,
        })
    }

    /// RFC4648 canonical padded Base64. Whitespace is deliberately rejected.
    pub fn from_base64(text: &str, limits: Limits) -> Result<Self> {
        Self::parse(&decode_base64(text.as_bytes(), limits.max_bytes)?, limits)
    }

    /// Clipboard/file transport: trim ASCII whitespace, then accept canonical
    /// Base64 or hex beginning with PSBT magic. Internal whitespace is rejected.
    pub fn parse_text(text: &str, limits: Limits) -> Result<Self> {
        let text = text.trim_matches(|ch: char| ch.is_ascii_whitespace());
        if text
            .get(..10)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("70736274ff"))
        {
            Self::parse(&decode_hex(text.as_bytes(), limits.max_bytes)?, limits)
        } else {
            Self::from_base64(text, limits)
        }
    }

    pub fn version(&self) -> Version {
        self.version
    }
    pub fn global(&self) -> &Map {
        &self.global
    }
    pub fn inputs(&self) -> &[Map] {
        &self.inputs
    }
    pub fn outputs(&self) -> &[Map] {
        &self.outputs
    }
    pub fn limits(&self) -> Limits {
        self.limits
    }
    pub fn serialized_len(&self) -> usize {
        self.serialized_len
    }

    /// Canonical lengths plus unchanged map order and key/value bytes.
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut bytes = allocate(self.serialized_len, Scope::Global)?;
        bytes.extend_from_slice(MAGIC);
        for map in std::iter::once(&self.global)
            .chain(&self.inputs)
            .chain(&self.outputs)
        {
            for record in &map.records {
                write_compact_size(record.key.len() as u64, &mut bytes);
                bytes.extend_from_slice(&record.key);
                write_compact_size(record.value.len() as u64, &mut bytes);
                bytes.extend_from_slice(&record.value);
            }
            bytes.push(0);
        }
        debug_assert_eq!(bytes.len(), self.serialized_len);
        Ok(bytes)
    }

    pub fn to_base64(&self) -> Result<String> {
        encode_base64(&self.serialize()?)
    }

    /// BIP370 locktime determination. Conflicting height-only/time-only inputs
    /// remain valid containers but cannot be turned into an unsigned transaction.
    pub fn locktime(&self) -> Result<u32> {
        if let Some(locktime) = self.v0_locktime {
            return Ok(locktime);
        }
        let mut has_requirement = false;
        let mut can_height = true;
        let mut can_time = true;
        let mut max_height = 0;
        let mut max_time = 0;
        for map in &self.inputs {
            let height = map.singleton(0x12).map(read_u32);
            let time = map.singleton(0x11).map(read_u32);
            if height.is_some() || time.is_some() {
                has_requirement = true;
                can_height &= height.is_some();
                can_time &= time.is_some();
                max_height = max_height.max(height.unwrap_or(0));
                max_time = max_time.max(time.unwrap_or(0));
            }
        }
        if !has_requirement {
            Ok(self.global.singleton(0x03).map(read_u32).unwrap_or(0))
        } else if can_height {
            Ok(max_height)
        } else if can_time {
            Ok(max_time)
        } else {
            Err(Error::new(ErrorKind::IncompatibleLocktimes, Scope::Global))
        }
    }

    /// Legacy unsigned transaction with empty scriptSigs and no witness.
    /// This does not finalize or extract a signed transaction.
    pub fn unsigned_transaction(&self) -> Result<Vec<u8>> {
        self.transaction_bytes(false)
    }

    /// BIP370 unique-identification preimage: all v2 sequences are set to ZERO.
    /// For v0, the global unsigned transaction is already the identifying form.
    /// Hashing this preimage is the caller's responsibility.
    pub fn identifier_transaction(&self) -> Result<Vec<u8>> {
        self.transaction_bytes(true)
    }

    fn transaction_bytes(&self, zero_sequences: bool) -> Result<Vec<u8>> {
        if self.version == Version::V0 {
            return copy_bytes(
                self.global.singleton(0).expect("validated v0 field"),
                Scope::Global,
            );
        }
        let locktime = self.locktime()?;
        let mut len = 8
            + compact_size_len(self.inputs.len() as u64)
            + compact_size_len(self.outputs.len() as u64);
        len = len
            .checked_add(
                self.inputs
                    .len()
                    .checked_mul(41)
                    .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, Scope::Global))?,
            )
            .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, Scope::Global))?;
        for map in &self.outputs {
            let script = map.singleton(0x04).expect("validated v2 script");
            len = len
                .checked_add(8 + compact_size_len(script.len() as u64))
                .and_then(|sum| sum.checked_add(script.len()))
                .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, Scope::Global))?;
        }
        if len > self.limits.max_bytes {
            return Err(Error::new(
                ErrorKind::LimitExceeded(Limit::Bytes),
                Scope::Global,
            ));
        }
        let mut bytes = allocate(len, Scope::Global)?;
        bytes.extend_from_slice(self.global.singleton(0x02).expect("validated v2 version"));
        write_compact_size(self.inputs.len() as u64, &mut bytes);
        for map in &self.inputs {
            bytes.extend_from_slice(map.singleton(0x0e).expect("validated v2 prevout"));
            bytes.extend_from_slice(map.singleton(0x0f).expect("validated v2 index"));
            bytes.push(0); // empty scriptSig
            let sequence = if zero_sequences {
                0
            } else {
                map.singleton(0x10).map(read_u32).unwrap_or(u32::MAX)
            };
            bytes.extend_from_slice(&sequence.to_le_bytes());
        }
        write_compact_size(self.outputs.len() as u64, &mut bytes);
        for map in &self.outputs {
            bytes.extend_from_slice(map.singleton(0x03).expect("validated v2 amount"));
            let script = map.singleton(0x04).expect("validated v2 script");
            write_compact_size(script.len() as u64, &mut bytes);
            bytes.extend_from_slice(script);
        }
        bytes.extend_from_slice(&locktime.to_le_bytes());
        debug_assert_eq!(len, bytes.len());
        Ok(bytes)
    }
}

fn global_info(map: &Map, limits: Limits) -> Result<(Version, usize, usize, Option<u32>)> {
    // Validate the version record's shape before interpreting it, then reject
    // unknown versions. Unknown *field* types are preserved; versions fail closed.
    for record in &map.records {
        interpret(record, Scope::Global)?;
    }
    let raw_version = map.singleton(0xfb).map(read_u32).unwrap_or(0);
    let version = match raw_version {
        0 => Version::V0,
        2 => Version::V2,
        other => {
            return Err(Error::new(
                ErrorKind::UnsupportedVersion(other),
                Scope::Global,
            ));
        }
    };
    validate_map(map, Scope::Global, version, limits)?;
    if version == Version::V0 {
        let raw = require(map, 0, Scope::Global)?;
        let tx = bitcoin_wire::Transaction::decode_legacy(raw, &wire_limits(limits))
            .map_err(|_| Error::new(ErrorKind::InvalidTransaction, Scope::Global))?;
        if tx
            .inputs
            .iter()
            .any(|input| !input.script_sig.is_empty() || !input.witness.is_empty())
        {
            return Err(invalid(
                0,
                "unsigned transaction contains scriptSig or witness",
                Scope::Global,
            ));
        }
        Ok((
            version,
            tx.inputs.len(),
            tx.outputs.len(),
            Some(tx.lock_time),
        ))
    } else {
        Ok((
            version,
            singleton_count(map, 0x04)?,
            singleton_count(map, 0x05)?,
            None,
        ))
    }
}

fn singleton_count(map: &Map, key_type: u64) -> Result<usize> {
    let raw = require(map, key_type, Scope::Global)?;
    let mut reader = Reader {
        bytes: raw,
        pos: 0,
        scope: Scope::Global,
    };
    let count = reader.compact_size()?;
    if reader.remaining() != 0 {
        return Err(invalid(key_type, "count has trailing data", Scope::Global));
    }
    usize::try_from(count).map_err(|_| Error::new(ErrorKind::LengthOverflow, Scope::Global))
}

fn validate_map(map: &Map, scope: Scope, version: Version, limits: Limits) -> Result<()> {
    for record in &map.records {
        let ty = record.key_type;
        let forbidden = match (scope, version) {
            (Scope::Global, Version::V0) => matches!(ty, 0x02..=0x06),
            (Scope::Global, Version::V2) => ty == 0,
            (Scope::Input(_), Version::V0) => matches!(ty, 0x0e..=0x12),
            (Scope::Output(_), Version::V0) => matches!(ty, 0x03 | 0x04),
            _ => false,
        };
        if forbidden {
            return Err(Error::new(ErrorKind::ForbiddenField(ty), scope));
        }
        let field = interpret(record, scope)?;
        match field {
            Field::NonWitnessUtxo(bytes) => {
                bitcoin_wire::Transaction::decode(bytes, &wire_limits(limits))
                    .map_err(|_| invalid(ty, "malformed previous transaction", scope))?;
            }
            Field::WitnessUtxo { .. } => {
                bitcoin_wire::decode_output(&record.value, &wire_limits(limits))
                    .map_err(|_| invalid(ty, "malformed transaction output", scope))?;
            }
            Field::FinalScriptWitness(bytes) => {
                bitcoin_wire::decode_witness(bytes, &wire_limits(limits))
                    .map_err(|_| invalid(ty, "malformed final witness stack", scope))?;
            }
            _ => {}
        }
    }
    match (scope, version) {
        (Scope::Global, Version::V0) => {
            require(map, 0, scope)?;
        }
        (Scope::Global, Version::V2) => {
            for ty in [0xfb, 0x02, 0x04, 0x05] {
                require(map, ty, scope)?;
            }
        }
        (Scope::Input(_), Version::V2) => {
            for ty in [0x0e, 0x0f] {
                require(map, ty, scope)?;
            }
        }
        (Scope::Output(_), Version::V2) => {
            for ty in [0x03, 0x04] {
                require(map, ty, scope)?;
            }
        }
        _ => {}
    }
    Ok(())
}

// The wire codec's Limits API is finalized together with its owner. Both the
// caller byte ceiling and the PSBT map ceiling also bound embedded transactions.
fn wire_limits(limits: Limits) -> bitcoin_wire::Limits {
    let mut wire = bitcoin_wire::Limits::default();
    wire.max_transaction_bytes = limits.max_value_bytes.min(limits.max_bytes);
    wire.max_script_bytes = wire.max_transaction_bytes;
    wire.max_witness_item_bytes = wire.max_transaction_bytes;
    wire.max_payload_bytes = wire.max_transaction_bytes;
    wire.max_inputs = limits.max_maps.saturating_sub(1);
    wire.max_outputs = limits.max_maps.saturating_sub(1);
    wire
}

fn require(map: &Map, key_type: u64, scope: Scope) -> Result<&[u8]> {
    map.singleton(key_type)
        .ok_or_else(|| Error::new(ErrorKind::MissingField(key_type), scope))
}

fn invalid(key_type: u64, reason: &'static str, scope: Scope) -> Error {
    Error::new(ErrorKind::InvalidField { key_type, reason }, scope)
}

fn interpret(record: &Record, scope: Scope) -> Result<Field<'_>> {
    // These readers operate inside a key or value, not the original container.
    // Do not present their local positions as absolute PSBT byte offsets.
    interpret_inner(record, scope).map_err(|mut error| {
        error.offset = None;
        error
    })
}

fn interpret_inner(record: &Record, scope: Scope) -> Result<Field<'_>> {
    let ty = record.key_type;
    let key = record.key_data();
    let value = record.value();
    let singleton = || {
        if key.is_empty() {
            Ok(())
        } else {
            Err(invalid(ty, "field must have no key data", scope))
        }
    };
    let u32_field = || {
        singleton()?;
        if value.len() != 4 {
            return Err(invalid(ty, "field must be four bytes", scope));
        }
        Ok(read_u32(value))
    };
    let origin = || parse_origin(value, ty, scope);
    let pubkey = || {
        if valid_public_key(key) {
            Ok(())
        } else {
            Err(invalid(ty, "invalid public key encoding", scope))
        }
    };
    if ty == 0xfc {
        let mut reader = Reader {
            bytes: key,
            pos: 0,
            scope,
        };
        let length = reader.length()?;
        let identifier = reader.take(length)?;
        let subtype = reader.compact_size()?;
        return Ok(Field::Proprietary(Proprietary {
            identifier,
            subtype,
            key_data: &key[reader.pos..],
            value,
        }));
    }
    match (scope, ty) {
        (Scope::Global, 0) => {
            singleton()?;
            Ok(Field::UnsignedTransaction(value))
        }
        (Scope::Global, 1) => {
            if key.len() != 78 || !valid_public_key(&key[45..]) {
                return Err(invalid(
                    ty,
                    "extended public key must contain 78 bytes and a compressed key",
                    scope,
                ));
            }
            let origin = origin()?;
            if origin.path.len() / 4 != usize::from(key[4]) {
                return Err(invalid(
                    ty,
                    "extended public key depth does not match origin path",
                    scope,
                ));
            }
            Ok(Field::ExtendedPublicKey {
                xpub: key.try_into().expect("checked length"),
                origin,
            })
        }
        (Scope::Global, 0xfb) => Ok(Field::PsbtVersion(u32_field()?)),
        (Scope::Global, 0x02) => Ok(Field::TransactionVersion(u32_field()? as i32)),
        (Scope::Global, 0x03) => Ok(Field::FallbackLocktime(u32_field()?)),
        (Scope::Global, 0x04 | 0x05) => {
            singleton()?;
            let mut reader = Reader {
                bytes: value,
                pos: 0,
                scope,
            };
            let count = reader.compact_size()?;
            if reader.remaining() != 0 {
                return Err(invalid(ty, "count has trailing data", scope));
            }
            Ok(if ty == 0x04 {
                Field::InputCount(count)
            } else {
                Field::OutputCount(count)
            })
        }
        (Scope::Global, 0x06) => {
            singleton()?;
            if value.len() != 1 {
                return Err(invalid(ty, "modifiable flags must be one byte", scope));
            }
            Ok(Field::TransactionModifiable(value[0])) // preserve reserved flag bits
        }
        (Scope::Input(_), 0) => {
            singleton()?;
            Ok(Field::NonWitnessUtxo(value))
        }
        (Scope::Input(_), 1) => {
            singleton()?;
            let mut reader = Reader {
                bytes: value,
                pos: 0,
                scope,
            };
            let amount = i64::from_le_bytes(reader.take(8)?.try_into().expect("checked length"));
            let length = reader.length()?;
            let script = reader.take(length)?;
            if reader.remaining() != 0 {
                return Err(invalid(ty, "output has trailing data", scope));
            }
            Ok(Field::WitnessUtxo { amount, script })
        }
        (Scope::Input(_), 2) => {
            pubkey()?;
            if !valid_der_signature(value) {
                return Err(invalid(
                    ty,
                    "invalid DER signature and sighash encoding",
                    scope,
                ));
            }
            Ok(Field::PartialSignature {
                public_key: key,
                signature: value,
            })
        }
        (Scope::Input(_), 3) => Ok(Field::SighashType(u32_field()?)),
        (Scope::Input(_), 4) | (Scope::Output(_), 0) => {
            singleton()?;
            Ok(Field::RedeemScript(value))
        }
        (Scope::Input(_), 5) | (Scope::Output(_), 1) => {
            singleton()?;
            Ok(Field::WitnessScript(value))
        }
        (Scope::Input(_), 6) | (Scope::Output(_), 2) => {
            pubkey()?;
            Ok(Field::Bip32Derivation {
                public_key: key,
                origin: origin()?,
            })
        }
        (Scope::Input(_), 7) => {
            singleton()?;
            Ok(Field::FinalScriptSig(value))
        }
        (Scope::Input(_), 8) => {
            singleton()?;
            Ok(Field::FinalScriptWitness(value))
        }
        (Scope::Input(_), 0x0a..=0x0d) => {
            let (length, algorithm) = match ty {
                0x0a => (20, PreimageHash::Ripemd160),
                0x0b => (32, PreimageHash::Sha256),
                0x0c => (20, PreimageHash::Hash160),
                _ => (32, PreimageHash::Hash256),
            };
            if key.len() != length {
                return Err(invalid(ty, "incorrect preimage hash length", scope));
            }
            Ok(Field::Preimage {
                algorithm,
                hash: key,
                preimage: value,
            })
        }
        (Scope::Input(_), 0x0e) => {
            singleton()?;
            if value.len() != 32 {
                return Err(invalid(ty, "previous TXID must be 32 bytes", scope));
            }
            Ok(Field::PreviousTxid(
                value.try_into().expect("checked length"),
            ))
        }
        (Scope::Input(_), 0x0f) => Ok(Field::OutputIndex(u32_field()?)),
        (Scope::Input(_), 0x10) => Ok(Field::Sequence(u32_field()?)),
        (Scope::Input(_), 0x11) => {
            let time = u32_field()?;
            if time < LOCKTIME_THRESHOLD {
                return Err(invalid(
                    ty,
                    "time locktime must be at least 500000000",
                    scope,
                ));
            }
            Ok(Field::RequiredTimeLocktime(time))
        }
        (Scope::Input(_), 0x12) => {
            let height = u32_field()?;
            if height == 0 || height >= LOCKTIME_THRESHOLD {
                return Err(invalid(
                    ty,
                    "height locktime must be between 1 and 499999999",
                    scope,
                ));
            }
            Ok(Field::RequiredHeightLocktime(height))
        }
        (Scope::Output(_), 3) => {
            singleton()?;
            if value.len() != 8 {
                return Err(invalid(ty, "output amount must be eight bytes", scope));
            }
            Ok(Field::OutputAmount(i64::from_le_bytes(
                value.try_into().expect("checked length"),
            )))
        }
        (Scope::Output(_), 4) => {
            singleton()?;
            Ok(Field::OutputScript(value))
        }
        _ => Ok(Field::Unknown {
            key_type: ty,
            key_data: key,
            value,
        }),
    }
}

fn parse_origin(bytes: &[u8], ty: u64, scope: Scope) -> Result<KeyOrigin<'_>> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(4) {
        return Err(invalid(
            ty,
            "origin must contain a fingerprint and complete four-byte path indexes",
            scope,
        ));
    }
    Ok(KeyOrigin {
        fingerprint: bytes[..4].try_into().expect("checked length"),
        path: &bytes[4..],
    })
}

fn valid_public_key(bytes: &[u8]) -> bool {
    matches!(
        (bytes.first(), bytes.len()),
        (Some(2 | 3), 33) | (Some(4), 65)
    )
}

// BIP66 encoding shape, including the final sighash byte. No ECDSA verification.
fn valid_der_signature(bytes: &[u8]) -> bool {
    if !(9..=73).contains(&bytes.len())
        || bytes[0] != 0x30
        || usize::from(bytes[1]) != bytes.len() - 3
        || bytes[2] != 0x02
    {
        return false;
    }
    let r_len = usize::from(bytes[3]);
    if r_len == 0 || 5 + r_len >= bytes.len() {
        return false;
    }
    if bytes[4] & 0x80 != 0 || (r_len > 1 && bytes[4] == 0 && bytes[5] & 0x80 == 0) {
        return false;
    }
    if bytes[4 + r_len] != 0x02 {
        return false;
    }
    let s_len = usize::from(bytes[5 + r_len]);
    if s_len == 0 || r_len + s_len + 7 != bytes.len() {
        return false;
    }
    let s_start = 6 + r_len;
    bytes[s_start] & 0x80 == 0
        && !(s_len > 1 && bytes[s_start] == 0 && bytes[s_start + 1] & 0x80 == 0)
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn check_map_count(inputs: usize, outputs: usize, limits: Limits) -> Result<()> {
    let count = inputs
        .checked_add(outputs)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, Scope::Global))?;
    if count > limits.max_maps {
        return Err(Error::new(
            ErrorKind::LimitExceeded(Limit::Maps),
            Scope::Global,
        ));
    }
    Ok(())
}

fn check_duplicates(records: &[Record], scope: Scope) -> Result<()> {
    let mut seen = BTreeSet::new();
    for record in records {
        if !seen.insert(&record.key) {
            return Err(Error::new(ErrorKind::DuplicateKey, scope));
        }
    }
    Ok(())
}

fn check_record_limits(key: usize, value: usize, scope: Scope, limits: Limits) -> Result<()> {
    if key > limits.max_key_bytes {
        return Err(Error::new(ErrorKind::LimitExceeded(Limit::KeyBytes), scope));
    }
    if value > limits.max_value_bytes {
        return Err(Error::new(
            ErrorKind::LimitExceeded(Limit::ValueBytes),
            scope,
        ));
    }
    Ok(())
}

fn count_record(in_map: usize, total: &mut usize, scope: Scope, limits: Limits) -> Result<()> {
    if in_map >= limits.max_records_per_map {
        return Err(Error::new(
            ErrorKind::LimitExceeded(Limit::RecordsPerMap),
            scope,
        ));
    }
    if *total >= limits.max_records {
        return Err(Error::new(ErrorKind::LimitExceeded(Limit::Records), scope));
    }
    *total += 1;
    Ok(())
}

fn measure_map(
    map: &Map,
    scope: Scope,
    limits: Limits,
    records: &mut usize,
    length: &mut usize,
) -> Result<()> {
    for (index, record) in map.records.iter().enumerate() {
        count_record(index, records, scope, limits)?;
        check_record_limits(record.key.len(), record.value.len(), scope, limits)?;
        let overhead =
            compact_size_len(record.key.len() as u64) + compact_size_len(record.value.len() as u64);
        *length = length
            .checked_add(overhead)
            .and_then(|len| len.checked_add(record.key.len()))
            .and_then(|len| len.checked_add(record.value.len()))
            .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, scope))?;
    }
    *length = length
        .checked_add(1)
        .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, scope))?;
    if *length > limits.max_bytes {
        return Err(Error::new(ErrorKind::LimitExceeded(Limit::Bytes), scope));
    }
    Ok(())
}

fn read_map(reader: &mut Reader<'_>, limits: Limits, total: &mut usize) -> Result<Map> {
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
        let offset = reader.pos;
        let key_len = reader.length()?;
        if key_len == 0 {
            return Ok(Map { records });
        }
        count_record(records.len(), total, reader.scope, limits)?;
        check_record_limits(key_len, 0, reader.scope, limits)?;
        let key = reader.take(key_len)?;
        let mut type_reader = Reader {
            bytes: key,
            pos: 0,
            scope: reader.scope,
        };
        let key_type = type_reader
            .compact_size()
            .map_err(|error| error.at(offset + compact_size_len(key_len as u64)))?;
        if !seen.insert(key) {
            return Err(Error::new(ErrorKind::DuplicateKey, reader.scope).at(offset));
        }
        let value_len = reader.length()?;
        check_record_limits(key_len, value_len, reader.scope, limits)?;
        let value = reader.take(value_len)?;
        records
            .try_reserve(1)
            .map_err(|_| Error::new(ErrorKind::AllocationFailed, reader.scope))?;
        records.push(Record {
            key: copy_bytes(key, reader.scope)?,
            value: copy_bytes(value, reader.scope)?,
            key_type,
            type_len: type_reader.pos,
        });
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    scope: Scope,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(length)
            .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, self.scope).at(self.pos))?;
        let bytes = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| Error::new(ErrorKind::UnexpectedEnd, self.scope).at(self.pos))?;
        self.pos = end;
        Ok(bytes)
    }

    fn compact_size(&mut self) -> Result<u64> {
        let offset = self.pos;
        let marker = self.take(1)?[0];
        let (value, minimum) = match marker {
            0xfd => (
                u64::from(u16::from_le_bytes(
                    self.take(2)?.try_into().expect("checked length"),
                )),
                0xfd,
            ),
            0xfe => (
                u64::from(u32::from_le_bytes(
                    self.take(4)?.try_into().expect("checked length"),
                )),
                0x1_0000,
            ),
            0xff => (
                u64::from_le_bytes(self.take(8)?.try_into().expect("checked length")),
                0x1_0000_0000,
            ),
            small => (u64::from(small), 0),
        };
        if value < minimum {
            return Err(Error::new(ErrorKind::NonCanonicalCompactSize, self.scope).at(offset));
        }
        Ok(value)
    }

    fn length(&mut self) -> Result<usize> {
        usize::try_from(self.compact_size()?)
            .map_err(|_| Error::new(ErrorKind::LengthOverflow, self.scope).at(self.pos))
    }
}

fn compact_size_len(value: u64) -> usize {
    match value {
        0..=0xfc => 1,
        0xfd..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}

fn write_compact_size(value: u64, bytes: &mut Vec<u8>) {
    match value {
        0..=0xfc => bytes.push(value as u8),
        0xfd..=0xffff => {
            bytes.push(0xfd);
            bytes.extend_from_slice(&(value as u16).to_le_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            bytes.push(0xfe);
            bytes.extend_from_slice(&(value as u32).to_le_bytes());
        }
        _ => {
            bytes.push(0xff);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn reserve_items<T>(count: usize, scope: Scope) -> Result<Vec<T>> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(count)
        .map_err(|_| Error::new(ErrorKind::AllocationFailed, scope))?;
    Ok(items)
}

fn allocate(length: usize, scope: Scope) -> Result<Vec<u8>> {
    reserve_items(length, scope)
}

fn copy_bytes(bytes: &[u8], scope: Scope) -> Result<Vec<u8>> {
    let mut copy = allocate(bytes.len(), scope)?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> Result<String> {
    let length = bytes
        .len()
        .checked_add(2)
        .and_then(|len| (len / 3).checked_mul(4))
        .ok_or_else(|| Error::new(ErrorKind::LengthOverflow, Scope::Global))?;
    let mut encoded = allocate(length, Scope::Global)?;
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        encoded.push(BASE64[usize::from(a >> 2)]);
        encoded.push(BASE64[usize::from(((a & 3) << 4) | (b >> 4))]);
        encoded.push(if chunk.len() > 1 {
            BASE64[usize::from(((b & 15) << 2) | (c >> 6))]
        } else {
            b'='
        });
        encoded.push(if chunk.len() > 2 {
            BASE64[usize::from(c & 63)]
        } else {
            b'='
        });
    }
    // Every byte was selected from an ASCII alphabet.
    Ok(String::from_utf8(encoded).expect("ASCII Base64 alphabet"))
}

fn base64_digit(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn decode_base64(text: &[u8], max_bytes: usize) -> Result<Vec<u8>> {
    let invalid = || Error::new(ErrorKind::InvalidBase64, Scope::Global);
    if !text.len().is_multiple_of(4) {
        return Err(invalid());
    }
    let padding = if text.ends_with(b"==") {
        2
    } else if text.ends_with(b"=") {
        1
    } else {
        0
    };
    let length = (text.len() / 4)
        .checked_mul(3)
        .and_then(|length| length.checked_sub(padding))
        .ok_or_else(invalid)?;
    if length > max_bytes {
        return Err(Error::new(
            ErrorKind::LimitExceeded(Limit::Bytes),
            Scope::Global,
        ));
    }
    let mut decoded = allocate(length, Scope::Global)?;
    for (index, chunk) in text.as_chunks::<4>().0.iter().enumerate() {
        let a = base64_digit(chunk[0]).ok_or_else(invalid)?;
        let b = base64_digit(chunk[1]).ok_or_else(invalid)?;
        let last = index == text.len() / 4 - 1;
        match (chunk[2], chunk[3]) {
            (b'=', b'=') if last && b & 15 == 0 => decoded.push((a << 2) | (b >> 4)),
            (c, b'=') if last => {
                let c = base64_digit(c).ok_or_else(invalid)?;
                if c & 3 != 0 {
                    return Err(invalid());
                }
                decoded.extend_from_slice(&[(a << 2) | (b >> 4), (b << 4) | (c >> 2)]);
            }
            (c, d) => {
                let c = base64_digit(c).ok_or_else(invalid)?;
                let d = base64_digit(d).ok_or_else(invalid)?;
                decoded.extend_from_slice(&[
                    (a << 2) | (b >> 4),
                    (b << 4) | (c >> 2),
                    (c << 6) | d,
                ]);
            }
        }
    }
    debug_assert_eq!(decoded.len(), length);
    Ok(decoded)
}

fn decode_hex(text: &[u8], max_bytes: usize) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return Err(Error::new(ErrorKind::InvalidHex, Scope::Global));
    }
    if text.len() / 2 > max_bytes {
        return Err(Error::new(
            ErrorKind::LimitExceeded(Limit::Bytes),
            Scope::Global,
        ));
    }
    let mut bytes = allocate(text.len() / 2, Scope::Global)?;
    let digit = |byte| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    for pair in text.as_chunks::<2>().0 {
        let high =
            digit(pair[0]).ok_or_else(|| Error::new(ErrorKind::InvalidHex, Scope::Global))?;
        let low = digit(pair[1]).ok_or_else(|| Error::new(ErrorKind::InvalidHex, Scope::Global))?;
        bytes.push(high << 4 | low);
    }
    Ok(bytes)
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    #[test]
    fn rfc4648_base64_vectors_and_invalid_padding() {
        for (bytes, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode_base64(bytes.as_bytes()).unwrap(), encoded);
            assert_eq!(
                decode_base64(encoded.as_bytes(), 100).unwrap(),
                bytes.as_bytes()
            );
        }
        for encoded in [
            "A",
            "AA",
            "AAA",
            "A===",
            "====",
            "Zg=",
            "Zg===",
            "Zh==",
            "Zm9=",
            "Zg==AAAA",
            "AA=A",
            "AAA-",
            "AAA_",
            "AA A",
            "AA\nA",
            "AAA\u{00a0}",
        ] {
            assert!(
                decode_base64(encoded.as_bytes(), 100).is_err(),
                "{encoded:?}"
            );
        }
        assert!(matches!(
            decode_base64(b"Zm9v", 2).unwrap_err().kind,
            ErrorKind::LimitExceeded(Limit::Bytes)
        ));
    }

    #[test]
    fn compact_size_boundaries_and_noncanonical_encodings() {
        for value in [
            0,
            1,
            252,
            253,
            65_535,
            65_536,
            0xffff_ffff,
            0x1_0000_0000,
            u64::MAX,
        ] {
            let mut bytes = Vec::new();
            write_compact_size(value, &mut bytes);
            assert_eq!(bytes.len(), compact_size_len(value));
            let mut reader = Reader {
                bytes: &bytes,
                pos: 0,
                scope: Scope::Global,
            };
            assert_eq!(reader.compact_size().unwrap(), value);
            assert_eq!(reader.remaining(), 0);
        }
        for bytes in [
            &[0xfd, 0xfc, 0][..],
            &[0xfe, 0xff, 0xff, 0, 0],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
        ] {
            let mut reader = Reader {
                bytes,
                pos: 0,
                scope: Scope::Global,
            };
            assert_eq!(
                reader.compact_size().unwrap_err().kind,
                ErrorKind::NonCanonicalCompactSize
            );
        }
    }
}
