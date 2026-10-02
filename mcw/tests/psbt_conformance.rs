//! Official reference vectors and adversarial format tests (synthetic data only).
//! Integrated by the application host once `mcw::psbt` is declared.

use mcw::bitcoin_wire::{self, Transaction};
use mcw::psbt::{ErrorKind, Field, Limit, Limits, MAGIC, Map, Psbt, Record, Scope, Version};

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| (byte as char).to_digit(16).unwrap() as u8;
            digit(pair[0]) << 4 | digit(pair[1])
        })
        .collect()
}

fn record(ty: u64, key: &[u8], value: &[u8]) -> Record {
    Record::new(ty, key, value).unwrap()
}
fn map(records: Vec<Record>) -> Map {
    Map::new(records).unwrap()
}

fn v2_global(inputs: u8, outputs: u8) -> Map {
    map(vec![
        record(0xfb, &[], &2u32.to_le_bytes()),
        record(2, &[], &2i32.to_le_bytes()),
        record(4, &[], &[inputs]),
        record(5, &[], &[outputs]),
    ])
}

fn input() -> Map {
    map(vec![
        record(0x0e, &[], &[0x11; 32]),
        record(0x0f, &[], &3u32.to_le_bytes()),
    ])
}

fn output() -> Map {
    map(vec![
        record(3, &[], &(-1i64).to_le_bytes()),
        record(4, &[], &[0x51]),
    ])
}

fn v2() -> Psbt {
    Psbt::from_maps(
        v2_global(1, 1),
        vec![input()],
        vec![output()],
        Limits::default(),
    )
    .unwrap()
}

fn v0_zero() -> Psbt {
    let tx = hex("02000000000000000000");
    Psbt::from_maps(
        map(vec![record(0, &[], &tx)]),
        vec![],
        vec![],
        Limits::default(),
    )
    .unwrap()
}

fn encode_unchecked(global: &Map, inputs: &[Map], outputs: &[Map]) -> Vec<u8> {
    fn compact(value: u64, result: &mut Vec<u8>) {
        match value {
            0..=252 => result.push(value as u8),
            253..=65535 => {
                result.push(253);
                result.extend_from_slice(&(value as u16).to_le_bytes());
            }
            65536..=0xffff_ffff => {
                result.push(254);
                result.extend_from_slice(&(value as u32).to_le_bytes());
            }
            _ => {
                result.push(255);
                result.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    let mut result = MAGIC.to_vec();
    for map in std::iter::once(global).chain(inputs).chain(outputs) {
        for record in map.records() {
            compact(record.key().len() as u64, &mut result);
            result.extend_from_slice(record.key());
            compact(record.value().len() as u64, &mut result);
            result.extend_from_slice(record.value());
        }
        result.push(0);
    }
    result
}

#[test]
fn all_official_bip174_and_bip370_cases() {
    let mut counts = [0usize; 4];
    for line in include_str!("psbt_vectors.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let row: Vec<_> = line.split('\t').collect();
        assert_eq!(row.len(), 6);
        let bytes = hex(row[4]);
        let parsed = Psbt::parse(&bytes, Limits::default());
        let from_base64 = Psbt::from_base64(row[5], Limits::default());
        if row[1] == "invalid" {
            counts[0] += 1;
            assert!(
                parsed.is_err(),
                "{} / {} accepted invalid hex",
                row[0],
                row[3]
            );
            assert!(
                from_base64.is_err(),
                "{} / {} accepted invalid Base64",
                row[0],
                row[3]
            );
        } else {
            counts[if row[1] == "signer" { 2 } else { 1 }] += 1;
            let parsed = parsed.unwrap_or_else(|error| panic!("{} / {}: {error}", row[0], row[3]));
            assert_eq!(parsed.serialize().unwrap(), bytes, "{}", row[3]);
            assert_eq!(parsed.to_base64().unwrap(), row[5], "{}", row[3]);
            assert_eq!(from_base64.unwrap(), parsed, "{}", row[3]);
            assert_eq!(Psbt::parse_text(row[4], Limits::default()).unwrap(), parsed);
            for (scope, map) in std::iter::once((Scope::Global, parsed.global()))
                .chain(
                    parsed
                        .inputs()
                        .iter()
                        .enumerate()
                        .map(|(i, map)| (Scope::Input(i), map)),
                )
                .chain(
                    parsed
                        .outputs()
                        .iter()
                        .enumerate()
                        .map(|(i, map)| (Scope::Output(i), map)),
                )
            {
                for record in map.records() {
                    record.field(scope).unwrap();
                }
            }
            let rebuilt = Psbt::from_maps(
                parsed.global().clone(),
                parsed.inputs().to_vec(),
                parsed.outputs().to_vec(),
                Limits::default(),
            )
            .unwrap();
            assert_eq!(rebuilt, parsed);
            if row[2] == "incompatible" {
                counts[3] += 1;
                assert_eq!(
                    parsed.locktime().unwrap_err().kind,
                    ErrorKind::IncompatibleLocktimes
                );
                assert!(parsed.unsigned_transaction().is_err());
            } else {
                if row[2] != "-" {
                    assert_eq!(
                        parsed.locktime().unwrap(),
                        row[2].parse::<u32>().unwrap(),
                        "{}",
                        row[3]
                    );
                }
                let tx = Transaction::decode_legacy(
                    &parsed.unsigned_transaction().unwrap(),
                    &bitcoin_wire::Limits::default(),
                )
                .unwrap();
                assert_eq!(tx.inputs.len(), parsed.inputs().len());
                assert_eq!(tx.outputs.len(), parsed.outputs().len());
                assert_eq!(tx.lock_time, parsed.locktime().unwrap());
                assert!(
                    tx.inputs
                        .iter()
                        .all(|input| input.script_sig.is_empty() && input.witness.is_empty())
                );
            }
        }
    }
    assert_eq!(
        counts[0], 44,
        "all 20 BIP174 and 24 BIP370 invalid cases must be present"
    );
    assert_eq!(counts[2], 4, "signer-only failures are container-valid");
    assert_eq!(counts[3], 1);
    assert_eq!(
        counts[1], 46,
        "all valid cases and role examples must be present"
    );
}

#[test]
fn exact_unsigned_and_identifier_transaction_bytes() {
    let psbt = v2();
    let expected = format!(
        "0200000001{}0300000000ffffffff01ffffffffffffffff015100000000",
        "11".repeat(32)
    );
    assert_eq!(psbt.unsigned_transaction().unwrap(), hex(&expected));
    let identifier = format!(
        "0200000001{}03000000000000000001ffffffffffffffff015100000000",
        "11".repeat(32)
    );
    assert_eq!(psbt.identifier_transaction().unwrap(), hex(&identifier));
    let global = psbt
        .global()
        .with_record(record(3, &[], &10u32.to_le_bytes()));
    let input = psbt.inputs()[0].with_record(record(0x10, &[], &123u32.to_le_bytes()));
    let updated = Psbt::from_maps(
        global,
        vec![input],
        psbt.outputs().to_vec(),
        Limits::default(),
    )
    .unwrap();
    let tx = Transaction::decode_legacy(
        &updated.unsigned_transaction().unwrap(),
        &bitcoin_wire::Limits::default(),
    )
    .unwrap();
    assert_eq!(tx.inputs[0].sequence, 123);
    assert_eq!(tx.lock_time, 10);
    let tx = Transaction::decode_legacy(
        &updated.identifier_transaction().unwrap(),
        &bitcoin_wire::Limits::default(),
    )
    .unwrap();
    assert_eq!(tx.inputs[0].sequence, 0);
    assert_eq!(tx.lock_time, 10);
    let v0 = v0_zero();
    assert_eq!(
        v0.unsigned_transaction().unwrap(),
        hex("02000000000000000000")
    );
    assert_eq!(
        v0.identifier_transaction().unwrap(),
        v0.unsigned_transaction().unwrap()
    );
}

#[test]
fn all_retained_nbitcoin_binary_and_base64_exports() {
    let mut count = 0;
    for line in include_str!("psbt_managed_vectors.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let row: Vec<_> = line.split('\t').collect();
        assert_eq!(row.len(), 6);
        let raw = hex(row[4]);
        let parsed = Psbt::parse(&raw, Limits::default())
            .unwrap_or_else(|error| panic!("NBitcoin export {}: {error}", row[3]));
        assert_eq!(
            parsed.serialize().unwrap(),
            raw,
            "NBitcoin export {}",
            row[3]
        );
        assert_eq!(
            parsed.to_base64().unwrap(),
            row[5],
            "NBitcoin export {}",
            row[3]
        );
        assert_eq!(
            Psbt::from_base64(row[5], Limits::default()).unwrap(),
            parsed
        );
        count += 1;
    }
    assert_eq!(count, 47);
}

#[test]
fn unknown_and_proprietary_records_keep_bytes_and_order() {
    for ty in [0x13, 0xef, 0xfd, 0x10000, 0x1_0000_0000, u64::MAX] {
        let global = v2_global(1, 1).with_record(record(ty, &[0, 0xff, 0x80], &[0, 0xff]));
        let psbt =
            Psbt::from_maps(global, vec![input()], vec![output()], Limits::default()).unwrap();
        let encoded = psbt.serialize().unwrap();
        let parsed = Psbt::parse(&encoded, Limits::default()).unwrap();
        assert_eq!(parsed.serialize().unwrap(), encoded);
        assert!(
            matches!(parsed.global().records().last().unwrap().field(Scope::Global).unwrap(), Field::Unknown { key_type, .. } if key_type == ty)
        );
    }
    // Proprietary: identifier length 3, "mcw", subtype 253 (canonical), subkey.
    let key = &[3, b'm', b'c', b'w', 0xfd, 0xfd, 0, 7, 8];
    let unknown = record(0xfd, &[9], &[8, 7]);
    let proprietary = record(0xfc, key, &[0, 0xfe, 0xff]);
    let global = v2_global(1, 1)
        .with_record(unknown.clone())
        .with_record(proprietary.clone());
    let input = input().with_record(proprietary.clone());
    let output = output().with_record(proprietary.clone());
    let psbt = Psbt::from_maps(global, vec![input], vec![output], Limits::default()).unwrap();
    let parsed = Psbt::from_base64(&psbt.to_base64().unwrap(), Limits::default()).unwrap();
    assert_eq!(parsed.serialize().unwrap(), psbt.serialize().unwrap());
    assert_eq!(parsed.global().records()[4], unknown);
    assert_eq!(parsed.global().records()[5], proprietary);
    for (scope, map) in [
        (Scope::Global, parsed.global()),
        (Scope::Input(0), &parsed.inputs()[0]),
        (Scope::Output(0), &parsed.outputs()[0]),
    ] {
        match map.get(0xfc, key).unwrap().field(scope).unwrap() {
            Field::Proprietary(field) => {
                assert_eq!(field.identifier, b"mcw");
                assert_eq!(field.subtype, 253);
                assert_eq!(field.key_data, &[7, 8]);
                assert_eq!(field.value, &[0, 0xfe, 0xff]);
            }
            other => panic!("unexpected field: {other:?}"),
        }
    }
    let empty_id = record(0xfc, &[0, 0], &[]);
    assert!(
        matches!(empty_id.field(Scope::Global).unwrap(), Field::Proprietary(field) if field.identifier.is_empty() && field.subtype == 0)
    );
}

#[test]
fn duplicate_full_keys_rejected_in_every_scope() {
    assert_eq!(
        Map::new(vec![record(99, &[1], &[2]), record(99, &[1], &[3])])
            .unwrap_err()
            .kind,
        ErrorKind::DuplicateKey
    );
    Map::new(vec![record(99, &[1], &[2]), record(99, &[2], &[2])]).unwrap();
    for index in 0..3 {
        let psbt = v2();
        let mut bytes = psbt.serialize().unwrap();
        let extra = if index == 0 {
            hex("01fb0402000000")
        } else if index == 1 {
            hex(&format!("010e20{}", "11".repeat(32)))
        } else {
            hex("01030100")
        };
        let original = if index == 0 {
            psbt.global()
        } else if index == 1 {
            &psbt.inputs()[0]
        } else {
            &psbt.outputs()[0]
        };
        let offset = if index == 0 {
            5
        } else {
            let mut position = 5;
            for map in std::iter::once(psbt.global())
                .chain(psbt.inputs())
                .take(index)
            {
                for record in map.records() {
                    position += 2 + record.key().len() + record.value().len();
                }
                position += 1;
            }
            position
        };
        assert!(!original.records().is_empty());
        bytes.splice(offset..offset, extra);
        let error = Psbt::parse(&bytes, Limits::default()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::DuplicateKey);
        assert_eq!(
            error.scope,
            [Scope::Global, Scope::Input(0), Scope::Output(0)][index]
        );
    }
}

#[test]
fn noncanonical_framing_key_types_counts_and_proprietary_lengths_rejected() {
    let canonical = v0_zero().serialize().unwrap();
    for prefix in [hex("fd0100"), hex("fe01000000"), hex("ff0100000000000000")] {
        let mut bytes = canonical.clone();
        bytes.splice(5..6, prefix);
        assert_eq!(
            Psbt::parse(&bytes, Limits::default()).unwrap_err().kind,
            ErrorKind::NonCanonicalCompactSize
        );
    }
    let mut bytes = canonical.clone();
    bytes.splice(7..8, hex("fd0a00"));
    assert_eq!(
        Psbt::parse(&bytes, Limits::default()).unwrap_err().kind,
        ErrorKind::NonCanonicalCompactSize
    );
    let mut bytes = canonical.clone();
    bytes.splice(5..7, hex("03fd0000"));
    assert_eq!(
        Psbt::parse(&bytes, Limits::default()).unwrap_err().kind,
        ErrorKind::NonCanonicalCompactSize
    );
    let mut bytes = canonical.clone();
    bytes.splice(bytes.len() - 1.., hex("fd0000"));
    assert_eq!(
        Psbt::parse(&bytes, Limits::default()).unwrap_err().kind,
        ErrorKind::NonCanonicalCompactSize
    );
    for count in [hex("fd0100"), hex("0100"), hex("fe01000000"), vec![]] {
        let global = v2_global(1, 1).with_record(record(4, &[], &count));
        assert!(
            Psbt::from_maps(
                global.clone(),
                vec![input()],
                vec![output()],
                Limits::default()
            )
            .is_err()
        );
        assert!(
            Psbt::parse(
                &encode_unchecked(&global, &[input()], &[output()]),
                Limits::default()
            )
            .is_err()
        );
    }
    for key in [
        hex("fd01007800"),
        hex("01"),
        hex("0178"),
        hex("00fd0000"),
        hex("ff0000000001000000"),
    ] {
        let global = v2_global(1, 1).with_record(record(0xfc, &key, &[]));
        assert!(
            Psbt::from_maps(global, vec![input()], vec![output()], Limits::default()).is_err(),
            "{key:?}"
        );
    }
}

#[test]
fn typed_lengths_pubkey_shapes_and_origins() {
    let compressed = [2; 33];
    let mut uncompressed = [0x11; 65];
    uncompressed[0] = 4;
    let origin = [1, 2, 3, 4, 0xff, 0xff, 0xff, 0x7f, 0, 0, 0, 0x80];
    for public_key in [&compressed[..], &uncompressed[..]] {
        let rec = record(6, public_key, &origin);
        match rec.field(Scope::Input(0)).unwrap() {
            Field::Bip32Derivation {
                public_key: key,
                origin: key_origin,
            } => {
                assert_eq!(key, public_key);
                assert_eq!(key_origin.fingerprint, &[1, 2, 3, 4]);
                assert_eq!(
                    key_origin.path().collect::<Vec<_>>(),
                    vec![0x7fff_ffff, 0x8000_0000]
                );
            }
            other => panic!("{other:?}"),
        }
    }
    for key in [
        vec![],
        vec![2; 32],
        vec![2; 34],
        vec![4; 33],
        vec![3; 65],
        vec![6; 65],
    ] {
        assert!(record(6, &key, &[0; 4]).field(Scope::Input(0)).is_err());
        assert!(record(2, &key, &[0; 4]).field(Scope::Output(0)).is_err());
    }
    for len in [0, 1, 3, 5, 7, 9] {
        assert!(
            record(6, &compressed, &vec![0; len])
                .field(Scope::Input(0))
                .is_err()
        );
    }
    for (scope, ty, required_len) in [
        (Scope::Global, 0xfb, 4),
        (Scope::Global, 2, 4),
        (Scope::Global, 3, 4),
        (Scope::Global, 6, 1),
        (Scope::Input(0), 3, 4),
        (Scope::Input(0), 0x0e, 32),
        (Scope::Input(0), 0x0f, 4),
        (Scope::Input(0), 0x10, 4),
        (Scope::Output(0), 3, 8),
    ] {
        for len in [0, required_len - 1, required_len + 1] {
            assert!(
                record(ty, &[], &vec![0; len]).field(scope).is_err(),
                "{scope:?}/{ty}"
            );
        }
        assert!(
            record(ty, &[1], &vec![0; required_len])
                .field(scope)
                .is_err()
        );
        record(ty, &[], &vec![0; required_len])
            .field(scope)
            .unwrap();
    }
    for (ty, hash_len) in [(0x0a, 20), (0x0b, 32), (0x0c, 20), (0x0d, 32)] {
        record(ty, &vec![0; hash_len], b"opaque preimage")
            .field(Scope::Input(0))
            .unwrap();
        assert!(
            record(ty, &vec![0; hash_len - 1], b"x")
                .field(Scope::Input(0))
                .is_err()
        );
    }
    let mut xpub = [0; 78];
    xpub[4] = 2;
    xpub[45] = 2;
    record(1, &xpub, &origin).field(Scope::Global).unwrap();
    assert!(record(1, &xpub, &[0; 8]).field(Scope::Global).is_err());
    assert!(
        record(1, &xpub[..77], &origin)
            .field(Scope::Global)
            .is_err()
    );
    xpub[45] = 4;
    assert!(record(1, &xpub, &origin).field(Scope::Global).is_err());
}

#[test]
fn der_signature_encoding_without_crypto_claims() {
    let public_key = [2; 33];
    let valid = hex("300602010102010101"); // R=1, S=1, SIGHASH_ALL
    record(2, &public_key, &valid)
        .field(Scope::Input(0))
        .unwrap();
    for invalid in [
        vec![],
        hex("3006020101020101"),
        hex("310602010102010101"),
        hex("300602018102010101"),
        hex("30070202000102010101"),
        hex("300602010102018101"),
        hex("30070201010202000101"),
        hex("300602000102010101"),
        hex("30060201ff02010101"),
    ] {
        assert!(
            record(2, &public_key, &invalid)
                .field(Scope::Input(0))
                .is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn complete_transaction_and_witness_containers_use_wire_parser() {
    let base = v2();
    let legacy = hex("02000000000000000000");
    let mut witness = hex(&format!(
        "02000000000101{}0000000000ffffffff0101000000000000000001015100000000",
        "11".repeat(32)
    ));
    let wire = bitcoin_wire::Limits::default();
    Transaction::decode(&witness, &wire).unwrap();
    let good = base.inputs()[0].with_record(record(0, &[], &witness));
    Psbt::from_maps(
        base.global().clone(),
        vec![good],
        base.outputs().to_vec(),
        Limits::default(),
    )
    .unwrap();
    let mut global = map(vec![record(0, &[], &witness)]);
    assert!(
        Psbt::from_maps(
            global.clone(),
            vec![Map::default()],
            vec![Map::default()],
            Limits::default()
        )
        .is_err()
    );
    witness.pop();
    let bad = base.inputs()[0].with_record(record(0, &[], &witness));
    assert!(
        Psbt::from_maps(
            base.global().clone(),
            vec![bad],
            base.outputs().to_vec(),
            Limits::default()
        )
        .is_err()
    );
    let signed_legacy = hex(&format!(
        "0200000001{}000000000151ffffffff0101000000000000000000000000",
        "11".repeat(32)
    ));
    global = map(vec![record(0, &[], &signed_legacy)]);
    assert!(
        Psbt::from_maps(
            global,
            vec![Map::default()],
            vec![Map::default()],
            Limits::default()
        )
        .is_err()
    );
    let mut trailing = legacy;
    trailing.push(0);
    assert!(
        Psbt::from_maps(
            map(vec![record(0, &[], &trailing)]),
            vec![],
            vec![],
            Limits::default()
        )
        .is_err()
    );
    for malformed in [
        vec![],
        hex("01000000000000000251"),
        hex("01000000000000000000"),
        hex("0100000000000000fd0000"),
    ] {
        let bad = base.inputs()[0].with_record(record(1, &[], &malformed));
        assert!(
            Psbt::from_maps(
                base.global().clone(),
                vec![bad],
                base.outputs().to_vec(),
                Limits::default()
            )
            .is_err()
        );
    }
    for final_witness in [hex("00"), hex("0100"), hex("020151020001")] {
        bitcoin_wire::decode_witness(&final_witness, &wire).unwrap();
        let good = base.inputs()[0].with_record(record(8, &[], &final_witness));
        Psbt::from_maps(
            base.global().clone(),
            vec![good],
            base.outputs().to_vec(),
            Limits::default(),
        )
        .unwrap();
    }
    for bad_witness in [
        vec![],
        hex("01"),
        hex("01015100"),
        hex("fd0000"),
        hex("02015102"),
    ] {
        assert!(bitcoin_wire::decode_witness(&bad_witness, &wire).is_err());
        let bad = base.inputs()[0].with_record(record(8, &[], &bad_witness));
        assert!(
            Psbt::from_maps(
                base.global().clone(),
                vec![bad],
                base.outputs().to_vec(),
                Limits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn version_rules_map_counts_and_required_fields() {
    for raw_version in [1u32, 3, u32::MAX] {
        let global = v2_global(1, 1).with_record(record(0xfb, &[], &raw_version.to_le_bytes()));
        assert_eq!(
            Psbt::from_maps(global, vec![input()], vec![output()], Limits::default())
                .unwrap_err()
                .kind,
            ErrorKind::UnsupportedVersion(raw_version)
        );
    }
    for ty in [0xfb, 2, 4, 5] {
        let global = v2_global(1, 1).without_record(ty, &[]);
        assert!(Psbt::from_maps(global, vec![input()], vec![output()], Limits::default()).is_err());
    }
    for ty in [0x0e, 0x0f] {
        let missing = input().without_record(ty, &[]);
        let error = Psbt::from_maps(
            v2_global(1, 1),
            vec![missing],
            vec![output()],
            Limits::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::MissingField(ty));
        assert_eq!(error.scope, Scope::Input(0));
    }
    for ty in [3, 4] {
        let missing = output().without_record(ty, &[]);
        let error = Psbt::from_maps(
            v2_global(1, 1),
            vec![input()],
            vec![missing],
            Limits::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::MissingField(ty));
        assert_eq!(error.scope, Scope::Output(0));
    }
    let zero = v0_zero();
    let explicit_zero = zero
        .global()
        .with_record(record(0xfb, &[], &0u32.to_le_bytes()));
    let explicit_zero = Psbt::from_maps(explicit_zero, vec![], vec![], Limits::default()).unwrap();
    assert_eq!(explicit_zero.version(), Version::V0);
    assert_ne!(
        explicit_zero.serialize().unwrap(),
        zero.serialize().unwrap()
    );
    assert_eq!(
        Psbt::parse(&explicit_zero.serialize().unwrap(), Limits::default()).unwrap(),
        explicit_zero
    );
    let mismatch = Psbt::from_maps(
        v2_global(2, 1),
        vec![input()],
        vec![output()],
        Limits::default(),
    )
    .unwrap_err();
    assert_eq!(mismatch.kind, ErrorKind::MapCountMismatch);
    let mut trailing = v2().serialize().unwrap();
    trailing.push(0);
    assert_eq!(
        Psbt::parse(&trailing, Limits::default()).unwrap_err().kind,
        ErrorKind::TrailingData
    );
}

#[test]
fn configurable_limits_are_enforced_during_parse_and_construction() {
    let base = v2();
    let raw = base.serialize().unwrap();
    let exact = Limits {
        max_bytes: raw.len(),
        max_maps: 3,
        max_records: 8,
        max_records_per_map: 4,
        max_key_bytes: 1,
        max_value_bytes: 32,
    };
    Psbt::parse(&raw, exact).unwrap();
    Psbt::from_maps(
        base.global().clone(),
        base.inputs().to_vec(),
        base.outputs().to_vec(),
        exact,
    )
    .unwrap();
    for (limits, resource) in [
        (
            Limits {
                max_bytes: raw.len() - 1,
                ..exact
            },
            Limit::Bytes,
        ),
        (
            Limits {
                max_maps: 2,
                ..exact
            },
            Limit::Maps,
        ),
        (
            Limits {
                max_records: 7,
                ..exact
            },
            Limit::Records,
        ),
        (
            Limits {
                max_records_per_map: 3,
                ..exact
            },
            Limit::RecordsPerMap,
        ),
        (
            Limits {
                max_key_bytes: 0,
                ..exact
            },
            Limit::KeyBytes,
        ),
        (
            Limits {
                max_value_bytes: 31,
                ..exact
            },
            Limit::ValueBytes,
        ),
    ] {
        assert_eq!(
            Psbt::parse(&raw, limits).unwrap_err().kind,
            ErrorKind::LimitExceeded(resource)
        );
        assert_eq!(
            Psbt::from_maps(
                base.global().clone(),
                base.inputs().to_vec(),
                base.outputs().to_vec(),
                limits
            )
            .unwrap_err()
            .kind,
            ErrorKind::LimitExceeded(resource)
        );
    }
    let global = v2_global(1, 1).with_record(record(4, &[], &hex("ffffffffffffffffff")));
    assert!(Psbt::parse(&encode_unchecked(&global, &[], &[]), Limits::default()).is_err());
    let mut huge = MAGIC.to_vec();
    huge.extend_from_slice(&hex("ffffffffffffffffff"));
    assert!(matches!(
        Psbt::parse(&huge, Limits::default()).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::KeyBytes)
    ));
    let mut huge = MAGIC.to_vec();
    huge.extend_from_slice(&hex("0100ffffffffffffffffff"));
    assert!(matches!(
        Psbt::parse(&huge, Limits::default()).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::ValueBytes)
    ));
}

#[test]
fn truncation_at_every_offset_and_mutation_roundtrips() {
    // All proper prefixes of canonical synthetic containers must fail framing.
    for raw in [v0_zero().serialize().unwrap(), v2().serialize().unwrap()] {
        for length in 0..raw.len() {
            assert!(
                Psbt::parse(&raw[..length], Limits::default()).is_err(),
                "accepted {length}/{} bytes",
                raw.len()
            );
        }
        // Bounded adversarial mutations may be valid (unknown metadata); if so,
        // the accepted bytes must round-trip exactly. No malformed input panics.
        for index in 0..raw.len() {
            for byte in [0, 1, 0x7f, 0xfc, 0xfd, 0xfe, 0xff] {
                let mut mutated = raw.clone();
                mutated[index] = byte;
                if let Ok(parsed) = Psbt::parse(&mutated, Limits::default()) {
                    assert_eq!(parsed.serialize().unwrap(), mutated);
                }
            }
        }
    }
}

#[test]
fn text_transport_trim_hex_case_and_strict_base64() {
    let psbt = v2();
    let encoded = psbt.to_base64().unwrap();
    assert_eq!(
        Psbt::parse_text(&format!(" \r\n\t{encoded}\t\r\n "), Limits::default()).unwrap(),
        psbt
    );
    assert!(Psbt::from_base64(&format!(" {encoded}"), Limits::default()).is_err());
    let raw = psbt.serialize().unwrap();
    let upper: String = raw.iter().map(|byte| format!("{byte:02X}")).collect();
    assert_eq!(Psbt::parse_text(&upper, Limits::default()).unwrap(), psbt);
    assert!(Psbt::parse_text(&(upper + "0"), Limits::default()).is_err());
    let mut wrapped = encoded.clone();
    wrapped.insert(4, '\n');
    assert!(Psbt::from_base64(&wrapped, Limits::default()).is_err());
    assert!(
        Psbt::from_base64(
            &encoded,
            Limits {
                max_bytes: raw.len() - 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn error_scopes_and_offsets_do_not_confuse_field_local_positions() {
    let error = Psbt::parse(MAGIC, Limits::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnexpectedEnd);
    assert_eq!(error.scope, Scope::Global);
    assert_eq!(error.offset, Some(5));
    let error = record(0xfc, &[], &[]).field(Scope::Output(7)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnexpectedEnd);
    assert_eq!(error.scope, Scope::Output(7));
    assert_eq!(error.offset, None);
    let mut malformed_type = v0_zero().serialize().unwrap();
    malformed_type.splice(5..7, hex("03fd0000"));
    let error = Psbt::parse(&malformed_type, Limits::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NonCanonicalCompactSize);
    assert_eq!(error.offset, Some(6));
}

#[test]
fn large_compact_size_key_and_value_boundaries_roundtrip() {
    let limits = Limits {
        max_bytes: 300_000,
        max_key_bytes: 70_000,
        max_value_bytes: 70_000,
        ..Limits::default()
    };
    for key_len in [1, 252, 253, 65_535, 65_536] {
        for value_len in [0, 252, 253, 65_535, 65_536] {
            let key_data = vec![0x73; key_len - 1];
            let value = vec![0x80; value_len];
            let global = v2_global(0, 0).with_record(record(0xef, &key_data, &value));
            let psbt = Psbt::from_maps(global, vec![], vec![], limits).unwrap();
            let bytes = psbt.serialize().unwrap();
            assert_eq!(bytes, encode_unchecked(psbt.global(), &[], &[]));
            assert_eq!(
                Psbt::parse(&bytes, limits).unwrap().serialize().unwrap(),
                bytes
            );
        }
    }
    let mut proprietary = vec![0]; // empty identifier
    proprietary.extend_from_slice(&hex("ffffffffffffffffff"));
    assert!(
        matches!(record(0xfc, &proprietary, &[]).field(Scope::Global).unwrap(), Field::Proprietary(field) if field.subtype == u64::MAX)
    );
}

#[test]
fn bounded_deterministic_noise_does_not_panic_or_normalize_accepted_bytes() {
    let limits = Limits {
        max_bytes: 512,
        max_maps: 20,
        max_records: 20,
        max_records_per_map: 10,
        max_key_bytes: 256,
        max_value_bytes: 256,
    };
    let mut rng = 0x6d63_772d_7073_6274u64;
    for iteration in 0..10_000 {
        let length = iteration % 257;
        let mut bytes = if iteration % 2 == 0 {
            MAGIC.to_vec()
        } else {
            vec![]
        };
        for _ in 0..length {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            bytes.push((rng >> 32) as u8);
        }
        if let Ok(parsed) = Psbt::parse(&bytes, limits) {
            assert_eq!(parsed.serialize().unwrap(), bytes);
        }
    }
}
