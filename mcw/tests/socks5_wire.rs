//! Literal standards vectors are independent of the production encoder.
//! Run directly with rustc --edition=2024 --test, or as a Cargo integration
//! test after the application host is integrated. No dev dependency needed.
#[allow(dead_code)]
#[path = "../src/socks5.rs"]
mod socks5;

use socks5::wire::*;

#[test]
fn rfc1928_greeting_and_strict_method_selection() {
    let credentials = Credentials::new(b"synthetic-user", b"synthetic-password").unwrap();
    let isolated = Authentication::UsernamePassword(&credentials);
    assert_eq!(Authentication::None.greeting(), [5, 1, 0]);
    assert_eq!(isolated.greeting(), [5, 1, 2]);
    for code in 0..=255 {
        for auth in [Authentication::None, isolated] {
            let expected = if code == auth.greeting()[2] {
                Ok(())
            } else if code == 255 {
                Err(ProtocolError::NoAcceptableMethod)
            } else {
                Err(ProtocolError::UnofferedMethod(code))
            };
            assert_eq!(auth.accept_method([5, code]), expected);
        }
    }
    assert_eq!(
        isolated.accept_method([4, 2]),
        Err(ProtocolError::InvalidVersion(4))
    );
}

#[test]
fn rfc1929_authentication_literal_and_status_vectors() {
    let credentials = Credentials::new(b"user", b"pass").unwrap();
    assert_eq!(
        encode_authentication(&credentials).as_bytes(),
        [1, 4, b'u', b's', b'e', b'r', 4, b'p', b'a', b's', b's']
    );
    for status in 0..=255 {
        let expected = if status == 0 {
            Ok(())
        } else {
            Err(ProtocolError::AuthenticationRejected(status))
        };
        assert_eq!(accept_authentication([1, status]), expected);
    }
    assert_eq!(
        accept_authentication([5, 0]),
        Err(ProtocolError::InvalidAuthVersion(5))
    );
}

#[test]
fn authentication_octets_and_tor_isolation_are_not_rewritten() {
    let credentials = Credentials::new(b"<torS0X>0", b"isolation-\x00\xff").unwrap();
    assert_eq!(
        encode_authentication(&credentials).as_bytes(),
        b"\x01\x09<torS0X>0\x0cisolation-\x00\xff"
    );
    let legacy = Credentials::new(b"legacy", b"legacy").unwrap();
    assert_eq!(
        encode_authentication(&legacy).as_bytes(),
        b"\x01\x06legacy\x06legacy"
    );
}

#[test]
fn authentication_bounds_are_rfc1929_octet_bounds() {
    assert!(matches!(
        Credentials::new(b"", b"x"),
        Err(ProtocolError::EmptyUsername)
    ));
    assert!(matches!(
        Credentials::new(b"x", b""),
        Err(ProtocolError::EmptyPassword)
    ));
    assert!(matches!(
        Credentials::new(&[1; 256], b"x"),
        Err(ProtocolError::UsernameTooLong)
    ));
    assert!(matches!(
        Credentials::new(b"x", &[1; 256]),
        Err(ProtocolError::PasswordTooLong)
    ));
    let credentials = Credentials::new(&[0xff; 255], &[0; 255]).unwrap();
    let packet = encode_authentication(&credentials);
    assert_eq!(packet.as_bytes().len(), MAX_AUTH_LEN);
    assert_eq!(packet.as_bytes()[..2], [1, 255]);
    assert_eq!(packet.as_bytes()[257], 255);
    assert!(packet.as_bytes()[2..257].iter().all(|byte| *byte == 255));
    assert!(packet.as_bytes()[258..].iter().all(|byte| *byte == 0));
}

#[test]
fn rfc1928_connect_ipv4_network_byte_order() {
    let destination = Destination::new(Address::Ipv4([192, 0, 2, 5]), 8333).unwrap();
    assert_eq!(
        encode_connect(&destination).as_bytes(),
        [5, 1, 0, 1, 192, 0, 2, 5, 0x20, 0x8d]
    );
    for port in [1, 80, 443, 65535] {
        let destination = Destination::new(Address::Ipv4([198, 51, 100, 1]), port).unwrap();
        assert_eq!(
            &encode_connect(&destination).as_bytes()[8..],
            &port.to_be_bytes()
        );
    }
    assert!(matches!(
        Destination::new(Address::Ipv4([0; 4]), 0),
        Err(ProtocolError::InvalidPort)
    ));
}

#[test]
fn rfc1928_connect_ipv6_exact_octets() {
    let address = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    let destination = Destination::new(Address::Ipv6(address), 443).unwrap();
    assert_eq!(
        encode_connect(&destination).as_bytes(),
        [
            5, 1, 0, 4, 0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0xbb
        ]
    );
}

#[test]
fn rfc1928_domain_length_exact_case_trailing_dot_and_onion() {
    let domain = DomainName::new(b"MiXeD.Example.invalid.").unwrap();
    let destination = Destination::new(Address::Domain(domain), 443).unwrap();
    assert_eq!(
        encode_connect(&destination).as_bytes(),
        b"\x05\x01\x00\x03\x16MiXeD.Example.invalid.\x01\xbb"
    );
    let domain = DomainName::new(b"synthetic.onion").unwrap();
    let destination = Destination::new(Address::Domain(domain), 80).unwrap();
    assert_eq!(
        encode_connect(&destination).as_bytes(),
        b"\x05\x01\x00\x03\x0fsynthetic.onion\x00\x50"
    );
}

#[test]
fn domain_boundaries_are_bounded_before_allocation() {
    assert_eq!(DomainName::new(b""), Err(ProtocolError::EmptyDomain));
    assert_eq!(
        DomainName::new(&[b'a'; 256]),
        Err(ProtocolError::DomainTooLong)
    );
    assert_eq!(
        DomainName::new(b"a\x00b"),
        Err(ProtocolError::DomainContainsNul)
    );
    let domain = DomainName::new(&[b'a'; 255]).unwrap();
    let destination = Destination::new(Address::Domain(domain), 65535).unwrap();
    let packet = encode_connect(&destination);
    assert_eq!(packet.as_bytes().len(), MAX_REQUEST_LEN);
    assert_eq!(packet.as_bytes()[4], 255);
    assert!(packet.as_bytes()[5..260].iter().all(|byte| *byte == b'a'));
    assert_eq!(&packet.as_bytes()[260..], &[255, 255]);
}

#[test]
fn tor_resolve_and_resolve_ptr_literal_vectors() {
    let domain = DomainName::new(b"example.invalid").unwrap();
    assert_eq!(
        encode_resolve(&domain).as_bytes(),
        b"\x05\xf0\x00\x03\x0fexample.invalid\x00\x00"
    );
    assert_eq!(
        encode_resolve_ptr([192, 0, 2, 1]).as_bytes(),
        [5, 0xf1, 0, 1, 192, 0, 2, 1, 0, 0]
    );
}

fn check_fragments_and_extra_data(frame: &[u8], expected: Address, port: u16) {
    for length in 0..frame.len() {
        assert_eq!(decode_reply(&frame[..length]), Ok(None), "prefix {length}");
    }
    let (reply, length) = decode_reply(frame).unwrap().unwrap();
    assert_eq!(length, frame.len());
    assert_eq!(reply.code, ReplyCode::Succeeded);
    assert_eq!(reply.bound.address, expected);
    assert_eq!(reply.bound.port, port);
    let mut coalesced = frame.to_vec();
    coalesced.extend_from_slice(b"APPLICATION-BANNER");
    assert_eq!(decode_reply(&coalesced), Ok(Some((reply, frame.len()))));
}

#[test]
fn rfc1928_reply_ipv4_all_fragment_boundaries() {
    check_fragments_and_extra_data(
        &[5, 0, 0, 1, 192, 0, 2, 5, 0x20, 0x8d],
        Address::Ipv4([192, 0, 2, 5]),
        8333,
    );
}

#[test]
fn rfc1928_reply_ipv6_all_fragment_boundaries() {
    let address = [0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9];
    check_fragments_and_extra_data(
        &[
            5, 0, 0, 4, 0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0,
        ],
        Address::Ipv6(address),
        0,
    );
}

#[test]
fn rfc1928_reply_domain_and_maximum_all_fragment_boundaries() {
    check_fragments_and_extra_data(
        b"\x05\x00\x00\x03\x0bfoo.invalid\x01\xbb",
        Address::Domain(DomainName::new(b"foo.invalid").unwrap()),
        443,
    );
    let mut maximum = vec![5, 0, 0, 3, 255];
    maximum.extend_from_slice(&[b'x'; 255]);
    maximum.extend_from_slice(&[255, 255]);
    assert_eq!(maximum.len(), MAX_REPLY_LEN);
    check_fragments_and_extra_data(
        &maximum,
        Address::Domain(DomainName::new(&[b'x'; 255]).unwrap()),
        65535,
    );
    check_fragments_and_extra_data(
        &[5, 0, 0, 3, 1, 255, 0, 1],
        Address::Domain(DomainName::new(&[255]).unwrap()),
        1,
    );
}

#[test]
fn every_reply_code_is_preserved_and_extensions_are_named() {
    let standard = [
        ReplyCode::Succeeded,
        ReplyCode::GeneralFailure,
        ReplyCode::RulesetDenied,
        ReplyCode::NetworkUnreachable,
        ReplyCode::HostUnreachable,
        ReplyCode::ConnectionRefused,
        ReplyCode::TtlExpired,
        ReplyCode::CommandNotSupported,
        ReplyCode::AddressTypeNotSupported,
    ];
    let tor = [
        ReplyCode::OnionDescriptorNotFound,
        ReplyCode::OnionDescriptorInvalid,
        ReplyCode::OnionIntroductionFailed,
        ReplyCode::OnionRendezvousFailed,
        ReplyCode::OnionMissingClientAuthorization,
        ReplyCode::OnionWrongClientAuthorization,
        ReplyCode::OnionInvalidAddress,
        ReplyCode::OnionIntroductionTimedOut,
    ];
    for code in 0..=255 {
        let expected = match code {
            0..=8 => standard[usize::from(code)],
            0xf0..=0xf7 => tor[usize::from(code - 0xf0)],
            _ => ReplyCode::Unassigned(code),
        };
        let (reply, length) = decode_reply(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0])
            .unwrap()
            .unwrap();
        assert_eq!(length, 10);
        assert_eq!(reply.code, expected);
        assert_eq!(reply.code.as_byte(), code);
    }
}

#[test]
fn malformed_header_and_domain_are_rejected() {
    for version in 0..=255 {
        if version != 5 {
            assert_eq!(
                decode_reply(&[version]),
                Err(ProtocolError::InvalidVersion(version))
            );
        }
    }
    for reserved in 1..=255 {
        assert_eq!(
            decode_reply(&[5, 0, reserved]),
            Err(ProtocolError::NonzeroReserved(reserved))
        );
    }
    for atyp in 0..=255 {
        if ![1, 3, 4].contains(&atyp) {
            assert_eq!(
                decode_reply(&[5, 0, 0, atyp]),
                Err(ProtocolError::UnknownAddressType(atyp))
            );
        }
    }
    assert_eq!(
        decode_reply(&[5, 0, 0, 3, 0]),
        Err(ProtocolError::EmptyDomain)
    );
    assert_eq!(
        decode_reply(&[5, 0, 0, 3, 1, 0, 0, 0]),
        Err(ProtocolError::DomainContainsNul)
    );
}

#[test]
fn parser_deterministic_adversarial_corpus_never_panics_or_overconsumes() {
    let mut seed = 0x94f5_674e_0192_1929_u64;
    for length in 0..=300 {
        for case in 0..64 {
            let mut bytes = Vec::with_capacity(length);
            for _ in 0..length {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                bytes.push(seed as u8);
            }
            if case % 2 == 0 && length >= 4 {
                bytes[..4].copy_from_slice(&[5, case as u8, 0, [1, 3, 4][case % 3]]);
            }
            if let Ok(Some((_, consumed))) = decode_reply(&bytes) {
                assert!(consumed <= bytes.len());
                assert!((8..=MAX_REPLY_LEN).contains(&consumed));
            }
        }
    }
}

#[test]
fn debug_and_error_formatting_never_reveal_payloads() {
    let credentials = Credentials::new(b"secret-user", b"secret-password").unwrap();
    let auth = Authentication::UsernamePassword(&credentials);
    let domain = DomainName::new(b"private.invalid").unwrap();
    let destination = Destination::new(Address::Domain(domain.clone()), 31337).unwrap();
    let bound = BoundEndpoint {
        address: Address::Ipv4([192, 0, 2, 1]),
        port: 31337,
    };
    let texts = [
        format!("{credentials:?}"),
        format!("{auth:?}"),
        format!("{domain:?}"),
        format!("{destination:?}"),
        format!("{bound:?}"),
        format!("{:?}", encode_authentication(&credentials)),
        format!("{:?}", encode_connect(&destination)),
        format!(
            "{:?}",
            Reply {
                code: ReplyCode::RulesetDenied,
                bound
            }
        ),
    ];
    for text in texts {
        for secret in [
            "secret-user",
            "secret-password",
            "private.invalid",
            "192",
            "31337",
        ] {
            assert!(!text.contains(secret));
        }
        assert!(text.contains("redacted"));
    }
}
