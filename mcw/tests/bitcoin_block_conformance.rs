//! Offline integration tests, also run unchanged through an actual-source rustc harness.
use mcw::bitcoin_block::{
    self as block, Block, BlockHash, BlockHeader, CompactTarget, Error, Limits, MerkleBlock,
    MerkleProof, PartialError, PartialMerkleTree, Resource,
};
use mcw::bitcoin_encoding as enc;
use mcw::bitcoin_wire::{DecodeMode, OutPoint, Transaction, TxId, TxIn, TxOut};
use std::mem::size_of;

fn unhex(text: &str) -> Vec<u8> {
    enc::hex_decode(text).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    enc::hex_encode(bytes).unwrap()
}
fn array(text: &str) -> [u8; 32] {
    let mut hash = [0; 32];
    hash.copy_from_slice(&unhex(text));
    hash
}
fn txids(text: &str) -> Vec<TxId> {
    unhex(text)
        .chunks_exact(32)
        .map(|chunk| {
            let mut hash = [0; 32];
            hash.copy_from_slice(chunk);
            TxId(hash)
        })
        .collect()
}
fn ids(count: usize) -> Vec<TxId> {
    (0..count)
        .map(|i| TxId(enc::double_sha256(&(i as u64).to_le_bytes()).unwrap()))
        .collect()
}
fn header() -> BlockHeader {
    BlockHeader {
        version: i32::MIN,
        previous_block_hash: [0x41; 32],
        merkle_root: [0x82; 32],
        time: u32::MAX,
        bits: 0xff12_3456,
        nonce: 0x8000_0001,
    }
}
fn transaction(witness: bool, index: u32) -> Transaction {
    Transaction {
        version: -2,
        lock_time: index,
        inputs: vec![TxIn {
            previous_output: OutPoint {
                txid: [0x55; 32],
                vout: index,
            },
            script_sig: vec![0, 0xff, 0x4c],
            sequence: 0x8000_0000,
            witness: if witness {
                vec![vec![], vec![0x42, 0, 0xff]]
            } else {
                vec![]
            },
        }],
        outputs: vec![TxOut {
            value: -1,
            script_pubkey: vec![0, 0x4d, 0xff],
        }],
    }
}
fn container(count: usize, witness: bool) -> Block {
    let mut b = Block {
        header: header(),
        transactions: (0..count).map(|i| transaction(witness, i as u32)).collect(),
    };
    b.header.merkle_root = b.merkle_root(&Limits::default()).unwrap().hash;
    b
}
fn assert_limit<T: std::fmt::Debug>(result: Result<T, Error>, resource: Resource) {
    assert!(
        matches!(result, Err(Error::LimitExceeded { resource: actual, .. }) if actual == resource),
        "{result:?}"
    );
}

#[test]
fn primary_core_raw_blocks_genesis_witness_hashes_and_sizes() {
    let limits = Limits::default();
    let (mut vectors, mut witness_transactions) = (0, 0);
    for line in include_str!("bitcoin_block_fixtures/blocks.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        let raw = unhex(fields[7]);
        let b = Block::decode(&raw, &limits).unwrap();
        assert_eq!(b.serialize(&limits).unwrap(), raw, "{}", fields[0]);
        assert_eq!(b.header.hash().unwrap().to_string(), fields[1]);
        assert_eq!(b.header.encode(), raw[..80]);
        assert_eq!(b.transactions.len(), fields[2].parse::<usize>().unwrap());
        let sizes = b.sizes(&limits).unwrap();
        assert_eq!(sizes.stripped, fields[4].parse::<usize>().unwrap());
        assert_eq!(sizes.total, fields[5].parse::<usize>().unwrap());
        assert_eq!(sizes.weight, fields[6].parse::<usize>().unwrap());
        assert_eq!(b.serialize_legacy(&limits).unwrap().len(), sizes.stripped);
        assert_eq!(sizes.weight, 3 * sizes.stripped + sizes.total);
        b.check_merkle_root(&limits).unwrap();
        let root = b.merkle_root(&limits).unwrap();
        assert!(!root.mutated);
        assert_eq!(root.hash, b.header.merkle_root);
        let actual = b.txids(&limits).unwrap();
        assert_eq!(
            actual
                .iter()
                .map(|id| hex(&id.0))
                .collect::<Vec<_>>()
                .join(","),
            fields[8]
        );
        let wtxids = b
            .transactions
            .iter()
            .map(|tx| tx.wtxid(&limits.transaction).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            wtxids
                .iter()
                .map(|id| hex(&id.0))
                .collect::<Vec<_>>()
                .join(","),
            fields[9]
        );
        let witness = b.transactions.iter().filter(|tx| tx.has_witness()).count();
        assert_eq!(witness, fields[3].parse::<usize>().unwrap());
        witness_transactions += witness;
        for (tx, (txid, wtxid)) in b.transactions.iter().zip(actual.iter().zip(wtxids)) {
            if tx.has_witness() {
                assert_ne!(*txid, wtxid);
            } else {
                assert_eq!(*txid, wtxid);
            }
        }
        let mut stream = raw.clone();
        stream.extend_from_slice(&[0xa1, 0xb2]);
        let exact_limits = Limits {
            max_block_bytes: raw.len(),
            ..limits
        };
        let (prefix, used) =
            Block::decode_prefix(&stream, &exact_limits, DecodeMode::Witness).unwrap();
        assert_eq!(prefix, b);
        assert_eq!(used, raw.len());
        assert_limit(Block::decode(&stream, &exact_limits), Resource::BlockBytes);
        let (proof, full) = b
            .merkle_proof((b.transactions.len() - 1) as u32, &limits)
            .unwrap();
        proof
            .verify(*actual.last().unwrap(), &full.hash, &limits)
            .unwrap();
        vectors += 1;
    }
    assert_eq!(vectors, 14);
    assert_eq!(witness_transactions, 4);
}

#[test]
fn header_fields_order_display_and_exact_length() {
    let h = header();
    let bytes = h.encode();
    assert_eq!(&bytes[..4], &i32::MIN.to_le_bytes());
    assert_eq!(&bytes[4..36], &[0x41; 32]);
    assert_eq!(&bytes[36..68], &[0x82; 32]);
    assert_eq!(BlockHeader::decode(&bytes).unwrap(), h);
    for len in 0..80 {
        assert!(matches!(
            BlockHeader::decode(&bytes[..len]),
            Err(Error::UnexpectedEnd { .. })
        ));
    }
    let mut extra = bytes.to_vec();
    extra.push(0);
    assert!(matches!(
        BlockHeader::decode(&extra),
        Err(Error::TrailingBytes {
            offset: 80,
            remaining: 1
        })
    ));
    assert_eq!(BlockHeader::decode_prefix(&extra).unwrap(), (h, 80));
    let expected = enc::double_sha256(&bytes).unwrap();
    assert_eq!(h.hash().unwrap().0, expected);
    let display = h.hash().unwrap().to_string();
    assert_eq!(
        BlockHash::from_display_hex(&display.to_uppercase())
            .unwrap()
            .0,
        expected
    );
    assert_eq!(
        BlockHash::from_display_hex(&display).unwrap().to_string(),
        display
    );
    assert!(matches!(
        BlockHash::from_display_hex("00"),
        Err(Error::HashTextLength { .. })
    ));
    assert!(matches!(
        BlockHash::from_display_hex(&"g".repeat(64)),
        Err(Error::Encoding(_))
    ));
    assert!(BlockHash::from_display_hex(&"é".repeat(32)).is_err());
    let sequential = BlockHash(std::array::from_fn(|index| index as u8));
    assert_eq!(
        sequential.to_string(),
        "1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100"
    );
    // Invalid compact data still round trips exactly; target checks are opt-in.
    assert!(matches!(h.target(false), Err(Error::TargetOverflow)));
}

#[test]
fn opaque_transaction_container_roundtrip_and_explicit_legacy_mode() {
    let limits = Limits::default();
    for witness in [false, true] {
        let b = container(3, witness);
        let raw = b.serialize(&limits).unwrap();
        assert_eq!(Block::decode(&raw, &limits).unwrap(), b);
        assert_eq!(b.header.version, i32::MIN);
        assert_eq!(b.transactions[0].outputs[0].value, -1);
        let stripped = b.serialize_legacy(&limits).unwrap();
        let decoded = Block::decode_legacy(&stripped, &limits).unwrap();
        assert!(!decoded.transactions.iter().any(Transaction::has_witness));
        assert_eq!(decoded.txids(&limits).unwrap(), b.txids(&limits).unwrap());
        assert_eq!(decoded.serialize(&limits).unwrap(), stripped);
    }
    let empty = Block {
        header: header(),
        transactions: vec![],
    };
    let raw = empty.serialize(&limits).unwrap();
    assert_eq!(raw.len(), 81);
    assert_eq!(Block::decode(&raw, &limits).unwrap(), empty);
    assert_eq!(empty.merkle_root(&limits).unwrap().hash, [0; 32]);
    assert!(empty.check_merkle_root(&limits).is_err());
    let b = Block {
        header: header(),
        transactions: vec![Transaction {
            version: 1,
            inputs: vec![],
            outputs: vec![TxOut {
                value: 1,
                script_pubkey: vec![],
            }],
            lock_time: 0,
        }],
    };
    let raw = b.serialize_legacy(&limits).unwrap();
    assert_eq!(Block::decode_legacy(&raw, &limits).unwrap(), b);
    assert!(Block::decode(&raw, &limits).is_err());
}

#[test]
fn block_rejects_truncation_nonminimal_count_trailing_data_and_bad_witness() {
    let limits = Limits::default();
    let b = container(2, true);
    let raw = b.serialize(&limits).unwrap();
    for end in 0..raw.len() {
        assert!(Block::decode(&raw[..end], &limits).is_err(), "prefix {end}");
    }
    let mut extra = raw.clone();
    extra.push(0);
    assert!(matches!(
        Block::decode(&extra, &limits),
        Err(Error::TrailingBytes { .. })
    ));
    for invalid in [
        &[0xfd, 2, 0][..],
        &[0xfe, 2, 0, 0, 0],
        &[0xff, 2, 0, 0, 0, 0, 0, 0, 0],
    ] {
        let mut bytes = b.header.encode().to_vec();
        bytes.extend_from_slice(invalid);
        bytes.extend_from_slice(&raw[81..]);
        assert!(matches!(
            Block::decode(&bytes, &limits),
            Err(Error::Wire {
                source: mcw::bitcoin_wire::Error::NonCanonicalCompactSize { .. },
                ..
            })
        ));
    }
    let mut huge = b.header.encode().to_vec();
    huge.extend_from_slice(&[0xff; 9]);
    assert_limit(Block::decode(&huge, &limits), Resource::Transactions);
    let mut unknown = raw.clone();
    unknown[81 + 5] = 2;
    assert!(matches!(
        Block::decode(&unknown, &limits),
        Err(Error::Wire {
            transaction: Some(0),
            source: mcw::bitcoin_wire::Error::UnknownWitnessFlags { flags: 2, .. },
            ..
        })
    ));
    let mut fewer = raw.clone();
    fewer[80] = 1;
    assert!(matches!(
        Block::decode(&fewer, &limits),
        Err(Error::TrailingBytes { .. })
    ));
    let mut more = raw;
    more[80] = 3;
    assert!(Block::decode(&more, &limits).is_err());
}

#[test]
fn block_aggregate_and_individual_resource_limits_are_enforced() {
    let defaults = Limits::default();
    let b = container(2, true);
    let raw = b.serialize(&defaults).unwrap();
    let exact = Limits {
        max_block_bytes: raw.len(),
        max_transactions: 2,
        ..defaults
    };
    assert_eq!(b.serialize(&exact).unwrap(), raw);
    assert_eq!(Block::decode(&raw, &exact).unwrap(), b);
    assert_limit(
        b.serialize(&Limits {
            max_block_bytes: raw.len() - 1,
            ..defaults
        }),
        Resource::BlockBytes,
    );
    assert_limit(
        Block::decode(
            &raw,
            &Limits {
                max_block_bytes: raw.len() - 1,
                ..defaults
            },
        ),
        Resource::BlockBytes,
    );
    assert_limit(
        Block::decode_prefix(
            &raw,
            &Limits {
                max_block_bytes: 79,
                ..defaults
            },
            DecodeMode::Witness,
        ),
        Resource::BlockBytes,
    );
    assert_limit(
        b.sizes(&Limits {
            max_transactions: 1,
            ..defaults
        }),
        Resource::Transactions,
    );
    assert_limit(
        Block::decode(
            &raw,
            &Limits {
                max_transactions: 1,
                ..defaults
            },
        ),
        Resource::Transactions,
    );
    let mut decoded = size_of::<Block>() + b.transactions.len() * size_of::<Transaction>();
    for tx in &b.transactions {
        decoded += tx.inputs.len() * size_of::<TxIn>() + tx.outputs.len() * size_of::<TxOut>();
        for input in &tx.inputs {
            decoded += input.script_sig.len()
                + input.witness.len() * size_of::<Vec<u8>>()
                + input.witness.iter().map(Vec::len).sum::<usize>();
        }
        decoded += tx
            .outputs
            .iter()
            .map(|output| output.script_pubkey.len())
            .sum::<usize>();
    }
    let exact = Limits {
        max_decoded_bytes: decoded,
        ..defaults
    };
    assert_eq!(b.serialize(&exact).unwrap(), raw);
    Block::decode(&raw, &exact).unwrap();
    assert_limit(
        b.serialize(&Limits {
            max_decoded_bytes: decoded - 1,
            ..defaults
        }),
        Resource::DecodedBytes,
    );
    assert!(
        Block::decode(
            &raw,
            &Limits {
                max_decoded_bytes: decoded - 1,
                ..defaults
            }
        )
        .is_err()
    );
    assert_limit(
        Block::decode(
            &raw,
            &Limits {
                max_decoded_bytes: 0,
                ..defaults
            },
        ),
        Resource::DecodedBytes,
    );
    let mut limited = defaults;
    limited.transaction.max_witness_item_bytes = 2;
    assert!(matches!(
        b.serialize(&limited),
        Err(Error::Wire {
            source: mcw::bitcoin_wire::Error::LimitExceeded {
                resource: mcw::bitcoin_wire::Resource::WitnessItemBytes,
                ..
            },
            ..
        })
    ));
    assert!(Block::decode(&raw, &limited).is_err());
    limited = defaults;
    limited.transaction.max_transaction_bytes = 1;
    assert!(Block::decode(&raw, &limited).is_err());
}

#[test]
fn merkle_duplicate_last_and_cve_2012_2459_mutations() {
    let limits = Limits::default();
    assert_eq!(
        block::merkle_root(&[], &limits).unwrap(),
        block::MerkleRoot {
            hash: [0; 32],
            mutated: false
        }
    );
    for count in 1..=64 {
        let leaves = ids(count);
        let root = block::merkle_root(&leaves, &limits).unwrap();
        assert!(!root.mutated);
        if count & (count - 1) != 0 {
            let suffix = 1usize << count.trailing_zeros();
            let mut duplicated = leaves.clone();
            duplicated.extend_from_slice(&leaves[count - suffix..]);
            let mutated = block::merkle_root(&duplicated, &limits).unwrap();
            assert_eq!(mutated.hash, root.hash);
            assert!(mutated.mutated);
        }
    }
    let leaves = ids(6);
    let original = block::merkle_root(&leaves, &limits).unwrap();
    let mut extended = leaves.clone();
    extended.extend_from_slice(&leaves[4..]);
    assert_eq!(
        block::merkle_root(&extended, &limits).unwrap().hash,
        original.hash
    );
    assert!(block::merkle_root(&extended, &limits).unwrap().mutated);
    let mut inner = ids(4);
    inner[2] = inner[0];
    inner[3] = inner[1];
    assert!(block::merkle_root(&inner, &limits).unwrap().mutated); // equal internal siblings
    let mut early = ids(4);
    early[1] = early[0];
    assert!(block::merkle_root(&early, &limits).unwrap().mutated); // any real pair, not just final pair
    let mut b = container(3, false);
    b.transactions.push(b.transactions[2].clone());
    assert!(matches!(
        b.check_merkle_root(&limits),
        Err(Error::MutatedTree)
    ));
}

#[test]
fn inclusion_proofs_all_positions_orders_bounds_and_shape() {
    let limits = Limits::default();
    for count in 1..=40 {
        let leaves = ids(count);
        let expected = block::merkle_root(&leaves, &limits).unwrap();
        for index in 0..count {
            let (proof, root) = MerkleProof::build(&leaves, index as u32, &limits).unwrap();
            assert_eq!(root, expected);
            proof.verify(leaves[index], &root.hash, &limits).unwrap();
            let mut short = proof.clone();
            if !short.siblings.is_empty() {
                short.siblings.pop();
                assert!(matches!(
                    short.root(leaves[index], &limits),
                    Err(Error::ProofLength { .. })
                ));
            }
            let mut extra = proof.clone();
            extra.siblings.push([0; 32]);
            assert!(matches!(
                extra.root(leaves[index], &limits),
                Err(Error::ProofLength { .. })
            ));
            if !proof.siblings.is_empty() {
                let mut changed = proof.clone();
                changed.siblings[0][0] ^= 0x80;
                assert!(changed.verify(leaves[index], &root.hash, &limits).is_err());
            }
            assert!(proof.verify(leaves[index], &[0; 32], &limits).is_err());
        }
        assert!(matches!(
            MerkleProof::build(&leaves, count as u32, &limits),
            Err(Error::InvalidIndex { .. })
        ));
    }
    let leaves = ids(3);
    let (mut proof, _) = MerkleProof::build(&leaves, 2, &limits).unwrap();
    assert_eq!(proof.siblings[0], leaves[2].0);
    proof.siblings[0][0] ^= 1;
    assert!(matches!(
        proof.root(leaves[2], &limits),
        Err(Error::InvalidDuplicateLast)
    ));
    let mutated = [TxId([1; 32]), TxId([1; 32])];
    let (proof, full) = MerkleProof::build(&mutated, 0, &limits).unwrap();
    assert!(full.mutated);
    assert!(matches!(
        proof.root(mutated[0], &limits),
        Err(Error::MutatedTree)
    ));
    assert!(MerkleProof::build(&[], 0, &limits).is_err());
    assert_limit(
        block::merkle_root(
            &ids(3),
            &Limits {
                max_merkle_leaves: 2,
                ..limits
            },
        ),
        Resource::MerkleLeaves,
    );
    assert_limit(
        block::merkle_root(
            &ids(3),
            &Limits {
                max_merkle_work_bytes: 95,
                ..limits
            },
        ),
        Resource::MerkleWorkBytes,
    );
    block::merkle_root(
        &ids(3),
        &Limits {
            max_merkle_work_bytes: 96,
            ..limits
        },
    )
    .unwrap();
    let high = MerkleProof {
        transaction_count: u32::MAX,
        transaction_index: u32::MAX - 1,
        siblings: vec![[7; 32]; 32],
    };
    assert!(
        std::panic::catch_unwind(|| high.root(
            TxId([6; 32]),
            &Limits {
                max_merkle_leaves: u32::MAX as usize,
                ..limits
            }
        ))
        .is_ok()
    );
}

#[test]
fn partial_core_vectors_exact_bytes_matches_and_merkleblock_payloads() {
    let limits = Limits::default();
    let mut count = 0;
    for line in include_str!("bitcoin_block_fixtures/partial.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let f = line.split('\t').collect::<Vec<_>>();
        let leaves = txids(f[2]);
        let mask = f[1].bytes().map(|byte| byte == b'1').collect::<Vec<_>>();
        let raw = unhex(f[3]);
        let built = PartialMerkleTree::build(&leaves, &mask, &limits).unwrap();
        assert_eq!(built.serialize(&limits).unwrap(), raw, "{}", f[0]);
        let decoded = PartialMerkleTree::decode(&raw, &limits).unwrap();
        assert_eq!(decoded, built);
        let extracted = decoded.extract_canonical(&limits).unwrap();
        assert_eq!(hex(&extracted.merkle_root), f[4]);
        assert_eq!(extracted.bits_used, f[6].parse::<usize>().unwrap());
        assert_eq!(
            extracted
                .matches
                .iter()
                .map(|m| m.index.to_string())
                .collect::<Vec<_>>()
                .join(","),
            f[5]
        );
        for matched in &extracted.matches {
            assert_eq!(matched.txid, leaves[matched.index as usize]);
        }
        let mut mb_raw = unhex(f[7]);
        mb_raw.extend_from_slice(&raw);
        let mb = MerkleBlock::decode(&mb_raw, &limits).unwrap();
        assert_eq!(mb.tree, built);
        assert_eq!(mb.serialize(&limits).unwrap(), mb_raw);
        assert_eq!(mb.extract(&limits).unwrap(), extracted);
        let mut stream = raw.clone();
        stream.extend_from_slice(&[0, 1]);
        assert_eq!(
            PartialMerkleTree::decode_prefix(&stream, &limits).unwrap(),
            (built, raw.len())
        );
        assert!(matches!(
            PartialMerkleTree::decode(&stream, &limits),
            Err(Error::TrailingBytes { .. })
        ));
        let mut stream = mb_raw.clone();
        stream.push(0);
        assert_eq!(
            MerkleBlock::decode_prefix(&stream, &limits).unwrap(),
            (mb, mb_raw.len())
        );
        assert!(matches!(
            MerkleBlock::decode(&stream, &limits),
            Err(Error::TrailingBytes { .. })
        ));
        count += 1;
    }
    assert_eq!(count, 33);
}

#[test]
fn partial_all_masks_odd_widths_compactsize_boundaries_and_roundtrip() {
    let limits = Limits::default();
    for count in 1..=7 {
        let leaves = ids(count);
        let expected = block::merkle_root(&leaves, &limits).unwrap().hash;
        for mask_bits in 0..1usize << count {
            let mask = (0..count)
                .map(|i| mask_bits & (1 << i) != 0)
                .collect::<Vec<_>>();
            let tree = PartialMerkleTree::build(&leaves, &mask, &limits).unwrap();
            let extract = tree.extract_canonical(&limits).unwrap();
            assert_eq!(extract.merkle_root, expected);
            assert_eq!(
                extract
                    .matches
                    .iter()
                    .map(|m| m.index as usize)
                    .collect::<Vec<_>>(),
                (0..count).filter(|i| mask[*i]).collect::<Vec<_>>()
            );
            let raw = tree.serialize(&limits).unwrap();
            assert_eq!(PartialMerkleTree::decode(&raw, &limits).unwrap(), tree);
        }
    }
    for count in [252, 253, 254, 1024] {
        let tree = PartialMerkleTree::build(&ids(count), &vec![true; count], &limits).unwrap();
        let raw = tree.serialize(&limits).unwrap();
        assert_eq!(raw[4], if count < 253 { count as u8 } else { 0xfd });
        assert_eq!(
            PartialMerkleTree::decode(&raw, &limits)
                .unwrap()
                .extract(&limits)
                .unwrap()
                .matches
                .len(),
            count
        );
    }
}

#[test]
fn partial_malformed_traversal_mutation_padding_and_hidden_limits() {
    let limits = Limits::default();
    let tree = PartialMerkleTree::build(&ids(3), &[true; 3], &limits).unwrap();
    let raw = tree.serialize(&limits).unwrap();
    for end in 0..raw.len() {
        assert!(PartialMerkleTree::decode(&raw[..end], &limits).is_err());
    }
    let mut nonminimal = raw[..4].to_vec();
    nonminimal.extend_from_slice(&[0xfd, 3, 0]);
    nonminimal.extend_from_slice(&raw[5..]);
    assert!(matches!(
        PartialMerkleTree::decode(&nonminimal, &limits),
        Err(Error::Wire {
            source: mcw::bitcoin_wire::Error::NonCanonicalCompactSize { .. },
            ..
        })
    ));
    for invalid in [0, block::MAX_PARTIAL_TRANSACTIONS + 1, u32::MAX] {
        let mut bytes = raw.clone();
        bytes[..4].copy_from_slice(&invalid.to_le_bytes());
        assert!(PartialMerkleTree::decode(&bytes, &limits).is_err());
    }
    let mut no_hash = tree.clone();
    no_hash.hashes.clear();
    assert!(matches!(
        no_hash.extract(&limits),
        Err(Error::InvalidPartial(PartialError::HashCount))
    ));
    let mut no_flags = tree.clone();
    no_flags.flags.clear();
    assert!(matches!(
        no_flags.extract(&limits),
        Err(Error::InvalidPartial(PartialError::FlagCount))
    ));
    let mut few_hashes = tree.clone();
    few_hashes.hashes.pop();
    assert!(matches!(
        few_hashes.extract(&limits),
        Err(Error::InvalidPartial(PartialError::ExhaustedHashes))
    ));
    let mut extra_hashes = PartialMerkleTree::build(&ids(3), &[false; 3], &limits).unwrap();
    extra_hashes.hashes.push([8; 32]);
    assert!(matches!(
        extra_hashes.extract(&limits),
        Err(Error::InvalidPartial(PartialError::UnusedHashes))
    ));
    let mut extra_bytes = PartialMerkleTree::build(&ids(8), &[false; 8], &limits).unwrap();
    extra_bytes.flags.push(0);
    assert!(matches!(
        extra_bytes.extract(&limits),
        Err(Error::InvalidPartial(PartialError::UnusedFlagBytes))
    ));
    let mut few_bits = PartialMerkleTree::build(&ids(8), &[true; 8], &limits).unwrap();
    few_bits.flags.truncate(1);
    assert!(matches!(
        few_bits.extract(&limits),
        Err(Error::InvalidPartial(PartialError::ExhaustedBits))
    ));
    let mut identical = PartialMerkleTree::build(&ids(2), &[true; 2], &limits).unwrap();
    identical.hashes[1] = identical.hashes[0];
    assert!(matches!(
        identical.extract(&limits),
        Err(Error::InvalidPartial(PartialError::IdenticalBranches))
    ));
    assert!(matches!(
        identical.serialize(&limits),
        Err(Error::InvalidPartial(PartialError::IdenticalBranches))
    ));
    let duplicate = [TxId([3; 32]), TxId([3; 32])];
    assert!(matches!(
        PartialMerkleTree::build(&duplicate, &[false; 2], &limits),
        Err(Error::MutatedTree)
    ));
    // An opaque root cannot reveal a mutation hidden under an unmatched subtree.
    let hidden = PartialMerkleTree {
        transaction_count: 2,
        hashes: vec![[1; 32]],
        flags: vec![0],
    };
    hidden.extract(&limits).unwrap();
    let mut padding = PartialMerkleTree::build(&ids(1), &[true], &limits).unwrap();
    padding.flags[0] |= 0xfe;
    padding.extract(&limits).unwrap();
    assert!(matches!(
        padding.extract_canonical(&limits),
        Err(Error::InvalidPartial(PartialError::NonZeroPadding))
    ));
    let encoded = padding.serialize(&limits).unwrap();
    assert_eq!(
        PartialMerkleTree::decode(&encoded, &limits)
            .unwrap()
            .serialize(&limits)
            .unwrap(),
        encoded
    );
    assert!(PartialMerkleTree::build(&[], &[], &limits).is_err());
    assert!(matches!(
        PartialMerkleTree::build(&ids(3), &[true; 2], &limits),
        Err(Error::MatchMaskLength { .. })
    ));
    assert!(
        PartialMerkleTree::build(
            &ids(block::MAX_PARTIAL_TRANSACTIONS as usize + 1),
            &[],
            &limits
        )
        .is_err()
    );
}

#[test]
fn partial_and_merkleblock_limits_revalidation_and_wrong_roots() {
    let limits = Limits::default();
    let b = container(9, true);
    let mask = vec![true; 9];
    let mb = MerkleBlock::build(&b, &mask, &limits).unwrap();
    let raw = mb.serialize(&limits).unwrap();
    let exact = Limits {
        max_partial_bytes: raw.len(),
        ..limits
    };
    assert_eq!(MerkleBlock::decode(&raw, &exact).unwrap(), mb);
    assert_eq!(mb.serialize(&exact).unwrap(), raw);
    assert_limit(
        mb.serialize(&Limits {
            max_partial_bytes: raw.len() - 1,
            ..limits
        }),
        Resource::PartialBytes,
    );
    assert_limit(
        MerkleBlock::decode(
            &raw,
            &Limits {
                max_partial_bytes: raw.len() - 1,
                ..limits
            },
        ),
        Resource::PartialBytes,
    );
    assert_limit(
        MerkleBlock::decode_prefix(
            &raw,
            &Limits {
                max_partial_bytes: 79,
                ..limits
            },
        ),
        Resource::PartialBytes,
    );
    assert_limit(
        mb.tree.extract(&Limits {
            max_merkle_leaves: 8,
            ..limits
        }),
        Resource::MerkleLeaves,
    );
    assert_limit(
        mb.tree.extract(&Limits {
            max_decoded_bytes: 1,
            ..limits
        }),
        Resource::DecodedBytes,
    );
    assert_limit(
        mb.tree.extract(&Limits {
            max_merkle_work_bytes: 1,
            ..limits
        }),
        Resource::MerkleWorkBytes,
    );
    assert_limit(
        PartialMerkleTree::build(
            &ids(9),
            &mask,
            &Limits {
                max_merkle_work_bytes: 1,
                ..limits
            },
        ),
        Resource::MerkleWorkBytes,
    );
    let tree_raw = mb.tree.serialize(&limits).unwrap();
    assert_eq!(
        mb.tree
            .serialize(&Limits {
                max_partial_bytes: tree_raw.len(),
                ..limits
            })
            .unwrap(),
        tree_raw
    );
    assert_limit(
        mb.tree.serialize(&Limits {
            max_partial_bytes: tree_raw.len() - 1,
            ..limits
        }),
        Resource::PartialBytes,
    );
    let mut wrong = mb.clone();
    wrong.header.merkle_root[0] ^= 1;
    assert!(matches!(wrong.extract(&limits), Err(Error::RootMismatch)));
    assert!(matches!(wrong.serialize(&limits), Err(Error::RootMismatch)));
    let mut incorrect = raw;
    incorrect[36] ^= 1;
    assert!(matches!(
        MerkleBlock::decode(&incorrect, &limits),
        Err(Error::RootMismatch)
    ));
    let mut wrong_block = b;
    wrong_block.header.merkle_root[0] ^= 1;
    assert!(matches!(
        MerkleBlock::build(&wrong_block, &mask, &limits),
        Err(Error::RootMismatch)
    ));
}

#[test]
fn compact_target_primary_core_vectors_sign_range_and_canonical_data() {
    // src/test/arith_uint256_tests.cpp bignum_SetCompact, plus genesis bits.
    for (bits, magnitude, normalized, negative, overflow) in [
        (0, "0", 0, false, false),
        (0x0012_3456, "0", 0, false, false),
        (0x0100_3456, "0", 0, false, false),
        (0x0200_0056, "0", 0, false, false),
        (0x0300_0000, "0", 0, false, false),
        (0x0400_0000, "0", 0, false, false),
        (0x0092_3456, "0", 0, false, false),
        (0x0180_3456, "0", 0, false, false),
        (0x0280_0056, "0", 0, false, false),
        (0x0380_0000, "0", 0, false, false),
        (0x0480_0000, "0", 0, false, false),
        (0x0112_3456, "12", 0x0112_0000, false, false),
        (0x01fe_dcba, "7e", 0x01fe_0000, true, false),
        (0x0212_3456, "1234", 0x0212_3400, false, false),
        (0x0312_3456, "123456", 0x0312_3456, false, false),
        (0x0412_3456, "12345600", 0x0412_3456, false, false),
        (0x0492_3456, "12345600", 0x0492_3456, true, false),
        (0x0500_9234, "92340000", 0x0500_9234, false, false),
        (
            0x2012_3456,
            "1234560000000000000000000000000000000000000000000000000000000000",
            0x2012_3456,
            false,
            false,
        ),
        (0xff12_3456, "0", 0, false, true),
    ] {
        let target = CompactTarget::from_bits(bits);
        assert_eq!(target.magnitude_be, array(&format!("{magnitude:0>64}")));
        assert_eq!(target.negative, negative);
        assert_eq!(target.overflow, overflow);
        assert_eq!(
            block::encode_compact_target(&target.magnitude_be, negative),
            normalized
        );
        assert_eq!(target.canonical, !overflow && bits == normalized);
    }
    let value = array(&format!("{:064x}", 0x80));
    assert_eq!(block::encode_compact_target(&value, false), 0x0200_8000);
    let mainnet = CompactTarget::from_bits(0x1d00_ffff);
    assert_eq!(
        hex(&mainnet.checked_positive(true).unwrap()),
        "00000000ffff0000000000000000000000000000000000000000000000000000"
    );
    assert!(matches!(
        CompactTarget::from_bits(0).checked_positive(false),
        Err(Error::ZeroTarget)
    ));
    assert!(matches!(
        CompactTarget::from_bits(0x01fe_dcba).checked_positive(false),
        Err(Error::NegativeTarget)
    ));
    assert!(matches!(
        CompactTarget::from_bits(0x2300_0001).checked_positive(false),
        Err(Error::TargetOverflow)
    ));
    assert!(matches!(
        CompactTarget::from_bits(0x0112_3456).checked_positive(true),
        Err(Error::NonCanonicalTarget)
    ));
    CompactTarget::from_bits(0x0112_3456)
        .checked_positive(false)
        .unwrap();
    for (bits, overflow) in [
        (0x2200_00ff, false),
        (0x2200_0100, true),
        (0x2100_ffff, false),
        (0x2101_0000, true),
        (0x207f_ffff, false),
        (0x2300_0000, false),
    ] {
        assert_eq!(CompactTarget::from_bits(bits).overflow, overflow);
    }
}

#[test]
fn deterministic_malformed_decoding_never_panics() {
    let limits = Limits {
        max_block_bytes: 512,
        max_transactions: 32,
        max_decoded_bytes: 4096,
        max_merkle_leaves: 32,
        max_merkle_work_bytes: 4096,
        max_partial_bytes: 512,
        ..Limits::default()
    };
    let mut state = 0x0123_4567_89ab_cdefu64;
    for case in 0..4000usize {
        let mut data = vec![0; case % 513];
        for byte in &mut data {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        if case % 3 == 0 && data.len() >= 81 {
            data[..80].copy_from_slice(&header().encode());
            data[80] = (case % 17) as u8;
        }
        let result = std::panic::catch_unwind(|| {
            let _ = BlockHeader::decode(&data);
            let _ = Block::decode(&data, &limits);
            let _ = Block::decode_prefix(&data, &limits, DecodeMode::Legacy);
            let _ = PartialMerkleTree::decode(&data, &limits);
            let _ = MerkleBlock::decode(&data, &limits);
            let _ = CompactTarget::from_bits(state as u32).checked_positive(case % 2 == 0);
        });
        assert!(result.is_ok(), "case {case}");
    }
}
