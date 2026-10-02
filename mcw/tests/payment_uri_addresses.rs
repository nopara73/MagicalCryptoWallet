//! End-to-end URI tests using the real first-party checksum/address service.
//! Fixtures originate in retained managed tests and BIP350. No funds, wallet
//! files, network connections, private keys, or transaction construction occur.
use mcw::bitcoin_encoding::{
    self as encoding, Address, LegacyAddress, LegacyKind, Network, WitnessAddress,
};
use mcw::payment_uri::{
    self as uri, Amount, BitcoinAddressValidator, Error, ParsedDestination, ParsingMode,
    PaymentDetails,
};

const BASE58_MAIN: &str = "18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX";
const V0_MAIN: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
const V0_TEST: &str = "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx";
// BIP350 valid witness version 1, 32-byte program (secp256k1 generator x).
const V1_MAIN: &str = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";

#[test]
fn retained_real_address_families_and_amounts() {
    for (text, network) in [
        (BASE58_MAIN, Network::Mainnet),
        ("17VZNX1SN5NtKa8UQFxwQbFeFc3iqRYhem", Network::Mainnet),
        ("3EktnHQD7RiAE6uzMj2ZifT9YgRrkSgzQX", Network::Mainnet),
        (V0_MAIN, Network::Mainnet),
        (
            "bc1qp6ejw8ptj9l9pkscmlf8fhhkrrjeawgpyjvtq8",
            Network::Mainnet,
        ),
        ("mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP", Network::Testnet),
        ("mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", Network::Testnet),
        ("2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc", Network::Testnet),
        (V0_TEST, Network::Testnet),
        (V1_MAIN, Network::Mainnet),
    ] {
        let input = format!(
            "bitcoin:{text}?amount=20.3&label=Luke-Jr&message=Donation%20for%20project%20xyz"
        );
        let result = uri::parse(&input, network, &BitcoinAddressValidator).unwrap();
        assert_eq!(result.destination().text(), text);
        assert_eq!(result.destination().network(), network);
        assert_eq!(
            encoding::address_encode(result.destination().address()).unwrap(),
            text
        );
        assert_eq!(result.details().amount.unwrap().satoshis(), 2_030_000_000);
        assert_eq!(result.details().label.as_deref(), Some("Luke-Jr"));
        assert_eq!(
            result.details().message.as_deref(),
            Some("Donation for project xyz")
        );
        let formatted = result.to_uri().unwrap();
        let round_trip = uri::parse(&formatted, network, &BitcoinAddressValidator).unwrap();
        assert_eq!(round_trip.destination(), result.destination());
        assert_eq!(round_trip.details(), result.details());
    }
}

#[test]
fn uppercase_qr_address_is_valid_and_original_case_survives() {
    for lowercase in [
        V0_MAIN,
        V1_MAIN,
        "bc1qufgy354j3kmvuch987xe4s40836x3h0lg8f5n2",
    ] {
        let uppercase = lowercase.to_ascii_uppercase();
        let input = format!("BITCOIN:{uppercase}?Label=MiXeD%20Case");
        let result =
            uri::parse_compatible(&input, Network::Mainnet, &BitcoinAddressValidator).unwrap();
        assert_eq!(result.original_uri(), Some(input.as_str()));
        assert_eq!(result.destination().text(), uppercase);
        assert_eq!(
            encoding::address_encode(result.destination().address()).unwrap(),
            lowercase
        );
        assert_eq!(result.details().label.as_deref(), Some("MiXeD Case"));
        assert_eq!(
            result.to_uri().unwrap(),
            format!("bitcoin:{uppercase}?label=MiXeD%20Case")
        );
    }
    assert!(
        uri::parse(
            &format!("bitcoin:BC{}", &V0_MAIN[2..]),
            Network::Mainnet,
            &BitcoinAddressValidator
        )
        .is_err()
    );
    // Lowercasing an entire URI would corrupt this case-sensitive Base58 payload.
    assert!(
        uri::parse(
            &format!("bitcoin:{}", BASE58_MAIN.to_ascii_lowercase()),
            Network::Mainnet,
            &BitcoinAddressValidator
        )
        .is_err()
    );
}

#[test]
fn expected_network_is_explicit_and_shared_prefixes_are_documented() {
    for main_address in [BASE58_MAIN, V0_MAIN, V1_MAIN] {
        for network in [
            Network::Testnet,
            Network::Testnet4,
            Network::Signet,
            Network::Regtest,
        ] {
            assert!(matches!(
                uri::parse(
                    &format!("bitcoin:{main_address}"),
                    network,
                    &BitcoinAddressValidator
                ),
                Err(Error::InvalidAddress(_))
            ));
        }
    }
    // Testnet, testnet4 and signet witness encodings share tb. Legacy prefixes
    // are also shared by regtest, so no parser can distinguish these networks
    // from the address alone; the selected network is carried in the result.
    for network in [Network::Testnet, Network::Testnet4, Network::Signet] {
        assert!(
            uri::parse(
                &format!("bitcoin:{V0_TEST}"),
                network,
                &BitcoinAddressValidator
            )
            .is_ok()
        );
    }
    assert!(
        uri::parse(
            &format!("bitcoin:{V0_TEST}"),
            Network::Regtest,
            &BitcoinAddressValidator
        )
        .is_err()
    );
    for network in [
        Network::Testnet,
        Network::Testnet4,
        Network::Signet,
        Network::Regtest,
    ] {
        let request = uri::parse(
            "bitcoin:mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn",
            network,
            &BitcoinAddressValidator,
        )
        .unwrap();
        assert_eq!(request.destination().network(), network);
    }
    let synthetic = Address::Witness(WitnessAddress {
        network: Network::Regtest,
        version: 0,
        program: vec![0x21; 20],
    });
    let text = encoding::address_encode(&synthetic).unwrap();
    assert!(text.starts_with("bcrt1"));
    assert_eq!(
        uri::parse(
            &format!("bitcoin:{text}"),
            Network::Regtest,
            &BitcoinAddressValidator
        )
        .unwrap()
        .destination()
        .address(),
        &synthetic
    );
    assert!(
        uri::parse(
            &format!("bitcoin:{text}"),
            Network::Testnet,
            &BitcoinAddressValidator
        )
        .is_err()
    );
}

#[test]
fn base58_checksum_mutations_are_rejected_before_uri_amounts() {
    for text in [
        BASE58_MAIN,
        "3EktnHQD7RiAE6uzMj2ZifT9YgRrkSgzQX",
        "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn",
    ] {
        let network = if text.starts_with('m') {
            Network::Testnet
        } else {
            Network::Mainnet
        };
        for mutation in [
            text[1..].to_string(),
            format!("{}x{}", &text[..4], &text[5..]),
            format!("{text}1"),
        ] {
            assert_eq!(
                uri::parse(
                    &format!("bitcoin:{mutation}?amount="),
                    network,
                    &BitcoinAddressValidator
                )
                .unwrap_err()
                .code(),
                4
            );
        }
    }
}

#[test]
fn bip350_wrong_checksum_mixed_case_padding_and_lengths_are_rejected() {
    for (text, network) in [
        (
            "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqh2y7hd",
            Network::Mainnet,
        ),
        (
            "bc1p38j9r5y49hruaue7wxjce0updqjuyyx0kh56v8s25huc6995vvpql3jow4",
            Network::Mainnet,
        ),
        ("bc1pw5dgrnzv", Network::Mainnet),
        (
            "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7v8n0nx0muaewav253zgeav",
            Network::Mainnet,
        ),
        (
            "tb1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vq47Zagq",
            Network::Testnet,
        ),
        (
            "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7v07qwwzcrf",
            Network::Mainnet,
        ),
        (
            "tb1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vpggkg4j",
            Network::Testnet,
        ),
    ] {
        assert_eq!(
            uri::parse(
                &format!("bitcoin:{text}?amount=1"),
                network,
                &BitcoinAddressValidator
            )
            .unwrap_err()
            .code(),
            4,
            "{text}"
        );
    }
}

#[test]
fn all_synthetic_address_versions_and_networks_round_trip_through_uri() {
    for network in [
        Network::Mainnet,
        Network::Testnet,
        Network::Testnet4,
        Network::Signet,
        Network::Regtest,
    ] {
        for version in 0..=16 {
            let address = Address::Witness(WitnessAddress {
                network,
                version,
                program: vec![version + 1; if version == 0 { 20 } else { 32 }],
            });
            let text = encoding::address_encode(&address).unwrap();
            let request = uri::new_request(
                &text,
                network,
                PaymentDetails {
                    amount: Some(Amount::from_satoshis(1).unwrap()),
                    label: Some("Synthetic test 東京".into()),
                    message: None,
                },
                &BitcoinAddressValidator,
            )
            .unwrap();
            let round_trip = uri::parse(
                &request.to_uri().unwrap(),
                network,
                &BitcoinAddressValidator,
            )
            .unwrap();
            assert_eq!(round_trip.destination().address(), &address);
            assert_eq!(round_trip.details(), request.details());
        }
        for kind in [LegacyKind::P2pkh, LegacyKind::P2sh] {
            let address = Address::Legacy(LegacyAddress {
                network,
                kind,
                hash: [0x42; 20],
            });
            let text = encoding::address_encode(&address).unwrap();
            assert_eq!(
                uri::parse(
                    &format!("bitcoin:{text}"),
                    network,
                    &BitcoinAddressValidator
                )
                .unwrap()
                .destination()
                .address(),
                &address
            );
        }
    }
}

#[test]
fn bip21_document_intentionally_invalid_address_is_not_accepted_as_payment() {
    // BIP21's own example warns that its addresses intentionally have invalid
    // checksums. A syntax-only parser accepting this is not address validation.
    assert!(matches!(
        uri::parse(
            "bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=20.3",
            Network::Mainnet,
            &BitcoinAddressValidator
        ),
        Err(Error::InvalidAddress(_))
    ));
}

#[test]
fn retained_extensions_and_bare_address_entry_point_use_real_validation() {
    let original = format!(
        "bitcoin:{BASE58_MAIN}?amount=0.02&label=Test%20%26%20label&pj=https%3A%2F%2Fexample.invalid&pjos=0&sp=unsupported&lightning=opaque"
    );
    let request =
        uri::parse_compatible(&original, Network::Mainnet, &BitcoinAddressValidator).unwrap();
    assert_eq!(
        request.to_uri().unwrap(),
        format!("bitcoin:{BASE58_MAIN}?amount=0.02&label=Test%20%26%20label")
    );
    for input in [
        "sp1unsupported",
        "bitcoin:sp1unsupported",
        "bitcoin:?sp=unsupported",
        "bitcoin:bc1?req-sp=x",
    ] {
        assert!(
            uri::parse_input(
                input,
                Network::Mainnet,
                &BitcoinAddressValidator,
                ParsingMode::ManagedCompatibility
            )
            .is_err()
        );
    }
    let ParsedDestination::Address(address) = uri::parse_input(
        &format!("  {BASE58_MAIN}  "),
        Network::Mainnet,
        &BitcoinAddressValidator,
        ParsingMode::ManagedCompatibility,
    )
    .unwrap() else {
        panic!("bare address expected")
    };
    assert_eq!(address.text(), BASE58_MAIN);
}
