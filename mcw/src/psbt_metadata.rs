//! Wallet PSBT metadata enrichment. Construction, policy and signing stay with
//! their existing owners. All packet edits are immutable and preserve unrelated
//! records. These operations neither authorize a spend nor validate signatures.
#![forbid(unsafe_code)]

use crate::{
    bitcoin_encoding::sha256,
    bitcoin_script::{self as script, OutputTemplate},
    bitcoin_wire::{self as wire, OutPoint, Transaction, TxOut},
    psbt::{self, Map, Psbt, Record},
    wallet_hashes::hash160,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Packet(psbt::Error),
    Wire,
    InvalidOrigin,
    InvalidPreviousTransaction,
    InconsistentUtxo,
    Limit,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Packet(_) => "invalid PSBT metadata container",
            Self::Wire => "invalid PSBT transaction metadata",
            Self::InvalidOrigin => "invalid wallet key origin",
            Self::InvalidPreviousTransaction => "invalid previous transaction",
            Self::InconsistentUtxo => "inconsistent PSBT UTXO metadata",
            Self::Limit => "PSBT metadata resource limit exceeded",
        })
    }
}
impl std::error::Error for Error {}
impl From<psbt::Error> for Error {
    fn from(value: psbt::Error) -> Self {
        Self::Packet(value)
    }
}
type Result<T> = std::result::Result<T, Error>;

/// Script lookup deliberately uses WITNESS_UTXO alone, as the wallet's former
/// AddKeyPaths helper did. Adding a parent later must not change that lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputInspection {
    pub previous_output: OutPoint,
    pub witness_script_pubkey: Option<Vec<u8>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    pub inputs: Vec<InputInspection>,
    pub output_scripts: Vec<Vec<u8>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyAssociation {
    pub public_key: [u8; 33],
    pub fingerprint: [u8; 4],
    pub path: Vec<u32>,
    pub script_pubkey: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviousTransaction {
    /// Bitcoin's raw transaction hash bytes, in outpoint serialization order.
    pub txid: [u8; 32],
    pub bytes: Vec<u8>,
}

fn transaction(packet: &Psbt) -> Result<Transaction> {
    Transaction::decode_legacy(&packet.unsigned_transaction()?, &wire::Limits::default())
        .map_err(|_| Error::Wire)
}
fn witness_output(map: &Map) -> Result<Option<TxOut>> {
    map.singleton(1)
        .map(|bytes| wire::decode_output(bytes, &wire::Limits::default()).map_err(|_| Error::Wire))
        .transpose()
}
pub fn inspect(packet: &Psbt) -> Result<Inspection> {
    let tx = transaction(packet)?;
    let inputs = tx
        .inputs
        .iter()
        .zip(packet.inputs())
        .map(|(input, map)| {
            Ok(InputInspection {
                previous_output: input.previous_output.clone(),
                witness_script_pubkey: witness_output(map)?.map(|output| output.script_pubkey),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Inspection {
        inputs,
        output_scripts: tx
            .outputs
            .into_iter()
            .map(|output| output.script_pubkey)
            .collect(),
    })
}

struct PreparedKey<'a> {
    association: &'a KeyAssociation,
    hash: [u8; 20],
    witness: Vec<u8>,
    public_key_script: Vec<u8>,
    origin: Vec<u8>,
}
impl<'a> PreparedKey<'a> {
    fn new(association: &'a KeyAssociation, limits: psbt::Limits) -> Result<Self> {
        if !matches!(association.public_key[0], 2 | 3) {
            return Err(Error::InvalidOrigin);
        }
        let length = association
            .path
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(4))
            .ok_or(Error::Limit)?;
        if length > limits.max_value_bytes
            || association.script_pubkey.len() > limits.max_value_bytes
        {
            return Err(Error::Limit);
        }
        let hash = hash160(&association.public_key).map_err(|_| Error::InvalidOrigin)?;
        let witness = script::p2wpkh(&hash).into_bytes();
        let mut public_key_script = vec![33];
        public_key_script.extend_from_slice(&association.public_key);
        public_key_script.push(0xac);
        let mut origin = Vec::new();
        origin.try_reserve_exact(length).map_err(|_| Error::Limit)?;
        origin.extend_from_slice(&association.fingerprint);
        for index in &association.path {
            origin.extend_from_slice(&index.to_le_bytes());
        }
        Ok(Self {
            association,
            hash,
            witness,
            public_key_script,
            origin,
        })
    }
}

// Index candidate keys rather than doing every key × every input/output. Exact
// eligibility is still checked below, including redeem/witness commitments.
#[derive(Default)]
struct KeyIndex {
    scripts: BTreeMap<Vec<u8>, Vec<usize>>,
    hashes: BTreeMap<[u8; 20], Vec<usize>>,
    public_keys: BTreeMap<[u8; 33], Vec<usize>>,
}
impl KeyIndex {
    fn new(keys: &[PreparedKey<'_>]) -> Result<Self> {
        let mut result = Self::default();
        for (index, key) in keys.iter().enumerate() {
            result.hashes.entry(key.hash).or_default().push(index);
            result
                .public_keys
                .entry(key.association.public_key)
                .or_default()
                .push(index);
            result
                .scripts
                .entry(key.association.script_pubkey.clone())
                .or_default()
                .push(index);
            for redeem in [&key.witness, &key.public_key_script] {
                let witness_hash = sha256(redeem).map_err(|_| Error::Wire)?;
                let witness_script = script::p2wsh(&witness_hash).into_bytes();
                for candidate in [
                    script::p2sh(&hash160(redeem).map_err(|_| Error::Wire)?).into_bytes(),
                    script::p2sh(&hash160(&witness_script).map_err(|_| Error::Wire)?).into_bytes(),
                    witness_script,
                ] {
                    result.scripts.entry(candidate).or_default().push(index);
                }
            }
        }
        Ok(result)
    }
    fn collect(&self, bytes: &[u8], candidates: &mut BTreeSet<usize>) {
        if let Some(indexes) = self.scripts.get(bytes) {
            candidates.extend(indexes);
        }
        match script::classify_output(bytes) {
            Ok(OutputTemplate::P2pkh(hash) | OutputTemplate::P2wpkh(hash)) => {
                if let Some(indexes) = self.hashes.get(hash) {
                    candidates.extend(indexes);
                }
            }
            Ok(OutputTemplate::P2pk { public_key, .. }) => {
                self.collect_public_key(public_key, candidates)
            }
            Ok(OutputTemplate::Multisig { public_keys, .. }) => {
                for public_key in public_keys {
                    self.collect_public_key(public_key, candidates);
                }
            }
            _ => (),
        }
    }
    fn collect_public_key(&self, bytes: &[u8], candidates: &mut BTreeSet<usize>) {
        if let Some(key) = compressed_key(bytes)
            && let Some(indexes) = self.public_keys.get(&key)
        {
            candidates.extend(indexes);
        }
    }
}
fn compressed_key(bytes: &[u8]) -> Option<[u8; 33]> {
    if bytes.len() == 33 && matches!(bytes[0], 2 | 3) {
        return bytes.try_into().ok();
    }
    if bytes.len() == 65 && bytes[0] == 4 {
        let mut key = [0; 33];
        key[0] = 2 | (bytes[64] & 1);
        key[1..].copy_from_slice(&bytes[1..33]);
        return Some(key);
    }
    None
}
fn compatible(bytes: &[u8], key: &PreparedKey<'_>, convert_witness: bool) -> bool {
    match script::classify_output(bytes) {
        Ok(OutputTemplate::P2pkh(hash)) => *hash == key.hash,
        Ok(OutputTemplate::P2wpkh(hash)) => convert_witness && *hash == key.hash,
        Ok(OutputTemplate::P2pk { public_key, .. }) => public_key == key.association.public_key,
        Ok(OutputTemplate::Multisig { public_keys, .. }) => public_keys
            .iter()
            .any(|bytes| *bytes == key.association.public_key),
        _ => false,
    }
}
fn coherent(utxo: &[u8], redeem: &[u8]) -> bool {
    match script::classify_output(utxo) {
        Ok(OutputTemplate::P2sh(hash)) => {
            if matches!(
                script::classify_output(redeem),
                Ok(OutputTemplate::P2wsh(_))
            ) {
                return false;
            }
            hash160(redeem).is_ok_and(|value| &value == hash)
                || sha256(redeem).is_ok_and(|value| {
                    hash160(script::p2wsh(&value).as_bytes()).is_ok_and(|value| &value == hash)
                })
        }
        Ok(OutputTemplate::P2wsh(hash)) => sha256(redeem).is_ok_and(|value| &value == hash),
        _ => false,
    }
}
fn signable_script<'a>(
    utxo: &'a [u8],
    redeem: Option<&'a [u8]>,
    witness: Option<&'a [u8]>,
) -> Option<&'a [u8]> {
    match script::classify_output(utxo).ok()? {
        OutputTemplate::P2sh(hash) => {
            let redeem = redeem?;
            if hash160(redeem).ok()?.as_slice() != hash {
                return None;
            }
            if let Ok(OutputTemplate::P2wsh(hash)) = script::classify_output(redeem) {
                let witness = witness?;
                (sha256(witness).ok()?.as_slice() == hash).then_some(witness)
            } else {
                Some(redeem)
            }
        }
        OutputTemplate::P2wsh(hash) => {
            if redeem.is_some() {
                return None;
            }
            let witness = witness?;
            (sha256(witness).ok()?.as_slice() == hash).then_some(witness)
        }
        _ => redeem.is_none().then_some(utxo),
    }
}
fn matches(utxo: &[u8], map: &Map, input: bool, key: &PreparedKey<'_>) -> bool {
    if utxo == key.association.script_pubkey || compatible(utxo, key, false) {
        return true;
    }
    let redeem = map.singleton(if input { 4 } else { 0 });
    let witness = map.singleton(if input { 5 } else { 1 });
    if let Some(code) = signable_script(utxo, redeem, witness) {
        return compatible(code, key, true);
    }
    // The former GetScriptCode falls back to Coin.TryToScriptCoin(pubkey).
    coherent(utxo, &key.witness) || coherent(utxo, &key.public_key_script)
}
fn finalized(map: &Map) -> bool {
    map.singleton(7).is_some() || map.singleton(8).is_some()
}
fn effective_output(map: &Map, outpoint: &OutPoint) -> Result<Option<TxOut>> {
    if let Some(bytes) = map.singleton(0) {
        let tx = Transaction::decode(bytes, &wire::Limits::default()).map_err(|_| Error::Wire)?;
        if tx
            .txid(&wire::Limits::default())
            .map_err(|_| Error::Wire)?
            .0
            != outpoint.txid
        {
            return Ok(None);
        }
        return Ok(tx.outputs.get(outpoint.vout as usize).cloned());
    }
    witness_output(map)
}
fn enrich_map(
    map: &Map,
    utxo: &[u8],
    input: bool,
    keys: &[PreparedKey<'_>],
    index: &KeyIndex,
    smart: bool,
) -> Result<Map> {
    let mut candidates = BTreeSet::new();
    index.collect(utxo, &mut candidates);
    for field in [if input { 4 } else { 0 }, if input { 5 } else { 1 }] {
        if let Some(bytes) = map.singleton(field) {
            index.collect(bytes, &mut candidates);
        }
    }
    let mut result = map.clone();
    for candidate in candidates {
        let key = &keys[candidate];
        if !matches(utxo, &result, input, key) {
            continue;
        }
        result = result.with_record(Record::new(
            if input { 6 } else { 2 },
            &key.association.public_key,
            &key.origin,
        )?);
        let redeem_type = if input { 4 } else { 0 };
        if smart && result.singleton(redeem_type).is_none()
            && script::classify_output(utxo).is_ok_and(|template| matches!(template, OutputTemplate::P2sh(hash) if hash160(&key.witness).is_ok_and(|value| &value == hash))) {
            result = result.with_record(Record::new(redeem_type, &[], &key.witness)?);
        }
    }
    Ok(result)
}

/// First add the same BIP174 compressed-key origins as the former helper,
/// including explicitly supplied Taproot scripts. Then attach available parents.
/// Missing parents are represented by absence from `previous_transactions`.
pub fn enrich(
    packet: &Psbt,
    associations: &[KeyAssociation],
    previous_transactions: &[PreviousTransaction],
    smart: bool,
) -> Result<Psbt> {
    let limits = packet.limits();
    let map_count = packet
        .inputs()
        .len()
        .checked_add(packet.outputs().len())
        .ok_or(Error::Limit)?;
    if associations.len() > map_count || previous_transactions.len() > packet.inputs().len() {
        return Err(Error::Limit);
    }
    let tx = transaction(packet)?;
    let keys = associations
        .iter()
        .map(|association| PreparedKey::new(association, limits))
        .collect::<Result<Vec<_>>>()?;
    let index = KeyIndex::new(&keys)?;
    let all_finalized = packet.inputs().iter().all(finalized);
    let mut inputs = Vec::with_capacity(packet.inputs().len());
    let mut outputs = Vec::with_capacity(packet.outputs().len());
    for (map, input) in packet.inputs().iter().zip(&tx.inputs) {
        let output = if all_finalized || finalized(map) {
            None
        } else {
            effective_output(map, &input.previous_output)?
        };
        inputs.push(match output {
            Some(output) => enrich_map(map, &output.script_pubkey, true, &keys, &index, smart)?,
            None => map.clone(),
        });
    }
    for (map, output) in packet.outputs().iter().zip(&tx.outputs) {
        outputs.push(if all_finalized {
            map.clone()
        } else {
            enrich_map(map, &output.script_pubkey, false, &keys, &index, smart)?
        });
    }
    let mut parents = BTreeMap::new();
    for previous in previous_transactions {
        if previous.bytes.len() > limits.max_value_bytes {
            return Err(Error::Limit);
        }
        let parent = Transaction::decode(&previous.bytes, &wire::Limits::default())
            .map_err(|_| Error::InvalidPreviousTransaction)?;
        if parent
            .txid(&wire::Limits::default())
            .map_err(|_| Error::InvalidPreviousTransaction)?
            .0
            != previous.txid
            || parents.insert(previous.txid, (previous, parent)).is_some()
        {
            return Err(Error::InvalidPreviousTransaction);
        }
    }
    for (map, input) in inputs.iter_mut().zip(&tx.inputs) {
        if let Some((previous, parent)) = parents.get(&input.previous_output.txid) {
            let output = parent
                .outputs
                .get(input.previous_output.vout as usize)
                .ok_or(Error::InconsistentUtxo)?;
            if witness_output(map)?.is_some_and(|witness| witness != *output) {
                return Err(Error::InconsistentUtxo);
            }
            *map = map.with_record(Record::new(0, &[], &previous.bytes)?);
        }
    }
    // from_maps checks cumulative packet, record and value limits before commit.
    Ok(Psbt::from_maps(
        packet.global().clone(),
        inputs,
        outputs,
        limits,
    )?)
}
