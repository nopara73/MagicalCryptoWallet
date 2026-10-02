//! Tests the bounded handler with the actual published frame and codec sources.
#![forbid(unsafe_code)]
#![allow(dead_code)]
#[path = "../src/bitcoin_encoding/address_service.rs"]
mod bitcoin_address_service;
use mcw::{bitcoin_encoding, bridge};

use bitcoin_address_service::{VALIDATE_ADDRESS, ValidationFailure, handle};
use bitcoin_encoding::{LegacyKind, Network, legacy_address_encode, witness_address_encode};
use bridge::{ERROR, Frame, REQUEST, RESPONSE};

fn frame(network: u8, text: &str) -> Frame {
    let mut payload = vec![network];
    payload.extend_from_slice(text.as_bytes());
    Frame {
        kind: REQUEST,
        id: 42,
        operation: VALIDATE_ADDRESS,
        payload,
    }
}

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn all_core_addresses_return_the_published_script_bytes() {
    let mut count = 0;
    for line in include_str!("bitcoin_encoding_fixtures/core_addresses.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        let network = match fields[0] {
            "main" => 0,
            "test" => 1,
            "testnet4" => 2,
            "signet" => 3,
            "regtest" => 4,
            _ => panic!("unknown fixture network"),
        };
        let request = frame(network, fields[1]);
        let result = handle(&request);
        assert_eq!(
            (result.kind, result.id, result.operation),
            (RESPONSE, 42, VALIDATE_ADDRESS)
        );
        let mut expected = vec![1];
        expected.extend(hex(fields[2]));
        assert_eq!(result.payload, expected, "{}", fields[1]);
        let mut encoded = Vec::new();
        result.write(&mut encoded).unwrap();
        assert_eq!(Frame::read(&mut &encoded[..]).unwrap(), Some(result));
        count += 1;
    }
    assert_eq!(count, 54);
}

#[test]
fn witness_version_opcodes_and_all_network_values_are_exact() {
    for (value, network) in [
        (0, Network::Mainnet),
        (1, Network::Testnet),
        (2, Network::Testnet4),
        (3, Network::Signet),
        (4, Network::Regtest),
    ] {
        for version in 0..=16 {
            let program = [version; 32];
            let text = witness_address_encode(network, version, &program).unwrap();
            let result = handle(&frame(value, &text.to_ascii_uppercase()));
            let opcode = if version == 0 { 0 } else { 0x50 + version };
            let mut expected = vec![1, opcode, 32];
            expected.extend(program);
            assert_eq!(result.payload, expected);
        }
        for kind in [LegacyKind::P2pkh, LegacyKind::P2sh] {
            let text = legacy_address_encode(network, kind, &[0; 20]).unwrap();
            assert_eq!(handle(&frame(value, &text)).payload[0], 1);
        }
    }
}

#[test]
fn typed_address_rejection_is_distinct_from_transport_errors() {
    let valid = witness_address_encode(Network::Mainnet, 0, &[1; 20]).unwrap();
    let mixed = valid.replacen("bc", "BC", 1);
    for (network, text, reason) in [
        (1, valid.as_str(), ValidationFailure::Network),
        (0, mixed.as_str(), ValidationFailure::MixedCase),
        (
            0,
            "1111111111111111111114oLvT3",
            ValidationFailure::Checksum,
        ),
        (0, "", ValidationFailure::Format),
        (0, " 1111111111111111111114oLvT2", ValidationFailure::Format),
    ] {
        let result = handle(&frame(network, text));
        assert_eq!(result.kind, RESPONSE);
        let mut expected = vec![0];
        expected.extend_from_slice(&(reason as u16).to_le_bytes());
        assert_eq!(result.payload, expected);
    }
    assert_eq!(handle(&frame(0, &"a".repeat(91))).payload, [0, 5, 0]);
    let mut invalid = frame(0, "ignored");
    invalid.payload = vec![0, 0xff];
    assert_eq!(handle(&invalid).kind, ERROR);
    for value in 5..=255 {
        assert_eq!(handle(&frame(value, &valid)).kind, ERROR);
    }
    invalid.payload.clear();
    assert_eq!(handle(&invalid).kind, ERROR);
    invalid = frame(0, &valid);
    invalid.id = 0;
    assert_eq!(handle(&invalid).kind, ERROR);
    invalid = frame(0, &valid);
    invalid.kind = bridge::CANCEL;
    assert_eq!(handle(&invalid).kind, ERROR);
    invalid = frame(0, &valid);
    invalid.operation += 1;
    assert_eq!(handle(&invalid).kind, ERROR);
}

#[test]
fn malformed_text_never_gets_trimmed_or_echoed_in_errors() {
    for text in [
        "private synthetic invalid string",
        "bc1",
        "bitcoin:bc1x",
        "\0",
        "é",
        "1111111111111111111114oLvT2\n",
    ] {
        let result = handle(&frame(0, text));
        assert_eq!(result.kind, RESPONSE);
        assert_eq!(result.payload.len(), 3);
        assert_eq!(result.payload[0], 0);
    }
    let request = frame(255, "private synthetic invalid string");
    let error = handle(&request);
    assert_eq!(error.kind, ERROR);
    assert!(!String::from_utf8_lossy(&error.payload).contains("private"));
}
