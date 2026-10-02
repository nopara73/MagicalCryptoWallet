#[path = "../src/bitcoin_encoding.rs"]
pub mod bitcoin_encoding;
#[path = "../src/privacy_service/mod.rs"]
pub mod privacy_service;
#[path = "../src/wallet_hashes.rs"]
pub mod wallet_hashes;

use privacy_service::{Error, cell::*, crypto::*, onion::OnionAddress, relay};

fn bytes(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn independent_hashlib_sha3_shake_and_streaming_vectors() {
    for line in include_str!("privacy_fixtures/hashes.tsv").lines() {
        let parts: Vec<_> = line.split('\t').collect();
        let message = bytes(parts[0]);
        assert_eq!(sha3_256(&message).as_slice(), bytes(parts[1]));
        let mut xof = [0; 300];
        shake256(&message, &mut xof).unwrap();
        assert_eq!(xof.as_slice(), bytes(parts[2]));
        for chunk_size in [1, 7, 8, 135, 136, 137] {
            let mut hash = Sha3_256::new();
            for chunk in message.chunks(chunk_size) {
                hash.update(chunk);
            }
            assert_eq!(hash.digest().as_slice(), bytes(parts[1]));
            assert_eq!(hash.digest().as_slice(), bytes(parts[1]));
        }
    }
    assert_eq!(
        sha3_256(b"abc").as_slice(),
        bytes("3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532")
    );
    let mut too_large = vec![0xa5; 65_537];
    assert_eq!(shake256(b"x", &mut too_large), Err(Error::LengthLimit));
    assert!(too_large.iter().all(|&b| b == 0xa5));
}

#[test]
fn published_zero_state_keccak_permutation() {
    // Keccak team's permutation example, little-endian lane values, independent
    // of SHA3 wrappers and of this implementation's round function.
    let expected = [
        0xf1258f7940e1dde7,
        0x84d5ccf933c0478a,
        0xd598261ea65aa9ee,
        0xbd1547306f80494d,
        0x8b284e056253d057,
        0xff97a42d7f8e6fd4,
        0x90fee5a0a44647c4,
        0x8c5bda0cd6192e76,
        0xad30a6f71b19059c,
        0x30935ab7d08ffc64,
        0xeb5aa93f2317d635,
        0xa9a6e6260d712103,
        0x81a57c16dbcf555f,
        0x43b831cd0347c826,
        0x01f22f1a11a5569f,
        0x05e5635a21d9ae61,
        0x64befef28cc970f2,
        0x613670957bc46611,
        0xb87c5a554fd00ecb,
        0x8c3ee88a1ccf32c8,
        0x940c7922ae3a2614,
        0x1841f924a2c509e4,
        0x16f53526e70465c2,
        0x75f644e97f30a13b,
        0xeaf1ff7b5ceca249,
    ];
    let mut state = [0; 25];
    crypto::hash::keccak_f1600(&mut state);
    assert_eq!(state, expected);
}

#[test]
fn published_onion_addresses_and_every_symbol_corruption() {
    let hosts = [
        "pg6mmjiyjmcrsslvykfwnntlaru7p5svn6y2ymmju6nubxndf4pscryd.onion",
        "sp3k262uwy4r2k3ycr5awluarykdpag6a7y33jxop4cs2lu5uz5sseqd.onion",
        "xa4r2iadxm55fbnqgwwi5mymqdcofiu3w6rpbtqn7b2dyn7mgwj64jyd.onion",
    ];
    for host in hosts {
        let address = OnionAddress::parse(host).unwrap();
        assert_eq!(address.host(), host);
        assert_eq!(OnionAddress::parse(&host.to_uppercase()).unwrap(), address);
        for i in 0..56 {
            let mut changed = host.as_bytes().to_vec();
            changed[i] = if changed[i] == b'a' { b'b' } else { b'a' };
            assert!(OnionAddress::parse(std::str::from_utf8(&changed).unwrap()).is_err());
        }
    }
    for invalid in [
        "",
        "abcdefghijklmnop.onion",
        "localhost",
        "[::1]",
        "example.onion.",
    ] {
        assert_eq!(
            OnionAddress::parse(invalid),
            Err(Error::InvalidOnionAddress)
        );
    }
}

#[test]
fn tor_wire_vectors_all_prefixes_and_coalesced_protocol_transition() {
    assert_eq!(versions(), [0, 0, 7, 0, 4, 0, 4, 0, 5]);
    let initial = [0, 0, 7, 0, 4, 0, 4, 0, 5];
    for n in 0..initial.len() {
        assert!(
            decode(&initial[..n], LinkProtocol::Negotiating)
                .unwrap()
                .is_none()
        );
    }
    let (cell, used) = decode(&initial, LinkProtocol::Negotiating)
        .unwrap()
        .unwrap();
    assert_eq!(used, 9);
    assert_eq!(negotiate(cell.body), Ok(LinkProtocol::V5));
    assert_eq!(negotiate(&[0, 5, 9]), Err(Error::InvalidVersion));
    assert_eq!(negotiate(&[0, 3]), Err(Error::InvalidVersion));
    assert_eq!(negotiate(&[0, 6, 0, 4]), Ok(LinkProtocol::V4));
    let mut fixed = vec![0x80, 0, 0, 1, 4, 7];
    fixed.resize(514, 0);
    fixed.extend_from_slice(b"NEXT");
    for n in 0..514 {
        assert!(decode(&fixed[..n], LinkProtocol::V4).unwrap().is_none());
    }
    let (cell, used) = decode(&fixed, LinkProtocol::V4).unwrap().unwrap();
    assert_eq!(cell.circuit_id, 0x80000001);
    assert_eq!(cell.body[0], 7);
    assert_eq!(&fixed[used..], b"NEXT");
    assert_eq!(encode(&cell, LinkProtocol::V4).unwrap(), fixed[..used]);
    let invalid = [0, 0, 0, 0, 3];
    assert_eq!(
        decode(&invalid, LinkProtocol::V4),
        Err(Error::InvalidCircuit)
    );
    for command in 0..=255 {
        if Command::from_byte(command).is_err() {
            assert_eq!(
                decode(&[0, 0, 0, 0, command], LinkProtocol::V5),
                Err(Error::InvalidCommand)
            );
        }
    }
}

#[test]
fn max_variable_cell_has_exact_consumption_and_all_commands() {
    let body = vec![0xaa; 65_535];
    let encoded = encode(
        &Cell {
            circuit_id: 0,
            command: Command::Vpadding,
            body: &body,
        },
        LinkProtocol::V5,
    )
    .unwrap();
    assert_eq!(encoded.len(), 65_542);
    assert_eq!(
        decode(&encoded, LinkProtocol::V5).unwrap().unwrap().0.body,
        body
    );
    let huge = vec![0; 65_536];
    assert_eq!(
        encode(
            &Cell {
                circuit_id: 0,
                command: Command::Certs,
                body: &huge
            },
            LinkProtocol::V5
        ),
        Err(Error::LengthLimit)
    );
    assert_eq!(
        encode(
            &Cell {
                circuit_id: 0,
                command: Command::PaddingNegotiate,
                body: &[]
            },
            LinkProtocol::V4
        ),
        Err(Error::InvalidVersion)
    );
}

#[test]
fn certs_lengths_duplicate_types_extensions_and_redaction() {
    let payload = [2, 4, 0, 1, 9, 5, 0, 2, 7, 8, 99];
    let certs = certificates(&payload).unwrap();
    assert_eq!(certs.len(), 2);
    assert_eq!(certs[1].encoded, [7, 8]);
    for n in 0..10 {
        assert!(certificates(&payload[..n]).is_err());
    }
    assert_eq!(
        certificates(&[2, 4, 0, 0, 4, 0, 0]).unwrap_err(),
        Error::InvalidCertificate
    );
    let mut cert = vec![0; 104];
    cert[0] = 1;
    cert[1] = 4;
    cert[6] = 1;
    cert[2..6].copy_from_slice(&0x10203u32.to_be_bytes());
    let parsed = ed25519_certificate(&cert).unwrap();
    assert_eq!(parsed.expires_at, 0x10203 * 3600);
    assert_eq!(parsed.signed_bytes.len(), 40);
    cert[39] = 1;
    cert.splice(40..40, [0, 32, 4, 1].into_iter().chain([9; 32]));
    assert_eq!(
        ed25519_certificate(&cert).unwrap().signing_key,
        Some([9; 32])
    );
    cert[42] = 99;
    assert_eq!(
        ed25519_certificate(&cert).unwrap_err(),
        Error::InvalidCertificate
    );
    cert[43] = 0;
    assert!(ed25519_certificate(&cert).is_ok());
    assert_eq!(format!("{:?}", certs[0]), "Certificate([redacted])");
}

#[test]
fn netinfo_client_never_discloses_local_clock_or_addresses() {
    for ip in ["127.0.0.1", "2001:db8::7"] {
        let body = client_netinfo(ip.parse().unwrap());
        assert_eq!(&body[..4], [0; 4]);
        let end = 6 + body[5] as usize;
        assert_eq!(body[end], 0);
        assert!(body[end..].iter().all(|&b| b == 0));
    }
}

#[test]
fn relay_padding_header_and_remote_begin_are_literal() {
    let data = relay::begin(b"MiXeD.example.", 443, 0).unwrap();
    assert_eq!(data, b"MiXeD.example.:443\0");
    let message = relay::Message {
        command: 1,
        stream_id: 9,
        data: &data,
    };
    let padding = vec![0xa5; 498 - data.len() - 4];
    let body = relay::encode(&message, &padding).unwrap();
    assert_eq!(
        &body[..11],
        [1, 0, 0, 0, 9, 0, 0, 0, 0, 0, data.len() as u8]
    );
    assert_eq!(&body[11 + data.len()..15 + data.len()], [0; 4]);
    assert_eq!(relay::decode(&body).unwrap(), message);
    assert_eq!(
        relay::begin(b"bad\0.example", 443, 0),
        Err(Error::InvalidRelay)
    );
    assert_eq!(
        relay::encode(
            &relay::Message {
                command: 2,
                stream_id: 0,
                data: &[]
            },
            &[0; 494]
        ),
        Err(Error::InvalidStream)
    );
}

#[test]
fn authenticated_sendme_cannot_inflate_or_replay_window() {
    let mut window = relay::CircuitWindow::new(1000, 1, 1).unwrap();
    for i in 0..100 {
        window.on_sent_data([i; 20]).unwrap();
    }
    assert_eq!(window.package_available(), 900);
    let mut ack = [0; 23];
    ack[..3].copy_from_slice(&[1, 0, 20]);
    ack[3..].fill(99);
    window.on_sendme(&ack).unwrap();
    assert_eq!(window.package_available(), 1000);
    assert_eq!(window.on_sendme(&ack), Err(Error::AuthenticationFailed));
    assert_eq!(window.package_available(), 0);
    assert_eq!(window.on_sent_data([0; 20]), Err(Error::Closed));
    let mut forged = relay::CircuitWindow::new(100, 1, 1).unwrap();
    for _ in 0..100 {
        forged.on_sent_data([0x71; 20]).unwrap();
    }
    assert_eq!(forged.on_sent_data([0; 20]), Err(Error::FlowControl));
    ack[3..].fill(0);
    assert_eq!(forged.on_sendme(&ack), Err(Error::AuthenticationFailed));
    let mut stream = relay::StreamWindow::new();
    assert_eq!(stream.on_sendme(), Err(Error::FlowControl));
    for _ in 0..50 {
        stream.on_sent_data().unwrap();
        stream.on_received_data().unwrap();
    }
    stream.on_sendme().unwrap();
    assert!(!stream.on_flushed(10));
    assert!(stream.on_flushed(9));
    assert!(!stream.on_flushed(0));
}

use privacy_service::crypto;
