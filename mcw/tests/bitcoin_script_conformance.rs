use mcw::bitcoin_encoding::{self, Address, LegacyAddress, LegacyKind, Network, WitnessAddress};
use mcw::bitcoin_script::opcodes::*;
use mcw::bitcoin_script::*;

fn utf8_hex(text: &str) -> String {
    String::from_utf8(bitcoin_encoding::hex_decode(text).unwrap()).unwrap()
}

fn parse_summary(script: &Script) -> String {
    match script.validate(ValidationPolicy::default()) {
        Ok(s) => format!(
            "OK:{}:{}:{}:{}",
            s.instruction_count,
            s.nonminimal_push_count,
            s.maximum_push_bytes,
            u8::from(s.is_push_only)
        ),
        Err(Error::TruncatedLength {
            offset,
            needed,
            available,
        }) => format!("ERR:length:{offset}:{needed}:{available}"),
        Err(Error::TruncatedPush {
            offset,
            declared,
            available,
        }) => format!("ERR:push:{offset}:{declared}:{available}"),
        result => panic!("unexpected parser result {result:?}"),
    }
}

#[test]
fn bitcoin_core_format_vectors() {
    let mut count = 0;
    for row in include_str!("bitcoin_script_fixtures/core_vectors.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 7);
        assert_eq!(fields[6], ".");
        let text = utf8_hex(fields[1]);
        let script = Script::from_asm(&text, AsmDialect::CoreFormat)
            .unwrap_or_else(|e| panic!("{}: {e}; asm {text}", fields[0]));
        assert_eq!(script.to_hex().unwrap(), fields[2], "{}", fields[0]);
        assert_eq!(parse_summary(&script), fields[3], "{}", fields[0]);
        assert_eq!(
            script.to_core_asm().unwrap(),
            utf8_hex(fields[4]),
            "{}",
            fields[0]
        );
        let format = script.to_format_asm().unwrap();
        assert_eq!(format, utf8_hex(fields[5]), "{}", fields[0]);
        assert_eq!(
            Script::from_asm(&format, AsmDialect::CoreFormat).unwrap(),
            script,
            "{}",
            fields[0]
        );
        count += 1;
    }
    assert_eq!(count, 2414);
}

#[test]
fn bitcoin_core_number_creation_vectors() {
    let mut count = 0;
    for row in include_str!("bitcoin_script_fixtures/numbers.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[2], ".");
        let (number, hex) = (fields[0], fields[1]);
        let number: i64 = number.parse().unwrap();
        let encoded = encode_script_number(number);
        assert_eq!(bitcoin_encoding::hex_encode(&encoded).unwrap(), hex);
        assert!(is_minimal_script_number(&encoded));
        assert_eq!(decode_script_number(&encoded, 9, true).unwrap(), number);
        let mut builder = ScriptBuilder::new();
        builder.push_number(number).unwrap();
        let script = builder.finish();
        assert_eq!(
            script
                .instructions()
                .next()
                .unwrap()
                .unwrap()
                .script_number(9, true)
                .unwrap(),
            Some(number)
        );
        count += 1;
    }
    assert_eq!(count, 189);
}

#[test]
fn exact_push_boundaries_and_roundtrips() {
    for length in [
        0,
        1,
        2,
        75,
        76,
        255,
        256,
        520,
        65_535,
        65_536,
        MAX_SCRIPT_BYTES - 5,
    ] {
        let data = vec![0x42; length];
        let mut builder = ScriptBuilder::new();
        builder.push_data(&data).unwrap();
        let script = builder.finish();
        let token = script.instructions().next().unwrap().unwrap();
        assert_eq!(token.pushed_bytes().unwrap(), data);
        assert_eq!(token.is_minimal_push(), Some(true));
        assert_eq!(token.raw_bytes(), script.as_bytes());
        let mut serializer = ScriptBuilder::new();
        serializer.append_instruction(token).unwrap();
        assert_eq!(serializer.finish(), script);
        for encoding in [
            PushEncoding::Direct,
            PushEncoding::PushData1,
            PushEncoding::PushData2,
            PushEncoding::PushData4,
        ] {
            let mut explicit = ScriptBuilder::new();
            let max = match encoding {
                PushEncoding::Direct => 75,
                PushEncoding::PushData1 => 255,
                PushEncoding::PushData2 => 65_535,
                PushEncoding::PushData4 => MAX_SCRIPT_BYTES - 5,
            };
            if length > max {
                assert!(explicit.push_data_exact(&data, encoding).is_err());
                assert!(explicit.as_bytes().is_empty());
                continue;
            }
            explicit.push_data_exact(&data, encoding).unwrap();
            let script = explicit.finish();
            let token = script.instructions().next().unwrap().unwrap();
            assert_eq!(
                token.kind(),
                InstructionKind::Push {
                    data: &data,
                    encoding
                }
            );
            assert_eq!(token.raw_bytes(), script.as_bytes());
            assert_eq!(Script::from_hex(&script.to_hex().unwrap()).unwrap(), script);
            let format = script.to_format_asm().unwrap();
            assert_eq!(
                Script::from_asm(&format, AsmDialect::CoreFormat).unwrap(),
                script
            );
        }
    }
}

#[test]
fn minimal_push_rules_preserve_distinct_stack_bytes() {
    for value in 0..=255_u8 {
        let mut builder = ScriptBuilder::new();
        builder.push_data(&[value]).unwrap();
        let script = builder.finish();
        let token = script.instructions().next().unwrap().unwrap();
        let opcode = if (1..=16).contains(&value) {
            Opcode(0x50 + value)
        } else if value == 0x81 {
            OP_1NEGATE
        } else {
            Opcode(1)
        };
        assert_eq!(token.opcode(), opcode);
        if opcode.0 == 1 {
            assert_eq!(token.pushed_bytes(), Some([value].as_slice()));
        }
    }
    assert_ne!(
        Script::from_hex("0100").unwrap(),
        Script::from_hex("00").unwrap()
    );
    for hex in [
        "4c00",
        "4d0000",
        "4e00000000",
        "0101",
        "0181",
        "4c0142",
        "4d010042",
        "4e0100000042",
    ] {
        let script = Script::from_hex(hex).unwrap();
        assert_eq!(
            script
                .validate(ValidationPolicy::default())
                .unwrap()
                .nonminimal_push_count,
            1
        );
        assert!(matches!(
            script.validate(ValidationPolicy {
                require_minimal_pushes: true,
                ..Default::default()
            }),
            Err(Error::NonMinimalPush { offset: 0 })
        ));
        assert_eq!(script.to_hex().unwrap(), hex);
    }
    let script = Script::from_hex("0100").unwrap();
    assert_eq!(
        script
            .validate(ValidationPolicy {
                require_minimal_pushes: true,
                ..Default::default()
            })
            .unwrap()
            .nonminimal_push_count,
        0
    );
    let mut builder = ScriptBuilder::new();
    builder.push_data(&[0x42; 521]).unwrap();
    assert!(matches!(
        builder.finish().validate(ValidationPolicy {
            maximum_push_bytes: 520,
            ..Default::default()
        }),
        Err(Error::PushTooLarge { length: 521, .. })
    ));
}

#[test]
fn truncated_pushes_and_fused_iteration() {
    let fixtures = [
        ("4c", "ERR:length:0:1:0"),
        ("4d01", "ERR:length:0:2:1"),
        ("4e010203", "ERR:length:0:4:3"),
        ("4effffffff", "ERR:push:0:4294967295:0"),
        ("0201", "ERR:push:0:2:1"),
        ("614d0001ff", "ERR:push:1:256:1"),
    ];
    for (hex, expected) in fixtures {
        let script = Script::from_hex(hex).unwrap();
        assert_eq!(parse_summary(&script), expected);
        assert_eq!(script.to_hex().unwrap(), hex);
        assert!(script.to_wallet_asm().is_err());
        let mut iter = script.instructions();
        for item in iter.by_ref() {
            if item.is_err() {
                break;
            }
        }
        assert_eq!(iter.size_hint(), (0, Some(0)));
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
        assert_eq!(
            Script::from_asm(&script.to_format_asm().unwrap(), AsmDialect::CoreFormat).unwrap(),
            script
        );
    }
}

#[test]
fn unknown_reserved_disabled_and_future_opcode_classification() {
    for byte in 79..=255 {
        let script = Script::from_bytes(&[byte]).unwrap();
        let token = script.instructions().next().unwrap().unwrap();
        assert_eq!(token.opcode(), Opcode(byte));
        assert_eq!(token.raw_bytes(), &[byte]);
        assert_eq!(
            Script::from_asm(&script.to_format_asm().unwrap(), AsmDialect::CoreFormat).unwrap(),
            script
        );
    }
    let success: Vec<_> = (0..=255)
        .filter(|byte| Opcode(*byte).is_tapscript_success())
        .collect();
    let expected: Vec<_> = [
        vec![80, 98],
        (126..=129).collect(),
        (131..=134).collect(),
        (137..=138).collect(),
        (141..=142).collect(),
        (149..=153).collect(),
        (187..=254).collect(),
    ]
    .concat();
    assert_eq!(success, expected);
    assert_eq!(OP_RESERVED.class(), OpcodeClass::Reserved);
    assert_eq!(OP_VERIF.class(), OpcodeClass::Disabled);
    assert_eq!(OP_CHECKSIGADD.class(), OpcodeClass::Crypto);
    assert!(Script::from_hex("50").unwrap().is_push_only().unwrap());
    assert!(!Script::from_hex("61").unwrap().is_push_only().unwrap());
    for (alias, expected) in [
        ("OP_NOP2", OP_CHECKLOCKTIMEVERIFY),
        ("OP_CLTV", OP_CHECKLOCKTIMEVERIFY),
        ("OP_CSV", OP_CHECKSEQUENCEVERIFY),
        ("OP_TRUE", OP_1),
        ("OP_FALSE", OP_0),
    ] {
        assert_eq!(Opcode::from_name(alias), Some(expected));
    }
}

#[test]
fn script_numbers_minimality_sign_and_overflow() {
    for data in [&[0][..], &[0x80], &[0, 0], &[1, 0], &[0, 0x80], &[1, 0x80]] {
        assert!(!is_minimal_script_number(data));
        assert_eq!(
            decode_script_number(data, 9, true),
            Err(Error::NonMinimalNumber)
        );
    }
    assert_eq!(decode_script_number(&[0x80], 4, false).unwrap(), 0);
    assert_eq!(decode_script_number(&[0x80, 0], 4, true).unwrap(), 128);
    assert_eq!(decode_script_number(&[0x80, 0x80], 4, true).unwrap(), -128);
    assert_eq!(
        decode_script_number(&[0xff, 0xff, 0xff, 0x7f], 4, true).unwrap(),
        2_147_483_647
    );
    let five = encode_script_number(2_147_483_648);
    assert!(matches!(
        decode_script_number(&five, 4, false),
        Err(Error::NumberTooLarge { .. })
    ));
    assert_eq!(decode_script_number(&five, 5, true).unwrap(), 2_147_483_648);
    assert_eq!(
        decode_script_number(&[0; 10], 9, false),
        Err(Error::NumberTooLarge {
            length: 10,
            maximum: 9
        })
    );
    assert_eq!(
        decode_script_number(&[], 10, false),
        Err(Error::InvalidNumberLimit { maximum: 10 })
    );
    assert_eq!(
        decode_script_number(&[0xff; 9], 9, false),
        Err(Error::NumberOverflow)
    );
    assert_eq!(
        decode_script_number(&[0, 0, 0, 0, 0, 0, 0, 0x80, 0], 9, true),
        Err(Error::NumberOverflow)
    );
    assert_eq!(
        decode_script_number(&encode_script_number(i64::MIN), 9, true).unwrap(),
        i64::MIN
    );
    for data in [&[][..], &[0], &[0x80], &[0, 0x80]] {
        assert!(!cast_to_bool(data));
    }
    for data in [&[1][..], &[0x80, 0], &[0, 1], &[0x80, 0x80]] {
        assert!(cast_to_bool(data));
    }
}

#[test]
fn wallet_asm_compatibility_and_explicit_lossiness() {
    let fixtures = [
        ("00", "0"),
        ("0100", "0"),
        ("51", "1"),
        ("59", "9"),
        ("5a", "a"),
        ("60", "10"),
        ("4f", "81"),
        (
            "76a914111111111111111111111111111111111111111188ac",
            "OP_DUP OP_HASH160 1111111111111111111111111111111111111111 OP_EQUALVERIFY OP_CHECKSIG",
        ),
        ("b1b2", "OP_CLTV OP_CSV"),
        ("bbfe", "OP_UNKNOWN(0xbb) OP_UNKNOWN(0xfe)"),
    ];
    for (hex, asm) in fixtures {
        let script = Script::from_hex(hex).unwrap();
        assert_eq!(script.to_wallet_asm().unwrap(), asm);
        let normalized = Script::from_asm(asm, AsmDialect::Wallet).unwrap();
        if hex == "0100" {
            assert_eq!(normalized.as_bytes(), &[0]);
        } else {
            assert_eq!(normalized, script);
        }
    }
    assert_eq!(
        Script::from_asm("OP_10", AsmDialect::Wallet)
            .unwrap()
            .as_bytes(),
        &[0x60]
    );
    assert_eq!(
        Script::from_asm("OP_16", AsmDialect::Wallet)
            .unwrap()
            .as_bytes(),
        &[1, 0x16]
    );
    for text in [
        "-1",
        "OP_1NEGATE",
        "abc",
        "0x61",
        "OP_PUSHDATA1",
        "OP_UNKNOWN(0x123)",
        "OP_UNKNOWN(0x01)tail",
    ] {
        assert!(
            Script::from_asm(text, AsmDialect::Wallet).is_err(),
            "{text}"
        );
    }
    assert_eq!(
        Script::from_asm("  OP_FALSE\tOP_TRUE\n", AsmDialect::Wallet)
            .unwrap()
            .as_bytes(),
        &[0, 81]
    );
    assert_ne!(
        Script::from_asm("10", AsmDialect::Wallet).unwrap(),
        Script::from_asm("10", AsmDialect::CoreFormat).unwrap()
    );
    let nonminimal = Script::from_hex("4c0142").unwrap();
    assert_eq!(nonminimal.to_wallet_asm().unwrap(), "42");
    assert_eq!(
        Script::from_asm("42", AsmDialect::Wallet)
            .unwrap()
            .to_hex()
            .unwrap(),
        "0142"
    );
    assert!(matches!(
        Script::from_asm("OP_DUP\u{00a0}OP_DROP", AsmDialect::Wallet),
        Err(Error::NonAsciiAsm { .. })
    ));
}

#[test]
fn core_display_is_distinct_from_lossless_format_grammar() {
    let script = Script::from_hex("4c018051010001810461626364bb4c").unwrap();
    assert_eq!(
        script.to_core_asm().unwrap(),
        "0 1 0 -1 1684234849 OP_UNKNOWN [error]"
    );
    assert_eq!(
        Script::from_asm(&script.to_format_asm().unwrap(), AsmDialect::CoreFormat).unwrap(),
        script
    );
    assert_eq!(
        Script::from_asm("'a'", AsmDialect::CoreFormat)
            .unwrap()
            .to_hex()
            .unwrap(),
        "0161"
    );
    assert_eq!(
        Script::from_asm("'a\rb'", AsmDialect::CoreFormat)
            .unwrap()
            .to_hex()
            .unwrap(),
        "03610d62"
    );
    assert!(Script::from_asm("DUP\rDROP", AsmDialect::CoreFormat).is_err());
    for text in [
        "4294967296",
        "-4294967296",
        "0x",
        "0x0",
        "OP_PUSHDATA1",
        "OP_CHECKSIGADD",
        "OP_1",
        "OP_NOP2",
    ] {
        assert!(
            Script::from_asm(text, AsmDialect::CoreFormat).is_err(),
            "{text}"
        );
    }
}

#[test]
fn exact_standard_output_templates_and_mutated_contract_bytes() {
    let hash20 = [0x42; 20];
    let hash32 = [0x43; 32];
    let fixtures = [
        (p2pkh(&hash20), OutputType::P2pkh),
        (p2sh(&hash20), OutputType::P2sh),
        (p2wpkh(&hash20), OutputType::P2wpkh),
        (p2wsh(&hash32), OutputType::P2wsh),
        (p2tr(&hash32), OutputType::P2tr),
        (p2anchor(), OutputType::P2anchor),
    ];
    for (script, kind) in fixtures {
        assert_eq!(script.output_template().unwrap().output_type(), kind);
        let address = address_from_script(script.as_bytes(), Network::Mainnet)
            .unwrap()
            .unwrap();
        assert_eq!(
            script_from_address_text(&address, Network::Mainnet).unwrap(),
            script
        );
        let mut changed = script.as_bytes().to_vec();
        changed.push(0x61);
        assert_eq!(
            classify_output(&changed).unwrap().output_type(),
            OutputType::Unknown
        );
    }
    // NBitcoin 10.0.13's fast P2PKH checker omits byte 23; do not reproduce that
    // malformed-template recognition bug as an address or key-identifier contract.
    let mut broken = p2pkh(&hash20).into_bytes();
    broken[23] = OP_DROP.0;
    assert_eq!(classify_output(&broken).unwrap(), OutputTemplate::Unknown);
    assert!(
        address_from_script(&broken, Network::Mainnet)
            .unwrap()
            .is_none()
    );
    assert_eq!(p2pkh(&hash20).as_bytes()[23], OP_EQUALVERIFY.0);
    assert!(matches!(
        classify_output(&[0, 20, 0]),
        Err(Error::TruncatedPush { .. })
    ));
}

#[test]
fn witness_envelopes_future_versions_and_nonminimal_policy() {
    for version in 0..=16 {
        for length in 2..=40 {
            let program = vec![0x42; length];
            let bytes = [
                &[if version == 0 { 0 } else { 80 + version }, length as u8][..],
                &program,
            ]
            .concat();
            let envelope = witness_program(&bytes).unwrap();
            assert_eq!(
                envelope,
                WitnessProgram {
                    version,
                    program: &program
                }
            );
            if version == 0 && !matches!(length, 20 | 32) {
                assert_eq!(
                    classify_output(&bytes).unwrap().output_type(),
                    OutputType::InvalidWitnessV0
                );
                assert!(witness_output(version, &program).is_err());
                assert!(address_from_script(&bytes, Network::Regtest).is_err());
            } else {
                assert_eq!(witness_output(version, &program).unwrap().as_bytes(), bytes);
                let address = address_from_script(&bytes, Network::Regtest)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    script_from_address_text(&address, Network::Regtest)
                        .unwrap()
                        .as_bytes(),
                    bytes
                );
            }
        }
    }
    for bytes in [
        &[0, 1, 0][..],
        &[0x50, 2, 0, 0],
        &[0x51, 0x4c, 2, 0, 0],
        &[0x61, 2, 0, 0],
    ] {
        assert!(witness_program(bytes).is_none());
    }
    assert!(witness_output(17, &[0; 32]).is_err());
    assert!(witness_output(1, &[0; 41]).is_err());
    let address = Address::Witness(WitnessAddress {
        network: Network::Mainnet,
        version: 0,
        program: vec![0; 2],
    });
    assert!(script_from_address(&address).is_err());
}

#[test]
fn retained_p2pk_multisig_and_op_return_formats() {
    for (prefix, length) in [(2, 33), (3, 33), (4, 65), (6, 65), (7, 65)] {
        let mut key = vec![0; length];
        key[0] = prefix;
        let script = p2pk(&key).unwrap();
        assert_eq!(
            script.output_template().unwrap().output_type(),
            OutputType::P2pk
        );
        assert!(
            address_from_script(script.as_bytes(), Network::Mainnet)
                .unwrap()
                .is_none()
        );
    }
    assert!(p2pk(&[0; 33]).is_err());
    assert!(p2pk(&[2; 65]).is_err());
    let mut key = [0; 33];
    key[0] = 2;
    for count in 1..=20 {
        let keys = vec![key.as_slice(); count];
        for required in 1..=count as u8 {
            let script = multisig(required, &keys).unwrap();
            assert_eq!(
                script.output_template().unwrap(),
                OutputTemplate::Multisig {
                    required,
                    public_keys: keys.clone()
                }
            );
        }
    }
    assert!(multisig(0, &[&key]).is_err());
    assert!(multisig(2, &[&key]).is_err());
    assert!(multisig(1, &vec![key.as_slice(); 21]).is_err());
    let mut nonminimal_key = ScriptBuilder::new();
    nonminimal_key
        .push_number(1)
        .unwrap()
        .push_data_exact(&key, PushEncoding::PushData1)
        .unwrap()
        .push_number(1)
        .unwrap()
        .append_opcode(OP_CHECKMULTISIG)
        .unwrap();
    assert_eq!(
        nonminimal_key
            .finish()
            .output_template()
            .unwrap()
            .output_type(),
        OutputType::Multisig
    );
    assert_eq!(
        Script::from_hex(
            "01012102000000000000000000000000000000000000000000000000000000000000000051ae"
        )
        .unwrap()
        .output_template()
        .unwrap()
        .output_type(),
        OutputType::Unknown
    );
    assert_eq!(
        op_return(&[b"test", &[1], &[]])
            .unwrap()
            .output_template()
            .unwrap()
            .output_type(),
        OutputType::NullData
    );
    assert_eq!(
        Script::from_hex("6a50")
            .unwrap()
            .output_template()
            .unwrap()
            .output_type(),
        OutputType::NullData
    );
    assert_eq!(
        Script::from_hex("6a61")
            .unwrap()
            .output_template()
            .unwrap()
            .output_type(),
        OutputType::NonstandardReturn
    );
    assert!(Script::from_hex("6a4c").unwrap().output_template().is_err());
}

#[test]
fn address_network_and_existing_payload_compatibility() {
    let networks = [
        Network::Mainnet,
        Network::Testnet,
        Network::Testnet4,
        Network::Signet,
        Network::Regtest,
    ];
    for network in networks {
        for kind in [LegacyKind::P2pkh, LegacyKind::P2sh] {
            let address = Address::Legacy(LegacyAddress {
                network,
                kind,
                hash: [0x42; 20],
            });
            let script = script_from_address(&address).unwrap();
            let text = bitcoin_encoding::address_encode(&address).unwrap();
            assert_eq!(
                address_from_script(script.as_bytes(), network).unwrap(),
                Some(text.clone())
            );
            assert_eq!(script_from_address_text(&text, network).unwrap(), script);
        }
    }
    let script =
        script_from_address_text("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", Network::Mainnet).unwrap();
    assert_eq!(
        script.to_hex().unwrap(),
        "76a91462e907b15cbf27d5425399ebf6f0fb50ebb88f1888ac"
    );
    assert!(
        script_from_address_text("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", Network::Testnet).is_err()
    );
}

#[test]
fn bounded_apis_and_failed_builder_additions_are_atomic() {
    let huge = vec![0x61; MAX_SCRIPT_BYTES + 1];
    assert!(matches!(
        Script::from_bytes(&huge),
        Err(Error::ScriptTooLarge { .. })
    ));
    assert!(matches!(
        instructions(&huge),
        Err(Error::ScriptTooLarge { .. })
    ));
    let mut builder = ScriptBuilder::new();
    builder.append_opcode(OP_DUP).unwrap();
    assert!(
        builder
            .push_data_exact(&[0; 76], PushEncoding::Direct)
            .is_err()
    );
    assert_eq!(builder.as_bytes(), &[OP_DUP.0]);
    assert!(builder.append_raw(&huge).is_err());
    assert_eq!(builder.as_bytes(), &[OP_DUP.0]);
    assert!(builder.push_data(&huge).is_err());
    assert_eq!(builder.as_bytes(), &[OP_DUP.0]);
    let script = Script::from_bytes(&huge[..MAX_SCRIPT_BYTES]).unwrap();
    assert_eq!(
        script
            .validate(ValidationPolicy::default())
            .unwrap()
            .instruction_count,
        MAX_SCRIPT_BYTES
    );
    assert!(matches!(
        Script::from_asm(&" ".repeat(MAX_ASM_BYTES + 1), AsmDialect::Wallet),
        Err(Error::AsmTooLarge { .. })
    ));
}

#[test]
fn arbitrary_bytes_roundtrip_and_token_serialization() {
    let mut state = 0x51c71a09_u32;
    for length in 0..512 {
        let data: Vec<_> = (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        let script = Script::from_bytes(&data).unwrap();
        assert_eq!(Script::from_hex(&script.to_hex().unwrap()).unwrap(), script);
        assert_eq!(
            Script::from_asm(&script.to_format_asm().unwrap(), AsmDialect::CoreFormat).unwrap(),
            script
        );
        let mut builder = ScriptBuilder::new();
        let mut offset = 0;
        for item in script.instructions() {
            match item {
                Ok(token) => {
                    assert_eq!(token.offset(), offset);
                    offset += token.raw_bytes().len();
                    builder.append_instruction(token).unwrap();
                }
                Err(_) => {
                    builder.append_raw(&data[offset..]).unwrap();
                    break;
                }
            }
        }
        assert_eq!(builder.finish(), script);
    }
}

#[test]
fn retained_application_source_fixtures_preserve_raw_identity() {
    let mut count = 0;
    for row in include_str!("bitcoin_script_fixtures/application_fixtures.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        let text = utf8_hex(fields[2]);
        let script = match fields[1] {
            "hex" => Script::from_hex(&text).unwrap(),
            "wallet" => Script::from_asm(&text, AsmDialect::Wallet).unwrap(),
            _ => panic!("fixture dialect"),
        };
        assert_eq!(
            Script::from_hex(&script.to_hex().unwrap()).unwrap(),
            script,
            "{}",
            fields[0]
        );
        assert_eq!(
            Script::from_asm(&script.to_format_asm().unwrap(), AsmDialect::CoreFormat).unwrap(),
            script,
            "{}",
            fields[0]
        );
        count += 1;
    }
    assert_eq!(count, 8, "retained literal fixture inventory changed");
}
