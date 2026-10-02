//! Independent managed-reader fixtures plus stream, bounds and bridge contracts.
#[path = "../src/privacy_service/control_codec/mod.rs"]
pub mod codec;

use codec::{Error, MAX_INPUT, MAX_LINE, MAX_LINES, Reply, Scan, service};

fn hex(input: &str) -> Vec<u8> {
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
        .collect()
}

fn fixtures() -> impl Iterator<Item = (&'static str, Vec<u8>, Reply)> {
    include_str!("privacy_control_fixtures/replies.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            (
                fields[0],
                hex(fields[1]),
                Reply {
                    status: fields[2].parse().unwrap(),
                    lines: fields[3]
                        .split(',')
                        .map(|s| String::from_utf8(hex(s)).unwrap())
                        .collect(),
                },
            )
        })
}

#[test]
fn independent_reply_oracle_and_every_fragment_boundary() {
    for (name, wire, expected) in fixtures() {
        for end in 0..wire.len() {
            assert_eq!(
                codec::reply(&wire[..end], false),
                Ok(Scan::NeedMore),
                "{name} prefix {end}"
            );
        }
        for eof in [false, true] {
            assert_eq!(
                codec::reply(&wire, eof),
                Ok(Scan::Complete {
                    consumed: wire.len(),
                    value: expected.clone()
                }),
                "{name}"
            );
        }
    }
}

#[test]
fn independent_dotnet_status_oracle() {
    for entry in include_str!("privacy_control_fixtures/status.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let (prefix, expected) = entry.split_once('\t').unwrap();
        let mut bytes = hex(prefix);
        bytes.extend_from_slice(b" OK\r\n");
        let result = codec::reply(&bytes, true);
        if expected == "x" {
            assert!(
                matches!(result, Err(Error::InvalidStatus(_))),
                "prefix {prefix}"
            );
        } else {
            assert_eq!(
                result,
                Ok(Scan::Complete {
                    consumed: bytes.len(),
                    value: Reply {
                        status: expected.parse().unwrap(),
                        lines: vec!["OK".into()]
                    }
                }),
                "prefix {prefix}"
            );
        }
    }
}

#[test]
fn coalesced_replies_are_never_consumed_together() {
    let mut input = Vec::new();
    let mut expected = Vec::new();
    for (_, wire, reply) in fixtures() {
        input.extend_from_slice(&wire);
        expected.push((wire.len(), reply));
    }
    let mut offset = 0;
    for (length, reply) in expected {
        assert_eq!(
            codec::reply(&input[offset..], true),
            Ok(Scan::Complete {
                consumed: length,
                value: reply
            })
        );
        offset += length;
    }
    assert_eq!(offset, input.len());
}

#[test]
fn exact_crlf_scan_handles_bare_cr_and_binary_ascii() {
    let wire = b"body\rnot-a-delimiter\r\r\nnext\r\n";
    let length = b"body\rnot-a-delimiter\r\r\n".len();
    for end in 0..length {
        assert_eq!(codec::line(&wire[..end], false), Ok(Scan::NeedMore));
    }
    assert_eq!(
        codec::line(wire, true),
        Ok(Scan::Complete {
            consumed: length,
            value: "body\rnot-a-delimiter\r".into()
        })
    );
    assert_eq!(
        codec::line(&[128, 255, 0, 10, 13, 10], true),
        Ok(Scan::Complete {
            consumed: 6,
            value: "??\0\n".into()
        })
    );
}

#[test]
fn eof_and_invalid_status_categories_preserve_managed_errors() {
    assert_eq!(codec::line(b"", true), Err(Error::NoMoreData));
    assert_eq!(codec::line(b"250 OK\r", true), Err(Error::IncompleteLine));
    assert_eq!(
        codec::reply(b"", true),
        Err(Error::NoReplyLine { incomplete: false })
    );
    assert_eq!(
        codec::reply(b"250 OK\r", true),
        Err(Error::NoReplyLine { incomplete: true })
    );
    assert_eq!(codec::reply(b"OK\r\n", true), Err(Error::MissingStatus));
    assert_eq!(
        codec::reply(b"xx OK\r\n", true),
        Err(Error::InvalidStatus(*b"xx "))
    );
    assert_eq!(
        codec::reply(b"250-partial\r\n", true),
        Err(Error::NoMoreData)
    );
    assert_eq!(
        codec::reply(b"250+data\r\n.\r\n250 OK\r", true),
        Err(Error::IncompleteLine)
    );
    for (_, wire, _) in fixtures() {
        for end in 0..wire.len() {
            assert!(codec::reply(&wire[..end], true).is_err());
        }
    }
}

#[test]
fn admission_limits_are_bounded_and_accept_exact_line_limit() {
    let mut line = vec![b'a'; MAX_LINE];
    line.extend_from_slice(b"\r\n");
    assert!(
        matches!(codec::line(&line, true), Ok(Scan::Complete { consumed, .. }) if consumed == MAX_LINE+2)
    );
    line.insert(0, b'a');
    assert_eq!(codec::line(&line, true), Err(Error::Limit));
    let mut reply = b"250+data\r\n".to_vec();
    for _ in 0..MAX_LINES {
        reply.extend_from_slice(b"a\r\n");
    }
    reply.extend_from_slice(b".\r\n250 OK\r\n");
    assert_eq!(codec::reply(&reply, true), Err(Error::Limit));
    let mut large = b"250+data\r\n".to_vec();
    while large.len() < MAX_INPUT {
        large.extend_from_slice(b"\r\n");
    }
    assert_eq!(codec::reply(&large, false), Err(Error::Limit));
    let mut coalesced = b"250 OK\r\n".to_vec();
    coalesced.resize(MAX_INPUT + 1, 0);
    assert!(matches!(
        codec::reply(&coalesced, true),
        Ok(Scan::Complete { consumed: 8, .. })
    ));
}

#[test]
fn bridge_packet_contract_and_rejections() {
    assert_eq!(service::dispatch(service::PARSE_REPLY, &[0]).unwrap(), [0]);
    assert_eq!(
        service::dispatch(service::PARSE_REPLY, &[1]).unwrap(),
        [2, 2, 0]
    );
    assert_eq!(
        service::dispatch(service::PARSE_LINE, &[1]).unwrap(),
        [2, 0]
    );
    assert_eq!(
        service::dispatch(service::PARSE_REPLY, b"\x012 O\r\n").unwrap(),
        [2, 4, b'2', b' ', b'O']
    );
    let packet = service::dispatch(service::PARSE_REPLY, b"\x00250 OK\r\n650 X\r\n").unwrap();
    assert_eq!(
        packet,
        [
            1, 8, 0, 0, 0, 250, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, b'O', b'K'
        ]
    );
    assert_eq!(
        service::dispatch(service::PARSE_LINE, &[2]),
        Err(service::ServiceError::InvalidRequest)
    );
    assert_eq!(
        service::dispatch(service::PARSE_REPLY, &[]),
        Err(service::ServiceError::InvalidRequest)
    );
    assert_eq!(
        service::dispatch(0xffff, &[0]),
        Err(service::ServiceError::UnsupportedOperation)
    );
    assert_eq!(
        service::dispatch(service::PARSE_REPLY, &vec![0; MAX_INPUT + 2]),
        Err(service::ServiceError::InvalidRequest)
    );
    for (_, wire, _) in fixtures() {
        let mut payload = vec![1];
        payload.extend_from_slice(&wire);
        assert!(
            service::dispatch(service::PARSE_REPLY, &payload)
                .unwrap()
                .len()
                <= service::MAX_RESPONSE
        );
    }
}

#[test]
fn hostile_small_inputs_are_total_and_diagnostics_redact_payloads() {
    let mut state = 0xfeed1234_u32;
    for size in 0..2048 {
        let bytes: Vec<_> = (0..size)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        for eof in [false, true] {
            let _ = codec::reply(&bytes, eof);
            let _ = codec::line(&bytes, eof);
        }
    }
    let result = codec::reply(b"250 PRIVATE_SYNTHETIC\r\n", true).unwrap();
    assert!(!format!("{result:?}").contains("PRIVATE_SYNTHETIC"));
}
