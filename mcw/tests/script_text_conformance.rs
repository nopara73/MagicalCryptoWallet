use mcw::{bitcoin_encoding, bitcoin_script};

// Test the actual component without waiting for shared host registration.
#[path = "../src/script_text.rs"]
mod script_text;

fn bytes(hex: &str) -> Vec<u8> {
    bitcoin_encoding::hex_decode(hex).unwrap()
}

#[test]
fn retained_library_reference_cases() {
    let corpus = include_str!("script_text_fixtures/vectors.tsv");
    let mut count = 0;
    for line in corpus.lines().filter(|line| !line.starts_with('#')) {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[3], ".");
        let data = bytes(fields[1]);
        let result = match fields[0] {
            "P" => script_text::parse_utf8(&data),
            "R" => script_text::render(&data).map(String::into_bytes),
            _ => panic!("unknown reference operation"),
        };
        let actual = result.map_or_else(
            |_| "ERR".to_owned(),
            |data| format!("OK:{}", bitcoin_encoding::hex_encode(&data).unwrap()),
        );
        assert_eq!(
            actual, fields[2],
            "reference case {count}: {} {}",
            fields[0], fields[1]
        );
        count += 1;
    }
    assert!(count > 10_000);
}

#[test]
fn seven_audited_parse_differences_are_preserved() {
    for text in [
        "OP_UNKNOWN(0xba",
        "OP_UNKNOWN(0xba)extra",
        "OP_UNKNOWN(0xbaanything",
    ] {
        assert_eq!(script_text::parse(text).unwrap().as_bytes(), &[0xba]);
        assert!(
            bitcoin_script::Script::from_asm(text, bitcoin_script::AsmDialect::Wallet).is_err()
        );
    }
    for text in [
        "\u{000b}OP_DUP\u{000b}",
        "\u{00a0}OP_DUP\u{00a0}",
        "\u{2003}OP_DUP\u{2003}",
    ] {
        assert_eq!(script_text::parse(text).unwrap().as_bytes(), &[0x76]);
    }
    assert_eq!(
        script_text::parse("OP_DUP\u{000b}OP_HASH160")
            .unwrap()
            .as_bytes(),
        &[0x76, 0xa9]
    );
}

#[test]
fn truncated_pushes_render_a_final_zero_without_allocating_declared_lengths() {
    for hex in [
        "01",
        "4c",
        "4c01",
        "4d",
        "4d01",
        "4dffff",
        "4e",
        "4eff",
        "4effff",
        "4effffff",
        "4effffffff",
        "4effffffff010203",
    ] {
        assert_eq!(script_text::render(&bytes(hex)).unwrap(), "0");
        assert!(
            bitcoin_script::Script::from_hex(hex)
                .unwrap()
                .to_wallet_asm()
                .is_err()
        );
    }
    for hex in ["760201", "764c", "764d01", "764effffffff010203"] {
        assert_eq!(script_text::render(&bytes(hex)).unwrap(), "OP_DUP 0");
    }
    assert_eq!(
        script_text::render(&bytes("514e00000000ac")).unwrap(),
        "1 0 OP_CHECKSIG"
    );
}

#[test]
fn existing_lossy_display_and_hex_number_semantics_remain() {
    assert_eq!(
        script_text::render(&bytes("4c010101005f4f")).unwrap(),
        "1 0 f 81"
    );
    assert_eq!(
        script_text::parse("OP_10 OP_16 81").unwrap().as_bytes(),
        &[0x60, 1, 0x16, 0x4f]
    );
    assert_eq!(
        script_text::parse("0 OP_DUP\tOP_HASH160\n01 OP_CLTV OP_CSV")
            .unwrap()
            .as_bytes(),
        &[0, 0x76, 0xa9, 0x51, 0xb1, 0xb2]
    );
    assert!(script_text::parse("-1").is_err());
    assert!(script_text::parse("OP_1NEGATE").is_err());
}

#[test]
fn ignored_unknown_suffixes_do_not_become_a_second_script_token() {
    for suffix in ["", ")", ")extra", "anything", "\u{2003})", "\u{1f600}"] {
        let text = format!("OP_UNKNOWN(0xff{suffix} OP_DUP");
        assert_eq!(script_text::parse(&text).unwrap().as_bytes(), &[0xff, 0x76]);
    }
    for text in [
        "OP_UNKNOWN(0x",
        "OP_UNKNOWN(0x)",
        "OP_UNKNOWN(0x0)",
        "OP_UNKNOWN(0xg0)",
        "OP_UNKNOWN(0x\u{0100})",
        "OP_UNKNOWN(0x\u{1f600})",
    ] {
        assert!(matches!(
            script_text::parse(text),
            Err(script_text::Error::InvalidUnknownOpcode { token: 0 })
        ));
    }
}

#[test]
fn unicode_whitespace_is_only_trimmed_at_text_boundaries() {
    for c in [
        0x85, 0xa0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008,
        0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
    ] {
        let c = char::from_u32(c).unwrap();
        assert_eq!(
            script_text::parse(&format!("{c}OP_DUP{c}"))
                .unwrap()
                .as_bytes(),
            &[0x76]
        );
        assert!(script_text::parse(&format!("OP_DUP{c}OP_HASH160")).is_err());
    }
    for c in ['\u{180e}', '\u{200b}', '\u{feff}'] {
        assert!(script_text::parse(&format!("{c}OP_DUP{c}")).is_err());
    }
}

#[test]
fn invalid_utf8_and_resource_bounds_fail_before_large_allocations() {
    assert_eq!(
        script_text::parse_utf8(&[0xff]),
        Err(script_text::Error::InvalidUtf8)
    );
    assert_eq!(
        script_text::parse_utf8(&[0xc0, 0x80]),
        Err(script_text::Error::InvalidUtf8)
    );
    assert_eq!(script_text::parse_utf8(&[]).unwrap(), Vec::<u8>::new());
    assert_eq!(script_text::render(&[]).unwrap(), "");
    let large = vec![0u8; bitcoin_script::MAX_SCRIPT_BYTES + 1];
    assert!(matches!(
        script_text::render(&large),
        Err(script_text::Error::Script(
            bitcoin_script::Error::ScriptTooLarge { .. }
        ))
    ));
    let text = " ".repeat(bitcoin_script::MAX_ASM_BYTES + 1);
    assert!(matches!(
        script_text::parse(&text),
        Err(script_text::Error::Script(
            bitcoin_script::Error::AsmTooLarge { .. }
        ))
    ));
}
