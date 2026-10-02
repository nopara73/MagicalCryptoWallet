//! Actual shipping JSON source, compiled either as an integration test or by
//! rustc --edition=2024 --test <absolute-path-to-this-file> -o <ignored-path>.
//! No second Cargo package, test crate dependency, or shipping executable.

#[path = "../src/json.rs"]
#[allow(dead_code)]
mod json;

use json::compat::{self, DecodeError, FieldCase, IntegerInput};
use json::*;
use std::path::{Path, PathBuf};

fn doc(text: &str) -> Document {
    parse(text.as_bytes(), &ParseOptions::default()).unwrap()
}
fn value(text: &str) -> Value {
    doc(text).into_root()
}
fn number(text: &str) -> Number {
    Number::parse(text).unwrap()
}
fn fixtures() -> PathBuf {
    Path::new(file!()).parent().unwrap().join("json_vectors")
}
fn allow_duplicates() -> ParseOptions {
    ParseOptions {
        duplicates: DuplicatePolicy::Preserve,
        ..ParseOptions::default()
    }
}
fn preserving_encoder() -> SerializeOptions {
    SerializeOptions {
        duplicates: DuplicatePolicy::Preserve,
        ..SerializeOptions::default()
    }
}
fn assert_error(text: &str, kind: ErrorKind) {
    assert_eq!(
        parse(text.as_bytes(), &ParseOptions::default())
            .unwrap_err()
            .kind,
        kind,
        "{text:?}"
    );
}

#[test]
fn independent_json_testsuite_all_318_vectors() {
    let directory = fixtures().join("JSONTestSuite/test_parsing");
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let mut yes = 0;
    let mut no = 0;
    let mut implementation = (0, 0);
    for path in files {
        let name = path.file_name().unwrap().to_str().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let result = parse(&bytes, &allow_duplicates());
        match &name[..1] {
            "y" => {
                assert!(result.is_ok(), "{name}: {result:?}");
                yes += 1;
            }
            "n" => {
                assert!(result.is_err(), "accepted invalid vector {name}");
                no += 1;
            }
            "i" => {
                if result.is_ok() {
                    implementation.0 += 1;
                } else {
                    implementation.1 += 1;
                }
            }
            other => panic!("unexpected fixture category {other}"),
        }
        if let Ok(document) = result {
            assert_eq!(document.source().as_bytes(), bytes, "source replay: {name}");
            let encoded = serialize(document.root(), &preserving_encoder()).unwrap();
            let reparsed = parse(encoded.as_bytes(), &allow_duplicates()).unwrap();
            assert_eq!(reparsed.root(), document.root(), "round trip: {name}");
            assert_eq!(
                serialize(reparsed.root(), &preserving_encoder()).unwrap(),
                encoded,
                "determinism: {name}"
            );
        }
    }
    assert_eq!(yes, 95);
    assert_eq!(no, 188);
    assert_eq!(implementation.0 + implementation.1, 35);
    println!(
        "JSONTestSuite: {yes} required accept, {no} required reject, {} optional accept, {} optional reject",
        implementation.0, implementation.1
    );
}

#[test]
fn rfc8259_scalar_roots_and_nested_values() {
    for input in [
        "null", "true", "false", "0", "-12.5e+2", "\"text\"", "[]", "{}",
    ] {
        assert_eq!(
            serialize(doc(input).root(), &SerializeOptions::default()).unwrap(),
            input
        );
    }
    let document = doc("\r\n {\"a\":[null,true,false,{\"b\":-0.125E+03}]}\t ");
    assert_eq!(
        document.source(),
        "\r\n {\"a\":[null,true,false,{\"b\":-0.125E+03}]}\t "
    );
    assert_eq!(
        serialize(document.root(), &SerializeOptions::default()).unwrap(),
        "{\"a\":[null,true,false,{\"b\":-0.125E+03}]}"
    );
}

#[test]
fn entire_number_lexeme_survives() {
    for token in [
        "-0",
        "-0.000",
        "1.2300",
        "1E+0003",
        "1e-400",
        "1e400",
        "9007199254740993",
        "18446744073709551615",
        "-170141183460469231731687303715884105728",
        "1e999999999999999999999999",
    ] {
        let document = doc(token);
        assert_eq!(document.root().as_number().unwrap().as_str(), token);
        assert_eq!(
            serialize(document.root(), &SerializeOptions::default()).unwrap(),
            token
        );
    }
    assert_ne!(number("1"), number("1.0"));
    assert_ne!(number("0"), number("-0"));
}

#[test]
fn exact_integer_limits_above_binary_float_precision() {
    assert_eq!(
        number("9007199254740993").to_i64_exact(),
        Ok(9_007_199_254_740_993)
    );
    assert_eq!(number("9223372036854775807").to_i64_exact(), Ok(i64::MAX));
    assert_eq!(number("-9223372036854775808").to_i64_exact(), Ok(i64::MIN));
    assert_eq!(number("18446744073709551615").to_u64_exact(), Ok(u64::MAX));
    assert_eq!(
        number("18446744073709551616").to_u64_exact(),
        Err(NumberConversionError::OutOfRange)
    );
    assert_eq!(
        number("9223372036854775808").to_i64_exact(),
        Err(NumberConversionError::OutOfRange)
    );
    assert_eq!(
        number("-1").to_u64_exact(),
        Err(NumberConversionError::OutOfRange)
    );
    assert_eq!(
        number(&i128::MAX.to_string()).to_i128_exact(),
        Ok(i128::MAX)
    );
    assert_eq!(
        number(&i128::MIN.to_string()).to_i128_exact(),
        Ok(i128::MIN)
    );
    assert_eq!(
        number("170141183460469231731687303715884105728").to_i128_exact(),
        Err(NumberConversionError::OutOfRange)
    );
    assert_eq!(
        number("-170141183460469231731687303715884105729").to_i128_exact(),
        Err(NumberConversionError::OutOfRange)
    );
}

#[test]
fn exact_decimal_exponents_and_satoshi_conversions() {
    for (token, units) in [
        ("0.00000001", 1),
        ("1e-8", 1),
        ("1.00000000", 100_000_000),
        ("21000000", 2_100_000_000_000_000),
        ("1.23456789", 123_456_789),
        ("-0.1", -10_000_000),
        ("123456789000e-11", 123_456_789),
        ("0.000000010000", 1),
        ("-0.00E+999999999999999999999", 0),
    ] {
        assert_eq!(number(token).to_scaled_i128(8), Ok(units), "{token}");
    }
    for token in ["0.000000001", "1e-9", "1.000000001", "-0.000000009"] {
        assert_eq!(
            number(token).to_scaled_i128(8),
            Err(NumberConversionError::NonIntegral)
        );
    }
    assert_eq!(
        number("1e400").to_i128_exact(),
        Err(NumberConversionError::OutOfRange)
    );
    assert_eq!(
        number("1e-400").to_i128_exact(),
        Err(NumberConversionError::NonIntegral)
    );
    assert_eq!(
        number("1e999999999999999999999999").to_i128_exact(),
        Err(NumberConversionError::ExponentOutOfRange)
    );
    assert_eq!(
        number("0e-999999999999999999999999").to_scaled_i128(u32::MAX),
        Ok(0)
    );
    assert_eq!(number("1.000").to_i128_exact(), Ok(1));
    assert_eq!(number("100e-2").to_i128_exact(), Ok(1));
    assert_eq!(
        number("10e-2").to_i128_exact(),
        Err(NumberConversionError::NonIntegral)
    );
}

#[test]
fn fixed_point_construction_preserves_requested_scale() {
    for units in [
        i128::MIN,
        -2_100_000_000_000_000,
        -1,
        0,
        1,
        123_456_789,
        i128::MAX,
    ] {
        for scale in [0, 1, 8, 38, 100, 4093] {
            let n = Number::from_scaled_i128(units, scale).unwrap();
            assert_eq!(n.to_scaled_i128(scale), Ok(units), "{units} scale {scale}");
            assert_eq!(number(n.as_str()), n);
        }
    }
    assert_eq!(
        Number::from_scaled_i128(1, 8).unwrap().as_str(),
        "0.00000001"
    );
    assert_eq!(
        Number::from_scaled_i128(-123_456_789, 8).unwrap().as_str(),
        "-1.23456789"
    );
    assert_eq!(
        Number::from_scaled_i128(0, 8).unwrap().as_str(),
        "0.00000000"
    );
    assert_eq!(
        Number::from_scaled_i128(1, u32::MAX),
        Err(ErrorKind::LimitExceeded(LimitKind::NumberBytes))
    );
}

#[test]
fn invalid_numbers_never_enter_number_type() {
    for token in [
        "", "-", "+1", "01", "-01", ".1", "1.", "1e", "1e+", "1e-", "--1", "1 2", "NaN",
        "Infinity", "0x10", " 1", "1\n", "١",
    ] {
        assert!(Number::parse(token).is_err(), "{token:?}");
    }
    assert!(Number::parse(&"1".repeat(4097)).is_err());
}

#[test]
fn string_tokens_and_unicode_are_lossless() {
    let input = r#"{"\u0061":"\uD834\uDd1E","slash":"\/","controls":"\b\f\n\r\t\u0000","quote":"\"\\","nfc":"é","nfd":"e\u0301"}"#;
    let document = doc(input);
    assert_eq!(
        serialize(document.root(), &SerializeOptions::default()).unwrap(),
        input
    );
    let object = document.root().as_object().unwrap();
    assert_eq!(object.members()[0].0.as_str(), "a");
    assert_eq!(object.members()[0].0.original_token(), Some(r#""\u0061""#));
    assert_eq!(object.get_unique("a").unwrap().unwrap().as_str(), Some("𝄞"));
    assert_ne!(
        object.get_unique("nfc").unwrap(),
        object.get_unique("nfd").unwrap()
    );
}

#[test]
fn every_unicode_scalar_round_trips_in_minimal_and_ascii_encodings() {
    let all_scalars: String = (0..=0x10ffff).filter_map(char::from_u32).collect();
    let tree = Value::string(all_scalars.clone());
    let limits = Limits {
        string_bytes: 8 * 1024 * 1024,
        input_bytes: 16 * 1024 * 1024,
        total_decoded_bytes: 8 * 1024 * 1024,
        ..Limits::default()
    };
    for strings in [StringEncoding::Minimal, StringEncoding::Ascii] {
        let encoded = serialize(
            &tree,
            &SerializeOptions {
                limits,
                strings,
                ..SerializeOptions::default()
            },
        )
        .unwrap();
        if strings == StringEncoding::Ascii {
            assert!(encoded.is_ascii());
        }
        let decoded = parse(
            encoded.as_bytes(),
            &ParseOptions {
                limits,
                ..ParseOptions::default()
            },
        )
        .unwrap();
        assert_eq!(decoded.root().as_str(), Some(all_scalars.as_str()));
    }
}

#[test]
fn every_surrogate_boundary_validates() {
    for high in 0xd800..=0xdbff {
        for low in [0xdc00, 0xdfff] {
            let text = format!("\"\\u{high:04x}\\u{low:04x}\"");
            let expected = char::from_u32(0x10000 + ((high - 0xd800) << 10) + low - 0xdc00)
                .unwrap()
                .to_string();
            assert_eq!(doc(&text).root().as_str(), Some(expected.as_str()));
        }
        assert_error(&format!("\"\\u{high:04x}\""), ErrorKind::UnpairedSurrogate);
    }
    for low in 0xdc00..=0xdfff {
        assert_error(&format!("\"\\u{low:04x}\""), ErrorKind::UnpairedSurrogate);
    }
    for text in [
        r#""\uD800\u0000""#,
        r#""\uDBFF\uD800""#,
        r#""\uDC00\uD800""#,
    ] {
        assert_error(text, ErrorKind::UnpairedSurrogate);
    }
}

#[test]
fn invalid_utf8_is_rejected_without_replacement_characters() {
    for bytes in [
        vec![0xff],
        vec![b'"', 0x80, b'"'],
        vec![b'"', 0xc0, 0xaf, b'"'],
        vec![b'"', 0xe0, 0x80, 0xaf, b'"'],
        vec![b'"', 0xed, 0xa0, 0x80, b'"'],
        vec![b'"', 0xf4, 0x90, 0x80, 0x80, b'"'],
        vec![b'"', 0xf8, 0x88, 0x80, 0x80, 0x80, b'"'],
        vec![b'"', 0xf0, 0x9f, 0x92],
        vec![b'n', b'u', b'l', b'l', 0xff],
    ] {
        assert_eq!(
            parse(&bytes, &ParseOptions::default()).unwrap_err().kind,
            ErrorKind::InvalidUtf8
        );
    }
    assert_eq!(doc("\"�\"").root().as_str(), Some("�"));
}

#[test]
fn strings_reject_bad_escapes_and_unescaped_controls() {
    assert_error(r#""\x00""#, ErrorKind::InvalidEscape);
    assert_error(r#""\v""#, ErrorKind::InvalidEscape);
    assert_error(r#""\u12x4""#, ErrorKind::InvalidUnicodeEscape);
    assert_error(r#""\u12""#, ErrorKind::InvalidUnicodeEscape);
    assert_error("\"unfinished", ErrorKind::UnterminatedString);
    for ch in 0..=0x1f {
        let text = format!("\"{}\"", char::from_u32(ch).unwrap());
        assert_error(&text, ErrorKind::UnescapedControl);
    }
}

#[test]
fn duplicate_policy_compares_decoded_keys_and_keeps_all_members() {
    assert_error(r#"{"a":1,"\u0061":2}"#, ErrorKind::DuplicateKey);
    let document = parse(br#"{"a":1,"\u0061":2}"#, &allow_duplicates()).unwrap();
    assert!(document.extensions_used().duplicate_keys);
    let object = document.root().as_object().unwrap();
    assert_eq!(object.members().len(), 2);
    assert_eq!(object.get_unique("a"), Err(AmbiguousKey));
    assert_eq!(
        serialize(document.root(), &SerializeOptions::default())
            .unwrap_err()
            .kind,
        ErrorKind::DuplicateKey
    );
    assert_eq!(
        serialize(document.root(), &preserving_encoder()).unwrap(),
        r#"{"a":1,"\u0061":2}"#
    );
    assert!(
        doc(r#"{"a":1,"A":2,"é":3,"e\u0301":4}"#)
            .root()
            .as_object()
            .is_some()
    );
}

#[test]
fn ordered_objects_and_nulls_are_never_silently_rewritten() {
    let input = r#"{"z":null,"a":1.00E+00,"unknown":{"$type":"System.DateTime","$ref":"1"}}"#;
    assert_eq!(
        serialize(doc(input).root(), &SerializeOptions::default()).unwrap(),
        input
    );
    let object = Object::new(vec![
        (JsonString::new("z"), Value::Null),
        (JsonString::new("a"), Value::Bool(false)),
    ]);
    assert_eq!(
        serialize(&Value::Object(object), &SerializeOptions::default()).unwrap(),
        r#"{"z":null,"a":false}"#
    );
}

#[test]
fn extensions_are_explicit_detected_and_source_replay_remains_exact() {
    for input in [
        "//comment\nnull",
        "/*comment*/true",
        "[1,]",
        "{\"a\":1,}",
        "\u{feff}null",
    ] {
        assert!(parse(input.as_bytes(), &ParseOptions::default()).is_err());
    }
    let input = "\u{feff}//first\n{/*inside*/\"a\":[1,/*tail*/],}//last";
    let options = ParseOptions {
        extensions: Extensions {
            comments: true,
            trailing_commas: true,
            utf8_bom: true,
        },
        ..ParseOptions::default()
    };
    let document = parse(input.as_bytes(), &options).unwrap();
    assert_eq!(document.source(), input);
    assert_eq!(
        document.extensions_used(),
        ExtensionsUsed {
            comments: true,
            trailing_commas: true,
            utf8_bom: true,
            duplicate_keys: false
        }
    );
    let encoded = serialize(document.root(), &SerializeOptions::default()).unwrap();
    assert_eq!(encoded, r#"{"a":[1]}"#);
    assert!(parse(encoded.as_bytes(), &ParseOptions::default()).is_ok());
    assert!(parse(br#"{'a':1}"#, &options).is_err());
    assert!(parse(br#"{a:1}"#, &options).is_err());
    assert!(parse(br#"[1,,]"#, &options).is_err());
    assert!(parse(br#"[, ]"#, &options).is_err());
    assert!(parse(b"/*unfinished", &options).is_err());
}

#[test]
fn legacy_config_profile_matches_comments_without_trailing_commas() {
    assert!(
        parse(
            br#"{/*comment*/"UseTor":true}"#,
            &ParseOptions::legacy_config()
        )
        .is_ok()
    );
    assert!(parse(br#"{"UseTor":true,}"#, &ParseOptions::legacy_config()).is_err());
    assert!(parse(b"\xef\xbb\xbf{}", &ParseOptions::legacy_config()).is_err());
}

#[test]
fn pretty_ascii_and_minimal_serialization_are_explicit() {
    let tree = value(r#"{"\u0061":["𝄞","\/","\u0001",null],"z":1.2300e+02}"#);
    let pretty = SerializeOptions {
        layout: Layout::Pretty { indent: 2 },
        strings: StringEncoding::Ascii,
        ..SerializeOptions::default()
    };
    assert_eq!(
        serialize(&tree, &pretty).unwrap(),
        "{\n  \"a\": [\n    \"\\ud834\\udd1e\",\n    \"/\",\n    \"\\u0001\",\n    null\n  ],\n  \"z\": 1.2300e+02\n}"
    );
    let minimal = SerializeOptions {
        strings: StringEncoding::Minimal,
        ..SerializeOptions::default()
    };
    assert_eq!(
        serialize(&tree, &minimal).unwrap(),
        r#"{"a":["𝄞","/","\u0001",null],"z":1.2300e+02}"#
    );
    assert_eq!(serialize(&value("[]"), &pretty).unwrap(), "[]");
    assert_eq!(serialize(&value("{}"), &pretty).unwrap(), "{}");
    for indent in [0, 9, 255] {
        assert_eq!(
            serialize(
                &tree,
                &SerializeOptions {
                    layout: Layout::Pretty { indent },
                    ..pretty
                }
            )
            .unwrap_err()
            .kind,
            ErrorKind::InvalidOptions
        );
    }
}

#[test]
fn error_locations_are_bytes_and_never_embed_secrets() {
    let input = "{\n  \"é\": 0,\n  \"private secret\": }";
    let error = parse(input.as_bytes(), &ParseOptions::default()).unwrap_err();
    assert_eq!(error.offset, input.find('}').unwrap());
    assert_eq!(error.line, 3);
    assert_eq!(error.column, 21);
    assert!(!error.to_string().contains("secret"));
    assert!(!format!("{error:?}").contains("secret"));
    let utf8 = parse(b"[0,\n\xff]", &ParseOptions::default()).unwrap_err();
    assert_eq!((utf8.offset, utf8.line, utf8.column), (4, 2, 1));
}

#[test]
fn syntax_error_categories_are_actionable() {
    assert_error("", ErrorKind::UnexpectedEof);
    assert_error("[", ErrorKind::UnexpectedEof);
    assert_error("[1", ErrorKind::UnexpectedEof);
    assert_error("{1:2}", ErrorKind::ExpectedObjectKey);
    assert_error("{\"a\" 2}", ErrorKind::ExpectedColon);
    assert_error("[1 2]", ErrorKind::ExpectedCommaOrEnd);
    assert_error("true false", ErrorKind::TrailingCharacters);
    assert_error("[NaN]", ErrorKind::ExpectedValue);
    assert_error("01", ErrorKind::InvalidNumber);
    assert_error("\u{feff}null", ErrorKind::BomNotAllowed);
}

fn limit_parse(input: &str, limits: Limits, expected: LimitKind) {
    let result = parse(
        input.as_bytes(),
        &ParseOptions {
            limits,
            ..ParseOptions::default()
        },
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded(expected));
}
fn limit_encode(tree: &Value, limits: Limits, expected: LimitKind) {
    let result = serialize(
        tree,
        &SerializeOptions {
            limits,
            ..SerializeOptions::default()
        },
    );
    assert_eq!(result.unwrap_err().kind, ErrorKind::LimitExceeded(expected));
}

#[test]
fn all_parse_limits_have_exact_boundary_cases() {
    let d = Limits::default();
    assert!(
        parse(
            b"null",
            &ParseOptions {
                limits: Limits {
                    input_bytes: 4,
                    ..d
                },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    limit_parse(
        "null",
        Limits {
            input_bytes: 3,
            ..d
        },
        LimitKind::InputBytes,
    );
    assert!(
        parse(
            b"[[0]]",
            &ParseOptions {
                limits: Limits { depth: 2, ..d },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    limit_parse("[[0]]", Limits { depth: 1, ..d }, LimitKind::Depth);
    assert!(
        parse(
            b"0",
            &ParseOptions {
                limits: Limits { depth: 0, ..d },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    limit_parse("[]", Limits { depth: 0, ..d }, LimitKind::Depth);
    assert!(
        parse(
            b"{\"a\":0}",
            &ParseOptions {
                limits: Limits { nodes: 3, ..d },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    limit_parse("{\"a\":0}", Limits { nodes: 2, ..d }, LimitKind::Nodes);
    limit_parse("null", Limits { nodes: 0, ..d }, LimitKind::Nodes);
    assert!(
        parse(
            b"[0]",
            &ParseOptions {
                limits: Limits {
                    container_entries: 1,
                    ..d
                },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    limit_parse(
        "[0,1]",
        Limits {
            container_entries: 1,
            ..d
        },
        LimitKind::ContainerEntries,
    );
    limit_parse(
        "{\"a\":0}",
        Limits {
            container_entries: 0,
            ..d
        },
        LimitKind::ContainerEntries,
    );
    assert!(
        parse(
            "\"é\"".as_bytes(),
            &ParseOptions {
                limits: Limits {
                    string_bytes: 2,
                    ..d
                },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    limit_parse(
        "\"é\"",
        Limits {
            string_bytes: 1,
            ..d
        },
        LimitKind::StringBytes,
    );
    limit_parse(
        r#""\uD834\uDD1E""#,
        Limits {
            string_bytes: 3,
            ..d
        },
        LimitKind::StringBytes,
    );
    limit_parse(
        "123",
        Limits {
            number_bytes: 2,
            ..d
        },
        LimitKind::NumberBytes,
    );
    assert!(
        parse(
            b"123",
            &ParseOptions {
                limits: Limits {
                    number_bytes: 3,
                    ..d
                },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    assert!(
        parse(
            b"{\"a\":\"bc\"}",
            &ParseOptions {
                limits: Limits {
                    total_decoded_bytes: 3,
                    ..d
                },
                ..ParseOptions::default()
            }
        )
        .is_ok()
    );
    limit_parse(
        "{\"a\":\"bc\"}",
        Limits {
            total_decoded_bytes: 2,
            ..d
        },
        LimitKind::TotalDecodedBytes,
    );
}

#[test]
fn all_serialization_limits_have_exact_boundary_cases() {
    let d = Limits::default();
    assert_eq!(
        serialize(
            &Value::Null,
            &SerializeOptions {
                limits: Limits {
                    output_bytes: 4,
                    ..d
                },
                ..SerializeOptions::default()
            }
        )
        .unwrap(),
        "null"
    );
    limit_encode(
        &Value::Null,
        Limits {
            output_bytes: 3,
            ..d
        },
        LimitKind::OutputBytes,
    );
    limit_encode(&value("[[0]]"), Limits { depth: 1, ..d }, LimitKind::Depth);
    limit_encode(
        &value("{\"a\":0}"),
        Limits { nodes: 2, ..d },
        LimitKind::Nodes,
    );
    limit_encode(
        &value("[0,1]"),
        Limits {
            container_entries: 1,
            ..d
        },
        LimitKind::ContainerEntries,
    );
    limit_encode(
        &Value::string("é"),
        Limits {
            string_bytes: 1,
            ..d
        },
        LimitKind::StringBytes,
    );
    limit_encode(
        &value("123"),
        Limits {
            number_bytes: 2,
            ..d
        },
        LimitKind::NumberBytes,
    );
    limit_encode(
        &value("{\"a\":\"bc\"}"),
        Limits {
            total_decoded_bytes: 2,
            ..d
        },
        LimitKind::TotalDecodedBytes,
    );
    let escaped = Value::string("\0");
    limit_encode(
        &escaped,
        Limits {
            output_bytes: 7,
            ..d
        },
        LimitKind::OutputBytes,
    );
    assert_eq!(
        serialize(
            &escaped,
            &SerializeOptions {
                limits: Limits {
                    output_bytes: 8,
                    ..d
                },
                ..SerializeOptions::default()
            }
        )
        .unwrap(),
        r#""\u0000""#
    );
}

#[test]
fn hard_depth_and_other_ceilings_cannot_be_disabled() {
    let mut cases = Vec::new();
    let mut limit = Limits::default();
    limit.depth = HARD_LIMITS.depth + 1;
    cases.push(limit);
    let mut limit = Limits::default();
    limit.input_bytes = usize::MAX;
    cases.push(limit);
    let mut limit = Limits::default();
    limit.output_bytes = usize::MAX;
    cases.push(limit);
    let mut limit = Limits::default();
    limit.nodes = usize::MAX;
    cases.push(limit);
    let mut limit = Limits::default();
    limit.container_entries = usize::MAX;
    cases.push(limit);
    let mut limit = Limits::default();
    limit.string_bytes = usize::MAX;
    cases.push(limit);
    let mut limit = Limits::default();
    limit.number_bytes = usize::MAX;
    cases.push(limit);
    let mut limit = Limits::default();
    limit.total_decoded_bytes = usize::MAX;
    cases.push(limit);
    for limits in cases {
        assert_eq!(
            parse(
                b"null",
                &ParseOptions {
                    limits,
                    ..ParseOptions::default()
                }
            )
            .unwrap_err()
            .kind,
            ErrorKind::InvalidOptions
        );
        assert_eq!(
            serialize(
                &Value::Null,
                &SerializeOptions {
                    limits,
                    ..SerializeOptions::default()
                }
            )
            .unwrap_err()
            .kind,
            ErrorKind::InvalidOptions
        );
    }
    let accepted = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    let options = ParseOptions {
        limits: Limits {
            depth: 128,
            ..Limits::default()
        },
        ..ParseOptions::default()
    };
    let document = parse(accepted.as_bytes(), &options).unwrap();
    assert!(
        serialize(
            document.root(),
            &SerializeOptions {
                limits: options.limits,
                ..SerializeOptions::default()
            }
        )
        .is_ok()
    );
    let rejected = format!("{}0{}", "[".repeat(129), "]".repeat(129));
    assert_eq!(
        parse(rejected.as_bytes(), &options).unwrap_err().kind,
        ErrorKind::LimitExceeded(LimitKind::Depth)
    );
}

#[test]
fn compatibility_adapters_are_typed_and_ambiguity_fails_closed() {
    let object = value(
        r#"{"TransactionId":"synthetic","Index":" +0003 ","UseTor":true,"null":null,"Date":"2026-10-02T00:00:00Z","$type":"System.IO.FileInfo"}"#,
    );
    let object = object.as_object().unwrap();
    assert_eq!(
        compat::string(
            compat::required_field(object, "transactionId", FieldCase::AsciiPascalAlias).unwrap()
        ),
        Ok("synthetic")
    );
    assert_eq!(
        compat::integer_u64(
            compat::required_field(object, "index", FieldCase::AsciiInsensitive).unwrap(),
            IntegerInput::NumberOrDecimalString
        ),
        Ok(3)
    );
    assert_eq!(compat::field(object, "missing", FieldCase::Exact), Ok(None));
    assert_eq!(
        compat::field(object, "null", FieldCase::Exact),
        Ok(Some(&Value::Null))
    );
    assert_eq!(
        compat::required_field(object, "missing", FieldCase::Exact),
        Err(DecodeError::MissingField)
    );
    assert_eq!(
        compat::tor_setting(object.get_unique("UseTor").unwrap().unwrap()),
        Ok("Enabled")
    );
    assert_eq!(compat::tor_setting(&Value::Bool(false)), Ok("Disabled"));
    let collision = value(r#"{"index":1,"Index":2}"#);
    assert_eq!(
        compat::field(
            collision.as_object().unwrap(),
            "index",
            FieldCase::AsciiPascalAlias
        ),
        Err(DecodeError::AmbiguousField)
    );
    let duplicate = parse(br#"{"Index":1,"Index":2}"#, &allow_duplicates()).unwrap();
    assert_eq!(
        compat::field(
            duplicate.root().as_object().unwrap(),
            "Index",
            FieldCase::Exact
        ),
        Err(DecodeError::AmbiguousField)
    );
    assert_eq!(
        compat::field(object, "", FieldCase::AsciiPascalAlias),
        Ok(None)
    );
    assert_eq!(
        object.get_unique("Date").unwrap().unwrap().as_str(),
        Some("2026-10-02T00:00:00Z")
    );
    assert_eq!(
        object.get_unique("$type").unwrap().unwrap().as_str(),
        Some("System.IO.FileInfo")
    );
}

#[test]
fn compatibility_integer_coercion_is_explicit_and_exact() {
    assert_eq!(
        compat::integer_i64(&value("9007199254740993"), IntegerInput::NumberOnly),
        Ok(9_007_199_254_740_993)
    );
    assert_eq!(
        compat::integer_i64(&Value::string("12"), IntegerInput::NumberOnly),
        Err(DecodeError::ExpectedNumber)
    );
    assert_eq!(
        compat::integer_i64(
            &Value::string(" +0012 "),
            IntegerInput::NumberOrDecimalString
        ),
        Ok(12)
    );
    for text in ["1.0", "1e2", "١", "1,000", "++1", "-", ""] {
        assert_eq!(
            compat::integer_i128(&Value::string(text), IntegerInput::NumberOrDecimalString),
            Err(DecodeError::InvalidIntegerString)
        );
    }
    assert_eq!(compat::scaled_i128(&value("0.00000001"), 8), Ok(1));
    assert_eq!(
        compat::scaled_i128(&Value::string("0.00000001"), 8),
        Err(DecodeError::ExpectedNumber)
    );
    assert_eq!(
        compat::boolean(&value("null")),
        Err(DecodeError::ExpectedBool)
    );
    assert_eq!(
        compat::object(&value("[]")),
        Err(DecodeError::ExpectedObject)
    );
    assert_eq!(
        compat::fixed_point(123_456_789, 8)
            .unwrap()
            .as_number()
            .unwrap()
            .as_str(),
        "1.23456789"
    );
}

#[test]
fn current_bitcoin_amount_string_contract_is_preserved() {
    for (text, satoshis) in [
        ("0.00000001", 1),
        ("1.23456789", 123_456_789),
        ("21000000", 2_100_000_000_000_000),
        ("-0.1", -10_000_000),
    ] {
        assert_eq!(
            compat::scaled_decimal_string_i128(&Value::string(text), 8),
            Ok(satoshis)
        );
        assert_eq!(
            compat::fixed_point_string(satoshis, 8, true)
                .unwrap()
                .as_str(),
            Some(text)
        );
    }
    assert_eq!(
        compat::fixed_point_string(100_000_000, 8, false)
            .unwrap()
            .as_str(),
        Some("1.00000000")
    );
    assert_eq!(
        compat::fixed_point_string(0, 8, true).unwrap().as_str(),
        Some("0")
    );
    assert_eq!(
        compat::scaled_decimal_string_i128(&value("0.1"), 8),
        Err(DecodeError::ExpectedString)
    );
    assert_eq!(
        compat::scaled_decimal_string_i128(&Value::string("0.000000001"), 8),
        Err(DecodeError::Number(NumberConversionError::NonIntegral))
    );
    for token in ["1e-8", "01", " +1 ", "1,000", ".1", "NaN"] {
        assert_eq!(
            compat::scaled_decimal_string_i128(&Value::string(token), 8),
            Err(DecodeError::InvalidDecimalString)
        );
    }
}

#[test]
fn synthetic_application_payloads_preserve_every_field_and_token() {
    let directory = fixtures().join("application");
    let mut count = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let input = std::fs::read(&path).unwrap();
        let document = parse(&input, &ParseOptions::legacy_config()).unwrap();
        assert_eq!(document.source().as_bytes(), input);
        let encoded = serialize(document.root(), &SerializeOptions::default()).unwrap();
        assert_eq!(
            parse(encoded.as_bytes(), &ParseOptions::default())
                .unwrap()
                .root(),
            document.root(),
            "{}",
            path.display()
        );
        count += 1;
    }
    assert_eq!(count, 12);
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn malformed_byte_corpus_never_panics_or_emits_invalid_json() {
    let mut seed = 0x77ec_09da_508a_353b;
    for _ in 0..20_000 {
        let length = (next(&mut seed) % 97) as usize;
        let bytes: Vec<_> = (0..length).map(|_| next(&mut seed) as u8).collect();
        if let Ok(document) = parse(&bytes, &ParseOptions::default()) {
            let serialized = serialize(document.root(), &SerializeOptions::default()).unwrap();
            assert_eq!(
                parse(serialized.as_bytes(), &ParseOptions::default())
                    .unwrap()
                    .root(),
                document.root()
            );
        }
    }
}

fn generated(seed: &mut u64, depth: usize) -> Value {
    let kind = next(seed) % if depth < 5 { 6 } else { 4 };
    match kind {
        0 => Value::Null,
        1 => Value::Bool(next(seed) % 2 == 0),
        2 => Value::Number(
            Number::from_scaled_i128(next(seed) as i64 as i128, (next(seed) % 12) as u32).unwrap(),
        ),
        3 => {
            let choices = ['\0', '\n', '"', '\\', '/', 'é', '𝄞', '中', '\u{2028}', 'a'];
            Value::string(
                (0..next(seed) % 20)
                    .map(|_| choices[(next(seed) % choices.len() as u64) as usize])
                    .collect::<String>(),
            )
        }
        4 => Value::Array(
            (0..next(seed) % 5)
                .map(|_| generated(seed, depth + 1))
                .collect(),
        ),
        _ => Value::Object(Object::new(
            (0..next(seed) % 5)
                .map(|i| {
                    (
                        JsonString::new(format!("field{i}")),
                        generated(seed, depth + 1),
                    )
                })
                .collect(),
        )),
    }
}

#[test]
fn generated_trees_round_trip_and_mutated_wire_inputs_are_safe() {
    let mut seed = 0x7298_7210_3890_651d;
    for _ in 0..1000 {
        let tree = generated(&mut seed, 0);
        for strings in [
            StringEncoding::Preserve,
            StringEncoding::Minimal,
            StringEncoding::Ascii,
        ] {
            for layout in [Layout::Compact, Layout::Pretty { indent: 2 }] {
                let options = SerializeOptions {
                    strings,
                    layout,
                    ..SerializeOptions::default()
                };
                let serialized = serialize(&tree, &options).unwrap();
                let document = doc(&serialized);
                assert_eq!(document.root(), &tree);
                assert_eq!(serialize(document.root(), &options).unwrap(), serialized);
                let mut bytes = serialized.into_bytes();
                let position = (next(&mut seed) % bytes.len() as u64) as usize;
                bytes[position] = next(&mut seed) as u8;
                if let Ok(mutated) = parse(&bytes, &ParseOptions::default()) {
                    let encoded = serialize(mutated.root(), &options).unwrap();
                    assert_eq!(doc(&encoded).root(), mutated.root());
                }
            }
        }
    }
}

#[test]
fn portable_domain_types_are_send_sync_without_platform_handles() {
    fn check<T: Send + Sync>() {}
    check::<Value>();
    check::<Document>();
    check::<Number>();
    check::<Object>();
}
