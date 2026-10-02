//! Bounded cache-header service checks using synthetic, independent Core vectors.
use mcw::{bitcoin_block::BlockHash, bitcoin_block_service, bitcoin_encoding};

#[test]
fn exact_core_synthetic_headers_and_raw_display_order() {
    let mut cases = 0;
    for line in include_str!("headers.tsv").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        let header = bitcoin_encoding::hex_decode(fields[1]).unwrap();
        let expected = bitcoin_encoding::hex_decode(fields[2]).unwrap();
        let digest = bitcoin_block_service::hash_header(&header).unwrap();
        assert_eq!(digest.as_slice(), expected, "{}", fields[0]);
        assert_eq!(BlockHash(digest).to_string(), fields[3], "{}", fields[0]);
        cases += 1;
    }
    assert_eq!(cases, 256);
}

#[test]
fn every_short_header_and_oversized_payload_is_rejected() {
    for length in (0..80).chain(81..=160).chain([1_048_560]) {
        assert!(bitcoin_block_service::hash_header(&vec![0; length]).is_err());
    }
}

#[test]
fn hashing_does_not_claim_consensus_validation() {
    // Zero target and an all-ones header are opaque, exact header inputs. This
    // small service must not impose or substitute block/PoW validation policy.
    assert!(bitcoin_block_service::hash_header(&[0; 80]).is_ok());
    assert!(bitcoin_block_service::hash_header(&[255; 80]).is_ok());
}
