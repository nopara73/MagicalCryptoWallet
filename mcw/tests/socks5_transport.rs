//! Local fake proxies only: no Tor daemon, external network, wallet or keys.
//! The fake server parses and asserts literal frames independently of `wire`.
#[allow(dead_code)]
#[path = "../src/socks5.rs"]
mod socks5;

use socks5::transport::*;
use socks5::wire::{
    Address, Authentication, Credentials, Destination, DomainName, ProtocolError, ReplyCode,
};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SUCCESS: [u8; 10] = [5, 0, 0, 1, 0, 0, 0, 0, 0, 0];
const DOMAIN_REQUEST: &[u8] = b"\x05\x01\x00\x03\x0fexample.invalid\x01\xbb";

fn options() -> ConnectOptions {
    ConnectOptions {
        proxy_connect_timeout: Duration::from_secs(1),
        total_timeout: Duration::from_secs(3),
        poll_interval: Duration::from_millis(10),
    }
}

fn control() -> IoControl {
    IoControl::new(Duration::from_secs(3), &Cancellation::new()).unwrap()
}

fn destination() -> Destination {
    Destination::new(
        Address::Domain(DomainName::new(b"example.invalid").unwrap()),
        443,
    )
    .unwrap()
}

fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(cause)
                if cause.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(2))
            }
            Err(cause) => panic!("synthetic proxy accept: {:?}", cause.kind()),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream.set_nodelay(true).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

fn server<T: Send + 'static>(
    body: impl FnOnce(TcpStream) -> T + Send + 'static,
) -> (SocketAddr, JoinHandle<T>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    let task = thread::spawn(move || body(accept(&listener)));
    (endpoint, task)
}

fn bytes(stream: &mut TcpStream, count: usize) -> Vec<u8> {
    let mut result = vec![0; count];
    stream.read_exact(&mut result).unwrap();
    result
}

fn assert_closed_without_application_data(stream: &mut TcpStream) {
    match stream.read(&mut [0; 1]) {
        Ok(0) => (),
        Err(cause)
            if matches!(
                cause.kind(),
                io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::NotConnected
            ) =>
        {
            ()
        }
        Ok(_) => panic!("application data was sent after a failed SOCKS handshake"),
        Err(cause) => panic!("socket did not close: {:?}", cause.kind()),
    }
}

fn request(stream: &mut TcpStream) -> Vec<u8> {
    let mut result = bytes(stream, 4);
    assert_eq!(result[0], 5);
    assert_eq!(result[2], 0);
    let count = match result[3] {
        1 => 4,
        4 => 16,
        3 => {
            let length = bytes(stream, 1)[0];
            assert_ne!(length, 0);
            result.push(length);
            usize::from(length)
        }
        _ => panic!("unexpected synthetic request address type"),
    };
    result.extend_from_slice(&bytes(stream, count + 2));
    result
}

fn no_auth(stream: &mut TcpStream) {
    assert_eq!(bytes(stream, 3), [5, 1, 0]);
    stream.write_all(&[5, 0]).unwrap();
}

fn auth(stream: &mut TcpStream, username: &[u8], password: &[u8]) {
    assert_eq!(bytes(stream, 3), [5, 1, 2]);
    stream.write_all(&[5, 2]).unwrap();
    let header = bytes(stream, 2);
    assert_eq!(header, [1, username.len() as u8]);
    assert_eq!(bytes(stream, usize::from(header[1])), username);
    assert_eq!(bytes(stream, 1), [password.len() as u8]);
    assert_eq!(bytes(stream, password.len()), password);
    stream.write_all(&[1, 0]).unwrap();
}

fn connected<T: Send + 'static>(
    body: impl FnOnce(TcpStream) -> T + Send + 'static,
) -> (SocksConnection, JoinHandle<T>) {
    let (proxy, task) = server(move |mut stream| {
        no_auth(&mut stream);
        assert_eq!(request(&mut stream), DOMAIN_REQUEST);
        stream.write_all(&SUCCESS).unwrap();
        body(stream)
    });
    let connection = SocksConnection::connect(
        proxy,
        &destination(),
        Authentication::None,
        options(),
        &Cancellation::new(),
    )
    .unwrap();
    (connection, task)
}

#[test]
fn exact_domain_forwarding_authentication_and_application_payload() {
    let (proxy, task) = server(|mut stream| {
        auth(&mut stream, b"synthetic-user", b"synthetic-password");
        assert_eq!(
            request(&mut stream),
            b"\x05\x01\x00\x03\x16MiXeD.Example.invalid.\x01\xbb"
        );
        let mut combined = SUCCESS.to_vec();
        combined.extend_from_slice(b"banner");
        stream.write_all(&combined).unwrap();
        assert_eq!(bytes(&mut stream, 4), b"ping");
        stream.write_all(b"pong").unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let credentials = Credentials::new(b"synthetic-user", b"synthetic-password").unwrap();
    let target = Destination::new(
        Address::Domain(DomainName::new(b"MiXeD.Example.invalid.").unwrap()),
        443,
    )
    .unwrap();
    let mut connection = SocksConnection::connect(
        proxy,
        &target,
        Authentication::UsernamePassword(&credentials),
        options(),
        &Cancellation::new(),
    )
    .unwrap();
    let mut banner = [0; 6];
    connection.read_exact(&mut banner, &control()).unwrap();
    assert_eq!(&banner, b"banner");
    connection.write_all(b"ping", &control()).unwrap();
    let mut response = [0; 4];
    connection.read_exact(&mut response, &control()).unwrap();
    assert_eq!(&response, b"pong");
    drop(connection);
    task.join().unwrap();
}

#[test]
fn exact_ipv4_ipv6_and_onion_destinations_go_only_to_proxy() {
    let cases = [
        (
            Destination::new(Address::Ipv4([192, 0, 2, 7]), 8333).unwrap(),
            vec![5, 1, 0, 1, 192, 0, 2, 7, 0x20, 0x8d],
        ),
        (
            Destination::new(
                Address::Ipv6([0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]),
                8333,
            )
            .unwrap(),
            vec![
                5, 1, 0, 4, 0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0x20, 0x8d,
            ],
        ),
        (
            Destination::new(
                Address::Domain(DomainName::new(b"synthetic.onion").unwrap()),
                80,
            )
            .unwrap(),
            b"\x05\x01\x00\x03\x0fsynthetic.onion\x00\x50".to_vec(),
        ),
    ];
    for (target, expected) in cases {
        let (proxy, task) = server(move |mut stream| {
            no_auth(&mut stream);
            assert_eq!(request(&mut stream), expected);
            stream.write_all(&SUCCESS).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        drop(
            SocksConnection::connect(
                proxy,
                &target,
                Authentication::None,
                options(),
                &Cancellation::new(),
            )
            .unwrap(),
        );
        task.join().unwrap();
    }
}

#[test]
fn fragmented_method_auth_and_reply_with_maximum_lengths() {
    let (proxy, task) = server(|mut stream| {
        assert_eq!(bytes(&mut stream, 3), [5, 1, 2]);
        for byte in [5, 2] {
            stream.write_all(&[byte]).unwrap();
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(bytes(&mut stream, 2), [1, 255]);
        assert_eq!(bytes(&mut stream, 255), [b'u'; 255]);
        assert_eq!(bytes(&mut stream, 1), [255]);
        assert_eq!(bytes(&mut stream, 255), [b'p'; 255]);
        for byte in [1, 0] {
            stream.write_all(&[byte]).unwrap();
            thread::sleep(Duration::from_millis(2));
        }
        let target = request(&mut stream);
        assert_eq!(target.len(), 262);
        assert_eq!(&target[..5], &[5, 1, 0, 3, 255]);
        assert_eq!(&target[5..260], &[b'd'; 255]);
        assert_eq!(&target[260..], &[255, 255]);
        let mut reply = vec![5, 0, 0, 3, 255];
        reply.extend_from_slice(&[b'r'; 255]);
        reply.extend_from_slice(&[255, 255]);
        for byte in reply {
            stream.write_all(&[byte]).unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        stream.write_all(b"after").unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let credentials = Credentials::new(&[b'u'; 255], &[b'p'; 255]).unwrap();
    let target = Destination::new(
        Address::Domain(DomainName::new(&[b'd'; 255]).unwrap()),
        65535,
    )
    .unwrap();
    let mut connection = SocksConnection::connect(
        proxy,
        &target,
        Authentication::UsernamePassword(&credentials),
        options(),
        &Cancellation::new(),
    )
    .unwrap();
    assert_eq!(
        connection.bound_endpoint().address,
        Address::Domain(DomainName::new(&[b'r'; 255]).unwrap())
    );
    assert_eq!(connection.bound_endpoint().port, 65535);
    let mut after = [0; 5];
    connection.read_exact(&mut after, &control()).unwrap();
    assert_eq!(&after, b"after");
    drop(connection);
    task.join().unwrap();
}

#[test]
fn every_ipv6_reply_split_preserves_a_coalesced_banner() {
    let frame = [
        5, 0, 0, 4, 0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0xbb,
    ];
    for split in 0..=frame.len() {
        let (proxy, task) = server(move |mut stream| {
            no_auth(&mut stream);
            assert_eq!(request(&mut stream), DOMAIN_REQUEST);
            stream.write_all(&frame[..split]).unwrap();
            thread::sleep(Duration::from_millis(2));
            let mut tail = frame[split..].to_vec();
            tail.extend_from_slice(b"BANNER");
            stream.write_all(&tail).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        let mut connection = SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            options(),
            &Cancellation::new(),
        )
        .unwrap();
        assert_eq!(connection.bound_endpoint().port, 443);
        let mut banner = [0; 6];
        connection.read_exact(&mut banner, &control()).unwrap();
        assert_eq!(&banner, b"BANNER");
        drop(connection);
        task.join().unwrap();
    }
}

#[test]
fn downgrade_and_other_unoffered_methods_close_without_credentials_or_connect() {
    for selected in [0, 1, 3, 0x80, 0xfe, 0xff] {
        let (proxy, task) = server(move |mut stream| {
            assert_eq!(bytes(&mut stream, 3), [5, 1, 2]);
            stream.write_all(&[5, selected]).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        let credentials = Credentials::new(b"must-not-send", b"must-not-send").unwrap();
        let failure = SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::UsernamePassword(&credentials),
            options(),
            &Cancellation::new(),
        )
        .unwrap_err();
        assert_eq!(failure.stage, Stage::MethodSelection);
        assert_eq!(
            failure.kind,
            ErrorKind::Protocol(if selected == 255 {
                ProtocolError::NoAcceptableMethod
            } else {
                ProtocolError::UnofferedMethod(selected)
            })
        );
        task.join().unwrap();
    }
}

#[test]
fn no_auth_never_answers_a_request_for_credentials() {
    let (proxy, task) = server(|mut stream| {
        assert_eq!(bytes(&mut stream, 3), [5, 1, 0]);
        stream.write_all(&[5, 2]).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    assert_eq!(
        SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            options(),
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::Protocol(ProtocolError::UnofferedMethod(2))
    );
    task.join().unwrap();
}

#[test]
fn authentication_failure_never_sends_connect() {
    for response in [[1, 1], [1, 255], [5, 0]] {
        let (proxy, task) = server(move |mut stream| {
            assert_eq!(bytes(&mut stream, 3), [5, 1, 2]);
            stream.write_all(&[5, 2]).unwrap();
            assert_eq!(bytes(&mut stream, 5), [1, 1, b'u', 1, b'p']);
            stream.write_all(&response).unwrap();
            assert_closed_without_application_data(&mut stream);
        });
        let credentials = Credentials::new(b"u", b"p").unwrap();
        let failure = SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::UsernamePassword(&credentials),
            options(),
            &Cancellation::new(),
        )
        .unwrap_err();
        assert_eq!(failure.stage, Stage::Authentication);
        assert_eq!(
            failure.kind,
            ErrorKind::Protocol(if response[0] == 1 {
                ProtocolError::AuthenticationRejected(response[1])
            } else {
                ProtocolError::InvalidAuthVersion(response[0])
            })
        );
        task.join().unwrap();
    }
}

#[test]
fn all_255_failure_codes_close_and_are_reported_without_addresses() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = listener.local_addr().unwrap();
    let task = thread::spawn(move || {
        for code in 1..=255 {
            let mut stream = accept(&listener);
            no_auth(&mut stream);
            assert_eq!(request(&mut stream), DOMAIN_REQUEST);
            stream
                .write_all(&[5, code, 0, 1, 192, 0, 2, 99, 0x7a, 0x69])
                .unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        }
    });
    for code in 1..=255 {
        let failure = SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            options(),
            &Cancellation::new(),
        )
        .unwrap_err();
        assert_eq!(
            failure,
            Error {
                stage: Stage::ProxyReply,
                kind: ErrorKind::ProxyRejected(ReplyCode::from_byte(code))
            }
        );
        let text = format!("{failure:?} {failure}");
        assert!(!text.contains("example.invalid"));
        assert!(!text.contains("192.0.2.99"));
        assert!(std::error::Error::source(&failure).is_none());
    }
    task.join().unwrap();
}

#[test]
fn proxy_refusal_never_falls_back_to_the_listening_destination() {
    let direct = TcpListener::bind("127.0.0.1:0").unwrap();
    direct.set_nonblocking(true).unwrap();
    let target = Destination::new(
        Address::Ipv4([127, 0, 0, 1]),
        direct.local_addr().unwrap().port(),
    )
    .unwrap();
    let (proxy, task) = server(|mut stream| {
        no_auth(&mut stream);
        let frame = request(&mut stream);
        assert_eq!(&frame[..8], &[5, 1, 0, 1, 127, 0, 0, 1]);
        stream.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    assert_eq!(
        SocksConnection::connect(
            proxy,
            &target,
            Authentication::None,
            options(),
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::ProxyRejected(ReplyCode::ConnectionRefused)
    );
    assert_eq!(
        direct.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    task.join().unwrap();
    let missing_proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let unavailable = missing_proxy.local_addr().unwrap();
    drop(missing_proxy);
    assert_eq!(
        SocksConnection::connect(
            unavailable,
            &target,
            Authentication::None,
            options(),
            &Cancellation::new()
        )
        .unwrap_err()
        .stage,
        Stage::ProxyConnect
    );
    assert_eq!(
        direct.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn malformed_replies_fail_and_close_before_application_data() {
    let cases = [
        (vec![4, 0, 0, 1], ProtocolError::InvalidVersion(4)),
        (vec![5, 0, 1, 1], ProtocolError::NonzeroReserved(1)),
        (vec![5, 0, 0, 2], ProtocolError::UnknownAddressType(2)),
        (vec![5, 0, 0, 3, 0], ProtocolError::EmptyDomain),
        (
            vec![5, 0, 0, 3, 1, 0, 0, 0],
            ProtocolError::DomainContainsNul,
        ),
    ];
    for (frame, expected) in cases {
        let (proxy, task) = server(move |mut stream| {
            no_auth(&mut stream);
            assert_eq!(request(&mut stream), DOMAIN_REQUEST);
            stream.write_all(&frame).unwrap();
            assert_closed_without_application_data(&mut stream);
        });
        assert_eq!(
            SocksConnection::connect(
                proxy,
                &destination(),
                Authentication::None,
                options(),
                &Cancellation::new()
            )
            .unwrap_err(),
            Error {
                stage: Stage::ProxyReply,
                kind: ErrorKind::Protocol(expected)
            }
        );
        task.join().unwrap();
    }
}

#[test]
fn truncated_reply_at_every_offset_is_clean_unexpected_eof() {
    for length in 0..SUCCESS.len() {
        let (proxy, task) = server(move |mut stream| {
            no_auth(&mut stream);
            assert_eq!(request(&mut stream), DOMAIN_REQUEST);
            stream.write_all(&SUCCESS[..length]).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        assert_eq!(
            SocksConnection::connect(
                proxy,
                &destination(),
                Authentication::None,
                options(),
                &Cancellation::new()
            )
            .unwrap_err(),
            Error {
                stage: Stage::ProxyReply,
                kind: ErrorKind::UnexpectedEof
            }
        );
        task.join().unwrap();
    }
}

#[test]
fn truncated_method_and_authentication_are_eof_errors() {
    for length in 0..2 {
        let (proxy, task) = server(move |mut stream| {
            assert_eq!(bytes(&mut stream, 3), [5, 1, 0]);
            stream.write_all(&[5, 0][..length]).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        assert_eq!(
            SocksConnection::connect(
                proxy,
                &destination(),
                Authentication::None,
                options(),
                &Cancellation::new()
            )
            .unwrap_err(),
            Error {
                stage: Stage::MethodSelection,
                kind: ErrorKind::UnexpectedEof
            }
        );
        task.join().unwrap();
        let (proxy, task) = server(move |mut stream| {
            assert_eq!(bytes(&mut stream, 3), [5, 1, 2]);
            stream.write_all(&[5, 2]).unwrap();
            assert_eq!(bytes(&mut stream, 5), [1, 1, b'u', 1, b'p']);
            stream.write_all(&[1, 0][..length]).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        let credentials = Credentials::new(b"u", b"p").unwrap();
        assert_eq!(
            SocksConnection::connect(
                proxy,
                &destination(),
                Authentication::UsernamePassword(&credentials),
                options(),
                &Cancellation::new()
            )
            .unwrap_err(),
            Error {
                stage: Stage::Authentication,
                kind: ErrorKind::UnexpectedEof
            }
        );
        task.join().unwrap();
    }
}

#[test]
fn cancellation_before_connect_does_not_open_a_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert_eq!(
        SocksConnection::connect(
            listener.local_addr().unwrap(),
            &destination(),
            Authentication::None,
            options(),
            &cancellation
        )
        .unwrap_err(),
        Error {
            stage: Stage::ProxyConnect,
            kind: ErrorKind::Cancelled
        }
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn cancellation_during_stalled_method_read_closes_the_socket() {
    let (started_tx, started_rx) = mpsc::channel();
    let (proxy, task) = server(move |mut stream| {
        assert_eq!(bytes(&mut stream, 3), [5, 1, 0]);
        started_tx.send(()).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let cancellation = Cancellation::new();
    let worker_token = cancellation.clone();
    let worker = thread::spawn(move || {
        SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            options(),
            &worker_token,
        )
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let start = Instant::now();
    cancellation.cancel();
    assert_eq!(
        worker.join().unwrap().unwrap_err(),
        Error {
            stage: Stage::MethodSelection,
            kind: ErrorKind::Cancelled
        }
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    task.join().unwrap();
}

#[test]
fn total_handshake_deadline_does_not_reset_between_stages() {
    let (proxy, task) = server(|mut stream| {
        assert_eq!(bytes(&mut stream, 3), [5, 1, 2]);
        thread::sleep(Duration::from_millis(80));
        stream.write_all(&[5, 2]).unwrap();
        assert_eq!(bytes(&mut stream, 5), [1, 1, b'u', 1, b'p']);
        thread::sleep(Duration::from_millis(80));
        let _ = stream.write_all(&[1, 0]);
        assert_closed_without_application_data(&mut stream);
    });
    let credentials = Credentials::new(b"u", b"p").unwrap();
    let mut short = options();
    short.total_timeout = Duration::from_millis(130);
    let start = Instant::now();
    let failure = SocksConnection::connect(
        proxy,
        &destination(),
        Authentication::UsernamePassword(&credentials),
        short,
        &Cancellation::new(),
    )
    .unwrap_err();
    assert_eq!(failure.kind, ErrorKind::TimedOut);
    assert_eq!(failure.stage, Stage::Authentication);
    assert!(start.elapsed() < Duration::from_secs(1));
    task.join().unwrap();
}

#[test]
fn slow_fragment_trickle_cannot_extend_the_total_deadline() {
    let (proxy, task) = server(|mut stream| {
        no_auth(&mut stream);
        assert_eq!(request(&mut stream), DOMAIN_REQUEST);
        for byte in SUCCESS {
            if stream.write_all(&[byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(35));
        }
        assert_closed_without_application_data(&mut stream);
    });
    let mut short = options();
    short.total_timeout = Duration::from_millis(100);
    let failure = SocksConnection::connect(
        proxy,
        &destination(),
        Authentication::None,
        short,
        &Cancellation::new(),
    )
    .unwrap_err();
    assert_eq!(failure.kind, ErrorKind::TimedOut);
    task.join().unwrap();
}

#[test]
fn invalid_timeout_and_proxy_port_fail_without_network_activity() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = listener.local_addr().unwrap();
    let mut invalid = options();
    invalid.total_timeout = Duration::ZERO;
    assert_eq!(
        SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            invalid,
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::InvalidTimeout
    );
    invalid = options();
    invalid.proxy_connect_timeout = Duration::ZERO;
    assert_eq!(
        SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            invalid,
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::InvalidTimeout
    );
    invalid = options();
    invalid.poll_interval = Duration::ZERO;
    assert_eq!(
        SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            invalid,
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::InvalidTimeout
    );
    invalid.poll_interval = Duration::from_secs(2);
    assert_eq!(
        SocksConnection::connect(
            proxy,
            &destination(),
            Authentication::None,
            invalid,
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::InvalidTimeout
    );
    assert_eq!(
        IoControl::new(Duration::MAX, &Cancellation::new())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidTimeout
    );
    assert_eq!(
        SocksConnection::connect(
            "127.0.0.1:0".parse().unwrap(),
            &destination(),
            Authentication::None,
            options(),
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::InvalidProxyPort
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn clean_peer_eof_preserves_the_writable_half() {
    let (mut connection, task) = connected(|mut stream| {
        stream.shutdown(Shutdown::Write).unwrap();
        assert_eq!(bytes(&mut stream, 4), b"late");
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    assert_eq!(connection.read(&mut [0; 1], &control()).unwrap(), 0);
    connection.write_all(b"late", &control()).unwrap();
    connection.shutdown(Shutdown::Write).unwrap();
    connection.shutdown(Shutdown::Write).unwrap();
    drop(connection);
    task.join().unwrap();
}

#[test]
fn premature_application_eof_aborts_and_forbids_retry() {
    let (mut connection, task) = connected(|mut stream| {
        stream.write_all(b"x").unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    assert_eq!(
        connection.read_exact(&mut [0; 2], &control()).unwrap_err(),
        Error {
            stage: Stage::Read,
            kind: ErrorKind::UnexpectedEof
        }
    );
    assert_eq!(
        connection
            .write_all(b"cannot-retry", &control())
            .unwrap_err()
            .kind,
        ErrorKind::Closed
    );
    task.join().unwrap();
}

#[test]
fn data_read_timeout_aborts_the_tunnel() {
    let (mut connection, task) = connected(|mut stream| {
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let deadline = IoControl::with_poll_interval(
        Duration::from_millis(60),
        &Cancellation::new(),
        Duration::from_millis(10),
    )
    .unwrap();
    assert_eq!(
        connection.read(&mut [0; 1], &deadline).unwrap_err(),
        Error {
            stage: Stage::Read,
            kind: ErrorKind::TimedOut
        }
    );
    assert_eq!(
        connection.write_all(b"x", &control()).unwrap_err().kind,
        ErrorKind::Closed
    );
    task.join().unwrap();
}

#[test]
fn data_read_cancellation_aborts_the_tunnel() {
    let (mut connection, task) = connected(|mut stream| {
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let cancellation = Cancellation::new();
    let deadline = IoControl::new(Duration::from_secs(3), &cancellation).unwrap();
    let worker = thread::spawn(move || connection.read(&mut [0; 1], &deadline));
    thread::sleep(Duration::from_millis(20));
    cancellation.cancel();
    assert_eq!(
        worker.join().unwrap().unwrap_err(),
        Error {
            stage: Stage::Read,
            kind: ErrorKind::Cancelled
        }
    );
    task.join().unwrap();
}

#[test]
fn write_deadline_closes_even_when_kernel_accepts_large_payload() {
    let (done_tx, done_rx) = mpsc::channel();
    let (mut connection, task) = connected(move |mut stream| {
        done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut buffer = [0; 8192];
        let mut received = 0;
        loop {
            let count = match stream.read(&mut buffer) {
                Ok(count) => count,
                Err(cause)
                    if matches!(
                        cause.kind(),
                        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    break;
                }
                Err(cause) => panic!("unexpected synthetic drain failure: {:?}", cause.kind()),
            };
            if count == 0 {
                break;
            }
            received += count;
        }
        received
    });
    let payload = vec![0x5a; 32 * 1024 * 1024];
    let deadline = IoControl::with_poll_interval(
        Duration::from_millis(1),
        &Cancellation::new(),
        Duration::from_millis(10),
    )
    .unwrap();
    let result = connection.write_all(&payload, &deadline);
    done_tx.send(()).unwrap();
    assert_eq!(
        result.unwrap_err(),
        Error {
            stage: Stage::Write,
            kind: ErrorKind::TimedOut
        }
    );
    assert_eq!(
        connection
            .write_all(b"cannot-retry", &control())
            .unwrap_err()
            .kind,
        ErrorKind::Closed
    );
    let received = task.join().unwrap();
    // Timeout can occur after the OS accepted any number of bytes, including
    // the whole buffer. The safety contract is closure and no automatic replay.
    assert!(received <= payload.len());
}

#[test]
fn established_abort_handle_wakes_read_and_closes_both_directions() {
    let (mut connection, task) = connected(|mut stream| {
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let abort = connection.abort_handle();
    let deadline = IoControl::with_poll_interval(
        Duration::from_secs(3),
        &Cancellation::new(),
        Duration::from_secs(1),
    )
    .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        connection.read(&mut [0; 1], &deadline)
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    thread::sleep(Duration::from_millis(20));
    let start = Instant::now();
    abort.abort();
    assert_eq!(worker.join().unwrap().unwrap_err().kind, ErrorKind::Closed);
    assert!(start.elapsed() < Duration::from_millis(800));
    abort.abort();
    task.join().unwrap();
}

#[test]
fn split_reader_and_writer_run_concurrently_and_preserve_half_close() {
    let (connection, task) = connected(|mut stream| {
        assert_eq!(bytes(&mut stream, 4), b"ping");
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        stream.write_all(b"pong").unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
    });
    let (mut reader, mut writer) = connection.into_split().unwrap();
    let read_task = thread::spawn(move || {
        let mut reply = [0; 4];
        reader.read_exact(&mut reply, &control()).unwrap();
        assert_eq!(&reply, b"pong");
        assert_eq!(reader.read(&mut [0; 1], &control()).unwrap(), 0);
        reader.shutdown().unwrap();
        reader.shutdown().unwrap();
    });
    writer.write_all(b"ping", &control()).unwrap();
    writer.shutdown().unwrap();
    writer.shutdown().unwrap();
    read_task.join().unwrap();
    drop(writer);
    task.join().unwrap();
}

#[test]
fn dropping_writer_half_sends_eof_while_reader_remains_usable() {
    let (connection, task) = connected(|mut stream| {
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        stream.write_all(b"after-eof").unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
    });
    let (mut reader, writer) = connection.into_split().unwrap();
    drop(writer);
    let mut payload = [0; 9];
    reader.read_exact(&mut payload, &control()).unwrap();
    assert_eq!(&payload, b"after-eof");
    assert_eq!(reader.read(&mut [0; 1], &control()).unwrap(), 0);
    task.join().unwrap();
}

#[test]
fn probe_sends_no_connect_and_does_not_claim_bootstrap() {
    let (proxy, task) = server(|mut stream| {
        no_auth(&mut stream);
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    probe(proxy, Authentication::None, options(), &Cancellation::new()).unwrap();
    task.join().unwrap();
}

#[test]
fn tor_remote_resolve_ipv4_and_ipv6_preserves_credentials_and_domain() {
    let cases = [
        (
            vec![5, 0, 0, 1, 192, 0, 2, 9, 0, 0],
            Address::Ipv4([192, 0, 2, 9]),
        ),
        (
            vec![
                5, 0, 0, 4, 0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0,
            ],
            Address::Ipv6([0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9]),
        ),
    ];
    for (reply, expected) in cases {
        let (proxy, task) = server(move |mut stream| {
            auth(&mut stream, b"<torS0X>0", b"dns-isolation");
            assert_eq!(
                request(&mut stream),
                b"\x05\xf0\x00\x03\x0fexample.invalid\x00\x00"
            );
            stream.write_all(&reply).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        let credentials = Credentials::new(b"<torS0X>0", b"dns-isolation").unwrap();
        assert_eq!(
            resolve(
                proxy,
                &DomainName::new(b"example.invalid").unwrap(),
                Authentication::UsernamePassword(&credentials),
                options(),
                &Cancellation::new()
            )
            .unwrap(),
            expected
        );
        task.join().unwrap();
    }
}

#[test]
fn tor_remote_resolve_ptr_and_wrong_reply_address_shapes() {
    let (proxy, task) = server(|mut stream| {
        no_auth(&mut stream);
        assert_eq!(request(&mut stream), [5, 0xf1, 0, 1, 192, 0, 2, 9, 0, 0]);
        stream
            .write_all(b"\x05\x00\x00\x03\x0fexample.invalid\x00\x00")
            .unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    assert_eq!(
        resolve_ptr(
            proxy,
            [192, 0, 2, 9],
            Authentication::None,
            options(),
            &Cancellation::new()
        )
        .unwrap()
        .as_bytes(),
        b"example.invalid"
    );
    task.join().unwrap();
    let (proxy, task) = server(|mut stream| {
        no_auth(&mut stream);
        assert_eq!(request(&mut stream)[1], 0xf0);
        stream.write_all(b"\x05\x00\x00\x03\x01x\x00\x00").unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    assert_eq!(
        resolve(
            proxy,
            &DomainName::new(b"example.invalid").unwrap(),
            Authentication::None,
            options(),
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::UnexpectedResolveAddress
    );
    task.join().unwrap();
    let (proxy, task) = server(|mut stream| {
        no_auth(&mut stream);
        assert_eq!(request(&mut stream)[1], 0xf1);
        stream.write_all(&SUCCESS).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    assert_eq!(
        resolve_ptr(
            proxy,
            [192, 0, 2, 9],
            Authentication::None,
            options(),
            &Cancellation::new()
        )
        .unwrap_err()
        .kind,
        ErrorKind::UnexpectedResolveAddress
    );
    task.join().unwrap();
}

#[test]
fn ipv6_numeric_proxy_endpoint_uses_the_same_protocol() {
    let listener = TcpListener::bind("[::1]:0").unwrap();
    let proxy = listener.local_addr().unwrap();
    let task = thread::spawn(move || {
        let mut stream = accept(&listener);
        no_auth(&mut stream);
        assert_eq!(request(&mut stream), DOMAIN_REQUEST);
        stream.write_all(&SUCCESS).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let connection = SocksConnection::connect(
        proxy,
        &destination(),
        Authentication::None,
        options(),
        &Cancellation::new(),
    )
    .unwrap();
    let text = format!("{connection:?} {:?}", connection.abort_handle());
    assert!(!text.contains("example.invalid"));
    assert!(!text.contains("::1"));
    drop(connection);
    task.join().unwrap();
}
