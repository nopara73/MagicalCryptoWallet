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

#[test]
fn incremental_decoder_matches_oracle_at_all_boundaries() {
    use codec::stream::{Decoder, Kind};
    for (name, wire, expected) in fixtures() {
        for split in 0..wire.len() {
            let mut reader = Decoder::new(Kind::Reply);
            assert_eq!(
                reader.feed(&wire[..split], false),
                Ok(Scan::NeedMore),
                "{name}/{split}"
            );
            assert_eq!(
                reader.feed(&wire[split..], true),
                Ok(Scan::Complete {
                    consumed: wire.len() - split,
                    value: expected.clone()
                }),
                "{name}/{split}"
            );
            assert_eq!(reader.work().examined, wire.len());
            assert!(reader.work().projected <= wire.len());
        }
    }
}

#[test]
fn near_cap_bytewise_input_has_linear_work_and_bounded_buffers() {
    use codec::stream::{Decoder, Kind, MAX_BUFFER_BYTES};
    let mut wire = b"250+data=\r\n".to_vec();
    let terminal = b".\r\n250 OK\r\n";
    let body = b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\n";
    while wire.len() + body.len() + terminal.len() <= MAX_INPUT - 2 {
        wire.extend_from_slice(body);
    }
    let tail = MAX_INPUT - wire.len() - terminal.len();
    wire.extend(std::iter::repeat_n(b'y', tail - 2));
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(terminal);
    assert_eq!(wire.len(), MAX_INPUT);
    let mut reader = Decoder::new(Kind::Reply);
    for (i, byte) in wire.iter().enumerate() {
        let result = reader.feed(std::slice::from_ref(byte), false).unwrap();
        if i + 1 == wire.len() {
            let Scan::Complete { consumed: 1, value } = result else {
                panic!("near-cap reply incomplete")
            };
            assert_eq!(value.status, 250);
            assert_eq!(value.lines[value.lines.len() - 2..], [".", "250 OK"]);
        } else {
            assert_eq!(result, Scan::NeedMore);
        }
    }
    let work = reader.work();
    assert_eq!(work.examined, MAX_INPUT);
    assert!(work.projected <= MAX_INPUT);
    assert!(work.peak_buffer_bytes <= MAX_BUFFER_BYTES, "{work:?}");
}

fn reader_packet(id: u64, mode: u8, bytes: &[u8]) -> Vec<u8> {
    let mut payload = id.to_le_bytes().to_vec();
    payload.push(mode);
    payload.extend_from_slice(bytes);
    payload
}

#[test]
fn incremental_service_quota_close_and_terminal_cleanup() {
    use codec::service::{BEGIN, CLOSE, FEED, MAX_CHUNK, MAX_READERS, Readers};
    let mut service = Readers::new();
    for id in 1..=MAX_READERS as u64 {
        assert_eq!(
            service.dispatch(BEGIN, &reader_packet(id, 0, &[])).unwrap(),
            [0]
        );
    }
    assert_eq!(service.active_count(), MAX_READERS);
    assert_eq!(
        service.dispatch(BEGIN, &reader_packet(100, 0, &[])),
        Err(codec::service::ServiceError::ReaderQuota)
    );
    assert_eq!(
        service.dispatch(FEED, &reader_packet(1, 0, &vec![b'x'; MAX_CHUNK + 1])),
        Err(codec::service::ServiceError::InvalidRequest)
    );
    assert_eq!(
        service
            .dispatch(FEED, &reader_packet(1, 0, b"250 par"))
            .unwrap(),
        [0]
    );
    assert_eq!(service.dispatch(CLOSE, &1_u64.to_le_bytes()).unwrap(), [0]);
    assert_eq!(service.dispatch(CLOSE, &1_u64.to_le_bytes()).unwrap(), [0]);
    assert_eq!(service.active_count(), MAX_READERS - 1);
    assert_eq!(
        service
            .dispatch(FEED, &reader_packet(2, 1, b"250 OK\r\nnext\r\n"))
            .unwrap()[1..5],
        8_u32.to_le_bytes()
    );
    assert_eq!(service.active_count(), MAX_READERS - 2);
    assert_eq!(
        service
            .dispatch(FEED, &reader_packet(3, 1, b"xx OK\r\n"))
            .unwrap(),
        [2, 4, b'x', b'x', b' ']
    );
    assert_eq!(service.active_count(), MAX_READERS - 3);
    service.clear();
    assert_eq!(service.active_count(), 0);
    for id in 1..=MAX_READERS as u64 {
        assert_eq!(
            service.dispatch(BEGIN, &reader_packet(id, 1, &[])).unwrap(),
            [0]
        );
    }
}

#[test]
fn incremental_eof_bare_cr_and_child_lifetime_cleanup() {
    let _exclusive = SERVICE_TEST_GATE.lock().unwrap();
    use codec::stream::{Decoder, Kind};
    let mut line = Decoder::new(Kind::Line);
    for byte in b"bare\rbody\r" {
        assert_eq!(line.feed(&[*byte], false), Ok(Scan::NeedMore));
    }
    assert_eq!(
        line.feed(b"\r\ncoalesced", true),
        Ok(Scan::Complete {
            consumed: 2,
            value: Reply {
                status: 0,
                lines: vec!["bare\rbody\r".into()]
            }
        })
    );
    assert_eq!(
        Decoder::new(Kind::Reply).feed(b"", true),
        Err(Error::NoReplyLine { incomplete: false })
    );
    let mut partial = Decoder::new(Kind::Reply);
    partial.feed(b"250+body\r\npart", false).unwrap();
    assert_eq!(partial.feed(b"", true), Err(Error::IncompleteLine));
    let handle = 777_u64;
    {
        let _scope = service::ChildScope::new();
        service::dispatch(service::BEGIN, &reader_packet(handle, 0, &[])).unwrap();
        service::dispatch(
            service::FEED,
            &reader_packet(handle, 0, b"250+abandoned\r\n"),
        )
        .unwrap();
    }
    assert_eq!(
        service::dispatch(service::FEED, &reader_packet(handle, 0, b".\r\n250 OK\r\n")),
        Err(service::ServiceError::InvalidRequest)
    );
}

static SERVICE_TEST_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn controlled_dispatch_preserves_packets_and_rejections() {
    use std::sync::atomic::AtomicBool;
    let token = AtomicBool::new(false);
    for (_, wire, _) in fixtures() {
        for eof in [0, 1] {
            for prefix in 0..=wire.len() {
                let mut packet = vec![eof];
                packet.extend_from_slice(&wire[..prefix]);
                for op in [service::PARSE_LINE, service::PARSE_REPLY] {
                    assert_eq!(
                        service::dispatch_control(op, &packet, &token),
                        service::dispatch(op, &packet)
                    );
                }
            }
        }
        let mut old = service::Readers::new();
        let mut controlled = service::Readers::new();
        let begin = reader_packet(1, 0, &[]);
        assert_eq!(
            controlled.dispatch_control(service::BEGIN, &begin, &token),
            old.dispatch(service::BEGIN, &begin)
        );
        for (index, &byte) in wire.iter().enumerate() {
            let packet = reader_packet(1, u8::from(index + 1 == wire.len()), &[byte]);
            assert_eq!(
                controlled.dispatch_control(service::FEED, &packet, &token),
                old.dispatch(service::FEED, &packet)
            );
        }
        assert_eq!(controlled.active_count(), 0);
    }
    for op in [service::PARSE_REPLY, service::PARSE_LINE, 0xffff] {
        for payload in [
            &[][..],
            &[2][..],
            &b"\x01xx OK\r\n"[..],
            &b"\x00250 partial"[..],
        ] {
            assert_eq!(
                service::dispatch_control(op, payload, &token),
                service::dispatch(op, payload)
            );
        }
    }
}

fn interrupt_at<T: Send + 'static>(
    point: codec::control::Point,
    minimum: usize,
    work: impl FnOnce(&std::sync::atomic::AtomicBool) -> T + Send + 'static,
) -> (usize, T) {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;
    let interrupted = Arc::new(AtomicBool::new(false));
    let worker_token = Arc::clone(&interrupted);
    let (entered, arrival) = mpsc::sync_channel(0);
    let (release, resume) = mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        codec::control::observe(
            move |actual, progress| {
                if actual == point && progress >= minimum {
                    entered.send(progress).unwrap();
                    resume.recv_timeout(Duration::from_secs(3)).unwrap();
                }
            },
            || work(&worker_token),
        )
    });
    let progress = arrival
        .recv_timeout(Duration::from_secs(3))
        .expect("work never reached its real checkpoint");
    interrupted.store(true, Ordering::Release);
    release.send(()).unwrap();
    (progress, worker.join().unwrap())
}

#[test]
fn late_begin_cancellation_reclaims_inserted_session() {
    let (_, (result, mut readers)) =
        interrupt_at(codec::control::Point::BeginCommitted, 0, |token| {
            let mut readers = service::Readers::new();
            let result =
                readers.dispatch_control(service::BEGIN, &reader_packet(41, 0, &[]), token);
            (result, readers)
        });
    assert_eq!(result, Err(service::ServiceError::Interrupted));
    assert_eq!(readers.active_count(), 0);
    readers
        .dispatch(service::BEGIN, &reader_packet(42, 0, &[]))
        .unwrap();
    assert_eq!(readers.active_count(), 1);
}

#[test]
fn active_feed_scan_cancellation_reclaims_session() {
    let (progress, (result, mut readers)) =
        interrupt_at(codec::control::Point::Scan, 4096, |token| {
            let mut readers = service::Readers::new();
            readers
                .dispatch(service::BEGIN, &reader_packet(51, 0, &[]))
                .unwrap();
            let mut body = b"250+data\r\n".to_vec();
            body.resize(service::MAX_CHUNK, b'x');
            let result =
                readers.dispatch_control(service::FEED, &reader_packet(51, 0, &body), token);
            (result, readers)
        });
    assert!(progress >= 4096 && progress < service::MAX_CHUNK);
    assert_eq!(result, Err(service::ServiceError::Interrupted));
    assert_eq!(readers.active_count(), 0);
    readers
        .dispatch(service::BEGIN, &reader_packet(52, 0, &[]))
        .unwrap();
    assert_eq!(
        readers.dispatch(service::FEED, &reader_packet(52, 1, b"250 OK\r\n")),
        service::dispatch(service::PARSE_REPLY, b"\x01250 OK\r\n")
    );
}

fn cancel_long_line_at(point: codec::control::Point) {
    let (progress, (result, readers)) = interrupt_at(point, 4096, |token| {
        let mut readers = service::Readers::new();
        readers
            .dispatch(service::BEGIN, &reader_packet(61, 1, &[]))
            .unwrap();
        for _ in 0..MAX_LINE / service::MAX_CHUNK {
            assert_eq!(
                readers
                    .dispatch(
                        service::FEED,
                        &reader_packet(61, 0, &vec![b'x'; service::MAX_CHUNK])
                    )
                    .unwrap(),
                [0]
            );
        }
        let result = readers.dispatch_control(service::FEED, &reader_packet(61, 1, b"\r\n"), token);
        (result, readers)
    });
    assert!(progress >= 4096 && progress < MAX_LINE);
    assert_eq!(result, Err(service::ServiceError::Interrupted));
    assert_eq!(readers.active_count(), 0);
}

#[test]
fn active_feed_projection_cancellation_reclaims_session() {
    cancel_long_line_at(codec::control::Point::Project);
}

#[test]
fn active_feed_encoding_cancellation_returns_no_partial_packet() {
    cancel_long_line_at(codec::control::Point::EncodeBytes);
}

#[test]
fn precancel_cleanup_and_global_control_api_are_fail_closed() {
    use std::sync::atomic::AtomicBool;
    let _exclusive = SERVICE_TEST_GATE.lock().unwrap();
    let _child = service::ChildScope::new();
    let canceled = AtomicBool::new(true);
    let live = AtomicBool::new(false);
    assert_eq!(
        service::dispatch_control(service::BEGIN, &reader_packet(71, 0, &[]), &canceled),
        Err(service::ServiceError::Interrupted)
    );
    assert_eq!(
        service::dispatch(service::FEED, &reader_packet(71, 1, b"250 OK\r\n")),
        Err(service::ServiceError::InvalidRequest)
    );
    service::dispatch_control(service::BEGIN, &reader_packet(72, 0, &[]), &live).unwrap();
    assert_eq!(
        service::dispatch_control(
            service::FEED,
            &reader_packet(72, 0, b"250+body\r\n"),
            &canceled
        ),
        Err(service::ServiceError::Interrupted)
    );
    assert_eq!(
        service::dispatch(service::FEED, &reader_packet(72, 1, b".\r\n250 OK\r\n")),
        Err(service::ServiceError::InvalidRequest)
    );
    service::dispatch(service::BEGIN, &reader_packet(73, 0, &[])).unwrap();
    assert_eq!(
        service::dispatch_control(service::CLOSE, &73_u64.to_le_bytes(), &canceled),
        Ok(vec![0])
    );
    assert_eq!(
        service::dispatch(service::FEED, &reader_packet(73, 1, b"250 OK\r\n")),
        Err(service::ServiceError::InvalidRequest)
    );
    for id in 100..100 + service::MAX_READERS as u64 {
        service::dispatch_control(service::BEGIN, &reader_packet(id, 0, &[]), &live).unwrap();
    }
    let mut decoder = codec::stream::Decoder::new(codec::stream::Kind::Reply);
    assert_eq!(
        decoder.feed_control(b"250 OK\r\n", true, &canceled),
        Err(Error::Interrupted)
    );
    assert_eq!(decoder.work().examined, 0);
    assert_eq!(decoder.feed(b"250 OK\r\n", true), Err(Error::Limit));
}
