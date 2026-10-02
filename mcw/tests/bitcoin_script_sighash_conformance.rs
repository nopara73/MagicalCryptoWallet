use mcw::bitcoin_encoding::{hex_decode, hex_encode};
use mcw::bitcoin_script;
use mcw::bitcoin_wire::{Limits, OutPoint, Transaction, TxIn, TxOut};
use mcw::script_service::sighash::*;

fn transaction(hex: &str) -> Transaction {
    Transaction::decode(&hex_decode(hex).unwrap(), &Limits::default()).unwrap()
}
fn spent_outputs(text: &str) -> Vec<TxOut> {
    text.split(';')
        .map(|item| {
            let (amount, script) = item.split_once(':').unwrap();
            TxOut {
                value: amount.parse().unwrap(),
                script_pubkey: hex_decode(script).unwrap(),
            }
        })
        .collect()
}

fn synthetic() -> (Transaction, Vec<TxOut>) {
    let inputs = (0..3)
        .map(|index| TxIn {
            previous_output: OutPoint {
                txid: [index as u8 + 1; 32],
                vout: index,
            },
            script_sig: vec![],
            witness: vec![],
            sequence: 0xffff_fffd,
        })
        .collect();
    let outputs = vec![
        TxOut {
            value: 50_000,
            script_pubkey: bitcoin_script::p2wpkh(&[0x42; 20]).into_bytes(),
        },
        TxOut {
            value: 100_000,
            script_pubkey: bitcoin_script::p2tr(&[0x43; 32]).into_bytes(),
        },
    ];
    let spent = (0..3)
        .map(|_| TxOut {
            value: 60_000,
            script_pubkey: bitcoin_script::p2tr(&[0x44; 32]).into_bytes(),
        })
        .collect();
    (
        Transaction {
            version: 2,
            inputs,
            outputs,
            lock_time: 100,
        },
        spent,
    )
}

#[test]
fn core_legacy_sighash_reference_vectors() {
    let mut count = 0;
    for row in include_str!("bitcoin_script_fixtures/legacy_sighash.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        let tx = transaction(fields[1]);
        let code = hex_decode(fields[2]).unwrap();
        let digest = legacy_sighash(
            &tx,
            fields[3].parse().unwrap(),
            &code,
            fields[4].parse().unwrap(),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(
            hex_encode(&digest).unwrap(),
            fields[5],
            "legacy vector {}",
            fields[0]
        );
        count += 1;
    }
    assert_eq!(count, 500);
}

#[test]
fn bip143_published_preimage_hashes() {
    let mut count = 0;
    for row in include_str!("bitcoin_script_fixtures/segwit_sighash.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        let tx = transaction(fields[0]);
        let code = hex_decode(fields[1]).unwrap();
        let index = fields[2].parse().unwrap();
        let amount = fields[3].parse().unwrap();
        let hash_type = fields[4].parse().unwrap();
        let digest =
            segwit_v0_sighash(&tx, index, &code, amount, hash_type, &Limits::default()).unwrap();
        let cache = SighashCache::new(&tx, &Limits::default()).unwrap();
        assert_eq!(
            digest,
            cache.segwit_v0(index, &code, amount, hash_type).unwrap()
        );
        assert_eq!(hex_encode(&digest).unwrap(), fields[5]);
        count += 1;
    }
    assert_eq!(count, 10);
}

#[test]
fn bip341_published_sigmsg_and_digest_vectors() {
    let mut count = 0;
    for row in include_str!("bitcoin_script_fixtures/taproot_sighash.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        let tx = transaction(fields[0]);
        let spent = spent_outputs(fields[1]);
        let index = fields[2].parse().unwrap();
        let hash_type = fields[3].parse().unwrap();
        let cache = SighashCache::with_spent_outputs(&tx, &spent, &Limits::default()).unwrap();
        let message = cache.taproot_message(index, hash_type, None, None).unwrap();
        assert_eq!(hex_encode(&message).unwrap(), fields[4]);
        assert_eq!(
            taproot_sighash_message(
                &tx,
                &spent,
                index,
                hash_type,
                None,
                None,
                &Limits::default()
            )
            .unwrap(),
            message
        );
        let digest = cache.taproot(index, hash_type, None, None).unwrap();
        assert_eq!(hex_encode(&digest).unwrap(), fields[5]);
        assert_eq!(
            taproot_sighash(
                &tx,
                &spent,
                index,
                hash_type,
                None,
                None,
                &Limits::default()
            )
            .unwrap(),
            digest
        );
        count += 1;
    }
    assert_eq!(count, 7);
}

#[test]
fn separator_and_signature_deletion_are_instruction_scoped() {
    let code = hex_decode("02ab42ab514c01ab").unwrap();
    assert_eq!(
        hex_encode(&without_code_separators(&code).unwrap()).unwrap(),
        "02ab42514c01ab"
    );
    let signature = [0x42, 0x43];
    let script = hex_decode("02424304024243000242434c024243").unwrap();
    let (without, removed) = find_and_delete_signature(&script, &signature).unwrap();
    assert_eq!(removed, 2);
    assert_eq!(hex_encode(&without).unwrap(), "04024243004c024243");
    assert!(without_code_separators(&[0x4c]).is_err());
    assert!(find_and_delete_signature(&[0x4c], &signature).is_err());
}

#[test]
fn single_missing_output_differs_across_signature_versions() {
    let (tx, spent) = synthetic();
    let legacy = legacy_sighash(&tx, 2, &[], 3, &Limits::default()).unwrap();
    assert_eq!(legacy[0], 1);
    assert_eq!(legacy[1..], [0; 31]);
    assert_ne!(
        segwit_v0_sighash(&tx, 2, &[], 60_000, 3, &Limits::default()).unwrap(),
        legacy
    );
    assert!(matches!(
        taproot_sighash(&tx, &spent, 2, 3, None, None, &Limits::default()),
        Err(Error::MissingSingleOutput { .. })
    ));
    assert!(legacy_sighash(&tx, 3, &[], 3, &Limits::default()).is_err());
}

#[test]
fn signatures_commit_to_amount_annex_extension_and_required_inputs() {
    let (tx, spent) = synthetic();
    let code = bitcoin_script::p2pkh(&[0x42; 20]).into_bytes();
    let digest = segwit_v0_sighash(&tx, 0, &code, 60_000, 1, &Limits::default()).unwrap();
    assert_ne!(
        digest,
        segwit_v0_sighash(&tx, 0, &code, 60_001, 1, &Limits::default()).unwrap()
    );
    let digest = taproot_sighash(&tx, &spent, 0, 0, None, None, &Limits::default()).unwrap();
    let annex = [0x50, 0x42];
    assert_ne!(
        digest,
        taproot_sighash(&tx, &spent, 0, 0, Some(&annex), None, &Limits::default()).unwrap()
    );
    let extension = TapScriptExtension {
        tapleaf_hash: [0x42; 32],
        key_version: 0,
        codesep_position: u32::MAX,
    };
    let extended =
        taproot_sighash(&tx, &spent, 0, 0, None, Some(extension), &Limits::default()).unwrap();
    assert_ne!(digest, extended);
    assert_ne!(
        extended,
        taproot_sighash(
            &tx,
            &spent,
            0,
            0,
            None,
            Some(TapScriptExtension {
                codesep_position: 0,
                ..extension
            }),
            &Limits::default()
        )
        .unwrap()
    );
    let mut changed_spent = spent.clone();
    changed_spent[1].value += 1;
    assert_ne!(
        digest,
        taproot_sighash(&tx, &changed_spent, 0, 0, None, None, &Limits::default()).unwrap()
    );
    let anyone = taproot_sighash(&tx, &spent, 0, 0x81, None, None, &Limits::default()).unwrap();
    assert_eq!(
        anyone,
        taproot_sighash(&tx, &changed_spent, 0, 0x81, None, None, &Limits::default()).unwrap()
    );
    let mut changed_tx = tx.clone();
    changed_tx.inputs[1].sequence -= 1;
    for hash_type in [1, 2, 3, 0x81, 0x82, 0x83] {
        let before =
            segwit_v0_sighash(&tx, 0, &code, 60_000, hash_type, &Limits::default()).unwrap();
        let after = segwit_v0_sighash(&changed_tx, 0, &code, 60_000, hash_type, &Limits::default())
            .unwrap();
        assert_eq!(before == after, hash_type != 1);
    }
}

#[test]
fn invalid_flags_annex_binding_counts_and_resource_limits_fail_explicitly() {
    let (tx, spent) = synthetic();
    for hash_type in [4, 0x80, 0x84, 0xff] {
        assert_eq!(
            taproot_sighash(&tx, &spent, 0, hash_type, None, None, &Limits::default()),
            Err(Error::InvalidTaprootHashType { hash_type })
        );
    }
    assert!(taproot_sighash(&tx, &spent[..2], 0, 0, None, None, &Limits::default()).is_err());
    assert_eq!(
        SighashCache::new(&tx, &Limits::default())
            .unwrap()
            .taproot(0, 0, None, None),
        Err(Error::MissingSpentOutputs)
    );
    assert!(taproot_sighash(&tx, &spent, 0, 0, Some(&[]), None, &Limits::default()).is_err());
    assert!(taproot_sighash(&tx, &spent, 0, 0, Some(&[0x51]), None, &Limits::default()).is_err());
    assert!(
        taproot_sighash(
            &tx,
            &spent,
            0,
            0,
            None,
            Some(TapScriptExtension {
                tapleaf_hash: [0; 32],
                key_version: 1,
                codesep_position: 0
            }),
            &Limits::default()
        )
        .is_err()
    );
    let small = Limits {
        max_transaction_bytes: 10,
        ..Limits::default()
    };
    assert!(legacy_sighash(&tx, 0, &[], 1, &small).is_err());
    assert!(segwit_v0_sighash(&tx, 0, &[], 60_000, 1, &small).is_err());
    let small_spent = Limits {
        max_payload_bytes: 10,
        ..Limits::default()
    };
    assert!(taproot_sighash(&tx, &spent, 0, 0, None, None, &small_spent).is_err());
}

#[test]
fn taproot_leaf_branch_tweak_hashes_preserve_raw_bytes_and_order() {
    let script = [0x51, 0xab, 0x4c];
    let leaf = tapleaf_hash(&script, 0xc0).unwrap();
    assert_ne!(leaf, tapleaf_hash(&script[..2], 0xc0).unwrap());
    assert!(tapleaf_hash(&script, 0xc1).is_err());
    let other = [0x42; 32];
    assert_eq!(
        tapbranch_hash(&leaf, &other).unwrap(),
        tapbranch_hash(&other, &leaf).unwrap()
    );
    assert_ne!(
        taptweak_hash(&other, None).unwrap(),
        taptweak_hash(&other, Some(&[0; 32])).unwrap()
    );
    // BIP341 first published internal-key/tweak fixture, no point operation here.
    let internal: [u8; 32] =
        hex_decode("d6889cb081036e0faefa3a35157ad71086b123b2b144b649798b494c300a961d")
            .unwrap()
            .try_into()
            .unwrap();
    assert_eq!(
        hex_encode(&taptweak_hash(&internal, None).unwrap()).unwrap(),
        "b86e7be8f39bab32a6f2c0443abbc210f0edac0e2c53d501b36b64437d9c6c70"
    );
}
