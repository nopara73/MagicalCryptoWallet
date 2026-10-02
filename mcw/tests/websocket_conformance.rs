//! First-party tests with RFC 6455 literal vectors and synthetic Nostr messages.
//! Actual production module is compiled here without a substitute or Cargo crate.
#![forbid(unsafe_code)]
#[path = "../src/websocket.rs"]
pub mod websocket;
use websocket::*;

const NONCE: [u8; 16] = *b"the sample nonce";
const MASK: [u8; 4] = [0x37, 0xfa, 0x21, 0x3d];
const ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

fn fields() -> Vec<Header<'static>> {
    vec![
        Header {
            name: "Upgrade",
            value: "websocket",
        },
        Header {
            name: "Connection",
            value: "Upgrade",
        },
        Header {
            name: "Sec-WebSocket-Accept",
            value: ACCEPT,
        },
    ]
}
fn client(limits: Limits) -> Client {
    let mut client = Client::new(ClientHandshake::new(NONCE, &[]).unwrap(), limits);
    client.accept_upgrade(101, &fields(), false).unwrap();
    client
}
fn drain(client: &mut Client) -> Vec<u8> {
    let bytes = client.outgoing().to_vec();
    assert!(!bytes.is_empty());
    for _ in &bytes {
        client.advance_written(1, false).unwrap();
    }
    assert!(client.outgoing().is_empty());
    bytes
}
// Independent synthetic server formatter for tests, not the production encoder.
fn peer_frame(opcode: u8, fin: bool, bytes: &[u8]) -> Vec<u8> {
    let mut output = vec![opcode | if fin { 0x80 } else { 0 }];
    match bytes.len() {
        0..=125 => output.push(bytes.len() as u8),
        126..=65535 => {
            output.push(126);
            output.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        }
        _ => {
            output.push(127);
            output.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        }
    }
    output.extend_from_slice(bytes);
    output
}
fn receive_bytes(client: &mut Client, bytes: &[u8]) -> Option<Event> {
    let mut event = None;
    for byte in bytes {
        let got = client.receive(&[*byte], false).unwrap();
        assert_eq!(got.consumed, 1);
        assert!(!got.backpressured);
        if got.event.is_some() {
            assert!(event.is_none());
            event = got.event;
        }
    }
    event
}
fn payload(bytes: &[u8], direction: Direction) -> Vec<u8> {
    parse_frame(bytes, direction, 1024 * 1024)
        .unwrap()
        .unwrap()
        .frame
        .payload()
        .unwrap()
}

#[test]
fn rfc6455_handshake_literal() {
    let handshake = ClientHandshake::new(NONCE, &["chat", "superchat"]).unwrap();
    assert_eq!(handshake.key(), "dGhlIHNhbXBsZSBub25jZQ==");
    assert_eq!(handshake.expected_accept(), ACCEPT);
    let got: Vec<_> = handshake
        .request_fields()
        .iter()
        .map(|h| (h.name, h.value))
        .collect();
    assert_eq!(
        got,
        [
            ("Upgrade", "websocket"),
            ("Connection", "Upgrade"),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ("Sec-WebSocket-Version", "13"),
            ("Sec-WebSocket-Protocol", "chat, superchat")
        ]
    );
    let mut response = fields();
    response.push(Header {
        name: "Sec-WebSocket-Protocol",
        value: "chat",
    });
    assert_eq!(
        handshake
            .validate_response(101, &response)
            .unwrap()
            .subprotocol
            .as_deref(),
        Some("chat")
    );
}

#[test]
fn handshake_case_ows_and_repeated_connection_fields() {
    let handshake = ClientHandshake::new(NONCE, &[]).unwrap();
    let response = [
        Header {
            name: "uPgRaDe",
            value: "\tWebSocket ",
        },
        Header {
            name: "connection",
            value: "keep-alive",
        },
        Header {
            name: "CONNECTION",
            value: " \tUPGRADE\t ",
        },
        Header {
            name: "sec-websocket-accept",
            value: " s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\t",
        },
        Header {
            name: "X-Other",
            value: "opaque é",
        },
    ];
    assert_eq!(
        handshake
            .validate_response(101, &response)
            .unwrap()
            .subprotocol,
        None
    );
    assert_eq!(handshake.request_fields().len(), 4);
}

#[test]
fn handshake_rejects_status_missing_fields_accept_and_injection() {
    let handshake = ClientHandshake::new(NONCE, &[]).unwrap();
    for status in [0, 100, 200, 301, 401, 426, 999] {
        assert_eq!(
            handshake.validate_response(status, &fields()),
            Err(Error::UnexpectedStatus)
        );
    }
    for index in 0..3 {
        let mut response = fields();
        response.remove(index);
        assert_eq!(
            handshake.validate_response(101, &response),
            Err([
                Error::MissingUpgrade,
                Error::MissingConnectionUpgrade,
                Error::InvalidAccept
            ][index])
        );
    }
    for value in [
        "",
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo",
        "S3pPLMBiTxaQ9kYGzzhZRbK+xOo=",
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo==",
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=, s3pPLMBiTxaQ9kYGzzhZRbK+xOo=",
        "\ns3pPLMBiTxaQ9kYGzzhZRbK+xOo=",
    ] {
        let mut response = fields();
        response[2].value = value;
        assert!(handshake.validate_response(101, &response).is_err());
    }
    for value in [
        "upgrade,",
        ",upgrade",
        "upgrade,,keep-alive",
        "upgrade;x=y",
        "up grade",
        "upgrade\r\nX: injected",
        "upgrade\0",
        "upgrade\u{7f}",
    ] {
        let mut response = fields();
        response[1].value = value;
        assert_eq!(
            handshake.validate_response(101, &response),
            Err(Error::InvalidHeader)
        );
    }
    for value in [
        "h2c, websocket",
        "websocket/13",
        "websocket, websocket",
        "websocketx",
    ] {
        let mut response = fields();
        response[0].value = value;
        assert_eq!(
            handshake.validate_response(101, &response),
            Err(Error::MissingUpgrade)
        );
    }
    for name in ["", "Upgrade ", ":status", "é", "X\n"] {
        let mut response = fields();
        response.push(Header { name, value: "v" });
        assert_eq!(
            handshake.validate_response(101, &response),
            Err(Error::InvalidHeader)
        );
    }
}

#[test]
fn handshake_duplicate_extension_and_protocol_rules() {
    let handshake = ClientHandshake::new(NONCE, &["chat", "Chat"]).unwrap();
    for index in [0, 2] {
        let mut response = fields();
        response.push(response[index]);
        assert_eq!(
            handshake.validate_response(101, &response),
            Err(Error::DuplicateHeader)
        );
    }
    for value in ["", "permessage-deflate", "x-private"] {
        let mut response = fields();
        response.push(Header {
            name: "Sec-WebSocket-Extensions",
            value,
        });
        assert_eq!(
            handshake.validate_response(101, &response),
            Err(Error::UnsupportedExtension)
        );
    }
    for value in ["", "CHAT", "unoffered", "chat, Chat", "chat;v=1"] {
        let mut response = fields();
        response.push(Header {
            name: "Sec-WebSocket-Protocol",
            value,
        });
        assert_eq!(
            handshake.validate_response(101, &response),
            Err(Error::InvalidSubprotocol)
        );
    }
    let mut response = fields();
    response.extend(
        [Header {
            name: "Sec-WebSocket-Protocol",
            value: "chat",
        }; 2],
    );
    assert_eq!(
        handshake.validate_response(101, &response),
        Err(Error::DuplicateHeader)
    );
    for protocols in [
        vec!["chat", "chat"],
        vec![""],
        vec!["bad protocol"],
        vec!["é"],
        vec!["bad\r\nX: y"],
        vec!["a,b"],
    ] {
        assert_eq!(
            ClientHandshake::new(NONCE, &protocols).unwrap_err(),
            Error::InvalidSubprotocol
        );
    }
    assert_eq!(
        ClientHandshake::new(NONCE, &[&"p".repeat(129)]).unwrap_err(),
        Error::InvalidSubprotocol
    );
    assert_eq!(
        ClientHandshake::new(NONCE, &vec!["p"; 33]).unwrap_err(),
        Error::InvalidSubprotocol
    );
}

#[test]
fn handshake_memory_bounds() {
    let handshake = ClientHandshake::new(NONCE, &[]).unwrap();
    assert_eq!(
        handshake.validate_response(
            101,
            &vec![
                Header {
                    name: "X",
                    value: "v"
                };
                129
            ]
        ),
        Err(Error::HandshakeTooLarge)
    );
    let large = "a".repeat(16 * 1024);
    let mut response = fields();
    response.push(Header {
        name: "X",
        value: &large,
    });
    assert_eq!(
        handshake.validate_response(101, &response),
        Err(Error::HandshakeTooLarge)
    );
    let long_name = "x".repeat(257);
    response = fields();
    response.push(Header {
        name: &long_name,
        value: "v",
    });
    assert_eq!(
        handshake.validate_response(101, &response),
        Err(Error::InvalidHeader)
    );
}

#[test]
fn rfc6455_literal_frames_and_mask() {
    let unmasked = b"\x81\x05Hello";
    let masked = [
        0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
    ];
    assert_eq!(payload(unmasked, Direction::ServerToClient), b"Hello");
    assert_eq!(payload(&masked, Direction::ClientToServer), b"Hello");
    assert_eq!(
        encode_frame(
            Direction::ClientToServer,
            Opcode::Text,
            true,
            b"Hello",
            Some(MASK),
            125
        )
        .unwrap(),
        masked
    );
    assert_eq!(
        encode_frame(
            Direction::ServerToClient,
            Opcode::Text,
            true,
            b"Hello",
            None,
            125
        )
        .unwrap(),
        unmasked
    );
    let mut connection = client(Limits::default());
    assert_eq!(receive_bytes(&mut connection, b"\x01\x03Hel"), None);
    assert_eq!(
        receive_bytes(&mut connection, b"\x80\x02lo"),
        Some(Event::Text("Hello".to_owned()))
    );
    assert_eq!(
        receive_bytes(&mut connection, b"\x89\x05Hello"),
        Some(Event::Ping(b"Hello".to_vec()))
    );
    connection.queue_pending_control(MASK, false).unwrap();
    assert_eq!(
        drain(&mut connection),
        [
            0x8a, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58
        ]
    );
}

#[test]
fn exact_consumption_and_all_prefixes() {
    for bytes in [
        peer_frame(1, true, b"Hello"),
        peer_frame(2, true, &vec![0xa5; 126]),
        peer_frame(2, true, &vec![0xff; 256]),
    ] {
        for prefix in 0..bytes.len() {
            assert!(
                parse_frame(&bytes[..prefix], Direction::ServerToClient, 1024)
                    .unwrap()
                    .is_none()
            );
        }
        let mut coalesced = bytes.clone();
        coalesced.extend_from_slice(b"trailing");
        assert_eq!(
            parse_frame(&coalesced, Direction::ServerToClient, 1024)
                .unwrap()
                .unwrap()
                .consumed,
            bytes.len()
        );
        for split in 0..=bytes.len() {
            let mut connection = client(Limits::default());
            let first = connection.receive(&bytes[..split], false).unwrap();
            assert_eq!(first.consumed, split);
            if split == bytes.len() {
                assert!(first.event.is_some());
                continue;
            }
            let second = connection.receive(&coalesced[split..], false).unwrap();
            assert!(second.event.is_some());
            assert_eq!(second.consumed, bytes.len() - split);
        }
    }
}

#[test]
fn coalesced_frames_leave_next_frame_unconsumed() {
    let stream = b"\x81\x01A\x81\x01B\x82\x00";
    let mut connection = client(Limits::default());
    let one = connection.receive(stream, false).unwrap();
    assert_eq!(one.consumed, 3);
    assert_eq!(one.event, Some(Event::Text("A".into())));
    let two = connection.receive(&stream[3..], false).unwrap();
    assert_eq!(two.consumed, 3);
    assert_eq!(two.event, Some(Event::Text("B".into())));
    let three = connection.receive(&stream[6..], false).unwrap();
    assert_eq!(three.consumed, 2);
    assert_eq!(three.event, Some(Event::Binary(vec![])));
}

#[test]
fn canonical_length_boundaries() {
    for length in [0, 1, 124, 125, 126, 127, 255, 256, 65535, 65536, 65537] {
        let data: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
        let expected = peer_frame(2, true, &data);
        assert_eq!(
            encode_frame(
                Direction::ServerToClient,
                Opcode::Binary,
                true,
                &data,
                None,
                65537
            )
            .unwrap(),
            expected
        );
        let parsed = parse_frame(&expected, Direction::ServerToClient, 65537)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.frame.payload().unwrap(), data);
        assert_eq!(parsed.consumed, expected.len());
        if length == 65536 {
            assert_eq!(&expected[..10], &[0x82, 0x7f, 0, 0, 0, 0, 0, 1, 0, 0]);
        }
        if length == 256 {
            assert_eq!(&expected[..4], &[0x82, 0x7e, 1, 0]);
        }
        for mask in [[0, 0, 0, 0], [0xff; 4], MASK] {
            let encoded = encode_frame(
                Direction::ClientToServer,
                Opcode::Binary,
                true,
                &data,
                Some(mask),
                65537,
            )
            .unwrap();
            assert_eq!(payload(&encoded, Direction::ClientToServer), data);
        }
    }
    for bytes in [
        &[0x82, 126, 0, 125][..],
        &[0x82, 127, 0, 0, 0, 0, 0, 0, 0xff, 0xff],
    ] {
        assert_eq!(
            parse_frame(bytes, Direction::ServerToClient, usize::MAX).unwrap_err(),
            Error::NonCanonicalLength
        );
    }
    assert_eq!(
        parse_frame(
            &[0x82, 127, 0x80, 0, 0, 0, 0, 0, 0, 0],
            Direction::ServerToClient,
            usize::MAX
        )
        .unwrap_err(),
        Error::LengthOverflow
    );
    assert_eq!(
        parse_frame(
            &[0x82, 127, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            Direction::ServerToClient,
            1024
        )
        .unwrap_err(),
        Error::FrameTooLarge
    );
}

#[test]
fn reserved_bits_opcodes_direction_and_controls_exhaustive() {
    for first in 0_u8..=255 {
        let result = inspect_header(&[first, 0], Direction::ServerToClient, 1024);
        let op = first & 15;
        let valid =
            first & 0x70 == 0 && [0, 1, 2, 8, 9, 10].contains(&op) && (op < 8 || first & 128 != 0);
        assert_eq!(result.is_ok(), valid, "first={first:02x}");
    }
    for opcode in [
        Opcode::Continuation,
        Opcode::Text,
        Opcode::Binary,
        Opcode::Close,
        Opcode::Ping,
        Opcode::Pong,
    ] {
        let first = 128 | opcode as u8;
        assert_eq!(
            inspect_header(&[first, 128], Direction::ServerToClient, 1024).unwrap_err(),
            Error::WrongMask
        );
        assert_eq!(
            inspect_header(&[first, 0], Direction::ClientToServer, 1024).unwrap_err(),
            Error::WrongMask
        );
        assert_eq!(
            encode_frame(Direction::ClientToServer, opcode, true, &[], None, 1024).unwrap_err(),
            Error::WrongMask
        );
        assert_eq!(
            encode_frame(
                Direction::ServerToClient,
                opcode,
                true,
                &[],
                Some(MASK),
                1024
            )
            .unwrap_err(),
            Error::WrongMask
        );
    }
    for op in [8, 9, 10] {
        for length in 126..=127 {
            assert_eq!(
                inspect_header(&[128 | op, length], Direction::ServerToClient, 1024).unwrap_err(),
                Error::InvalidControl
            );
        }
    }
    for op in [Opcode::Close, Opcode::Ping, Opcode::Pong] {
        assert_eq!(
            encode_frame(Direction::ClientToServer, op, false, &[], Some(MASK), 1024).unwrap_err(),
            Error::InvalidControl
        );
        assert_eq!(
            encode_frame(
                Direction::ClientToServer,
                op,
                true,
                &[0; 126],
                Some(MASK),
                1024
            )
            .unwrap_err(),
            Error::InvalidControl
        );
    }
    assert!(
        parse_frame(
            &peer_frame(9, true, &[0xff; 125]),
            Direction::ServerToClient,
            125
        )
        .unwrap()
        .is_some()
    );
}

#[test]
fn close_payload_wire_codes_reasons_and_limits() {
    for direction in [Direction::ClientToServer, Direction::ServerToClient] {
        for code in 0_u16..=65535 {
            let expected = (1000..=1003).contains(&code)
                || (1007..=1014).contains(&code)
                || (3000..=4999).contains(&code);
            let expected = expected && !(code == 1010 && direction == Direction::ServerToClient);
            assert_eq!(valid_close_code(code, direction), expected);
        }
    }
    assert_eq!(
        close_payload(Some(1000), "bye", Direction::ClientToServer).unwrap(),
        b"\x03\xe8bye"
    );
    assert_eq!(
        close_payload(None, "", Direction::ClientToServer).unwrap(),
        b""
    );
    assert_eq!(
        close_payload(None, "reason", Direction::ClientToServer).unwrap_err(),
        Error::InvalidClose
    );
    assert_eq!(
        close_payload(Some(1000), &"é".repeat(62), Direction::ClientToServer).unwrap_err(),
        Error::InvalidControl
    );
    assert_eq!(
        close_payload(Some(1000), &"a".repeat(123), Direction::ClientToServer)
            .unwrap()
            .len(),
        125
    );
    for bytes in [
        vec![0],
        vec![3, 0xed],
        vec![3, 0xee],
        vec![3, 0xf7],
        vec![0, 0],
        vec![0xff, 0xff],
    ] {
        assert_eq!(
            parse_frame(&peer_frame(8, true, &bytes), Direction::ServerToClient, 125).unwrap_err(),
            Error::InvalidClose
        );
    }
    assert_eq!(
        parse_frame(
            &peer_frame(8, true, &[3, 0xe8, 0xc0, 0x80]),
            Direction::ServerToClient,
            125
        )
        .unwrap_err(),
        Error::InvalidUtf8
    );
    for code in [
        1000_u16, 1001, 1002, 1003, 1007, 1008, 1009, 1011, 1012, 1013, 1014, 3000, 3999, 4000,
        4999,
    ] {
        assert!(
            parse_frame(
                &peer_frame(8, true, &code.to_be_bytes()),
                Direction::ServerToClient,
                125
            )
            .unwrap()
            .is_some()
        );
    }
}

#[test]
fn utf8_valid_boundaries_and_bad_scalars() {
    for text in [
        "",
        "a\0b",
        "é",
        "€",
        "😀",
        "\u{80}\u{7ff}\u{800}\u{d7ff}\u{e000}\u{ffff}\u{10000}\u{10ffff}",
    ] {
        let bytes = peer_frame(1, true, text.as_bytes());
        assert_eq!(payload(&bytes, Direction::ServerToClient), text.as_bytes());
    }
    for bad in [
        &[0x80][..],
        &[0xc0, 0x80],
        &[0xc1, 0xbf],
        &[0xc2],
        &[0xe0, 0x80, 0x80],
        &[0xed, 0xa0, 0x80],
        &[0xf0, 0x80, 0x80, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0xf5, 0x80, 0x80, 0x80],
        &[0xff],
        &[0xe2, 0x28, 0xa1],
    ] {
        assert_eq!(
            parse_frame(&peer_frame(1, true, bad), Direction::ServerToClient, 125).unwrap_err(),
            Error::InvalidUtf8
        );
        assert_eq!(
            encode_frame(
                Direction::ClientToServer,
                Opcode::Text,
                true,
                bad,
                Some(MASK),
                125
            )
            .unwrap_err(),
            Error::InvalidUtf8
        );
        let masked = encode_frame(
            Direction::ClientToServer,
            Opcode::Binary,
            true,
            bad,
            Some(MASK),
            125,
        )
        .unwrap();
        let mut text = masked;
        text[0] = 0x81;
        assert_eq!(
            parse_frame(&text, Direction::ClientToServer, 125).unwrap_err(),
            Error::InvalidUtf8
        );
    }
}

#[test]
fn fragmented_utf8_every_split_with_interleaved_ping_and_pong() {
    let text = "Aé€😀Z";
    for split in 0..=text.len() {
        let mut connection = client(Limits::default());
        assert_eq!(
            receive_bytes(
                &mut connection,
                &peer_frame(1, false, &text.as_bytes()[..split])
            ),
            None
        );
        assert_eq!(
            receive_bytes(&mut connection, b"\x89\x01P"),
            Some(Event::Ping(vec![b'P']))
        );
        assert_eq!(connection.pending_control(), Some(Opcode::Pong));
        connection.queue_pending_control(MASK, false).unwrap();
        assert_eq!(
            payload(&drain(&mut connection), Direction::ClientToServer),
            b"P"
        );
        assert_eq!(
            receive_bytes(&mut connection, b"\x8a\x01Q"),
            Some(Event::Pong(vec![b'Q']))
        );
        assert_eq!(
            receive_bytes(
                &mut connection,
                &peer_frame(0, true, &text.as_bytes()[split..])
            ),
            Some(Event::Text(text.into()))
        );
    }
}

#[test]
fn fragmented_bad_utf8_fails_at_invalid_continuation_or_final() {
    for (first, last) in [
        (vec![0xe2], vec![0x28, 0xa1]),
        (vec![0xed], vec![0xa0, 0x80]),
        (vec![0xf4], vec![0x90, 0x80, 0x80]),
        (vec![0xc2], vec![]),
    ] {
        let mut connection = client(Limits::default());
        assert_eq!(
            connection
                .receive(&peer_frame(1, false, &first), false)
                .unwrap()
                .event,
            None
        );
        assert_eq!(
            connection
                .receive(&peer_frame(0, true, &last), false)
                .unwrap_err(),
            Error::InvalidUtf8
        );
        assert_eq!(connection.state(), State::Failed);
        assert_eq!(
            connection.receive(b"\x81\x00", false).unwrap_err(),
            Error::InvalidState
        );
    }
}

#[test]
fn fragmentation_order_binary_and_empty_fragments() {
    let mut connection = client(Limits::default());
    assert_eq!(
        connection.receive(b"\x80\x00", false).unwrap_err(),
        Error::InvalidFragmentation
    );
    for new_op in [1, 2] {
        let mut connection = client(Limits::default());
        connection.receive(b"\x01\x00", false).unwrap();
        assert_eq!(
            connection
                .receive(&peer_frame(new_op, true, b"x"), false)
                .unwrap_err(),
            Error::InvalidFragmentation
        );
    }
    let mut connection = client(Limits::default());
    assert_eq!(receive_bytes(&mut connection, b"\x02\x02\xff\x00"), None);
    assert_eq!(receive_bytes(&mut connection, b"\x00\x00"), None);
    assert_eq!(
        receive_bytes(&mut connection, b"\x80\x01\x80"),
        Some(Event::Binary(vec![0xff, 0, 0x80]))
    );
    assert_eq!(
        receive_bytes(&mut connection, b"\x81\x00"),
        Some(Event::Text("".into()))
    );
}

#[test]
fn frame_message_and_fragment_limits_before_payload_buffering() {
    for args in [
        (0, 1, 1),
        (1, 0, 1),
        (1, 1, 0),
        (16 * 1024 * 1024 + 1, 1, 1),
        (1, 64 * 1024 * 1024 + 1, 1),
        (1, 1, 65537),
    ] {
        assert_eq!(
            Limits::new(args.0, args.1, args.2).unwrap_err(),
            Error::InvalidLimits
        );
    }
    let limits = Limits::new(125, 5, 2).unwrap();
    let mut connection = client(limits);
    assert_eq!(
        connection.receive(&[0x82, 126, 0, 126], false).unwrap_err(),
        Error::FrameTooLarge
    );
    let mut connection = client(limits);
    assert_eq!(
        connection.receive(&[0x82, 6], false).unwrap_err(),
        Error::MessageTooLarge
    );
    let mut connection = client(limits);
    connection.receive(b"\x02\x03abc", false).unwrap();
    assert_eq!(
        connection.receive(&[0x80, 3], false).unwrap_err(),
        Error::MessageTooLarge
    );
    let mut connection = client(limits);
    connection.receive(b"\x01\x00", false).unwrap();
    connection.receive(b"\x00\x00", false).unwrap();
    assert_eq!(
        connection.receive(b"\x80\x00", false).unwrap_err(),
        Error::TooManyFragments
    );
    let mut connection = client(limits);
    connection
        .queue_frame(Opcode::Binary, false, b"abc", MASK, false)
        .unwrap();
    drain(&mut connection);
    assert_eq!(
        connection
            .queue_frame(Opcode::Continuation, true, b"def", MASK, false)
            .unwrap_err(),
        Error::MessageTooLarge
    );
    assert_eq!(connection.state(), State::Open);
    connection
        .queue_frame(Opcode::Continuation, true, b"de", MASK, false)
        .unwrap();
    drain(&mut connection);
}

#[test]
fn pending_pong_applies_lossless_backpressure_and_exact_consumption() {
    let mut connection = client(Limits::default());
    connection
        .queue_frame(Opcode::Text, true, b"outgoing", MASK, false)
        .unwrap();
    let got = connection.receive(b"\x89\x01P\x81\x01D", false).unwrap();
    assert_eq!(got.consumed, 3);
    assert_eq!(got.event, Some(Event::Ping(b"P".to_vec())));
    let blocked = connection.receive(b"\x81\x01D", false).unwrap();
    assert!(blocked.backpressured);
    assert_eq!(blocked.consumed, 0);
    assert_eq!(
        connection.queue_pending_control(MASK, false).unwrap_err(),
        Error::Backpressure
    );
    assert_eq!(
        connection
            .queue_frame(Opcode::Text, true, b"more", MASK, false)
            .unwrap_err(),
        Error::Backpressure
    );
    drain(&mut connection);
    connection
        .queue_pending_control([1, 2, 3, 4], false)
        .unwrap();
    let got = connection.receive(b"\x81\x01D", false).unwrap();
    assert_eq!(got.event, Some(Event::Text("D".into())));
    let reply = drain(&mut connection);
    assert_eq!(&reply[2..6], &[1, 2, 3, 4]);
    assert_eq!(payload(&reply, Direction::ClientToServer), b"P");
}

#[test]
fn outgoing_fragmented_utf8_control_interleaving_and_immutable_bytes() {
    let text = "é😀";
    for split in 0..=text.len() {
        let mut connection = client(Limits::default());
        connection
            .queue_frame(Opcode::Text, false, &text.as_bytes()[..split], MASK, false)
            .unwrap();
        let first = drain(&mut connection);
        connection
            .queue_frame(Opcode::Ping, true, b"P", [1, 2, 3, 4], false)
            .unwrap();
        drain(&mut connection);
        connection
            .queue_frame(
                Opcode::Continuation,
                true,
                &text.as_bytes()[split..],
                [5, 6, 7, 8],
                false,
            )
            .unwrap();
        let last = drain(&mut connection);
        let mut decoded = payload(&first, Direction::ClientToServer);
        decoded.extend(payload(&last, Direction::ClientToServer));
        assert_eq!(decoded, text.as_bytes());
    }
    let mut connection = client(Limits::default());
    connection
        .queue_frame(Opcode::Text, false, &[0xe2], MASK, false)
        .unwrap();
    drain(&mut connection);
    assert_eq!(
        connection
            .queue_frame(Opcode::Continuation, true, &[0x28, 0xa1], MASK, false)
            .unwrap_err(),
        Error::InvalidUtf8
    );
    assert_eq!(
        connection
            .queue_frame(Opcode::Binary, true, b"x", MASK, false)
            .unwrap_err(),
        Error::InvalidFragmentation
    );
    connection
        .queue_frame(Opcode::Continuation, true, &[0x82, 0xac], MASK, false)
        .unwrap();
    let stable = connection.outgoing().to_vec();
    assert_eq!(
        connection
            .queue_frame(Opcode::Text, true, b"x", MASK, false)
            .unwrap_err(),
        Error::Backpressure
    );
    assert_eq!(connection.outgoing(), stable);
    connection.advance_written(2, false).unwrap();
    assert_eq!(connection.outgoing(), &stable[2..]);
    drain(&mut connection);
}

#[test]
fn peer_close_echo_and_clean_eof_requires_written_reply() {
    for close in [b"\x88\x00".as_slice(), b"\x88\x05\x03\xe8bye".as_slice()] {
        let mut connection = client(Limits::default());
        let got = connection.receive(close, false).unwrap();
        assert_eq!(got.consumed, close.len());
        assert!(matches!(got.event, Some(Event::Close(_))));
        assert_eq!(connection.state(), State::Closing);
        assert_eq!(connection.pending_control(), Some(Opcode::Close));
        let blocked = connection.receive(b"\x81\x01x", false).unwrap();
        assert_eq!(blocked.consumed, 0);
        assert!(blocked.backpressured);
        connection.queue_pending_control(MASK, false).unwrap();
        assert!(!connection.close_sent());
        assert_eq!(connection.state(), State::Closing);
        let echo = drain(&mut connection);
        assert_eq!(payload(&echo, Direction::ClientToServer), &close[2..]);
        assert!(connection.close_sent());
        assert_eq!(connection.state(), State::Closed);
        assert_eq!(
            connection.receive(close, false).unwrap_err(),
            Error::InvalidState
        );
        connection.transport_eof().unwrap();
    }
    let mut connection = client(Limits::default());
    connection.receive(b"\x88\x00", false).unwrap();
    assert_eq!(
        connection.transport_eof().unwrap_err(),
        Error::UnexpectedEof
    );
    assert_eq!(connection.state(), State::Failed);
}

#[test]
fn simultaneous_close_before_and_after_local_write() {
    for sent in [false, true] {
        let mut connection = client(Limits::default());
        connection
            .queue_close(Some(1000), "local", MASK, false)
            .unwrap();
        assert_eq!(connection.state(), State::Closing);
        if sent {
            drain(&mut connection);
        }
        connection.receive(b"\x88\x04\x03\xe9go", false).unwrap();
        assert_eq!(connection.close_received().unwrap().code, Some(1001));
        assert_eq!(connection.pending_control(), None);
        if sent {
            assert_eq!(connection.state(), State::Closed);
        } else {
            assert_eq!(connection.state(), State::Closing);
            assert_eq!(
                payload(&drain(&mut connection), Direction::ClientToServer),
                b"\x03\xe8local"
            );
        }
        assert_eq!(connection.state(), State::Closed);
        connection.transport_eof().unwrap();
    }
}

#[test]
fn peer_close_discards_unstarted_data_but_completes_partial_frame() {
    for partial in [false, true] {
        let mut connection = client(Limits::default());
        connection
            .queue_frame(Opcode::Text, true, b"synthetic", MASK, false)
            .unwrap();
        let original = connection.outgoing().to_vec();
        if partial {
            connection.advance_written(3, false).unwrap();
        }
        connection.receive(b"\x88\x02\x03\xe8", false).unwrap();
        if partial {
            assert_eq!(connection.outgoing(), &original[3..]);
            assert_eq!(
                connection.queue_pending_control(MASK, false).unwrap_err(),
                Error::Backpressure
            );
            drain(&mut connection);
        } else {
            assert!(connection.outgoing().is_empty());
        }
        connection
            .queue_pending_control([9, 8, 7, 6], false)
            .unwrap();
        assert_eq!(
            payload(&drain(&mut connection), Direction::ClientToServer),
            b"\x03\xe8"
        );
        assert_eq!(connection.state(), State::Closed);
    }
}

#[test]
fn locally_closing_suppresses_data_but_answers_ping_until_peer_close() {
    let mut connection = client(Limits::default());
    connection.queue_close(Some(1000), "", MASK, false).unwrap();
    drain(&mut connection);
    assert_eq!(
        connection
            .queue_frame(Opcode::Text, true, b"no", MASK, false)
            .unwrap_err(),
        Error::InvalidState
    );
    assert_eq!(connection.receive(b"\x81\x01D", false).unwrap().event, None);
    assert_eq!(
        connection.receive(b"\x89\x01P", false).unwrap().event,
        Some(Event::Ping(vec![b'P']))
    );
    connection
        .queue_pending_control([1, 2, 3, 4], false)
        .unwrap();
    drain(&mut connection);
    connection.receive(b"\x88\x00", false).unwrap();
    assert_eq!(connection.state(), State::Closed);
}

#[test]
fn truncated_frame_and_abnormal_eof_are_terminal() {
    let frame = b"\x81\x05Hello";
    for prefix in 0..=frame.len() {
        let mut connection = client(Limits::default());
        connection.receive(&frame[..prefix], false).unwrap();
        assert_eq!(
            connection.transport_eof().unwrap_err(),
            Error::UnexpectedEof
        );
        assert_eq!(connection.state(), State::Failed);
    }
    let mut connection = client(Limits::default());
    connection.receive(b"\x01\x00", false).unwrap();
    assert_eq!(
        connection.transport_eof().unwrap_err(),
        Error::UnexpectedEof
    );
    let mut connection = client(Limits::default());
    connection.queue_close(None, "", MASK, false).unwrap();
    drain(&mut connection);
    assert_eq!(
        connection.transport_eof().unwrap_err(),
        Error::UnexpectedEof
    );
}

#[test]
fn cancellation_at_every_boundary_aborts_and_discards_pending_work() {
    let mut connecting = Client::new(ClientHandshake::new(NONCE, &[]).unwrap(), Limits::default());
    assert_eq!(
        connecting.accept_upgrade(101, &fields(), true).unwrap_err(),
        Error::Cancelled
    );
    assert_eq!(connecting.state(), State::Aborted);
    for boundary in 0..5 {
        let mut connection = client(Limits::default());
        connection.receive(b"\x01\x03abc", false).unwrap();
        connection.receive(b"\x80\x05xy", false).unwrap();
        match boundary {
            0 => assert_eq!(
                connection.receive(b"zzz", true).unwrap_err(),
                Error::Cancelled
            ),
            1 => assert_eq!(
                connection
                    .queue_frame(Opcode::Text, true, b"x", MASK, true)
                    .unwrap_err(),
                Error::Cancelled
            ),
            2 => {
                connection
                    .queue_frame(Opcode::Text, true, b"x", MASK, false)
                    .unwrap();
                connection.advance_written(1, false).unwrap();
                assert_eq!(
                    connection.advance_written(1, true).unwrap_err(),
                    Error::Cancelled
                );
            }
            3 => {
                assert_eq!(
                    connection.queue_pending_control(MASK, true).unwrap_err(),
                    Error::Cancelled
                );
            }
            _ => assert_eq!(
                connection
                    .queue_close(Some(1000), "", MASK, true)
                    .unwrap_err(),
                Error::Cancelled
            ),
        }
        assert_eq!(connection.state(), State::Aborted);
        assert!(connection.outgoing().is_empty());
        assert_eq!(connection.pending_control(), None);
        assert_eq!(
            connection.receive(b"", false).unwrap_err(),
            Error::InvalidState
        );
    }
    let mut connection = client(Limits::default());
    connection.receive(b"\x89\x01P", false).unwrap();
    connection.abort();
    assert_eq!(connection.pending_control(), None);
    connection.abort();
    assert_eq!(connection.state(), State::Aborted);
}

#[test]
fn invalid_write_acknowledgement_fails_and_handshake_state_is_enforced() {
    let mut connection = Client::new(ClientHandshake::new(NONCE, &[]).unwrap(), Limits::default());
    assert_eq!(
        connection.receive(b"", false).unwrap_err(),
        Error::InvalidState
    );
    assert_eq!(
        connection
            .queue_frame(Opcode::Binary, true, b"", MASK, false)
            .unwrap_err(),
        Error::InvalidState
    );
    connection.accept_upgrade(101, &fields(), false).unwrap();
    assert_eq!(
        connection
            .accept_upgrade(101, &fields(), false)
            .unwrap_err(),
        Error::InvalidState
    );
    connection
        .queue_frame(Opcode::Binary, true, b"", MASK, false)
        .unwrap();
    connection.advance_written(0, false).unwrap();
    assert_eq!(connection.outgoing().len(), 6);
    assert_eq!(
        connection.advance_written(7, false).unwrap_err(),
        Error::InvalidWriteCount
    );
    assert_eq!(connection.state(), State::Failed);
    assert!(connection.outgoing().is_empty());
    let mut connection = Client::new(ClientHandshake::new(NONCE, &[]).unwrap(), Limits::default());
    assert_eq!(
        connection
            .accept_upgrade(200, &fields(), false)
            .unwrap_err(),
        Error::UnexpectedStatus
    );
    assert_eq!(connection.state(), State::Failed);
}

#[test]
fn synthetic_nostr_messages_preserve_exact_text_and_fragmentation() {
    for text in [
        r#"["REQ","synthetic-subscription",{"kinds":[1],"authors":["synthetic-author"],"limit":1}]"#,
        r#"["EOSE","synthetic-subscription"]"#,
        r#"["NOTICE","synthetic only"]"#,
        r#"["CLOSE","synthetic-subscription"]"#,
        "[\"NOTICE\",\"€ 😀\"]",
    ] {
        let mut connection = client(Limits::default());
        assert_eq!(
            receive_bytes(&mut connection, &peer_frame(1, true, text.as_bytes())),
            Some(Event::Text(text.into()))
        );
        for chunk in text.as_bytes().chunks(3).enumerate() {
            let (index, bytes) = chunk;
            let fin = (index + 1) * 3 >= text.len();
            connection
                .queue_frame(
                    if index == 0 {
                        Opcode::Text
                    } else {
                        Opcode::Continuation
                    },
                    fin,
                    bytes,
                    [(index % 256) as u8, 2, 3, 4],
                    false,
                )
                .unwrap();
            let sent = drain(&mut connection);
            assert_eq!(payload(&sent, Direction::ClientToServer), bytes);
        }
    }
}

#[test]
fn debug_and_errors_redact_payloads_and_protocols() {
    let secret = "synthetic-private-do-not-log";
    let handshake = ClientHandshake::new(NONCE, &[secret]).unwrap();
    for debug in [
        format!("{handshake:?}"),
        format!(
            "{:?}",
            Header {
                name: secret,
                value: secret
            }
        ),
        format!("{:?}", Event::Text(secret.into())),
        format!(
            "{:?}",
            Event::Close(CloseData {
                code: Some(1000),
                reason: secret.into()
            })
        ),
        format!(
            "{:?}",
            Negotiated {
                subprotocol: Some(secret.into())
            }
        ),
    ] {
        assert!(!debug.contains(secret));
    }
    let encoded = peer_frame(1, true, secret.as_bytes());
    let frame = parse_frame(&encoded, Direction::ServerToClient, 125)
        .unwrap()
        .unwrap();
    assert!(!format!("{frame:?}").contains(secret));
    let mut connection = client(Limits::default());
    connection
        .queue_frame(Opcode::Text, true, secret.as_bytes(), MASK, false)
        .unwrap();
    assert!(!format!("{connection:?}").contains(secret));
    assert!(!Error::InvalidUtf8.to_string().contains(secret));
}

#[test]
fn bounded_adversarial_stream_corpus_never_panics_or_overconsumes() {
    let limits = Limits::new(256, 512, 8).unwrap();
    for first in 0_u8..=255 {
        for second in 0_u8..=255 {
            let bytes = [first, second, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4];
            for direction in [Direction::ServerToClient, Direction::ClientToServer] {
                if let Ok(Some(parsed)) = parse_frame(&bytes, direction, 256) {
                    assert!(parsed.consumed <= bytes.len());
                    assert_eq!(
                        parsed.frame.payload().unwrap().len(),
                        parsed.frame.header.payload_len
                    );
                }
            }
            let mut connection = client(limits);
            if let Ok(received) = connection.receive(&bytes, false) {
                assert!(received.consumed <= bytes.len());
            }
        }
    }
}
