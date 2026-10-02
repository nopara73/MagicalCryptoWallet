//! Independently written literal vectors from RFC 9110/9112 semantics.
//! No encoder-generated response is used as a decoder oracle. Synthetic data.
#[allow(dead_code)]
#[path = "../src/http1.rs"]
mod http1;
use http1::*;

fn h(name: &[u8], value: &[u8]) -> Header {
    Header {
        name: name.to_vec(),
        value: value.to_vec(),
    }
}

fn context(method: &[u8], version: Version, headers: &[Header]) -> RequestContext {
    RequestContext::new(method, version, headers, Limits::default()).unwrap()
}

fn decoder(method: &[u8]) -> ResponseDecoder {
    ResponseDecoder::new(context(method, Version::Http11, &[]), Limits::default()).unwrap()
}

fn decode(method: &[u8], wire: &[u8], eof: bool) -> (Response, usize) {
    let mut d = decoder(method);
    let mut pos = 0;
    while pos < wire.len() {
        let r = d.feed(&wire[pos..]).unwrap();
        pos += r.consumed;
        if r.status != DecodeStatus::Informational {
            break;
        }
    }
    if eof {
        d.finish().unwrap();
    }
    (d.into_response().unwrap(), pos)
}

fn rejects(wire: &[u8], expected: Error) {
    let mut d = decoder(b"GET");
    let mut pos = 0;
    loop {
        match d.feed(&wire[pos..]) {
            Err(error) => {
                assert_eq!(error, expected, "wire={wire:?}");
                break;
            }
            Ok(result) => {
                pos += result.consumed;
                if result.status == DecodeStatus::Informational {
                    continue;
                }
                panic!("expected {expected:?}, got {result:?} at {pos}");
            }
        }
    }
    assert_eq!(d.feed(b"HTTP/1.1 200 OK\r\n\r\n"), Err(Error::Poisoned));
    assert_eq!(d.finish(), Err(Error::Poisoned));
    assert!(d.response().is_none());
}

fn encode(
    method: &[u8],
    target: &[u8],
    headers: &[Header],
    body: RequestBody<'_>,
) -> Result<EncodedRequest, Error> {
    serialize_request(
        &Request {
            version: Version::Http11,
            method,
            target,
            headers,
            body,
        },
        Limits::default(),
    )
}

#[test]
fn literal_get_and_post_request_wire_and_continue_split() {
    let headers = [
        h(b"Host", b" www.example.org"),
        h(b"User-Agent", b" synthetic"),
        h(b"X-Opaque", b"\t\xff \t"),
    ];
    let get = encode(
        b"GET",
        b"/pub/WWW/TheProject.html?q=%2F",
        &headers,
        RequestBody::Empty,
    )
    .unwrap();
    assert_eq!(get.bytes, b"GET /pub/WWW/TheProject.html?q=%2F HTTP/1.1\r\nHost: www.example.org\r\nUser-Agent: synthetic\r\nX-Opaque:\t\xff \t\r\n\r\n");
    assert_eq!(get.head_bytes, get.bytes.len());
    let post = encode(
        b"POST",
        b"/wabisabi/status",
        &[
            h(b"Host", b" example.invalid"),
            h(b"Expect", b" 100-continue"),
        ],
        RequestBody::Bytes(b"{\"x\":1}"),
    )
    .unwrap();
    assert_eq!(post.bytes, b"POST /wabisabi/status HTTP/1.1\r\nHost: example.invalid\r\nExpect: 100-continue\r\nContent-Length: 7\r\n\r\n{\"x\":1}");
    assert_eq!(&post.bytes[post.head_bytes..], b"{\"x\":1}");
    let empty = encode(b"POST", b"/", &[h(b"Host", b"x")], RequestBody::Bytes(b"")).unwrap();
    assert!(empty.bytes.ends_with(b"Content-Length: 0\r\n\r\n"));
}

#[test]
fn literal_chunked_request_trailers_and_empty_slice_handling() {
    let chunks: &[&[u8]] = &[b"Wiki", b"", b"pedia", b"\0\xff"];
    let trailers = [h(b"X-Checksum", b" opaque")];
    let encoded = encode(
        b"POST",
        b"/",
        &[h(b"Host", b" example.invalid")],
        RequestBody::Chunked {
            chunks,
            trailers: &trailers,
        },
    )
    .unwrap();
    assert_eq!(encoded.bytes, b"POST / HTTP/1.1\r\nHost: example.invalid\r\nTransfer-Encoding: chunked\r\nTrailer: X-Checksum\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n2\r\n\0\xff\r\n0\r\nX-Checksum: opaque\r\n\r\n");
    let encoded = encode(
        b"POST",
        b"/",
        &[h(b"Host", b"x")],
        RequestBody::Chunked {
            chunks: &[],
            trailers: &[],
        },
    )
    .unwrap();
    assert!(encoded.bytes.ends_with(b"\r\n\r\n0\r\n\r\n"));
}

#[test]
fn request_all_four_target_forms_ipv6_and_default_ports() {
    for (method, target, host) in [
        (
            &b"GET"[..],
            &b"/a:b@c%2f?q=one/two?three"[..],
            &b"example.invalid"[..],
        ),
        (
            b"GET",
            b"https://example.invalid:443/path",
            b"EXAMPLE.invalid",
        ),
        (b"OPTIONS", b"http://example.invalid", b"example.invalid:80"),
        (b"CONNECT", b"example.invalid:443", b"example.invalid"),
        (b"CONNECT", b"[2001:db8::1]:443", b"[2001:db8::1]:443"),
        (
            b"GET",
            b"http://[v1.foo:bar]:8080/?x=%ff",
            b"[v1.foo:bar]:8080",
        ),
        (b"OPTIONS", b"*", b"example.invalid"),
    ] {
        encode(method, target, &[h(b"Host", host)], RequestBody::Empty).unwrap();
    }
    let no_host_10 = serialize_request(
        &Request {
            version: Version::Http10,
            method: b"GET",
            target: b"/",
            headers: &[],
            body: RequestBody::Empty,
        },
        Limits::default(),
    )
    .unwrap();
    assert_eq!(no_host_10.bytes, b"GET / HTTP/1.0\r\n\r\n");
}

#[test]
fn request_rejects_injection_invalid_uri_and_authority_mismatch() {
    for method in [&b""[..], b"GE T", b"GET\r\nX: y", b"G\0ET", b"G\xffET"] {
        assert!(matches!(
            encode(method, b"/", &[h(b"Host", b"x")], RequestBody::Empty),
            Err(Error::InvalidMethod)
        ));
    }
    for target in [
        &b""[..],
        b"/x\r\nInjected: 1",
        b"/x#fragment",
        b"/x y",
        b"/x\\y",
        b"/x\xff",
        b"/%",
        b"/%0z",
        b"*",
        b"host:80",
        b"ftp://x/",
        b"http://user@x/",
        b"http://x:0/",
        b"http://x:65536/",
        b"http://[:::]/",
        b"http://x:/",
        b"http://x\t/",
        b"http://x#f",
        b"/x[0]",
    ] {
        assert!(
            encode(b"GET", target, &[h(b"Host", b"x")], RequestBody::Empty).is_err(),
            "{target:?}"
        );
    }
    for target in [&b"x"[..], b"http://x:443", b"x:0", b"[::1]", b"x:443/path"] {
        assert!(encode(b"CONNECT", target, &[h(b"Host", b"x")], RequestBody::Empty).is_err());
    }
    for (target, host) in [
        (&b"https://other/"[..], &b"x"[..]),
        (b"http://x:81/", b"x"),
        (b"x:443", b"other:443"),
        (b"x:443", b"x:80"),
    ] {
        let method = if target.contains(&b'/') {
            b"GET".as_slice()
        } else {
            b"CONNECT"
        };
        assert!(matches!(
            encode(method, target, &[h(b"Host", host)], RequestBody::Empty),
            Err(Error::HostMismatch)
        ));
    }
}

#[test]
fn request_host_count_header_syntax_and_opaque_value_rules() {
    for headers in [
        vec![],
        vec![h(b"Host", b"")],
        vec![h(b"Host", b"x"), h(b"HOST", b"x")],
        vec![h(b"Host", b"x y")],
        vec![h(b"Host", b"user@x")],
    ] {
        assert!(matches!(
            encode(b"GET", b"/", &headers, RequestBody::Empty),
            Err(Error::InvalidHost)
        ));
    }
    for header in [
        h(b"Bad Name", b"x"),
        h(b"X:", b"x"),
        h(b"", b"x"),
        h(b"X", b"\r\nHost: evil"),
        h(b"X", b"\0"),
        h(b"X", b"\x7f"),
        h(b"X", b"\x0b"),
    ] {
        assert!(matches!(
            encode(
                b"GET",
                b"/",
                &[h(b"Host", b"x"), header],
                RequestBody::Empty
            ),
            Err(Error::InvalidHeader)
        ));
    }
}

#[test]
fn request_framing_is_canonical_and_conflicts_never_serialize() {
    let good = encode(
        b"POST",
        b"/",
        &[h(b"Host", b"x"), h(b"content-length", b" 003, 3")],
        RequestBody::Bytes(b"abc"),
    )
    .unwrap();
    assert_eq!(
        good.bytes,
        b"POST / HTTP/1.1\r\nHost:x\r\nContent-Length: 3\r\n\r\nabc"
    );
    for (extra, body) in [
        (vec![h(b"Content-Length", b"2")], RequestBody::Bytes(b"abc")),
        (vec![h(b"Content-Length", b"1")], RequestBody::Empty),
        (
            vec![h(b"Content-Length", b"3"), h(b"Content-Length", b"3")],
            RequestBody::Bytes(b"abc"),
        ),
        (
            vec![h(b"Content-Length", b"3,4")],
            RequestBody::Bytes(b"abc"),
        ),
        (
            vec![h(b"Content-Length", b"+3")],
            RequestBody::Bytes(b"abc"),
        ),
        (
            vec![h(b"Transfer-Encoding", b"chunked")],
            RequestBody::Bytes(b"abc"),
        ),
        (
            vec![h(b"Transfer-Encoding", b"gzip, chunked")],
            RequestBody::Empty,
        ),
        (
            vec![
                h(b"Transfer-Encoding", b"chunked"),
                h(b"Content-Length", b"0"),
            ],
            RequestBody::Empty,
        ),
        (vec![h(b"Trailer", b"X-Checksum")], RequestBody::Empty),
    ] {
        let mut headers = vec![h(b"Host", b"x")];
        headers.extend(extra);
        assert!(encode(b"POST", b"/", &headers, body).is_err());
    }
    let chunks = &[b"abc".as_slice()];
    assert!(
        encode(
            b"POST",
            b"/",
            &[h(b"Host", b"x"), h(b"Content-Length", b"3")],
            RequestBody::Chunked {
                chunks,
                trailers: &[]
            }
        )
        .is_err()
    );
    assert!(
        serialize_request(
            &Request {
                version: Version::Http10,
                method: b"POST",
                target: b"/",
                headers: &[],
                body: RequestBody::Chunked {
                    chunks,
                    trailers: &[]
                }
            },
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn literal_fixed_body_preserves_header_case_ows_duplicate_cookies_and_bytes() {
    let wire = b"HTTP/1.1 200 fine\xff\r\ncOnTeNt-LeNgTh: 4\r\nX-Opaque:\t\xff  \t\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\n\r\n\0\xff\r\nNEXT";
    let (r, n) = decode(b"GET", wire, false);
    assert_eq!(r.head.reason, b"fine\xff");
    assert_eq!(r.head.headers[0].name, b"cOnTeNt-LeNgTh");
    assert_eq!(r.head.headers[1].value, b"\t\xff  \t");
    assert_eq!(r.head.headers[1].trimmed_value(), b"\xff");
    assert_eq!(
        r.head
            .headers
            .iter()
            .filter(|h| h.is(b"set-cookie"))
            .count(),
        2
    );
    assert_eq!(r.body, b"\0\xff\r\n");
    assert_eq!(&wire[n..], b"NEXT");
    assert_eq!(r.wire_bytes as usize, n);
    assert_eq!(r.framing, Framing::ContentLength(4));
    assert_eq!(r.connection, ConnectionUse::Reusable);
}

#[test]
fn equal_duplicate_and_list_lengths_recover_without_combining_fields() {
    for fields in [
        &b"Content-Length: 3\r\nContent-Length: 003\r\n"[..],
        b"Content-Length: 003, 3\r\n",
        b"Content-Length: 3 ,\t3,0003\r\n",
    ] {
        let mut wire = b"HTTP/1.1 200 OK\r\n".to_vec();
        wire.extend(fields);
        wire.extend(b"\r\nabcTAIL");
        let (r, n) = decode(b"GET", &wire, false);
        assert_eq!(r.body, b"abc");
        assert_eq!(&wire[n..], b"TAIL");
    }
}

#[test]
fn invalid_and_conflicting_lengths_fail_closed() {
    for (value, error) in [
        (&b"3,4"[..], Error::ConflictingContentLength),
        (b"", Error::InvalidContentLength),
        (b"+3", Error::InvalidContentLength),
        (b"-1", Error::InvalidContentLength),
        (b"3,", Error::InvalidContentLength),
        (b",3", Error::InvalidContentLength),
        (b"3,,3", Error::InvalidContentLength),
        (b"0x3", Error::InvalidContentLength),
        (b"3 3", Error::InvalidContentLength),
        (b"18446744073709551616", Error::InvalidContentLength),
    ] {
        let mut wire = b"HTTP/1.1 200 OK\r\nContent-Length:".to_vec();
        wire.extend(value);
        wire.extend(b"\r\n\r\nabc");
        rejects(&wire, error);
    }
    rejects(
        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\ncontent-length: 4\r\n\r\n",
        Error::ConflictingContentLength,
    );
}

#[test]
fn ambiguous_or_unsupported_transfer_codings_fail_closed() {
    rejects(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\n\r\n",
        Error::AmbiguousFraming,
    );
    for value in [
        &b""[..],
        b"identity",
        b"gzip",
        b"gzip, chunked",
        b"chunked, gzip",
        b"chunked, chunked",
        b"chunked;foo=bar",
        b",chunked",
        b"chunked,",
        b"chunked\tchunked",
    ] {
        let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: ".to_vec();
        wire.extend(value);
        wire.extend(b"\r\n\r\n");
        rejects(&wire, Error::UnsupportedTransferEncoding);
    }
    rejects(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n",
        Error::UnsupportedTransferEncoding,
    );
    rejects(
        b"HTTP/1.0 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
        Error::UnsupportedTransferEncoding,
    );
}

#[test]
fn content_encoding_stays_opaque_and_chunking_only_deframes() {
    let (r, n) = decode(b"GET", b"HTTP/1.1 200 OK\r\nContent-Encoding: br\r\nTransfer-Encoding: ChUnKeD\r\n\r\n3\r\n\x8b\0\xff\r\n0\r\n\r\nTAIL", false);
    assert_eq!(r.body, b"\x8b\0\xff");
    assert_eq!(r.head.headers[0].trimmed_value(), b"br");
    assert_eq!(n, r.wire_bytes as usize);
    assert_eq!(r.framing, Framing::Chunked);
}

#[test]
fn literal_chunked_body_extensions_and_separate_trailers() {
    let wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTrailer: X-Checksum\r\n\r\n4;foo=bar\r\nWiki\r\n5 ; q = \"quoted\\\"\\\\\xff\";flag\r\npedia\r\n000;end=\"\"\r\nX-Checksum:\tvalue \t\r\nX-Extra: other\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length:0\r\n\r\n";
    let (r, n) = decode(b"GET", wire, false);
    assert_eq!(r.body, b"Wikipedia");
    assert_eq!(r.trailers.len(), 2);
    assert_eq!(r.trailers[0].value, b"\tvalue \t");
    assert!(!r.head.headers.iter().any(|h| h.is(b"X-Checksum")));
    assert!(wire[n..].starts_with(b"HTTP/1.1"));
}

#[test]
fn chunk_extension_grammar_and_overflow_rejection() {
    for line in [
        &b""[..],
        b"+1",
        b"-1",
        b" 1",
        b"0x1",
        b"g",
        b"1 ",
        b"1;",
        b"1;=x",
        b"1;foo=",
        b"1;foo=\"unfinished",
        b"1;foo=\"bad\x7f\"",
        b"1;foo=\"bad\x01\"",
        b"1;foo=\"bad\\\x01\"",
        b"1;foo=\"ok\"oops",
        b"1;foo=(bad)",
        b"1;foo ",
        b"10000000000000000",
    ] {
        let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        wire.extend(line);
        wire.extend(b"\r\n");
        rejects(&wire, Error::InvalidChunk);
    }
    rejects(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nxX",
        Error::InvalidCrlf,
    );
    rejects(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\rX",
        Error::InvalidCrlf,
    );
}

#[test]
fn trailers_cannot_change_framing_routing_authentication_or_content_controls() {
    for name in [
        &b"Content-Length"[..],
        b"Transfer-Encoding",
        b"Host",
        b"Connection",
        b"Trailer",
        b"Upgrade",
        b"TE",
        b"Content-Encoding",
        b"Content-Type",
        b"Authorization",
        b"Set-Cookie",
        b"Location",
        b"Retry-After",
        b"Cache-Control",
        b"Proxy-Connection",
    ] {
        let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n".to_vec();
        wire.extend(name);
        wire.extend(b": bad\r\n\r\n");
        rejects(&wire, Error::InvalidTrailer);
        let trailers = [h(name, b"bad")];
        assert!(matches!(
            encode(
                b"POST",
                b"/",
                &[h(b"Host", b"x")],
                RequestBody::Chunked {
                    chunks: &[],
                    trailers: &trailers
                }
            ),
            Err(Error::InvalidTrailer)
        ));
        let mut wire = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTrailer: ".to_vec();
        wire.extend(name);
        wire.extend(b"\r\n\r\n");
        rejects(&wire, Error::InvalidTrailer);
    }
    rejects(b"HTTP/1.1 200 OK\r\nConnection: X-Late\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Late: bad\r\n\r\n", Error::InvalidTrailer);
    rejects(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTrailer: X,,Y\r\n\r\n",
        Error::InvalidTrailer,
    );
}

#[test]
fn strict_crlf_status_and_field_rules_reject_response_splitting() {
    for wire in [
        &b"HTTP/1.1 200 OK\n\n"[..],
        b"HTTP/1.1 200 OK\rX",
        b"HTTP/1.1 200 OK\r\nX:y\n\r\n",
        b"HTTP/1.1 200 OK\r\n\n",
    ] {
        rejects(wire, Error::InvalidCrlf);
    }
    for wire in [
        &b"HTTP/1.1\t200 OK\r\n\r\n"[..],
        b" HTTP/1.1 200 OK\r\n\r\n",
        b"HTTP/1.1 20 OK\r\n\r\n",
        b"HTTP/1.1 200\r\n\r\n",
        b"HTTP/1.1 099 X\r\n\r\n",
        b"HTTP/1.1 600 X\r\n\r\n",
        b"HTTP/1.1 20x X\r\n\r\n",
        b"HTTP/1.1 200 bad\x7f\r\n\r\n",
    ] {
        rejects(wire, Error::InvalidStatus);
    }
    rejects(b"HTTP/2.0 200 OK\r\n\r\n", Error::UnsupportedVersion);
    for line in [
        &b"X : y"[..],
        b": y",
        b"X",
        b"X@: y",
        b"X:y\0",
        b"X:y\x7f",
        b"X:y\x0b",
    ] {
        let mut wire = b"HTTP/1.1 200 OK\r\n".to_vec();
        wire.extend(line);
        wire.extend(b"\r\n\r\n");
        rejects(&wire, Error::InvalidHeader);
    }
    rejects(
        b"HTTP/1.1 200 OK\r\nX: y\r\n folded\r\n\r\n",
        Error::ObsoleteFold,
    );
    rejects(b"HTTP/1.1 200 OK\r\n\tX:y\r\n\r\n", Error::ObsoleteFold);
}

#[test]
fn empty_reason_and_unknown_status_codes_preserve_class_semantics() {
    let (r, _) = decode(b"GET", b"HTTP/1.1 299 \r\nContent-Length: 0\r\n\r\n", false);
    assert_eq!(r.head.status, 299);
    assert!(r.head.reason.is_empty());
    let (r, _) = decode(
        b"GET",
        b"HTTP/1.1 599 private\r\nContent-Length: 1\r\n\r\nx",
        false,
    );
    assert_eq!(r.body, b"x");
}

#[test]
fn head_204_304_end_at_headers_and_metadata_length_is_not_body_size() {
    for (method, head) in [
        (
            &b"HEAD"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Length: 18446744073709551615\r\n\r\n"[..],
        ),
        (
            b"HEAD",
            b"HTTP/1.1 500 X\r\nTransfer-Encoding: chunked\r\n\r\n",
        ),
        (b"GET", b"HTTP/1.1 204 No Content\r\n\r\n"),
        (
            b"GET",
            b"HTTP/1.1 304 Not Modified\r\nContent-Length: 999999999\r\n\r\n",
        ),
    ] {
        let mut wire = head.to_vec();
        wire.extend(b"FOLLOWING");
        let (r, n) = decode(method, &wire, false);
        assert_eq!(n, head.len());
        assert!(r.body.is_empty());
        assert!(r.trailers.is_empty());
        assert_eq!(r.framing, Framing::NoBody);
        assert_eq!(&wire[n..], b"FOLLOWING");
    }
    let mut d = decoder(b"head");
    assert_eq!(
        d.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc")
            .unwrap()
            .status,
        DecodeStatus::Complete
    );
    assert_eq!(d.response().unwrap().body, b"abc");
}

#[test]
fn prohibited_framing_on_1xx_and_204_and_ambiguous_head_rejected() {
    for status in [100, 103, 101, 204] {
        for field in ["Content-Length: 0", "Transfer-Encoding: chunked"] {
            rejects(
                format!("HTTP/1.1 {status} X\r\n{field}\r\n\r\n").as_bytes(),
                Error::AmbiguousFraming,
            );
        }
    }
    let mut d = decoder(b"HEAD");
    assert_eq!(
        d.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n"),
        Err(Error::AmbiguousFraming)
    );
}

#[test]
fn informational_responses_are_events_before_final_and_do_not_eat_body() {
    let first = b"HTTP/1.1 103 Early Hints\r\nLink: </synthetic>\r\n\r\n";
    let second = b"HTTP/1.1 100 Continue\r\n\r\n";
    let last = b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nxTAIL";
    let mut wire = first.to_vec();
    wire.extend(second);
    wire.extend(last);
    let mut d = decoder(b"POST");
    let a = d.feed(&wire).unwrap();
    assert_eq!(a.consumed, first.len());
    assert_eq!(a.status, DecodeStatus::Informational);
    assert_eq!(d.informational()[0].status, 103);
    assert!(d.response().is_none());
    let b = d.feed(&wire[a.consumed..]).unwrap();
    assert_eq!(b.consumed, second.len());
    assert_eq!(b.status, DecodeStatus::Informational);
    let c = d.feed(&wire[a.consumed + b.consumed..]).unwrap();
    assert_eq!(c.status, DecodeStatus::Complete);
    let r = d.into_response().unwrap();
    assert_eq!(r.informational.len(), 2);
    assert_eq!(r.body, b"x");
    assert_eq!(&wire[a.consumed + b.consumed + c.consumed..], b"TAIL");
}

#[test]
fn close_delimited_body_requires_orderly_eof_and_cannot_be_reused() {
    for version in ["1.0", "1.1"] {
        let wire = format!("HTTP/{version} 200 OK\r\nX:y\r\n\r\nopaque");
        let mut d = decoder(b"GET");
        let result = d.feed(wire.as_bytes()).unwrap();
        assert_eq!(result.status, DecodeStatus::NeedMore);
        assert!(d.response().is_none());
        d.finish().unwrap();
        let r = d.into_response().unwrap();
        assert_eq!(r.body, b"opaque");
        assert_eq!(r.connection, ConnectionUse::MustClose);
        assert_eq!(r.framing, Framing::UntilClose);
    }
    let mut d = decoder(b"GET");
    d.feed(b"HTTP/1.1 200 OK\r\n\r\npartial").unwrap();
    d.abort();
    assert!(d.response().is_none());
    assert_eq!(d.finish(), Err(Error::Poisoned));
}

#[test]
fn connection_reuse_matrix_close_precedence_and_eof() {
    for (request_version, request_fields, response_version, response_fields, expected) in [
        (Version::Http11, "", "1.1", "", ConnectionUse::Reusable),
        (
            Version::Http11,
            "Connection: close",
            "1.1",
            "",
            ConnectionUse::MustClose,
        ),
        (
            Version::Http11,
            "",
            "1.1",
            "Connection: keep-alive, CLOSE",
            ConnectionUse::MustClose,
        ),
        (Version::Http11, "", "1.0", "", ConnectionUse::MustClose),
        (
            Version::Http11,
            "",
            "1.0",
            "Connection: keep-alive",
            ConnectionUse::Reusable,
        ),
        (Version::Http10, "", "1.1", "", ConnectionUse::MustClose),
        (
            Version::Http10,
            "",
            "1.0",
            "Connection: keep-alive",
            ConnectionUse::MustClose,
        ),
        (
            Version::Http10,
            "Connection: keep-alive",
            "1.0",
            "Connection: keep-alive",
            ConnectionUse::Reusable,
        ),
        (
            Version::Http10,
            "Connection: keep-alive",
            "1.0",
            "",
            ConnectionUse::MustClose,
        ),
        (
            Version::Http11,
            "",
            "1.1",
            "Proxy-Connection: close",
            ConnectionUse::MustClose,
        ),
        (
            Version::Http11,
            "",
            "1.0",
            "Proxy-Connection: keep-alive",
            ConnectionUse::MustClose,
        ),
    ] {
        let headers = if request_fields.is_empty() {
            vec![]
        } else {
            vec![h(
                b"Connection",
                request_fields
                    .strip_prefix("Connection: ")
                    .unwrap()
                    .as_bytes(),
            )]
        };
        let mut d = ResponseDecoder::new(
            context(b"GET", request_version, &headers),
            Limits::default(),
        )
        .unwrap();
        let fields = if response_fields.is_empty() {
            String::new()
        } else {
            format!("{response_fields}\r\n")
        };
        let wire = format!("HTTP/{response_version} 200 OK\r\n{fields}Content-Length:0\r\n\r\n");
        d.feed(wire.as_bytes()).unwrap();
        assert_eq!(d.response().unwrap().connection, expected);
        d.finish().unwrap();
        assert_eq!(d.response().unwrap().connection, ConnectionUse::MustClose);
    }
    let (r, _) = decode(
        b"GET",
        b"HTTP/1.1 103 X\r\nConnection: close\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length:0\r\n\r\n",
        false,
    );
    assert_eq!(r.connection, ConnectionUse::MustClose);
}

#[test]
fn connection_tokens_cannot_hide_framing_or_inject_parameters() {
    for value in [
        &b"Content-Length"[..],
        b"Transfer-Encoding",
        b"Host",
        b"Trailer",
        b"close,,keep-alive",
        b"close;foo=1",
        b"",
        b"\"close\"",
    ] {
        let mut wire = b"HTTP/1.1 200 OK\r\nConnection: ".to_vec();
        wire.extend(value);
        wire.extend(b"\r\nContent-Length:0\r\n\r\n");
        rejects(&wire, Error::InvalidConnection);
    }
    let (r, _) = decode(
        b"GET",
        b"HTTP/1.1 200 OK\r\nConnection: X-Extension, TE\r\nContent-Length:0\r\n\r\n",
        false,
    );
    assert_eq!(r.connection, ConnectionUse::Reusable);
}

#[test]
fn connect_success_ignores_framing_and_stops_at_tunnel_boundary() {
    for status in [200, 201, 204, 299] {
        let head = format!(
            "HTTP/1.1 {status} tunnel\r\nContent-Length: not-a-number\r\nTransfer-Encoding: unsupported\r\n\r\n"
        );
        let mut wire = head.as_bytes().to_vec();
        wire.extend(b"\x16\x03\x01\0\xff");
        let (r, n) = decode(b"CONNECT", &wire, false);
        assert_eq!(n, head.len());
        assert_eq!(r.framing, Framing::Tunnel);
        assert_eq!(r.connection, ConnectionUse::Tunnel);
        assert!(r.body.is_empty());
        assert_eq!(&wire[n..], b"\x16\x03\x01\0\xff");
    }
    let (r, n) = decode(
        b"CONNECT",
        b"HTTP/1.1 407 Auth\r\nContent-Length:3\r\n\r\nerrTAIL",
        false,
    );
    assert_eq!(r.body, b"err");
    assert_eq!(r.framing, Framing::ContentLength(3));
    assert_eq!(n, r.wire_bytes as usize);
}

#[test]
fn upgrade_requires_negotiation_and_preserves_protocol_bytes() {
    let headers = [
        h(b"Connection", b" Upgrade"),
        h(b"Upgrade", b" example/V1, Other"),
    ];
    let mut d = ResponseDecoder::new(
        context(b"GET", Version::Http11, &headers),
        Limits::default(),
    )
    .unwrap();
    let head =
        b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: EXAMPLE/V1\r\n\r\n";
    let mut wire = head.to_vec();
    wire.extend(b"\0\xffopaque");
    let result = d.feed(&wire).unwrap();
    assert_eq!(result.consumed, head.len());
    assert_eq!(result.status, DecodeStatus::Complete);
    assert_eq!(d.response().unwrap().framing, Framing::Upgrade);
    assert_eq!(d.response().unwrap().connection, ConnectionUse::Upgrade);
    assert_eq!(&wire[result.consumed..], b"\0\xffopaque");
    let mut d = decoder(b"GET");
    assert_eq!(d.feed(head), Err(Error::InvalidUpgrade));
    for response in [
        b"HTTP/1.1 101 X\r\nConnection: Upgrade\r\nUpgrade: example/v1\r\n\r\n".as_slice(),
        b"HTTP/1.1 101 X\r\nUpgrade: example/V1\r\n\r\n",
        b"HTTP/1.1 101 X\r\nConnection: Upgrade, close\r\nUpgrade: example/V1\r\n\r\n",
        b"HTTP/1.1 101 X\r\nConnection: Upgrade\r\nUpgrade: unrelated\r\n\r\n",
        b"HTTP/1.1 101 X\r\nConnection: Upgrade\r\n\r\n",
        b"HTTP/1.0 101 X\r\nConnection: Upgrade\r\nUpgrade: example/V1\r\n\r\n",
    ] {
        let mut d = ResponseDecoder::new(
            context(b"GET", Version::Http11, &headers),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(d.feed(response), Err(Error::InvalidUpgrade));
    }
}

#[test]
fn invalid_upgrade_requests_are_rejected_and_multiple_layers_checked() {
    for headers in [
        vec![h(b"Upgrade", b"example")],
        vec![h(b"Connection", b"Upgrade")],
        vec![
            h(b"Connection", b"Upgrade, close"),
            h(b"Upgrade", b"example"),
        ],
        vec![h(b"Connection", b"Upgrade"), h(b"Upgrade", b"example/")],
        vec![
            h(b"Connection", b"Upgrade"),
            h(b"Upgrade", b"example/V1/extra"),
        ],
    ] {
        assert!(RequestContext::new(b"GET", Version::Http11, &headers, Limits::default()).is_err());
    }
    let headers = [
        h(b"Connection", b"Upgrade"),
        h(b"Upgrade", b"layer/V1, example"),
    ];
    let mut d = ResponseDecoder::new(
        context(b"GET", Version::Http11, &headers),
        Limits::default(),
    )
    .unwrap();
    d.feed(b"HTTP/1.1 101 X\r\nConnection: Upgrade\r\nUpgrade: example, layer/V1\r\n\r\n")
        .unwrap();
    assert_eq!(d.response().unwrap().connection, ConnectionUse::Upgrade);
}

#[test]
fn reset_content_205_consumes_valid_empty_framing_but_rejects_content() {
    for wire in [
        &b"HTTP/1.1 205 Reset\r\nContent-Length:0\r\n\r\nTAIL"[..],
        b"HTTP/1.1 205 Reset\r\nTransfer-Encoding:chunked\r\n\r\n0\r\n\r\nTAIL",
    ] {
        let (r, n) = decode(b"POST", wire, false);
        assert!(r.body.is_empty());
        assert_eq!(&wire[n..], b"TAIL");
    }
    rejects(
        b"HTTP/1.1 205 Reset\r\nContent-Length:1\r\n\r\nx",
        Error::UnexpectedBody,
    );
    rejects(
        b"HTTP/1.1 205 Reset\r\nTransfer-Encoding:chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n",
        Error::UnexpectedBody,
    );
    rejects(b"HTTP/1.1 205 Reset\r\n\r\nx", Error::UnexpectedBody);
    let (r, _) = decode(b"GET", b"HTTP/1.1 205 Reset\r\n\r\n", true);
    assert!(r.body.is_empty());
}

#[test]
fn pipelined_responses_are_consumed_one_at_a_time_without_resynchronization() {
    let one = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\none";
    let two = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\ntwo\r\n0\r\n\r\n";
    let three = b"HTTP/1.1 304 Cached\r\n\r\n";
    let mut wire = one.to_vec();
    wire.extend(two);
    wire.extend(three);
    let (a, n1) = decode(b"GET", &wire, false);
    assert_eq!(n1, one.len());
    assert_eq!(a.body, b"one");
    let (b, n2) = decode(b"GET", &wire[n1..], false);
    assert_eq!(n2, two.len());
    assert_eq!(b.body, b"two");
    let (c, n3) = decode(b"GET", &wire[n1 + n2..], false);
    assert_eq!(n3, three.len());
    assert!(c.body.is_empty());
}

#[test]
fn every_two_way_split_and_bytewise_fragment_preserves_results_and_consumption() {
    let corpus: &[(&[u8], &[u8], bool)] = &[
        (b"GET", b"HTTP/1.1 200 OK\r\nContent-Length:4\r\nX:\xff\r\n\r\n\0abc", false),
        (b"GET", b"HTTP/1.1 103 X\r\n\r\nHTTP/1.1 100 X\r\n\r\nHTTP/1.1 200 X\r\nTransfer-Encoding:chunked\r\n\r\n1;f=\"x\\\"y\"\r\na\r\n2\r\nbc\r\n0\r\nX:ok\r\n\r\n", false),
        (b"HEAD", b"HTTP/1.1 200 OK\r\nContent-Length:500\r\n\r\n", false),
        (b"CONNECT", b"HTTP/1.1 200 OK\r\n\r\n", false),
        (b"GET", b"HTTP/1.0 200 OK\r\n\r\nclose\0\xff", true),
    ];
    let mut partitions = 0;
    for (method, wire, eof) in corpus {
        let expected = decode(method, wire, *eof).0;
        for split in 0..=wire.len() {
            let mut d = decoder(method);
            let mut total = 0;
            for fragment in [&wire[..split], &wire[split..]] {
                let mut pos = 0;
                loop {
                    let result = d.feed(&fragment[pos..]).unwrap();
                    pos += result.consumed;
                    total += result.consumed;
                    if result.status != DecodeStatus::Informational {
                        break;
                    }
                }
                assert_eq!(pos, fragment.len());
            }
            if *eof {
                d.finish().unwrap();
            }
            assert_eq!(d.into_response().unwrap(), expected);
            assert_eq!(total, wire.len());
            partitions += 1;
        }
        let mut d = decoder(method);
        for byte in *wire {
            assert_eq!(d.feed(&[*byte]).unwrap().consumed, 1);
        }
        if *eof {
            d.finish().unwrap();
        }
        assert_eq!(d.into_response().unwrap(), expected);
    }
    println!(
        "literal two-way partitions verified: {partitions}; bytewise corpus: {}",
        corpus.len()
    );
}

#[test]
fn truncation_at_every_prefix_discards_incomplete_message_including_last_crlf() {
    let corpus: &[&[u8]] = &[
        b"HTTP/1.1 200 OK\r\nContent-Length:3\r\n\r\nabc",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding:chunked\r\n\r\n3\r\nabc\r\n0\r\nX:ok\r\n\r\n",
        b"HTTP/1.1 100 X\r\n\r\nHTTP/1.1 200 X\r\nContent-Length:0\r\n\r\n",
    ];
    let mut prefixes = 0;
    for wire in corpus {
        for end in 0..wire.len() {
            let mut d = decoder(b"GET");
            let mut pos = 0;
            loop {
                let r = d.feed(&wire[pos..end]).unwrap();
                pos += r.consumed;
                if r.status != DecodeStatus::Informational {
                    break;
                }
            }
            assert_eq!(
                d.finish(),
                Err(Error::Truncated),
                "end={end}, wire={wire:?}"
            );
            assert!(d.response().is_none());
            assert_eq!(d.feed(b"abc"), Err(Error::Poisoned));
            prefixes += 1;
        }
    }
    println!("incomplete literal prefixes rejected: {prefixes}");
}

#[test]
fn size_and_count_limits_cover_each_resource_and_exact_boundaries() {
    let wire = b"HTTP/1.1 200 OK\r\nContent-Length:3\r\nX:y\r\n\r\nabc";
    let head_len = wire.len() - 3;
    let mut limits = Limits {
        max_head_bytes: head_len,
        max_body_bytes: 3,
        max_fields: 2,
        max_wire_bytes: wire.len() as u64,
        ..Limits::default()
    };
    let mut d = ResponseDecoder::new(context(b"GET", Version::Http11, &[]), limits).unwrap();
    d.feed(wire).unwrap();
    assert!(d.response().is_some());
    for (resource, changed) in [
        (
            Resource::Line,
            Limits {
                max_line_bytes: 13,
                ..limits
            },
        ),
        (
            Resource::Head,
            Limits {
                max_head_bytes: head_len - 1,
                ..limits
            },
        ),
        (
            Resource::Body,
            Limits {
                max_body_bytes: 2,
                ..limits
            },
        ),
        (
            Resource::Fields,
            Limits {
                max_fields: 1,
                ..limits
            },
        ),
        (
            Resource::Wire,
            Limits {
                max_wire_bytes: wire.len() as u64 - 1,
                ..limits
            },
        ),
    ] {
        let mut d = ResponseDecoder::new(context(b"GET", Version::Http11, &[]), changed).unwrap();
        assert_eq!(d.feed(wire), Err(Error::Limit(resource)));
    }
    let chunk_wire =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding:chunked\r\n\r\n1;f=x\r\na\r\n0\r\nX:y\r\n\r\n";
    limits = Limits::default();
    for (resource, changed) in [
        (
            Resource::ChunkLine,
            Limits {
                max_chunk_line_bytes: 4,
                ..limits
            },
        ),
        (
            Resource::Chunks,
            Limits {
                max_chunks: 1,
                ..limits
            },
        ),
        (
            Resource::Trailers,
            Limits {
                max_trailer_bytes: 6,
                ..limits
            },
        ),
        (
            Resource::TrailerFields,
            Limits {
                max_trailers: 0,
                ..limits
            },
        ),
        (
            Resource::Body,
            Limits {
                max_body_bytes: 0,
                ..limits
            },
        ),
    ] {
        let mut d = ResponseDecoder::new(context(b"GET", Version::Http11, &[]), changed).unwrap();
        assert_eq!(d.feed(chunk_wire), Err(Error::Limit(resource)));
    }
    let info_limits = Limits {
        max_informational: 0,
        ..limits
    };
    let mut d = ResponseDecoder::new(context(b"GET", Version::Http11, &[]), info_limits).unwrap();
    assert_eq!(
        d.feed(b"HTTP/1.1 100 X\r\n\r\n"),
        Err(Error::Limit(Resource::Informational))
    );
    let mut d = ResponseDecoder::new(
        context(b"GET", Version::Http11, &[]),
        Limits {
            max_body_bytes: 3,
            ..limits
        },
    )
    .unwrap();
    assert_eq!(
        d.feed(b"HTTP/1.1 200 OK\r\n\r\n1234"),
        Err(Error::Limit(Resource::Body))
    );
}

#[test]
fn bounded_chunk_overhead_and_informational_aggregate_resist_resource_abuse() {
    let limits = Limits {
        max_chunks: 3,
        ..Limits::default()
    };
    let mut d = ResponseDecoder::new(context(b"GET", Version::Http11, &[]), limits).unwrap();
    assert_eq!(d.feed(b"HTTP/1.1 200 OK\r\nTransfer-Encoding:chunked\r\n\r\n1\r\na\r\n1\r\nb\r\n1\r\nc\r\n0\r\n\r\n"), Err(Error::Limit(Resource::Chunks)));
    let limits = Limits {
        max_head_bytes: 100,
        max_wire_bytes: 25,
        ..Limits::default()
    };
    let mut d = ResponseDecoder::new(context(b"GET", Version::Http11, &[]), limits).unwrap();
    assert_eq!(
        d.feed(b"HTTP/1.1 100 X\r\n\r\n").unwrap().status,
        DecodeStatus::Informational
    );
    assert_eq!(
        d.feed(b"HTTP/1.1 100 X\r\n\r\n"),
        Err(Error::Limit(Resource::Wire))
    );
}

#[test]
fn request_limits_and_absolute_configuration_ceilings_are_enforced() {
    for limits in [
        Limits {
            max_line_bytes: 0,
            ..Limits::default()
        },
        Limits {
            max_body_bytes: usize::MAX,
            ..Limits::default()
        },
        Limits {
            max_fields: 4097,
            ..Limits::default()
        },
        Limits {
            max_informational: 65,
            ..Limits::default()
        },
        Limits {
            max_wire_bytes: u64::MAX,
            ..Limits::default()
        },
    ] {
        assert_eq!(limits.validate().unwrap_err(), Error::InvalidLimits);
    }
    let headers = [h(b"Host", b"x")];
    let request = Request {
        version: Version::Http11,
        method: b"POST",
        target: b"/",
        headers: &headers,
        body: RequestBody::Bytes(b"abcd"),
    };
    assert!(matches!(
        serialize_request(
            &request,
            Limits {
                max_body_bytes: 3,
                ..Limits::default()
            }
        ),
        Err(Error::Limit(Resource::Body))
    ));
    assert!(matches!(
        serialize_request(
            &request,
            Limits {
                max_fields: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit(Resource::Fields))
    ));
    assert!(matches!(
        serialize_request(
            &request,
            Limits {
                max_head_bytes: 16,
                ..Limits::default()
            }
        ),
        Err(Error::Limit(Resource::Head))
    ));
    let empty_chunks = [b"".as_slice(); 4];
    let request = Request {
        version: Version::Http11,
        method: b"POST",
        target: b"/",
        headers: &headers,
        body: RequestBody::Chunked {
            chunks: &empty_chunks,
            trailers: &[],
        },
    };
    assert!(matches!(
        serialize_request(
            &request,
            Limits {
                max_chunks: 4,
                ..Limits::default()
            }
        ),
        Err(Error::Limit(Resource::Chunks))
    ));
}

#[test]
fn complete_decoder_leaves_input_intact_and_abort_never_exposes_a_response() {
    let mut d = decoder(b"GET");
    assert_eq!(
        d.feed(b"").unwrap(),
        FeedResult {
            consumed: 0,
            status: DecodeStatus::NeedMore
        }
    );
    d.feed(b"HTTP/1.1 200 OK\r\nContent-Length:0\r\n\r\n")
        .unwrap();
    assert_eq!(
        d.feed(b"NEXT").unwrap(),
        FeedResult {
            consumed: 0,
            status: DecodeStatus::Complete
        }
    );
    d.abort();
    assert!(d.response().is_none());
    assert_eq!(d.into_response().unwrap_err(), Error::Poisoned);
    assert_eq!(
        decoder(b"GET").into_response().unwrap_err(),
        Error::Incomplete
    );
}

#[test]
fn te_negotiation_never_advertises_an_unimplemented_transfer_coding() {
    for value in [&b"trailers"[..], b"TrAiLeRs", b""] {
        encode(
            b"GET",
            b"/",
            &[h(b"Host", b"x"), h(b"Connection", b"TE"), h(b"TE", value)],
            RequestBody::Empty,
        )
        .unwrap();
    }
    for value in [
        &b"chunked"[..],
        b"gzip",
        b"gzip, trailers",
        b"trailers;q=1",
        b"trailers,",
        b"trailers, trailers",
    ] {
        assert!(matches!(
            encode(
                b"GET",
                b"/",
                &[h(b"Host", b"x"), h(b"Connection", b"TE"), h(b"TE", value)],
                RequestBody::Empty
            ),
            Err(Error::UnsupportedTransferEncoding)
        ));
    }
    assert!(matches!(
        encode(
            b"GET",
            b"/",
            &[h(b"Host", b"x"), h(b"TE", b"trailers")],
            RequestBody::Empty
        ),
        Err(Error::InvalidConnection)
    ));
}

#[test]
fn connect_and_trace_request_content_cannot_corrupt_protocol_boundaries() {
    for (method, target) in [(&b"CONNECT"[..], &b"x:443"[..]), (b"TRACE", b"/")] {
        for body in [
            RequestBody::Bytes(b"x"),
            RequestBody::Chunked {
                chunks: &[],
                trailers: &[],
            },
        ] {
            assert!(matches!(
                encode(method, target, &[h(b"Host", b"x")], body),
                Err(Error::UnexpectedBody)
            ));
        }
        encode(method, target, &[h(b"Host", b"x")], RequestBody::Empty).unwrap();
    }
    for name in [&b"Authorization"[..], b"Cookie", b"Proxy-Authorization"] {
        assert!(matches!(
            encode(
                b"TRACE",
                b"/",
                &[h(b"Host", b"x"), h(name, b"synthetic-secret")],
                RequestBody::Empty
            ),
            Err(Error::InvalidHeader)
        ));
    }
    let headers = [h(b"Connection", b"Upgrade"), h(b"Upgrade", b"example")];
    let mut d = ResponseDecoder::new(
        context(b"GET", Version::Http11, &headers),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        d.feed(b"HTTP/1.1 103 X\r\nConnection: close\r\n\r\n")
            .unwrap()
            .status,
        DecodeStatus::Informational
    );
    assert_eq!(
        d.feed(b"HTTP/1.1 101 X\r\nConnection: Upgrade\r\nUpgrade: example\r\n\r\n"),
        Err(Error::InvalidUpgrade)
    );
}

#[test]
fn every_body_octet_survives_and_debug_output_redacts_peer_and_wallet_bytes() {
    let body: Vec<u8> = (0..=255).collect();
    let encoded = encode(
        b"POST",
        b"/",
        &[h(b"Host", b"x"), h(b"Authorization", b"synthetic-secret")],
        RequestBody::Bytes(&body),
    )
    .unwrap();
    assert_eq!(&encoded.bytes[encoded.head_bytes..], body);
    assert!(!format!("{encoded:?}").contains("synthetic-secret"));
    let mut wire =
        b"HTTP/1.1 200 synthetic-secret\r\nContent-Length:256\r\nX:synthetic-secret\r\n\r\n"
            .to_vec();
    wire.extend(&body);
    let (response, n) = decode(b"GET", &wire, false);
    assert_eq!(response.body, body);
    assert_eq!(n, wire.len());
    assert!(
        !format!(
            "{response:?} {:?} {:?}",
            response.head, response.head.headers[1]
        )
        .contains("synthetic-secret")
    );
}

#[test]
fn deterministic_all_octet_mutations_are_bounded_and_never_panic_or_overconsume() {
    let literals: &[&[u8]] = &[
        b"HTTP/1.1 200 OK\r\nContent-Length:1\r\n\r\nx",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding:chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n",
    ];
    let mut mutations = 0;
    for literal in literals {
        for index in 0..literal.len() {
            for byte in 0..=255 {
                let mut input = literal.to_vec();
                input[index] = byte;
                let mut d = decoder(b"GET");
                let mut pos = 0;
                loop {
                    match d.feed(&input[pos..]) {
                        Ok(r) => {
                            assert!(r.consumed <= input.len() - pos);
                            pos += r.consumed;
                            if r.status == DecodeStatus::Informational {
                                assert!(r.consumed > 0);
                                continue;
                            }
                        }
                        Err(_) => {
                            assert!(d.response().is_none());
                            break;
                        }
                    }
                    let _ = d.finish();
                    if let Some(response) = d.response() {
                        assert!(response.body.len() <= Limits::default().max_body_bytes);
                        assert_eq!(response.wire_bytes as usize, pos);
                    }
                    break;
                }
                mutations += 1;
            }
        }
    }
    println!("single-octet literal mutations checked: {mutations}");
}
