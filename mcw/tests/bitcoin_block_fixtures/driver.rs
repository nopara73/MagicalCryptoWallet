//! Test-only line protocol, compiled solely into an ignored evidence directory.
use mcw::bitcoin_block::{
    self as block, Block, BlockHeader, CompactTarget, Limits, MerkleBlock, MerkleProof,
    PartialMerkleTree,
};
use mcw::{bitcoin_encoding as enc, bitcoin_wire::TxId};
use std::io::{self, BufRead};

fn bytes(text: &str) -> Vec<u8> {
    enc::hex_decode(text).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    enc::hex_encode(bytes).unwrap()
}
fn leaves(text: &str) -> Vec<TxId> {
    let raw = bytes(text);
    assert_eq!(raw.len() % 32, 0);
    raw.chunks_exact(32)
        .map(|chunk| {
            let mut hash = [0; 32];
            hash.copy_from_slice(chunk);
            TxId(hash)
        })
        .collect()
}
fn hashes(values: &[[u8; 32]]) -> String {
    values
        .iter()
        .map(|hash| hex(hash))
        .collect::<Vec<_>>()
        .join(",")
}
fn matches(tree: &PartialMerkleTree, limits: &Limits) -> Result<String, block::Error> {
    let result = tree.extract(limits)?;
    Ok(format!(
        "{}|{}|{}|{}",
        hex(&result.merkle_root),
        result
            .matches
            .iter()
            .map(|m| m.index.to_string())
            .collect::<Vec<_>>()
            .join(","),
        result.bits_used,
        tree.extract_canonical(limits).is_ok()
    ))
}
fn run(parts: &[&str]) -> Result<String, block::Error> {
    let limits = Limits::default();
    match parts[0] {
        "H" => {
            let header = BlockHeader::decode(&bytes(parts[1]))?;
            Ok(format!("{}|{}", hex(&header.encode()), header.hash()?))
        }
        "B" => {
            let b = Block::decode(&bytes(parts[1]), &limits)?;
            let sizes = b.sizes(&limits)?;
            let root = b.merkle_root(&limits)?;
            let txids = b.txids(&limits)?;
            let wtxids = b
                .transactions
                .iter()
                .map(|tx| tx.wtxid(&limits.transaction).map(|id| hex(&id.0)))
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            Ok(format!(
                "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                hex(&b.serialize(&limits)?),
                b.header.hash()?,
                b.transactions.len(),
                sizes.stripped,
                sizes.total,
                sizes.weight,
                hex(&root.hash),
                root.mutated,
                txids
                    .iter()
                    .map(|id| hex(&id.0))
                    .collect::<Vec<_>>()
                    .join(","),
                wtxids.join(",")
            ))
        }
        "M" => {
            let root = block::merkle_root(&leaves(parts[1]), &limits)?;
            Ok(format!("{}|{}", hex(&root.hash), root.mutated))
        }
        "P" => {
            let (proof, root) =
                MerkleProof::build(&leaves(parts[1]), parts[2].parse().unwrap(), &limits)?;
            Ok(format!(
                "{}|{}|{}",
                hex(&root.hash),
                root.mutated,
                hashes(&proof.siblings)
            ))
        }
        "T" => {
            let leaves = leaves(parts[1]);
            let mask = parts[2]
                .bytes()
                .map(|byte| byte == b'1')
                .collect::<Vec<_>>();
            let tree = PartialMerkleTree::build(&leaves, &mask, &limits)?;
            Ok(format!(
                "{}|{}",
                hex(&tree.serialize(&limits)?),
                matches(&tree, &limits)?
            ))
        }
        "E" => {
            let tree = PartialMerkleTree::decode(&bytes(parts[1]), &limits)?;
            Ok(format!(
                "{}|{}",
                hex(&tree.serialize(&limits)?),
                matches(&tree, &limits)?
            ))
        }
        "MB" => {
            let b = MerkleBlock::decode(&bytes(parts[1]), &limits)?;
            Ok(format!(
                "{}|{}",
                hex(&b.serialize(&limits)?),
                matches(&b.tree, &limits)?
            ))
        }
        "C" => {
            let bits = u32::from_str_radix(parts[1], 16).unwrap();
            let target = CompactTarget::from_bits(bits);
            Ok(format!(
                "{}|{}|{}|{}|{:08x}",
                hex(&target.magnitude_be),
                target.negative,
                target.overflow,
                target.canonical,
                block::encode_compact_target(&target.magnitude_be, target.negative)
            ))
        }
        "TC" => {
            let raw = bytes(parts[1]);
            let mut target = [0; 32];
            target.copy_from_slice(&raw);
            Ok(format!(
                "{:08x}",
                block::encode_compact_target(&target, parts[2] == "1")
            ))
        }
        _ => panic!("unsupported test command"),
    }
}
pub fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let parts = line.split('|').collect::<Vec<_>>();
        match run(&parts) {
            Ok(result) => println!("OK|{result}"),
            Err(error) => println!("ERR|{error:?}"),
        }
    }
}
