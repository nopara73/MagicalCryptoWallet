//! Integration conformance suite; also runnable through an ignored rustc harness.
use mcw::bitcoin_encoding;
use mcw::bitcoin_wire::{
    self as wire, DecodeMode, Error, Limits, OutPoint, Resource, Transaction, TxId, TxIn, TxOut,
};

fn unhex(text: &str) -> Vec<u8> {
    bitcoin_encoding::hex_decode(text).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    bitcoin_encoding::hex_encode(bytes).unwrap()
}

fn example(witness: bool) -> Transaction {
    Transaction {
        version: -2,
        inputs: vec![
            TxIn {
                previous_output: OutPoint {
                    txid: [0x31; 32],
                    vout: 7,
                },
                script_sig: vec![0xff, 0x4c, 0, 0],
                sequence: 0x8040_0001,
                witness: if witness {
                    vec![vec![], vec![0, 0xff, 0x50]]
                } else {
                    vec![]
                },
            },
            TxIn {
                previous_output: OutPoint {
                    txid: [0x82; 32],
                    vout: u32::MAX,
                },
                script_sig: vec![],
                sequence: u32::MAX,
                witness: vec![],
            },
        ],
        outputs: vec![
            TxOut {
                value: -1,
                script_pubkey: vec![0, 0xff, 0x4d],
            },
            TxOut {
                value: i64::MAX,
                script_pubkey: vec![],
            },
        ],
        lock_time: 0xf123_4567,
    }
}

fn empty() -> Transaction {
    Transaction {
        version: 2,
        inputs: vec![],
        outputs: vec![],
        lock_time: 0,
    }
}

fn assert_limit(result: Result<impl std::fmt::Debug, Error>, resource: Resource) {
    assert!(
        matches!(result, Err(Error::LimitExceeded { resource: actual, .. }) if actual == resource),
        "{result:?}"
    );
}

#[test]
fn compactsize_published_boundaries_are_exact() {
    // Bitcoin Core serialize_tests.cpp compactsize; includes all encoding widths.
    for (value, encoded) in [
        (0, "00"),
        (1, "01"),
        (252, "fc"),
        (253, "fdfd00"),
        (254, "fdfe00"),
        (65535, "fdffff"),
        (65536, "fe00000100"),
        (0xffff_ffff, "feffffffff"),
        (0x1_0000_0000, "ff0000000001000000"),
        (u64::MAX, "ffffffffffffffffff"),
    ] {
        let mut bytes = vec![];
        wire::write_compact_size(value, &mut bytes).unwrap();
        assert_eq!(hex(&bytes), encoded);
        assert_eq!(bytes.len(), wire::compact_size_len(value));
        assert_eq!(
            wire::decode_compact_size(&bytes).unwrap(),
            (value, bytes.len())
        );
        let mut appended = bytes.clone();
        appended.extend_from_slice(&[8, 9]);
        assert_eq!(
            wire::decode_compact_size(&appended).unwrap(),
            (value, bytes.len())
        );
        for len in 0..bytes.len() {
            assert!(matches!(
                wire::decode_compact_size(&bytes[..len]),
                Err(Error::UnexpectedEnd { .. })
            ));
        }
    }
    let mut existing = vec![0x7a];
    wire::write_compact_size(253, &mut existing).unwrap();
    assert_eq!(existing, [0x7a, 0xfd, 0xfd, 0]);
}

#[test]
fn compactsize_nonminimal_forms_rejected() {
    for text in [
        "fd0000",
        "fdfc00",
        "fe00000000",
        "feffff0000",
        "ff0000000000000000",
        "ffffffffff00000000",
    ] {
        assert_eq!(
            wire::decode_compact_size(&unhex(text)),
            Err(Error::NonCanonicalCompactSize { offset: 0 })
        );
    }
}

fn fields(tx: &Transaction) -> String {
    let mut lines = vec![(tx.version as u32).to_string(), tx.lock_time.to_string()];
    for input in &tx.inputs {
        let witness = input
            .witness
            .iter()
            .map(|item| hex(item))
            .collect::<Vec<_>>()
            .join(",");
        lines.push(format!(
            "i:{}:{}:{}:{}:{}",
            hex(&input.previous_output.txid),
            input.previous_output.vout,
            input.sequence,
            hex(&input.script_sig),
            witness
        ));
    }
    for output in &tx.outputs {
        lines.push(format!("o:{}:{}", output.value, hex(&output.script_pubkey)));
    }
    lines.join("\n")
}

#[test]
fn bitcoin_core_and_retained_fixtures_exact_fields_hashes_sizes() {
    let limits = Limits::default();
    let mut counts = [0; 4];
    for row in include_str!("bitcoin_wire_fixtures/reference.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let columns = row.split('\t').collect::<Vec<_>>();
        assert_eq!(columns.len(), 13);
        let name = columns[0];
        let raw = unhex(columns[2]);
        let tx = if columns[1] == "legacy" {
            Transaction::decode_legacy(&raw, &limits)
        } else {
            Transaction::decode(&raw, &limits)
        }
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(tx.serialize(&limits).unwrap(), raw, "{name}: wire");
        assert_eq!(
            hex(&tx.serialize_legacy(&limits).unwrap()),
            columns[3],
            "{name}: stripped"
        );
        assert_eq!(
            tx.txid(&limits).unwrap().to_string(),
            columns[4],
            "{name}: txid"
        );
        assert_eq!(
            tx.wtxid(&limits).unwrap().to_string(),
            columns[5],
            "{name}: wtxid"
        );
        let sizes = tx.sizes(&limits).unwrap();
        assert_eq!(sizes.weight.to_string(), columns[6], "{name}: weight");
        assert_eq!(sizes.virtual_size.to_string(), columns[7], "{name}: vsize");
        assert_eq!(
            (tx.version as u32).to_string(),
            columns[8],
            "{name}: version bits"
        );
        assert_eq!(tx.lock_time.to_string(), columns[9], "{name}: locktime");
        assert_eq!(
            tx.inputs.len().to_string(),
            columns[10],
            "{name}: vin count"
        );
        assert_eq!(
            tx.outputs.len().to_string(),
            columns[11],
            "{name}: vout count"
        );
        let fingerprint = bitcoin_encoding::sha256(fields(&tx).as_bytes()).unwrap();
        assert_eq!(hex(&fingerprint), columns[12], "{name}: every field");
        assert_eq!(sizes.total, raw.len());
        assert_eq!(sizes.stripped, unhex(columns[3]).len());
        let group = if name.starts_with("core-tx_valid") {
            0
        } else if name.starts_with("core-tx_invalid") {
            1
        } else if name.starts_with("retained") {
            2
        } else {
            3
        };
        counts[group] += 1;
    }
    // tx_invalid refers to consensus/script failure, not malformed serialization.
    assert_eq!(counts, [120, 93, 6, 141]);
}

#[test]
fn legacy_and_witness_roundtrips_preserve_opaque_scripts() {
    let limits = Limits::default();
    for witness in [false, true] {
        let tx = example(witness);
        let raw = tx.serialize(&limits).unwrap();
        assert_eq!(Transaction::decode(&raw, &limits).unwrap(), tx);
        let legacy = tx.serialize_legacy(&limits).unwrap();
        let mut stripped = tx.clone();
        for input in &mut stripped.inputs {
            input.witness.clear();
        }
        assert_eq!(
            Transaction::decode_legacy(&legacy, &limits).unwrap(),
            stripped
        );
        assert_eq!(stripped.serialize(&limits).unwrap(), legacy);
        if witness {
            assert_eq!(&raw[4..6], &[0, 1]);
        } else {
            assert_eq!(tx.txid(&limits).unwrap(), tx.wtxid(&limits).unwrap());
        }
    }
}

#[test]
fn hashes_use_stripped_and_witness_preimages_and_raw_digest_order() {
    let limits = Limits::default();
    let tx = example(true);
    let txid = tx.txid(&limits).unwrap();
    let wtxid = tx.wtxid(&limits).unwrap();
    assert_ne!(txid, wtxid);
    assert_eq!(
        txid.0,
        bitcoin_encoding::double_sha256(&tx.serialize_legacy(&limits).unwrap()).unwrap()
    );
    assert_eq!(
        wtxid.0,
        bitcoin_encoding::double_sha256(&tx.serialize(&limits).unwrap()).unwrap()
    );
    let mut modified = tx.clone();
    modified.inputs[0].witness[1].push(5);
    assert_eq!(modified.txid(&limits).unwrap(), txid);
    assert_ne!(modified.wtxid(&limits).unwrap(), wtxid);
    modified.inputs[0].script_sig.push(8);
    assert_ne!(modified.txid(&limits).unwrap(), txid);
    let raw = std::array::from_fn::<u8, 32, _>(|index| index as u8);
    let mut reversed = raw;
    reversed.reverse();
    assert_eq!(TxId(raw).to_string(), hex(&reversed));
    let mut child = example(false);
    child.inputs[0].previous_output.txid = txid.0;
    assert_eq!(&child.serialize(&limits).unwrap()[5..37], &txid.0);
}

#[test]
fn empty_stack_and_stack_with_empty_element_differ() {
    let limits = Limits::default();
    let mut tx = example(false);
    assert!(!tx.has_witness());
    tx.inputs[0].witness = vec![vec![]];
    assert!(tx.has_witness());
    let raw = tx.serialize(&limits).unwrap();
    assert_eq!(&raw[4..6], &[0, 1]);
    assert_eq!(Transaction::decode(&raw, &limits).unwrap(), tx);
    let sizes = tx.sizes(&limits).unwrap();
    // marker+flag, first stack count+empty length, second stack count.
    assert_eq!(sizes.total - sizes.stripped, 5);
    assert_eq!(sizes.weight, sizes.stripped * 4 + 5);
    assert_eq!(sizes.virtual_size, sizes.stripped + 2);
}

#[test]
fn signed_amounts_version_and_unsigned_fields_preserved() {
    let limits = Limits::default();
    for value in [i64::MIN, -2, -1, 0, 1, 2_100_000_000_000_001, i64::MAX] {
        for version in [i32::MIN, -1, 0, 1, 2, i32::MAX] {
            let mut tx = example(false);
            tx.version = version;
            tx.outputs[0].value = value;
            tx.lock_time = u32::MAX;
            let bytes = tx.serialize(&limits).unwrap();
            assert_eq!(Transaction::decode(&bytes, &limits).unwrap(), tx);
        }
    }
}

#[test]
fn empty_and_zero_input_transactions_use_explicit_legacy_mode() {
    let limits = Limits::default();
    let tx = empty();
    let bytes = unhex("02000000000000000000");
    assert_eq!(tx.serialize(&limits).unwrap(), bytes);
    assert_eq!(Transaction::decode(&bytes, &limits).unwrap(), tx);
    assert_eq!(Transaction::decode_legacy(&bytes, &limits).unwrap(), tx);
    let mut unsigned = tx.clone();
    unsigned.outputs.push(TxOut {
        value: 1,
        script_pubkey: vec![],
    });
    let raw = unsigned.serialize_legacy(&limits).unwrap();
    assert_eq!(Transaction::decode_legacy(&raw, &limits).unwrap(), unsigned);
    assert!(Transaction::decode(&raw, &limits).is_err());
    unsigned.outputs.push(TxOut {
        value: 2,
        script_pubkey: vec![],
    });
    let raw = unsigned.serialize_legacy(&limits).unwrap();
    assert!(matches!(
        Transaction::decode(&raw, &limits),
        Err(Error::UnknownWitnessFlags { flags: 2, .. })
    ));
    assert_eq!(Transaction::decode_legacy(&raw, &limits).unwrap(), unsigned);
}

#[test]
fn unknown_optional_flags_fail_without_guessing() {
    let limits = Limits::default();
    let mut raw = example(true).serialize(&limits).unwrap();
    for flags in 2..=255 {
        raw[5] = flags;
        assert_eq!(
            Transaction::decode(&raw, &limits),
            Err(Error::UnknownWitnessFlags { offset: 5, flags })
        );
    }
}

#[test]
fn superfluous_witness_encoding_rejected() {
    let limits = Limits::default();
    let legacy = example(false).serialize(&limits).unwrap();
    let mut bytes = legacy[..4].to_vec();
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&legacy[4..legacy.len() - 4]);
    bytes.extend_from_slice(&[0, 0]); // two input stacks; both empty
    bytes.extend_from_slice(&legacy[legacy.len() - 4..]);
    assert_eq!(
        Transaction::decode(&bytes, &limits),
        Err(Error::SuperfluousWitness)
    );
    assert_eq!(
        Transaction::decode(&unhex("020000000001000000000000"), &limits),
        Err(Error::SuperfluousWitness)
    );
}

#[test]
fn every_truncation_fails_and_trailing_bytes_require_prefix_api() {
    let limits = Limits::default();
    for tx in [empty(), example(false), example(true)] {
        let raw = tx.serialize(&limits).unwrap();
        for length in 0..raw.len() {
            assert!(
                Transaction::decode(&raw[..length], &limits).is_err(),
                "truncation length {length}"
            );
        }
        let mut stream = raw.clone();
        stream.extend_from_slice(&[0xaa, 0xbb]);
        assert_eq!(
            Transaction::decode(&stream, &limits),
            Err(Error::TrailingBytes {
                offset: raw.len(),
                remaining: 2
            })
        );
        assert_eq!(
            Transaction::decode_prefix(&stream, &limits, DecodeMode::Witness).unwrap(),
            (tx, raw.len())
        );
    }
}

#[test]
fn compactsize_is_canonical_in_every_transaction_container() {
    let limits = Limits::default();
    let legacy = example(false).serialize(&limits).unwrap();
    // Input count at 4; first script length at 41; output count after both inputs.
    let output_offset = 4 + 1 + 41 + 4 + 41;
    for offset in [4, 41, output_offset, output_offset + 1 + 8] {
        let value = legacy[offset];
        assert!(value < 0xfd);
        let mut bad = legacy[..offset].to_vec();
        bad.extend_from_slice(&[0xfd, value, 0]);
        bad.extend_from_slice(&legacy[offset + 1..]);
        assert_eq!(
            Transaction::decode(&bad, &limits),
            Err(Error::NonCanonicalCompactSize { offset })
        );
    }
    let witnessed = example(true).serialize(&limits).unwrap();
    let start = witnessed.len() - 4 - 1 - (1 + 1 + 1 + 3);
    for offset in [start, start + 1, start + 2] {
        let value = witnessed[offset];
        let mut bad = witnessed[..offset].to_vec();
        bad.extend_from_slice(&[0xfd, value, 0]);
        bad.extend_from_slice(&witnessed[offset + 1..]);
        assert_eq!(
            Transaction::decode(&bad, &limits),
            Err(Error::NonCanonicalCompactSize { offset })
        );
    }
}

#[test]
fn individual_and_aggregate_limits_apply_to_decoding_and_encoding() {
    let tx = example(true);
    let original = Limits::default();
    let raw = tx.serialize(&original).unwrap();
    for (resource, limits) in [
        (
            Resource::TransactionBytes,
            Limits {
                max_transaction_bytes: raw.len() - 1,
                ..original
            },
        ),
        (
            Resource::Inputs,
            Limits {
                max_inputs: 1,
                ..original
            },
        ),
        (
            Resource::Outputs,
            Limits {
                max_outputs: 1,
                ..original
            },
        ),
        (
            Resource::ScriptBytes,
            Limits {
                max_script_bytes: 3,
                ..original
            },
        ),
        (
            Resource::WitnessItemsPerInput,
            Limits {
                max_witness_items_per_input: 1,
                ..original
            },
        ),
        (
            Resource::TotalWitnessItems,
            Limits {
                max_total_witness_items: 1,
                ..original
            },
        ),
        (
            Resource::WitnessItemBytes,
            Limits {
                max_witness_item_bytes: 2,
                ..original
            },
        ),
        (
            Resource::PayloadBytes,
            Limits {
                max_payload_bytes: 9,
                ..original
            },
        ),
        (
            Resource::DecodedBytes,
            Limits {
                max_decoded_bytes: 0,
                ..original
            },
        ),
    ] {
        assert_limit(Transaction::decode(&raw, &limits), resource);
        assert_limit(tx.serialize(&limits), resource);
        assert_limit(tx.txid(&limits), resource);
        assert_limit(tx.wtxid(&limits), resource);
    }
    let tight = Limits {
        max_transaction_bytes: raw.len(),
        max_inputs: 2,
        max_outputs: 2,
        max_script_bytes: 4,
        max_witness_items_per_input: 2,
        max_total_witness_items: 2,
        max_witness_item_bytes: 3,
        max_payload_bytes: 10,
        ..original
    };
    assert_eq!(tx.serialize(&tight).unwrap(), raw);
    assert_eq!(Transaction::decode(&raw, &tight).unwrap(), tx);
}

#[test]
fn allocation_budget_includes_empty_witness_metadata() {
    let tx = example(true);
    let mut limits = Limits::default();
    let raw = tx.serialize(&limits).unwrap();
    let needed = std::mem::size_of::<Transaction>()
        + tx.inputs.len() * std::mem::size_of::<TxIn>()
        + tx.outputs.len() * std::mem::size_of::<TxOut>()
        + 2 * std::mem::size_of::<Vec<u8>>()
        + 10;
    limits.max_decoded_bytes = needed;
    assert_eq!(Transaction::decode(&raw, &limits).unwrap(), tx);
    assert_eq!(tx.serialize(&limits).unwrap(), raw);
    limits.max_decoded_bytes -= 1;
    assert_limit(Transaction::decode(&raw, &limits), Resource::DecodedBytes);
    assert_limit(tx.serialize(&limits), Resource::DecodedBytes);
}

#[test]
fn aggregate_witness_item_limit_spans_all_inputs() {
    let mut tx = example(true);
    tx.inputs[1].witness = vec![vec![]];
    let defaults = Limits::default();
    let raw = tx.serialize(&defaults).unwrap();
    let limits = Limits {
        max_total_witness_items: 2,
        ..defaults
    };
    assert_limit(
        Transaction::decode(&raw, &limits),
        Resource::TotalWitnessItems,
    );
    assert_limit(tx.serialize(&limits), Resource::TotalWitnessItems);
}

#[test]
fn prefix_limit_applies_only_to_consumed_transaction() {
    let tx = example(true);
    let raw = tx.serialize(&Limits::default()).unwrap();
    let limits = Limits {
        max_transaction_bytes: raw.len(),
        ..Limits::default()
    };
    let mut block = raw.clone();
    block.extend_from_slice(&raw);
    assert_limit(
        Transaction::decode(&block, &limits),
        Resource::TransactionBytes,
    );
    assert_eq!(
        Transaction::decode_prefix(&block, &limits, DecodeMode::Witness).unwrap(),
        (tx, raw.len())
    );
    let short = Limits {
        max_transaction_bytes: raw.len() - 1,
        ..limits
    };
    assert_limit(
        Transaction::decode_prefix(&block, &short, DecodeMode::Witness),
        Resource::TransactionBytes,
    );
}

#[test]
fn byte_budget_preflights_declared_vectors_before_metadata_allocation() {
    let raw = example(false).serialize(&Limits::default()).unwrap();
    let limits = Limits {
        max_transaction_bytes: 10,
        max_decoded_bytes: std::mem::size_of::<Transaction>() + 1,
        ..Limits::default()
    };
    // The input vector cannot fit in the wire budget. Its metadata allocation
    // would also fail, but the impossible wire declaration must fail first.
    assert_limit(
        Transaction::decode_prefix(&raw, &limits, DecodeMode::Witness),
        Resource::TransactionBytes,
    );
}

#[test]
fn default_four_million_byte_boundary_and_hashing_work_exactly() {
    let limits = Limits::default();
    let mut tx = example(false);
    tx.inputs.truncate(1);
    tx.inputs[0].script_sig.clear();
    tx.outputs.truncate(1);
    tx.outputs[0].script_pubkey = vec![0x50; 4_000_000 - 64];
    let raw = tx.serialize(&limits).unwrap();
    assert_eq!(raw.len(), 4_000_000);
    assert_eq!(Transaction::decode(&raw, &limits).unwrap(), tx);
    assert_eq!(tx.txid(&limits).unwrap(), tx.wtxid(&limits).unwrap());
    tx.outputs[0].script_pubkey.push(0x51);
    assert_limit(tx.serialize(&limits), Resource::TransactionBytes);
    let mut too_long = raw;
    too_long.push(0);
    assert_limit(
        Transaction::decode(&too_long, &limits),
        Resource::TransactionBytes,
    );
}

#[test]
fn huge_declared_lengths_fail_before_allocating() {
    let limits = Limits::default();
    let mut inputs = vec![2, 0, 0, 0];
    wire::write_compact_size(u64::MAX, &mut inputs).unwrap();
    assert_limit(Transaction::decode(&inputs, &limits), Resource::Inputs);
    let mut impossible = vec![2, 0, 0, 0, 0xfd, 0xff, 0xff];
    assert!(matches!(
        Transaction::decode(&impossible, &limits),
        Err(Error::UnexpectedEnd { .. })
    ));
    impossible[4..].copy_from_slice(&[0xfd, 0, 1]);
    assert!(matches!(
        Transaction::decode(&impossible, &limits),
        Err(Error::UnexpectedEnd { .. })
    ));
    let mut output = vec![0; 8];
    wire::write_compact_size(u64::MAX, &mut output).unwrap();
    assert_limit(wire::decode_output(&output, &limits), Resource::ScriptBytes);
    let mut witness = vec![];
    wire::write_compact_size(u64::MAX, &mut witness).unwrap();
    assert_limit(
        wire::decode_witness(&witness, &limits),
        Resource::WitnessItemsPerInput,
    );
    let mut item = vec![1];
    wire::write_compact_size(u64::MAX, &mut item).unwrap();
    assert_limit(
        wire::decode_witness(&item, &limits),
        Resource::WitnessItemBytes,
    );
}

#[test]
fn standalone_txout_and_final_scriptwitness_are_exact_and_bounded() {
    let limits = Limits::default();
    let output = TxOut {
        value: i64::MIN,
        script_pubkey: vec![0xff, 0x4e, 0, 0],
    };
    let bytes = wire::serialize_output(&output, &limits).unwrap();
    assert_eq!(wire::decode_output(&bytes, &limits).unwrap(), output);
    for length in 0..bytes.len() {
        assert!(wire::decode_output(&bytes[..length], &limits).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(matches!(
        wire::decode_output(&extra, &limits),
        Err(Error::TrailingBytes { .. })
    ));
    for stack in [vec![], vec![vec![]], vec![vec![0, 0xff], vec![], vec![3]]] {
        let bytes = wire::serialize_witness(&stack, &limits).unwrap();
        assert_eq!(wire::decode_witness(&bytes, &limits).unwrap(), stack);
        for length in 0..bytes.len() {
            assert!(wire::decode_witness(&bytes[..length], &limits).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(matches!(
            wire::decode_witness(&extra, &limits),
            Err(Error::TrailingBytes { .. })
        ));
    }
    assert_eq!(
        wire::decode_witness(&[0], &limits).unwrap(),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(
        wire::decode_witness(&unhex("fd0000"), &limits),
        Err(Error::NonCanonicalCompactSize { offset: 0 })
    );
    let script_limit = Limits {
        max_script_bytes: 3,
        ..limits
    };
    assert_limit(
        wire::decode_output(&bytes, &script_limit),
        Resource::ScriptBytes,
    );
    assert_limit(
        wire::serialize_output(&output, &script_limit),
        Resource::ScriptBytes,
    );
    let stack = vec![vec![0; 3]];
    let blob = wire::serialize_witness(&stack, &limits).unwrap();
    let item_limit = Limits {
        max_witness_item_bytes: 2,
        ..limits
    };
    assert_limit(
        wire::decode_witness(&blob, &item_limit),
        Resource::WitnessItemBytes,
    );
    assert_limit(
        wire::serialize_witness(&stack, &item_limit),
        Resource::WitnessItemBytes,
    );
}

#[test]
fn deterministic_random_input_and_mutations_never_panic_or_change_accepted_bytes() {
    let limits = Limits {
        max_transaction_bytes: 512,
        max_inputs: 8,
        max_outputs: 8,
        max_script_bytes: 128,
        max_witness_items_per_input: 8,
        max_total_witness_items: 16,
        max_witness_item_bytes: 128,
        max_payload_bytes: 384,
        max_decoded_bytes: 4096,
    };
    let mut state = 0xfedc_ba98_7654_3210u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..10_000 {
        let length = (next() % 256) as usize;
        let raw = (0..length).map(|_| next() as u8).collect::<Vec<_>>();
        if let Ok(tx) = Transaction::decode(&raw, &limits) {
            assert_eq!(tx.serialize(&limits).unwrap(), raw);
        }
        if let Ok(tx) = Transaction::decode_legacy(&raw, &limits) {
            assert_eq!(tx.serialize_legacy(&limits).unwrap(), raw);
        }
        let _ = wire::decode_output(&raw, &limits);
        let _ = wire::decode_witness(&raw, &limits);
        let _ = wire::decode_compact_size(&raw);
    }
    let original = example(true).serialize(&limits).unwrap();
    for index in 0..original.len() {
        for bit in 0..8 {
            let mut raw = original.clone();
            raw[index] ^= 1 << bit;
            if let Ok(tx) = Transaction::decode(&raw, &limits) {
                assert_eq!(tx.serialize(&limits).unwrap(), raw);
            }
        }
    }
}
