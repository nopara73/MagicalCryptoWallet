//! Offline conformance tests. Can be run by cargo test after host integration,
//! or compiled with rustc --test without adding a package or a shipping binary.
#![forbid(unsafe_code)]

#[path = "../src/bitcoin_encoding.rs"]
mod bitcoin_encoding;

use bitcoin_encoding::*;

fn fixture_bytes(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    assert!(text.len().is_multiple_of(2));
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn fixture_text(text: &str) -> String {
    String::from_utf8(fixture_bytes(text)).unwrap()
}

fn rows(fixture: &str) -> impl Iterator<Item = Vec<&str>> {
    fixture
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t').collect())
}

fn network_for_chain(chain: &str) -> Network {
    match chain {
        "main" => Network::Mainnet,
        "test" => Network::Testnet,
        "testnet4" => Network::Testnet4,
        "signet" => Network::Signet,
        "regtest" => Network::Regtest,
        other => panic!("unexpected fixture chain {other}"),
    }
}

fn script_bytes(address: &Address) -> Vec<u8> {
    match address {
        Address::Legacy(a) => match a.kind {
            LegacyKind::P2pkh => {
                let mut script = vec![0x76, 0xa9, 0x14];
                script.extend(a.hash);
                script.extend([0x88, 0xac]);
                script
            }
            LegacyKind::P2sh => {
                let mut script = vec![0xa9, 0x14];
                script.extend(a.hash);
                script.push(0x87);
                script
            }
        },
        Address::Witness(a) => {
            let opcode = if a.version == 0 { 0 } else { 0x50 + a.version };
            let mut script = vec![opcode, a.program.len() as u8];
            script.extend_from_slice(&a.program);
            script
        }
    }
}

#[test]
fn all_bip173_and_bip350_published_vectors() {
    let mut count = 0;
    for row in rows(include_str!("bitcoin_encoding_fixtures/bip_vectors.tsv")) {
        count += 1;
        let text = fixture_text(row[2]);
        let variant = if row[0] == "173" {
            ChecksumVariant::Bech32
        } else {
            ChecksumVariant::Bech32m
        };
        match row[1] {
            "generic-valid" => {
                let data = bech32_decode(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
                assert_eq!(data.variant, variant, "{text}");
                // Preserves accepted HRP spelling; canonicalization is explicit.
                assert_eq!(text.rsplit_once('1').unwrap().0, data.hrp);
                assert_eq!(
                    bech32_encode(&data.hrp.to_ascii_lowercase(), &data.data, variant).unwrap(),
                    text.to_ascii_lowercase()
                );
            }
            "generic-invalid" => assert!(bech32_decode(&text).is_err(), "{text:?}"),
            "witness-valid" => {
                let network = if text[..2].eq_ignore_ascii_case("bc") {
                    Network::Mainnet
                } else {
                    Network::Testnet
                };
                let address =
                    address_decode(&text, network).unwrap_or_else(|e| panic!("{text}: {e}"));
                assert_eq!(script_bytes(&address), fixture_bytes(row[3]), "{text}");
                assert_eq!(
                    address_encode(&address).unwrap(),
                    text.to_ascii_lowercase(),
                    "{text}"
                );
                let opposite = if network == Network::Mainnet {
                    Network::Testnet
                } else {
                    Network::Mainnet
                };
                assert_eq!(
                    witness_address_decode(&text, opposite),
                    Err(Error::WrongNetwork { expected: opposite })
                );
            }
            "witness-obsolete" => {
                assert_eq!(
                    bech32_decode(&text).unwrap().variant,
                    ChecksumVariant::Bech32
                );
                assert!(
                    matches!(
                        witness_address_decode(&text, Network::Mainnet),
                        Err(Error::WrongChecksumVariant { .. })
                    ),
                    "{text}"
                );
            }
            "witness-invalid" => {
                for network in [
                    Network::Mainnet,
                    Network::Testnet,
                    Network::Testnet4,
                    Network::Signet,
                    Network::Regtest,
                ] {
                    assert!(
                        witness_address_decode(&text, network).is_err(),
                        "{text} on {network:?}"
                    );
                    assert!(
                        address_decode(&text, network).is_err(),
                        "{text} on {network:?}"
                    );
                }
            }
            other => panic!("unknown vector category {other}"),
        }
    }
    assert_eq!(count, 79, "ensure no primary vectors are silently omitted");
}

#[test]
fn bitcoin_core_base58_vectors() {
    let mut count = 0;
    for row in rows(include_str!("bitcoin_encoding_fixtures/base58_vectors.tsv")) {
        count += 1;
        let bytes = fixture_bytes(row[0]);
        let text = if row[1] == "-" { "" } else { row[1] };
        assert_eq!(base58_encode(&bytes).unwrap(), text);
        assert_eq!(base58_decode(text).unwrap(), bytes);
    }
    assert!(count >= 20);
}

#[test]
fn bitcoin_core_valid_address_vectors() {
    let mut count = 0;
    for row in rows(include_str!("bitcoin_encoding_fixtures/core_addresses.tsv")) {
        count += 1;
        let network = network_for_chain(row[0]);
        let address = address_decode(row[1], network).unwrap_or_else(|e| panic!("{}: {e}", row[1]));
        assert_eq!(script_bytes(&address), fixture_bytes(row[2]), "{}", row[1]);
        let expected = match &address {
            Address::Legacy(_) => row[1].to_owned(),
            Address::Witness(_) => row[1].to_ascii_lowercase(),
        };
        assert_eq!(address_encode(&address).unwrap(), expected);
    }
    assert!(count >= 30);
}

#[test]
fn bitcoin_core_invalid_address_vectors() {
    let mut count = 0;
    for row in rows(include_str!(
        "bitcoin_encoding_fixtures/core_invalid_addresses.tsv"
    )) {
        count += 1;
        let text = fixture_text(row[0]);
        for network in [
            Network::Mainnet,
            Network::Testnet,
            Network::Testnet4,
            Network::Signet,
            Network::Regtest,
        ] {
            assert!(
                address_decode(&text, network).is_err(),
                "{text:?} on {network:?}"
            );
        }
    }
    assert!(count >= 50);
}

#[test]
fn hex_contract_and_bounds() {
    let all: Vec<u8> = (0..=255).collect();
    let text = hex_encode(&all).unwrap();
    assert_eq!(hex_decode(&text).unwrap(), all);
    assert_eq!(hex_decode(&text.to_ascii_uppercase()).unwrap(), all);
    assert_eq!(hex_decode("aBcD").unwrap(), [0xab, 0xcd]);
    assert_eq!(hex_encode(&[]).unwrap(), "");
    assert_eq!(hex_decode("").unwrap(), []);
    assert_eq!(hex_decode("0"), Err(Error::OddHexLength));
    for text in ["0x", " 0", "0 ", "\n0", "0g", "＋", "é", "００"] {
        assert!(hex_decode(text).is_err(), "{text:?}");
    }
    assert_eq!(
        hex_decode("aZ"),
        Err(Error::InvalidCharacter {
            codec: Codec::Hex,
            index: 1,
            byte: b'Z'
        })
    );
    let bytes = vec![0xa5; MAX_DATA_BYTES];
    assert_eq!(hex_decode(&hex_encode(&bytes).unwrap()).unwrap(), bytes);
    assert!(matches!(
        hex_encode(&vec![0; MAX_DATA_BYTES + 1]),
        Err(Error::SizeLimit {
            codec: Codec::Hex,
            ..
        })
    ));
    assert!(matches!(
        hex_decode(&"00".repeat(MAX_DATA_BYTES + 1)),
        Err(Error::SizeLimit {
            codec: Codec::Hex,
            ..
        })
    ));
}

#[test]
fn sha256_published_short_long_and_large_vectors() {
    let examples: &[(&[u8], &str)] = &[
        (
            b"",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
           hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ];
    for (input, expected) in examples {
        assert_eq!(sha256(input).unwrap().to_vec(), fixture_bytes(expected));
        for stride in 1..=input.len().max(1) {
            let mut hash = Sha256::default();
            for fragment in input.chunks(stride) {
                hash.update(&[]).unwrap();
                hash.update(fragment).unwrap();
            }
            assert_eq!(
                hash.finalize().to_vec(),
                fixture_bytes(expected),
                "stride {stride}"
            );
        }
    }
    let million = vec![b'a'; 1_000_000];
    let expected =
        fixture_bytes("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    assert_eq!(sha256(&million).unwrap().to_vec(), expected);
    for stride in [1, 7, 55, 56, 63, 64, 65, 127, 8191] {
        let mut hash = Sha256::new();
        for fragment in million.chunks(stride) {
            hash.update(fragment).unwrap();
        }
        assert_eq!(hash.finalize().to_vec(), expected, "stride {stride}");
    }
    let mut prefix = Sha256::new();
    prefix.update(b"a").unwrap();
    let mut copy = prefix.clone();
    prefix.update(b"bc").unwrap();
    copy.update(b"bd").unwrap();
    assert_eq!(prefix.finalize(), sha256(b"abc").unwrap());
    assert_eq!(copy.finalize(), sha256(b"abd").unwrap());
}

#[test]
fn sha256_padding_boundaries_and_binary_data() {
    for length in [
        0, 1, 2, 31, 55, 56, 57, 63, 64, 65, 119, 120, 121, 127, 128, 129, 255, 1024, 65537,
    ] {
        let input: Vec<u8> = (0..length).map(|i| (i % 256) as u8).collect();
        let expected = sha256(&input).unwrap();
        for stride in [1, 2, 3, 7, 31, 55, 56, 63, 64, 65, 127, 129, 4096] {
            let mut hash = Sha256::new();
            for chunk in input.chunks(stride) {
                hash.update(chunk).unwrap();
            }
            assert_eq!(hash.finalize(), expected, "length {length} stride {stride}");
        }
    }
}

#[test]
fn base58_leading_zeros_invalid_alphabet_and_bounds() {
    assert_eq!(base58_encode(&[]).unwrap(), "");
    assert_eq!(base58_decode("").unwrap(), []);
    assert_eq!(base58_encode(&[0, 0, 1]).unwrap(), "112");
    assert_eq!(base58_decode("112").unwrap(), [0, 0, 1]);
    for zeros in [1, 2, 32, 128, MAX_BASE58_BYTES] {
        assert_eq!(base58_encode(&vec![0; zeros]).unwrap(), "1".repeat(zeros));
        assert_eq!(base58_decode(&"1".repeat(zeros)).unwrap(), vec![0; zeros]);
    }
    for text in [
        "0", "O", "I", "l", " 1", "1 ", "\t1", "1\n", "1\0", "é", "１", "1_",
    ] {
        assert!(
            matches!(base58_decode(text), Err(Error::InvalidCharacter { .. })),
            "{text:?}"
        );
    }
    assert_eq!(
        base58_decode("1O"),
        Err(Error::InvalidCharacter {
            codec: Codec::Base58,
            index: 1,
            byte: b'O'
        })
    );
    let maximum = vec![255; MAX_BASE58_BYTES];
    let text = base58_encode(&maximum).unwrap();
    assert_eq!(base58_decode(&text).unwrap(), maximum);
    assert!(matches!(
        base58_encode(&vec![0; MAX_BASE58_BYTES + 1]),
        Err(Error::SizeLimit { .. })
    ));
    assert!(matches!(
        base58_decode(&"1".repeat(MAX_BASE58_BYTES + 1)),
        Err(Error::SizeLimit { .. })
    ));
    assert!(matches!(
        base58_decode(&"z".repeat(MAX_BASE58_TEXT)),
        Err(Error::SizeLimit { .. })
    ));
    assert!(matches!(
        base58_decode(&"1".repeat(MAX_BASE58_TEXT + 1)),
        Err(Error::SizeLimit { .. })
    ));
}

#[test]
fn base58check_checksum_and_payload_contract() {
    let payload = fixture_bytes("00010966776006953d5567439e5e39f86a0d273bee");
    let expected = "16UwLL9Risc3QfPqBUvKofHmBQ7wMtjvM";
    assert_eq!(base58check_encode(&payload).unwrap(), expected);
    assert_eq!(base58check_decode(expected).unwrap(), payload);
    assert_eq!(
        base58check_decode(&base58check_encode(&[]).unwrap()).unwrap(),
        []
    );
    for text in ["", "1", "111"] {
        assert_eq!(base58check_decode(text), Err(Error::ChecksumTooShort));
    }
    for index in 0..expected.len() {
        let mut changed = expected.as_bytes().to_vec();
        changed[index] = if changed[index] == b'1' { b'2' } else { b'1' };
        assert_eq!(
            base58check_decode(std::str::from_utf8(&changed).unwrap()),
            Err(Error::InvalidChecksum)
        );
    }
    let payload = vec![0xab; MAX_BASE58CHECK_PAYLOAD];
    assert_eq!(
        base58check_decode(&base58check_encode(&payload).unwrap()).unwrap(),
        payload
    );
    assert!(matches!(
        base58check_encode(&vec![0; MAX_BASE58CHECK_PAYLOAD + 1]),
        Err(Error::SizeLimit { .. })
    ));
}

#[test]
fn bech32_hrp_symbols_case_and_bounds() {
    let text = bech32_encode("abc", &[0, 1, 31], ChecksumVariant::Bech32m).unwrap();
    let data = bech32_decode(&text).unwrap();
    assert_eq!(data.data, [0, 1, 31]);
    assert_eq!(data.hrp, "abc");
    assert_eq!(
        bech32_decode(&text.to_ascii_uppercase()).unwrap().hrp,
        "ABC"
    );
    assert_eq!(
        bech32_encode("ABC", &[], ChecksumVariant::Bech32),
        Err(Error::UppercaseHrp)
    );
    for hrp in ["", " ", "\x7f", "é"] {
        assert_eq!(
            bech32_encode(hrp, &[], ChecksumVariant::Bech32),
            Err(Error::InvalidHrp)
        );
    }
    assert_eq!(
        bech32_encode("bc", &[32], ChecksumVariant::Bech32),
        Err(Error::InvalidDataValue {
            index: 0,
            value: 32
        })
    );
    assert_eq!(bech32_decode("a12UEL5L"), Err(Error::MixedCase));
    assert_eq!(bech32_decode("abc"), Err(Error::MissingSeparator));
    assert_eq!(bech32_decode("a1qqqqq"), Err(Error::ChecksumTooShort));
    assert_eq!(bech32_decode("1qqqqqq"), Err(Error::InvalidHrp));
    let hrp = "a".repeat(83);
    assert_eq!(
        bech32_encode(&hrp, &[], ChecksumVariant::Bech32)
            .unwrap()
            .len(),
        90
    );
    assert!(bech32_encode(&hrp, &[0], ChecksumVariant::Bech32).is_err());
    let symbols = [0; 82];
    assert_eq!(
        bech32_encode("a", &symbols, ChecksumVariant::Bech32m)
            .unwrap()
            .len(),
        90
    );
    assert!(bech32_encode("a", &[0; 83], ChecksumVariant::Bech32m).is_err());
    assert!(matches!(
        bech32_decode(&"a".repeat(91)),
        Err(Error::SizeLimit { .. })
    ));
    // Last separator belongs to the HRP, even when the HRP contains '1'.
    let text = bech32_encode("a1b", &[0], ChecksumVariant::Bech32).unwrap();
    assert_eq!(bech32_decode(&text).unwrap().hrp, "a1b");
    for variant in [ChecksumVariant::Bech32, ChecksumVariant::Bech32m] {
        let text = bech32_encode("bc", &[1; 40], variant).unwrap();
        for index in 3..text.len() {
            let mut changed = text.as_bytes().to_vec();
            changed[index] = if changed[index] == b'q' { b'p' } else { b'q' };
            assert!(bech32_decode(std::str::from_utf8(&changed).unwrap()).is_err());
        }
    }
}

#[test]
fn bit_conversion_contract_and_padding() {
    assert_eq!(convert_bits(&[255], 8, 5, true).unwrap(), [31, 28]);
    assert_eq!(convert_bits(&[31, 28], 5, 8, false).unwrap(), [255]);
    assert_eq!(
        convert_bits(&[31, 29], 5, 8, false),
        Err(Error::InvalidPadding)
    );
    assert_eq!(convert_bits(&[0], 5, 8, false), Err(Error::InvalidPadding));
    assert_eq!(
        convert_bits(&[32], 5, 8, false),
        Err(Error::InvalidDataValue {
            index: 0,
            value: 32
        })
    );
    for (from, to) in [(0, 8), (8, 0), (9, 8), (8, 9)] {
        assert_eq!(
            convert_bits(&[], from, to, true),
            Err(Error::InvalidBitWidth)
        );
    }
    for length in 0..=128 {
        let bytes: Vec<u8> = (0..length).map(|i| (i * 97) as u8).collect();
        assert_eq!(
            convert_bits(&convert_bits(&bytes, 8, 5, true).unwrap(), 5, 8, false).unwrap(),
            bytes
        );
    }
    for from in 1..=8 {
        for to in 1..=8 {
            // 840 is divisible by every supported symbol width, so converting
            // back is unambiguous without a separately supplied symbol count.
            let data: Vec<u8> = (0..840u16)
                .map(|n| (n & ((1u16 << from) - 1)) as u8)
                .collect();
            assert_eq!(
                convert_bits(
                    &convert_bits(&data, from, to, true).unwrap(),
                    to,
                    from,
                    false
                )
                .unwrap(),
                data
            );
        }
    }
    assert!(matches!(
        convert_bits(&vec![0; MAX_DATA_BYTES + 1], 8, 5, true),
        Err(Error::SizeLimit { .. })
    ));
    assert!(matches!(
        convert_bits(&vec![0; MAX_DATA_BYTES], 8, 1, true),
        Err(Error::SizeLimit { .. })
    ));
}

#[test]
fn every_witness_version_length_and_network() {
    for network in [
        Network::Mainnet,
        Network::Testnet,
        Network::Testnet4,
        Network::Signet,
        Network::Regtest,
    ] {
        for version in 0..=17 {
            for length in 0..=41 {
                let program: Vec<u8> = (0..length).map(|i| (i * 13) as u8).collect();
                let result = witness_address_encode(network, version, &program);
                let valid = version <= 16
                    && (2..=40).contains(&length)
                    && (version != 0 || length == 20 || length == 32);
                if valid {
                    let text = result.unwrap();
                    let decoded = witness_address_decode(&text, network).unwrap();
                    assert_eq!(
                        decoded,
                        WitnessAddress {
                            network,
                            version,
                            program: program.clone()
                        }
                    );
                    assert_eq!(
                        address_decode(&text, network).unwrap(),
                        Address::Witness(decoded)
                    );
                    assert_eq!(
                        bech32_decode(&text).unwrap().variant,
                        if version == 0 {
                            ChecksumVariant::Bech32
                        } else {
                            ChecksumVariant::Bech32m
                        }
                    );
                } else {
                    assert!(
                        result.is_err(),
                        "{network:?} version {version} length {length}"
                    );
                }
            }
        }
    }
}

#[test]
fn witness_typed_errors_no_trimming_or_repair() {
    let network = Network::Mainnet;
    assert_eq!(
        witness_address_encode(network, 17, &[0; 20]),
        Err(Error::InvalidWitnessVersion(17))
    );
    assert_eq!(
        witness_address_encode(network, 0, &[0; 2]),
        Err(Error::InvalidWitnessProgramLength {
            version: 0,
            length: 2
        })
    );
    let text = witness_address_encode(network, 1, &[1; 32]).unwrap();
    for changed in [
        format!(" {text}"),
        format!("{text} "),
        format!("{text}\n"),
        format!("bitcoin:{text}"),
    ] {
        assert!(address_decode(&changed, network).is_err());
    }
    let generic = bech32_encode("tc", &[0; 33], ChecksumVariant::Bech32).unwrap();
    assert_eq!(
        witness_address_decode(&generic, network),
        Err(Error::UnknownWitnessHrp)
    );
    let empty = bech32_encode("bc", &[], ChecksumVariant::Bech32).unwrap();
    assert_eq!(
        witness_address_decode(&empty, network),
        Err(Error::MissingWitnessVersion)
    );
    for (version, variant) in [(0, ChecksumVariant::Bech32m), (1, ChecksumVariant::Bech32)] {
        let mut symbols = vec![version];
        symbols.extend(convert_bits(&[1; 20], 8, 5, true).unwrap());
        let text = bech32_encode("bc", &symbols, variant).unwrap();
        assert!(matches!(
            witness_address_decode(&text, network),
            Err(Error::WrongChecksumVariant { .. })
        ));
    }
    let regtest = witness_address_encode(Network::Regtest, 0, &[1; 20]).unwrap();
    assert!(regtest.starts_with("bcrt1q"));
    assert_eq!(
        witness_address_decode(&regtest, Network::Testnet),
        Err(Error::WrongNetwork {
            expected: Network::Testnet
        })
    );
}

#[test]
fn legacy_network_kind_length_and_version_rules() {
    let hash = [0x42; 20];
    for network in [
        Network::Mainnet,
        Network::Testnet,
        Network::Testnet4,
        Network::Signet,
        Network::Regtest,
    ] {
        for kind in [LegacyKind::P2pkh, LegacyKind::P2sh] {
            let text = legacy_address_encode(network, kind, &hash).unwrap();
            let decoded = legacy_address_decode(&text, network).unwrap();
            assert_eq!(
                decoded,
                LegacyAddress {
                    network,
                    kind,
                    hash
                }
            );
            assert_eq!(address_encode(&Address::Legacy(decoded)).unwrap(), text);
            let opposite = if network == Network::Mainnet {
                Network::Testnet
            } else {
                Network::Mainnet
            };
            assert_eq!(
                legacy_address_decode(&text, opposite),
                Err(Error::WrongNetwork { expected: opposite })
            );
        }
    }
    let mainnet = legacy_address_encode(Network::Mainnet, LegacyKind::P2pkh, &[0; 20]).unwrap();
    assert_eq!(mainnet, "1111111111111111111114oLvT2");
    let payload = [0x01; 21];
    let invalid_version = base58check_encode(&payload).unwrap();
    assert_eq!(
        legacy_address_decode(&invalid_version, Network::Mainnet),
        Err(Error::InvalidLegacyVersion(1))
    );
    let wrong_size = base58check_encode(&[0; 20]).unwrap();
    assert_eq!(
        legacy_address_decode(&wrong_size, Network::Mainnet),
        Err(Error::InvalidLegacyLength(20))
    );
    let missing_version = base58check_encode(&[]).unwrap();
    assert_eq!(
        legacy_address_decode(&missing_version, Network::Mainnet),
        Err(Error::InvalidLegacyLength(0))
    );
    // Encoded legacy test-network bytes provide no information to distinguish them.
    let testnet = legacy_address_encode(Network::Testnet, LegacyKind::P2sh, &hash).unwrap();
    for network in [Network::Testnet4, Network::Signet, Network::Regtest] {
        assert_eq!(
            legacy_address_decode(&testnet, network).unwrap().network,
            network
        );
    }
    let tb = witness_address_encode(Network::Signet, 1, &hash).unwrap();
    assert_eq!(
        witness_address_decode(&tb, Network::Testnet4)
            .unwrap()
            .network,
        Network::Testnet4
    );
}

#[test]
fn explicit_encoding_revalidates_mutated_addresses_and_errors_display() {
    let invalid = Address::Witness(WitnessAddress {
        network: Network::Mainnet,
        version: 255,
        program: vec![0; 32],
    });
    assert_eq!(
        address_encode(&invalid),
        Err(Error::InvalidWitnessVersion(255))
    );
    let error = hex_decode("zz").unwrap_err();
    let displayed: &dyn std::error::Error = &error;
    assert!(displayed.to_string().contains("offset 0"));
}
