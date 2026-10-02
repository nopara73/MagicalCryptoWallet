//! BIP21/retained-caller semantics. These tests deliberately use a typed test
//! validator so URI/amount failures are isolated from address codecs. Actual
//! checksums and network compatibility are tested in payment_uri_addresses.rs.
use mcw::bitcoin_encoding::Network;
use mcw::payment_uri::{
    self as uri, AddressValidator, Amount, AmountError, Error, FormatError, OptionalParameter,
    ParsedDestination, ParsingMode, PaymentDetails, PercentError, QueryError,
};

const A: &str = "SyntheticAddress123";
const STRICT: ParsingMode = ParsingMode::Bip21;
const COMPAT: ParsingMode = ParsingMode::ManagedCompatibility;

#[derive(Debug, Eq, PartialEq)]
struct SyntaxValidator;
impl AddressValidator for SyntaxValidator {
    type Address = (String, Network);
    type Error = &'static str;
    fn validate(&self, text: &str, network: Network) -> Result<Self::Address, Self::Error> {
        if text == A {
            Ok((text.to_owned(), network))
        } else {
            Err("test rejection")
        }
    }
}

fn parse(
    query: &str,
    mode: ParsingMode,
) -> Result<uri::PaymentUri<(String, Network)>, Error<&'static str>> {
    uri::parse_with_mode(
        &format!("bitcoin:{A}{query}"),
        Network::Mainnet,
        &SyntaxValidator,
        mode,
    )
}

#[test]
fn exact_decimal_examples_and_money_range() {
    for (text, sats, canonical) in [
        ("0", 0, "0"),
        ("0.00000000", 0, "0"),
        (".00000001", 1, "0.00000001"),
        ("0.00000100", 100, "0.000001"),
        (".1", 10_000_000, "0.1"),
        ("1.", 100_000_000, "1"),
        ("00001.23450000", 123_450_000, "1.2345"),
        ("20.3", 2_030_000_000, "20.3"),
        ("50.00", 5_000_000_000, "50"),
        (
            "20999999.99999999",
            2_099_999_999_999_999,
            "20999999.99999999",
        ),
        ("21000000.00000000", uri::MAX_SATOSHIS, "21000000"),
    ] {
        let amount = Amount::parse_btc(text).unwrap();
        assert_eq!(amount.satoshis(), sats, "{text}");
        assert_eq!(amount.to_btc(), canonical);
        assert_eq!(Amount::parse_btc(canonical).unwrap(), amount);
    }
    assert_eq!(
        Amount::from_satoshis(uri::MAX_SATOSHIS + 1),
        Err(AmountError::OutOfRange)
    );
    assert_eq!(
        Amount::from_satoshis(u64::MAX),
        Err(AmountError::OutOfRange)
    );
}

#[test]
fn invalid_decimals_are_never_rounded_or_normalized() {
    assert_eq!(Amount::parse_btc(""), Err(AmountError::Empty));
    for text in [
        ".", "+1", "-0", "-0.01", " 1", "1 ", "1,000", "100'000", "1e-8", "1E8", "1.2.3", "1..",
        "NaN", "inf", "١", "１", "0x1", "1\n", "\0",
    ] {
        assert_eq!(
            Amount::parse_btc(text),
            Err(AmountError::InvalidDecimal),
            "{text:?}"
        );
    }
    for text in ["0.000000001", "1.000000000", "0.123456789"] {
        assert_eq!(
            Amount::parse_btc(text),
            Err(AmountError::TooManyDecimalPlaces)
        );
    }
    for text in [
        "21000000.00000001",
        "21000001",
        "18446744073709551616",
        "99999999999999999999999",
    ] {
        assert_eq!(Amount::parse_btc(text), Err(AmountError::OutOfRange));
    }
    assert_eq!(
        Amount::parse_btc(&format!("{}1", "0".repeat(1_000)))
            .unwrap()
            .satoshis(),
        100_000_000
    );
}

#[test]
fn original_payload_survives_case_and_encoding() {
    let input = format!("bItCoIn:{A}?amount=0001.23000000&label=MiXeD%20Case&message=%2541&x=%2f");
    let result = uri::parse(&input, Network::Regtest, &SyntaxValidator).unwrap();
    assert_eq!(result.original_uri(), Some(input.as_str()));
    assert_eq!(result.destination().text(), A);
    assert_eq!(result.destination().network(), Network::Regtest);
    assert_eq!(
        result.destination().address(),
        &(A.to_owned(), Network::Regtest)
    );
    assert_eq!(result.details().label.as_deref(), Some("MiXeD Case"));
    assert_eq!(result.details().message.as_deref(), Some("%41"));
    assert_eq!(
        result.to_uri().unwrap(),
        format!("bitcoin:{A}?amount=1.23&label=MiXeD%20Case&message=%2541")
    );
    assert_eq!(
        result.optional_parameters(),
        &[OptionalParameter {
            name: "x".into(),
            value: Some("/".into())
        }]
    );
    assert_eq!(
        result.to_uri_with_optional_parameters().unwrap(),
        format!("bitcoin:{A}?amount=1.23&label=MiXeD%20Case&message=%2541&x=%2F")
    );
}

#[test]
fn scheme_is_case_insensitive_but_strict_query_keys_are_not() {
    for scheme in ["bitcoin", "BITCOIN", "BitCoin"] {
        assert!(uri::parse(&format!("{scheme}:{A}"), Network::Mainnet, &SyntaxValidator).is_ok());
    }
    let strict = parse("?Amount=2&Label=A&Message=B&REQ-x=1", STRICT).unwrap();
    assert_eq!(strict.details(), &PaymentDetails::default());
    assert_eq!(strict.optional_parameters().len(), 4);
    let compat = parse("?Amount=2&Label=A&Message=B", COMPAT).unwrap();
    assert_eq!(compat.details().amount.unwrap().satoshis(), 200_000_000);
    assert_eq!(compat.details().label.as_deref(), Some("A"));
    assert_eq!(compat.details().message.as_deref(), Some("B"));
    assert_eq!(parse("?REQ-x=1", COMPAT).unwrap_err().code(), 9);
    assert!(parse("?label=a&Label=b", STRICT).is_ok());
    for query in [
        "?label=a&Label=b",
        "?message=a&Message=b",
        "?unknown=1&UNKNOWN=2",
    ] {
        assert_eq!(parse(query, COMPAT).unwrap_err().code(), 6);
    }
}

#[test]
fn decoded_duplicates_and_required_parameters_fail_closed() {
    for mode in [STRICT, COMPAT] {
        for query in [
            "?amount=1&amount=2",
            "?label=&label=",
            "?message=a&message=b",
            "?unknown=first&unknown=second",
            "?flag&flag=2",
            "?amount=1&am%6funt=2",
        ] {
            assert_eq!(parse(query, mode).unwrap_err().code(), 6, "{query}");
        }
        for query in [
            "?req-thing",
            "?req-thing=",
            "?req-amount=1",
            "?req-label=x",
            "?%72eq-sp=x",
            "?req-pj=https%3A%2F%2Fexample.invalid",
        ] {
            assert_eq!(parse(query, mode).unwrap_err().code(), 9, "{query}");
        }
        assert_eq!(parse("?amount", mode).unwrap_err().code(), 7);
        assert_eq!(parse("?amount=", mode).unwrap_err().code(), 7);
        assert_eq!(parse("?label", mode).unwrap_err().code(), 5);
        assert_eq!(parse("?message", mode).unwrap_err().code(), 5);
        assert_eq!(parse("?=bad", mode).unwrap_err().code(), 5);
    }
}

#[test]
fn optional_data_never_changes_destination_and_is_omitted_by_retained_formatter() {
    let query = "?amount=0.02&label=Test%20%26%20label&pj=https%3A%2F%2Fexample.invalid&pjos=0&sp=unsupported&lightning=opaque&flag&empty=";
    let result = parse(query, COMPAT).unwrap();
    assert_eq!(result.destination().text(), A);
    assert_eq!(result.details().amount.unwrap().satoshis(), 2_000_000);
    assert_eq!(result.optional_parameters().len(), 6);
    assert_eq!(result.optional_parameters()[4].value, None);
    assert_eq!(result.optional_parameters()[5].value.as_deref(), Some(""));
    assert_eq!(
        result.to_uri().unwrap(),
        format!("bitcoin:{A}?amount=0.02&label=Test%20%26%20label")
    );
    assert!(parse("?&&label=&&message=&", STRICT).is_ok());
    assert_eq!(
        parse("?amount=0", STRICT).unwrap().to_uri().unwrap(),
        format!("bitcoin:{A}?amount=0")
    );
    assert_eq!(
        parse("", STRICT).unwrap().to_uri().unwrap(),
        format!("bitcoin:{A}")
    );
    assert_eq!(
        parse("?", STRICT).unwrap().to_uri().unwrap(),
        format!("bitcoin:{A}")
    );
}

#[test]
fn utf8_labels_and_messages_are_lossless() {
    let text = "Árvíztűrő tükörfúrógép 東京 🦀 & = + ? # % /";
    let encoded = uri::percent_encode(text).unwrap();
    assert!(encoded.contains("%C3%81"));
    assert!(encoded.contains("%F0%9F%A6%80"));
    assert!(encoded.contains("%26%20%3D%20%2B"));
    let result = parse(&format!("?label={encoded}&message={encoded}"), STRICT).unwrap();
    assert_eq!(result.details().label.as_deref(), Some(text));
    assert_eq!(result.details().message.as_deref(), Some(text));
    assert_eq!(uri::percent_decode(&encoded, STRICT).unwrap(), text);
    assert_eq!(
        uri::percent_decode("%C3%a1%F0%9f%a6%80", STRICT).unwrap(),
        "á🦀"
    );
    assert_eq!(uri::percent_decode("A+%2bB", STRICT).unwrap(), "A++B");
    assert_eq!(uri::percent_decode("A+%2bB", COMPAT).unwrap(), "A +B");
    assert_eq!(
        parse("?label=A+B", STRICT)
            .unwrap()
            .details()
            .label
            .as_deref(),
        Some("A+B")
    );
    assert_eq!(
        parse("?label=A+B", COMPAT)
            .unwrap()
            .details()
            .label
            .as_deref(),
        Some("A B")
    );
    assert_eq!(
        parse("?label=東京 = x", COMPAT)
            .unwrap()
            .details()
            .label
            .as_deref(),
        Some("東京 = x")
    );
    assert_eq!(parse("?label=東京", STRICT).unwrap_err().code(), 5);
    assert_eq!(parse("?label=A B", STRICT).unwrap_err().code(), 5);
    assert_eq!(parse("?label=a=b", STRICT).unwrap_err().code(), 5);
    assert_eq!(
        parse("?label=a=b", COMPAT)
            .unwrap()
            .details()
            .label
            .as_deref(),
        Some("a=b")
    );
    assert_eq!(
        parse("?label=%00", STRICT)
            .unwrap()
            .details()
            .label
            .as_deref(),
        Some("\0")
    );
}

#[test]
fn malformed_escapes_and_invalid_utf8_are_rejected_without_replacement() {
    for mode in [STRICT, COMPAT] {
        for encoded in ["%", "%0", "%GG", "%0g", "a%u0041", "%F", "%1/", "%+1"] {
            assert!(
                matches!(
                    uri::percent_decode(encoded, mode),
                    Err(PercentError::InvalidEscape { .. })
                ),
                "{encoded}"
            );
            assert_eq!(
                parse(&format!("?label={encoded}"), mode)
                    .unwrap_err()
                    .code(),
                5
            );
            assert_eq!(
                parse(&format!("?unknown={encoded}"), mode)
                    .unwrap_err()
                    .code(),
                5
            );
        }
        for encoded in [
            "%80",
            "%FF",
            "%C0%AF",
            "%E0%80%80",
            "%ED%A0%80",
            "%F4%90%80%80",
            "%E2%82",
        ] {
            assert!(
                matches!(
                    uri::percent_decode(encoded, mode),
                    Err(PercentError::InvalidUtf8 { .. })
                ),
                "{encoded}"
            );
            assert_eq!(
                parse(&format!("?message={encoded}"), mode)
                    .unwrap_err()
                    .code(),
                5
            );
        }
        assert_eq!(parse("?unknown%GG=x", mode).unwrap_err().code(), 5);
        assert_eq!(parse("?label=x\n", mode).unwrap_err().code(), 5);
    }
    assert_eq!(
        uri::percent_decode("ab%GG", STRICT),
        Err(PercentError::InvalidEscape { offset: 2 })
    );
    assert_eq!(
        uri::percent_decode("ab%FF", STRICT),
        Err(PercentError::InvalidUtf8 { offset: 2 })
    );
    assert_eq!(uri::percent_decode("%2541", STRICT).unwrap(), "%41");
    assert_eq!(
        uri::percent_encode(&"x".repeat(uri::MAX_COMPONENT_BYTES + 1)),
        Err(PercentError::TooLong)
    );
    assert_eq!(
        uri::percent_decode(&"x".repeat(uri::MAX_COMPONENT_BYTES + 1), STRICT),
        Err(PercentError::TooLong)
    );
}

#[test]
fn uri_structure_and_error_precedence_are_explicit() {
    for input in [
        "",
        "garbage",
        A,
        "1bitcoin:x",
        " bitcoin:x",
        "bitcoin://host/address",
        "bitcoin:x#fragment",
    ] {
        assert_eq!(
            uri::parse(input, Network::Mainnet, &SyntaxValidator)
                .unwrap_err()
                .code(),
            1,
            "{input}"
        );
    }
    assert_eq!(
        uri::parse("https:x", Network::Mainnet, &SyntaxValidator)
            .unwrap_err()
            .code(),
        2
    );
    for input in ["bitcoin:", "bitcoin:?amount=1", "bitcoin:?sp=unsupported"] {
        assert_eq!(
            uri::parse(input, Network::Mainnet, &SyntaxValidator)
                .unwrap_err()
                .code(),
            3
        );
    }
    for input in [
        "bitcoin:WrongAddress?amount=",
        "bitcoin:WrongAddress?req-x=1",
        "bitcoin:WrongAddress?label=%GG",
    ] {
        assert_eq!(
            uri::parse(input, Network::Mainnet, &SyntaxValidator)
                .unwrap_err()
                .code(),
            4
        );
    }
    for path in ["/address", "address/", "a%41", "a:b", "a b"] {
        assert_eq!(
            uri::parse(
                &format!("bitcoin:{path}"),
                Network::Mainnet,
                &SyntaxValidator
            )
            .unwrap_err()
            .code(),
            4
        );
    }
    assert!(matches!(
        parse("?amount=1%2C000", STRICT),
        Err(Error::InvalidAmountValue(_))
    ));
    assert!(matches!(
        parse("?label=%FF", STRICT),
        Err(Error::InvalidQuery(QueryError::Percent(_)))
    ));
    assert_eq!(parse("?amount=%20", COMPAT).unwrap_err().code(), 7);
    assert_eq!(parse("?amount=%20", STRICT).unwrap_err().code(), 8);
}

#[test]
fn input_entry_point_and_utf16_limit_match_retained_behavior() {
    let trimmed = format!(" \tBITCOIN:{A}?label=Mixed%20Case\r\n");
    let result = uri::parse_input(&trimmed, Network::Signet, &SyntaxValidator, COMPAT).unwrap();
    let ParsedDestination::PaymentUri(request) = result else {
        panic!("URI expected")
    };
    assert_eq!(request.original_uri(), Some(trimmed.trim()));
    let ParsedDestination::Address(address) = uri::parse_input(
        &format!("  {A}  "),
        Network::Testnet4,
        &SyntaxValidator,
        COMPAT,
    )
    .unwrap() else {
        panic!("address expected")
    };
    assert_eq!(address.network(), Network::Testnet4);
    assert_eq!(address.text(), A);
    assert_eq!(
        uri::parse_input(" \n ", Network::Mainnet, &SyntaxValidator, COMPAT)
            .unwrap_err()
            .code(),
        11
    );
    let prefix = format!("bitcoin:{A}?label=");
    let room = uri::MAX_INPUT_UTF16_UNITS - prefix.len();
    assert!(
        uri::parse_compatible(
            &format!("{prefix}{}", "x".repeat(room)),
            Network::Mainnet,
            &SyntaxValidator
        )
        .is_ok()
    );
    assert_eq!(
        uri::parse_compatible(
            &format!("{prefix}{}", "x".repeat(room + 1)),
            Network::Mainnet,
            &SyntaxValidator
        )
        .unwrap_err()
        .code(),
        10
    );
    assert!(
        uri::parse_compatible(
            &format!("{prefix}{}", "🦀".repeat(room / 2)),
            Network::Mainnet,
            &SyntaxValidator
        )
        .is_ok()
    );
    assert_eq!(
        uri::parse_compatible(
            &format!("{prefix}{}", "🦀".repeat(room / 2 + 1)),
            Network::Mainnet,
            &SyntaxValidator
        )
        .unwrap_err()
        .code(),
        10
    );
}

#[test]
fn builder_and_formatter_are_checked_and_preserve_empty_values() {
    let details = PaymentDetails {
        amount: Some(Amount::ZERO),
        label: Some("A&B=東京".into()),
        message: Some("".into()),
    };
    let request = uri::new_request(A, Network::Testnet, details.clone(), &SyntaxValidator).unwrap();
    assert_eq!(request.details(), &details);
    assert_eq!(request.original_uri(), None);
    assert_eq!(
        request.to_uri().unwrap(),
        format!("bitcoin:{A}?amount=0&label=A%26B%3D%E6%9D%B1%E4%BA%AC&message=")
    );
    assert_eq!(
        uri::new_request("WrongAddress", Network::Mainnet, details, &SyntaxValidator)
            .unwrap_err()
            .code(),
        4
    );
    assert_eq!(
        uri::new_request(
            A,
            Network::Mainnet,
            PaymentDetails {
                label: Some("x".repeat(1_000)),
                ..Default::default()
            },
            &SyntaxValidator
        )
        .unwrap_err()
        .code(),
        10
    );
    // Compatible raw Unicode can fit the input limit but expand beyond it on
    // serialization; the formatter reports this instead of emitting bad output.
    let request = parse(&format!("?label={}", "東京".repeat(200)), COMPAT).unwrap();
    assert_eq!(request.to_uri(), Err(FormatError::InputTooLong));
}

#[test]
fn every_single_satoshi_and_many_large_amounts_round_trip() {
    for sats in (0..10_000).chain([uri::MAX_SATOSHIS - 1, uri::MAX_SATOSHIS]) {
        let amount = Amount::from_satoshis(sats).unwrap();
        assert_eq!(
            Amount::parse_btc(&amount.to_btc()).unwrap().satoshis(),
            sats
        );
    }
    let mut state = 0x007a_11c0_de21_u64;
    for _ in 0..20_000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let sats = state % (uri::MAX_SATOSHIS + 1);
        let amount = Amount::from_satoshis(sats).unwrap();
        assert_eq!(
            Amount::parse_btc(&amount.to_btc()).unwrap().satoshis(),
            sats
        );
    }
}

#[test]
fn arbitrary_unicode_and_delimiters_do_not_panic_or_double_decode() {
    let atoms = [
        "", "%", "%20", "%FF", "%2541", "?", "&", "=", "#", "+", "é", "🦀", "\0", "\n", "req-",
        "amount",
    ];
    for a in atoms {
        for b in atoms {
            for c in atoms {
                let query = format!("?{a}{b}={c}&label={a}{c}");
                let _ = parse(&query, STRICT);
                let _ = parse(&query, COMPAT);
                let text = format!("{a}{b}{c}");
                let encoded = uri::percent_encode(&text).unwrap();
                assert_eq!(uri::percent_decode(&encoded, STRICT).unwrap(), text);
            }
        }
    }
}
