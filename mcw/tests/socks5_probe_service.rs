#[allow(dead_code)]
#[path = "../src/socks5.rs"]
mod socks5;

use socks5::probe_service::{self as service, Error, Failure, PROBE};
use socks5::transport::Cancellation;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn exact_numeric_loopback_request_and_reject_every_invalid_boundary() {
    assert_eq!(
        service::decode_request(&[1, 1, 127, 0, 0, 1, 0x95, 6])
            .unwrap()
            .port(),
        38150
    );
    let mut ipv6 = vec![1, 4];
    ipv6.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    ipv6.extend_from_slice(&[0x95, 6]);
    assert_eq!(
        service::decode_request(&ipv6).unwrap().ip(),
        "::1".parse::<std::net::IpAddr>().unwrap()
    );
    for valid in [&[1, 1, 127, 0, 0, 1, 0x95, 6][..], &ipv6] {
        for length in 0..valid.len() {
            assert_eq!(
                service::decode_request(&valid[..length]),
                Err(Error::InvalidPayload)
            );
        }
        let mut trailing = valid.to_vec();
        trailing.push(0);
        assert_eq!(
            service::decode_request(&trailing),
            Err(Error::InvalidPayload)
        );
    }
    for invalid in [
        vec![0, 1, 127, 0, 0, 1, 0, 1],
        vec![1, 3, 127, 0, 0, 1, 0, 1],
        vec![1, 1, 192, 0, 2, 1, 0, 1],
        vec![1, 1, 127, 0, 0, 1, 0, 0],
    ] {
        assert_eq!(
            service::decode_request(&invalid),
            Err(Error::InvalidPayload)
        );
    }
    let mut mapped = vec![1, 4];
    mapped.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 127, 0, 0, 1]);
    mapped.extend_from_slice(&[0, 1]);
    assert_eq!(service::decode_request(&mapped), Err(Error::InvalidPayload));
    assert_eq!(
        service::execute(0xffff, &[], &Cancellation::new()),
        Err(Error::UnknownOperation)
    );
}

#[test]
fn probe_handler_uses_no_auth_and_sends_no_connect() {
    for (response, expected) in [
        (vec![5, 0], [1, 1, 0]),
        (vec![4], [1, 0, Failure::InvalidVersion as u8]),
        (vec![5, 2], [1, 0, Failure::MethodRejected as u8]),
        (vec![5, 255], [1, 0, Failure::MethodRejected as u8]),
        (vec![5], [1, 0, Failure::UnexpectedEof as u8]),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            stream.write_all(&response).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        let mut request = vec![1, 1, 127, 0, 0, 1];
        request.extend_from_slice(&port.to_be_bytes());
        assert_eq!(
            service::execute(PROBE, &request, &Cancellation::new()).unwrap(),
            expected
        );
        task.join().unwrap();
    }
}

#[test]
fn stalled_probe_has_one_bounded_deadline_and_closes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut greeting = [0; 3];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 1, 0]);
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
    });
    let mut request = vec![1, 1, 127, 0, 0, 1];
    request.extend_from_slice(&port.to_be_bytes());
    let start = Instant::now();
    assert_eq!(
        service::execute(PROBE, &request, &Cancellation::new()).unwrap(),
        [1, 0, Failure::TimedOut as u8]
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    task.join().unwrap();
}

#[test]
fn cancelled_probe_and_invalid_request_open_no_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut request = vec![1, 1, 127, 0, 0, 1];
    request.extend_from_slice(&listener.local_addr().unwrap().port().to_be_bytes());
    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert_eq!(
        service::execute(PROBE, &request, &cancellation).unwrap(),
        [1, 0, Failure::Cancelled as u8]
    );
    request.push(0);
    assert_eq!(
        service::execute(PROBE, &request, &Cancellation::new()),
        Err(Error::InvalidPayload)
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
