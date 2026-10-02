//! A test-only std TCP fixture serving independent literal HTTP responses.
//! Neither sockets nor this test executable are part of the HTTP domain engine.
#[allow(dead_code)]
#[path = "../src/http1.rs"]
mod http1;
use http1::*;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

fn header(name: &[u8], value: &[u8]) -> Header {
    Header {
        name: name.to_vec(),
        value: value.to_vec(),
    }
}

fn fixture(
    expected: &'static [u8],
    response: &'static [u8],
    fragment: usize,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(e) => panic!("synthetic fixture accept failed: {e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = vec![0; expected.len()];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(
            request, expected,
            "server verifies request independently of encoder"
        );
        for part in response.chunks(fragment) {
            stream.write_all(part).unwrap();
        }
        // Drop produces an orderly EOF; no live external endpoint is contacted.
    });
    (address, worker)
}

fn exchange(
    request: &Request<'_>,
    expected: &'static [u8],
    response: &'static [u8],
    fragment: usize,
) -> (Response, Vec<u8>) {
    let (address, worker) = fixture(expected, response, fragment);
    let encoded = serialize_request(request, Limits::default()).unwrap();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(&encoded.bytes).unwrap();
    let mut decoder = ResponseDecoder::new(encoded.context, Limits::default()).unwrap();
    let mut extra = Vec::new();
    let mut buffer = [0u8; 7];
    loop {
        let n = stream.read(&mut buffer).unwrap();
        if n == 0 {
            decoder.finish().unwrap();
            break;
        }
        if decoder.response().is_some() {
            extra.extend_from_slice(&buffer[..n]);
            continue;
        }
        let mut pos = 0;
        while pos < n {
            let progress = decoder.feed(&buffer[pos..n]).unwrap();
            pos += progress.consumed;
            if progress.status == DecodeStatus::Complete {
                extra.extend_from_slice(&buffer[pos..n]);
                break;
            }
            if progress.status == DecodeStatus::NeedMore {
                assert_eq!(pos, n);
                break;
            }
        }
    }
    worker.join().unwrap();
    (decoder.into_response().unwrap(), extra)
}

#[test]
fn reference_server_fixed_and_informational_chunked_wallet_json() {
    let headers = [
        header(b"Host", b" example.invalid"),
        header(b"Content-Type", b" application/json"),
    ];
    let request = Request {
        version: Version::Http11,
        method: b"POST",
        target: b"/wabisabi/status",
        headers: &headers,
        body: RequestBody::Bytes(b"{}"),
    };
    let expected = b"POST /wabisabi/status HTTP/1.1\r\nHost: example.invalid\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
    for fragment in [1, 3, 64] {
        let (response, extra) = exchange(&request, expected, b"HTTP/1.1 103 Hint\r\nX-Fixture: synthetic\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n1\r\n{\r\n1;fixture=yes\r\n}\r\n0\r\nX-Checksum: opaque\r\n\r\nNEXT", fragment);
        assert_eq!(response.informational.len(), 1);
        assert_eq!(response.body, b"{}");
        assert_eq!(response.trailers[0].trimmed_value(), b"opaque");
        assert_eq!(extra, b"NEXT");
        assert_eq!(
            response.connection,
            ConnectionUse::MustClose,
            "observed EOF closes reusable wire connection"
        );
        let (response, extra) = exchange(
            &request,
            expected,
            b"HTTP/1.1 200 OK\r\nContent-Length:2\r\n\r\n{}",
            fragment,
        );
        assert_eq!(response.body, b"{}");
        assert!(extra.is_empty());
    }
}

#[test]
fn reference_server_checks_literal_chunked_request_and_orderly_close_body() {
    let headers = [header(b"Host", b" example.invalid")];
    let trailers = [header(b"X-Checksum", b" synthetic")];
    let chunks = &[b"abc".as_slice(), b"\0\xff"];
    let request = Request {
        version: Version::Http11,
        method: b"POST",
        target: b"/fixture",
        headers: &headers,
        body: RequestBody::Chunked {
            chunks,
            trailers: &trailers,
        },
    };
    let (response, extra) = exchange(&request, b"POST /fixture HTTP/1.1\r\nHost: example.invalid\r\nTransfer-Encoding: chunked\r\nTrailer: X-Checksum\r\n\r\n3\r\nabc\r\n2\r\n\0\xff\r\n0\r\nX-Checksum: synthetic\r\n\r\n", b"HTTP/1.0 200 OK\r\nContent-Type: application/octet-stream\r\n\r\n\xff\0opaque", 1);
    assert_eq!(response.body, b"\xff\0opaque");
    assert_eq!(response.framing, Framing::UntilClose);
    assert!(extra.is_empty());
}

#[test]
fn reference_server_head_and_connect_preserve_following_bytes() {
    let headers = [header(b"Host", b" example.invalid")];
    let head = Request {
        version: Version::Http11,
        method: b"HEAD",
        target: b"/",
        headers: &headers,
        body: RequestBody::Empty,
    };
    let (response, extra) = exchange(
        &head,
        b"HEAD / HTTP/1.1\r\nHost: example.invalid\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length:9000\r\n\r\nFOLLOWING",
        2,
    );
    assert!(response.body.is_empty());
    assert_eq!(extra, b"FOLLOWING");
    let connect = Request {
        version: Version::Http11,
        method: b"CONNECT",
        target: b"example.invalid:443",
        headers: &headers,
        body: RequestBody::Empty,
    };
    let (response, extra) = exchange(&connect, b"CONNECT example.invalid:443 HTTP/1.1\r\nHost: example.invalid\r\n\r\n", b"HTTP/1.1 200 Tunnel\r\nContent-Length:garbage\r\nTransfer-Encoding:garbage\r\n\r\n\x16\x03\x01\0\xff", 1);
    assert_eq!(response.connection, ConnectionUse::Tunnel);
    assert_eq!(extra, b"\x16\x03\x01\0\xff");
}

#[test]
fn reference_server_negotiated_upgrade_detaches_at_the_exact_header_boundary() {
    let headers = [
        header(b"Host", b" example.invalid"),
        header(b"Connection", b" Upgrade"),
        header(b"Upgrade", b" synthetic/V1"),
    ];
    let request = Request {
        version: Version::Http11,
        method: b"GET",
        target: b"/",
        headers: &headers,
        body: RequestBody::Empty,
    };
    let (response, extra) = exchange(&request, b"GET / HTTP/1.1\r\nHost: example.invalid\r\nConnection: Upgrade\r\nUpgrade: synthetic/V1\r\n\r\n", b"HTTP/1.1 101 Switch\r\nConnection: Upgrade\r\nUpgrade: SYNTHETIC/V1\r\n\r\n\xff\0new-protocol", 3);
    assert_eq!(response.connection, ConnectionUse::Upgrade);
    assert_eq!(extra, b"\xff\0new-protocol");
}
