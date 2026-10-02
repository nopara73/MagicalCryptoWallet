//! Tests compile the actual codecs and HTTP/1 framing, without a runtime fallback.
#![forbid(unsafe_code)]
#[path = "../src/compression.rs"]
pub mod compression;
#[path = "../src/content_service/mod.rs"]
pub mod content_service;
#[path = "../src/http1.rs"]
pub mod http1;

use compression::{EncodeMethod, EncodeOptions, Format, Limit, TrailingData};
use content_service::brotli;
use content_service::{Abort, Coding, ErrorKind, Limits};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn content_packet(data: &[u8], headers: &[&[u8]]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u16.to_le_bytes());
    p.extend_from_slice(&5000u16.to_le_bytes());
    p.push(headers.len() as u8);
    p.push(0);
    p.extend_from_slice(&(data.len() as u32).to_le_bytes());
    for h in headers {
        p.extend_from_slice(&(h.len() as u16).to_le_bytes());
        p.extend_from_slice(h);
    }
    p.extend_from_slice(data);
    p
}
fn packet_failure(p: &[u8]) -> u16 {
    assert_eq!(p.len(), 22);
    assert_eq!(&p[..3], &[1, 0, 1]);
    u16::from_le_bytes([p[3], p[4]])
}

#[test]
fn bounded_adapter_has_exact_proofs_and_no_general_session_state() {
    let plain = b"{\"fastestFee\":8.25,\"halfHourFee\":6,\"hourFee\":4,\"economyFee\":2}";
    let gz = packed(plain, Format::Gzip);
    let br = br_stored(&gz);
    let p = content_service::adapter::execute(&content_packet(&br, &[b"gzip, br"]), &mut || Ok(()))
        .unwrap();
    assert_eq!(&p[..3], &[1, 0, 0]);
    assert_eq!(
        u32::from_le_bytes(p[3..7].try_into().unwrap()) as usize,
        br.len()
    );
    assert_eq!(
        u32::from_le_bytes(p[7..11].try_into().unwrap()) as usize,
        plain.len()
    );
    assert_eq!(p[11], 2);
    assert_eq!(p[12], 3);
    assert_eq!(p[33], 1);
    assert_eq!(&p[54..], plain);
    assert!(p.len() < 1_048_576 - 16);
    let large = vec![0; content_service::adapter::MAX_BODY];
    let p =
        content_service::adapter::execute(&content_packet(&large, &[]), &mut || Ok(())).unwrap();
    assert_eq!(p.len(), large.len() + 12);
    assert_eq!(p[11], 0);
    assert_eq!(&p[12..], large);
}
#[test]
fn bounded_adapter_rejects_malformed_trailing_oversized_payloads() {
    use content_service::adapter::{self, Failure};
    let good = content_packet(b"abc", &[]);
    let mut bads = vec![vec![], vec![0; 9], good[..good.len() - 1].to_vec()];
    let mut p = good.clone();
    p.push(0);
    bads.push(p);
    let mut p = good.clone();
    p[2..4].fill(0);
    bads.push(p);
    for (index, value) in [(0, 2), (3, 255), (4, 5), (5, 1), (9, 1)] {
        let mut p = good.clone();
        p[index] = value;
        bads.push(p);
    }
    for p in bads {
        assert_eq!(
            packet_failure(&adapter::execute(&p, &mut || Ok(())).unwrap()),
            Failure::MalformedRequest as u16
        );
    }
    let p = content_packet(b"encoded", &[b"unknown"]);
    assert_eq!(
        packet_failure(&adapter::execute(&p, &mut || Ok(())).unwrap()),
        Failure::UnsupportedEncoding as u16
    );
    let p = content_packet(b"", &[b"gzip"]);
    assert_eq!(
        packet_failure(&adapter::execute(&p, &mut || Ok(())).unwrap()),
        Failure::Truncated as u16
    );
    let mut gz = packed(b"secret-free synthetic", Format::Gzip);
    let i = gz.len() - 8;
    gz[i] ^= 1;
    let reply = adapter::execute(&content_packet(&gz, &[b"gzip"]), &mut || Ok(())).unwrap();
    assert_eq!(packet_failure(&reply), Failure::Checksum as u16);
    assert!(u64::from_le_bytes(reply[14..22].try_into().unwrap()) > 0);
}
#[test]
fn stateless_adapter_withholds_body_on_host_cancellation() {
    use content_service::adapter::{self, Failure};
    let packet = content_packet(b"synthetic bounded response", &[]);
    let reply = adapter::execute(&packet, &mut || Err(Abort::Cancelled)).unwrap();
    assert_eq!(packet_failure(&reply), Failure::Cancelled as u16);
    let next = adapter::execute(&packet, &mut || Ok(())).unwrap();
    assert_eq!(&next[..3], &[1, 0, 0]);
    assert_eq!(&next[12..], b"synthetic bounded response");
}
#[test]
fn host_callback_interrupts_partial_native_work_without_a_body() {
    use content_service::adapter::{self, Failure};
    let br = br_stored(&vec![42; 128 * 1024]);
    let packet = content_packet(&br, &[b"br"]);
    let mut calls = 0;
    let reply = adapter::execute(&packet, &mut || {
        calls += 1;
        if calls == 20 {
            Err(Abort::Cancelled)
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(packet_failure(&reply), Failure::Cancelled as u16);
    assert_eq!(calls, 20);
    let produced = u64::from_le_bytes(reply[14..22].try_into().unwrap());
    assert!(produced > 0 && produced < 128 * 1024);
}
#[test]
fn service_deadline_is_checked_inside_native_work() {
    use content_service::adapter::{self, Failure};
    let mut p = content_packet(&br_repeat(20000), &[b"br"]);
    p[2..4].copy_from_slice(&1u16.to_le_bytes());
    let reply = adapter::execute(&p, &mut || {
        std::thread::sleep(Duration::from_millis(2));
        Ok(())
    })
    .unwrap();
    assert_eq!(packet_failure(&reply), Failure::Deadline as u16);
}
#[test]
fn debug_and_options_never_dump_plaintext_or_dictionary_bytes() {
    let secret = b"SYNTHETIC-SECRET-DICTIONARY-AND-BODY";
    let data_repr = format!("{secret:?}");
    let mut options = compression::DecodeOptions::new(Format::Zlib);
    options.dictionary = compression::Dictionary::Use(secret);
    for text in [
        format!("{:?}", options.dictionary),
        format!("{options:?}"),
        format!(
            "{:?}",
            compression::decode(
                &packed(secret, Format::Zlib),
                compression::DecodeOptions::new(Format::Zlib)
            )
            .unwrap()
        ),
        format!(
            "{:?}",
            br_decode(&br_stored(secret), compression::Limits::default()).unwrap()
        ),
        format!("{:?}", decode(secret, &[], Limits::default()).unwrap()),
    ] {
        assert!(!text.contains(&data_repr));
        assert!(!text.contains(std::str::from_utf8(secret).unwrap()));
    }
}

struct Bits {
    bytes: Vec<u8>,
    bit: usize,
}
impl Bits {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bit: 0,
        }
    }
    fn raw(&mut self, value: u32, n: usize) {
        for i in 0..n {
            if self.bit % 8 == 0 {
                self.bytes.push(0);
            }
            let last = self.bytes.len() - 1;
            self.bytes[last] |= (((value >> i) & 1) as u8) << (self.bit % 8);
            self.bit += 1;
        }
    }
    fn align(&mut self) {
        while self.bit % 8 != 0 {
            self.raw(0, 1);
        }
    }
    fn simple(&mut self, symbols: &[u32], width: usize) {
        self.raw(1, 2);
        self.raw(symbols.len() as u32 - 1, 2);
        for &s in symbols {
            self.raw(s, width);
        }
    }
    fn final_empty(&mut self) {
        self.raw(3, 2);
        self.align();
    }
    fn stored(&mut self, data: &[u8]) {
        assert!(!data.is_empty() && data.len() <= 65536);
        self.raw(0, 1);
        self.raw(0, 2);
        self.raw(data.len() as u32 - 1, 16);
        self.raw(1, 1);
        self.align();
        for &v in data {
            self.raw(v as u32, 8);
        }
    }
    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.bytes
    }
}
fn br_stored(data: &[u8]) -> Vec<u8> {
    let mut b = Bits::new();
    b.raw(0, 1);
    for part in data.chunks(65536) {
        b.stored(part);
    }
    b.final_empty();
    b.finish()
}
fn literal_prefix(b: &mut Bits, size: usize) {
    b.raw(0, 1);
    b.raw(1, 1);
    b.raw(0, 1);
    b.raw(0, 2);
    b.raw(size as u32 - 1, 16);
    for _ in 0..3 {
        b.raw(0, 1);
    }
    b.raw(0, 2);
    b.raw(0, 4);
    b.raw(0, 2);
    b.raw(0, 1);
    b.raw(0, 1);
}
fn br_repeat(size: usize) -> Vec<u8> {
    let bases = [
        0, 1, 2, 3, 4, 5, 6, 8, 10, 14, 18, 26, 34, 50, 66, 98, 130, 194, 322, 578, 1090, 2114,
        6210, 22594,
    ];
    let extra = [
        0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 8, 9, 10, 12, 14, 24,
    ];
    let ic = bases.iter().rposition(|&n| n <= size).unwrap();
    let group = if ic < 8 {
        0
    } else if ic < 16 {
        4
    } else {
        7
    };
    let command = group * 64 + (ic & 7) * 8;
    let mut b = Bits::new();
    literal_prefix(&mut b, size);
    b.simple(&[b'A' as u32], 8);
    b.simple(&[command as u32], 10);
    b.simple(&[0], 6);
    b.raw((size - bases[ic]) as u32, extra[ic]);
    b.finish()
}
fn packed(data: &[u8], format: Format) -> Vec<u8> {
    let mut opts = EncodeOptions::new(format);
    opts.method = EncodeMethod::Fixed;
    compression::encode(data, opts).unwrap()
}
fn decode(
    data: &[u8],
    fields: &[&[u8]],
    limits: Limits,
) -> Result<content_service::DecodedBody, content_service::Error> {
    content_service::decode(data, fields, limits, &mut || Ok(()))
}
fn br_decode(data: &[u8], limits: compression::Limits) -> Result<brotli::Decoded, brotli::Error> {
    brotli::decode(data, limits, 4096, TrailingData::Reject, &mut || Ok(()))
}

#[test]
fn header_order_case_ows_and_aliases() {
    assert_eq!(
        content_service::parse_codings(&[b" GZip , deflate ", b"\tbr, IDENTITY, x-gzip\t"], 8)
            .unwrap(),
        [
            Coding::Gzip,
            Coding::Deflate,
            Coding::Brotli,
            Coding::Identity,
            Coding::Gzip
        ]
    );
    assert_eq!(content_service::ACCEPT_ENCODING, b"gzip, deflate, br");
    assert!(content_service::parse_codings(&[], 0).unwrap().is_empty());
}
#[test]
fn invalid_and_unknown_codings_never_return_encoded_bytes() {
    for token in [
        b"".as_slice(),
        b",gzip",
        b"gzip,",
        b"g zip",
        b"gzip\r\nX: y",
        b"br; q=1",
    ] {
        assert_eq!(
            decode(b"encoded", &[token], Limits::default())
                .unwrap_err()
                .kind,
            ErrorKind::InvalidEncoding
        );
    }
    assert_eq!(
        decode(b"encoded", &[b"zstd"], Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::UnsupportedEncoding
    );
    assert_eq!(
        content_service::parse_codings(&[b"gzip,br"], 1)
            .unwrap_err()
            .kind,
        ErrorKind::TooManyEncodings
    );
    assert_eq!(
        content_service::parse_codings(&[&vec![b' '; 2049]], 8)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidEncoding
    );
    let many = vec![b"identity".as_slice(); 33];
    assert_eq!(
        content_service::parse_codings(&many, usize::MAX)
            .unwrap_err()
            .kind,
        ErrorKind::TooManyEncodings
    );
}
#[test]
fn all_codings_exact_consumption_and_reverse_layer_proof() {
    let plain = b"synthetic coordinator response with exact amounts: 123456789";
    let gz = packed(plain, Format::Gzip);
    let z = packed(&gz, Format::Zlib);
    let br = br_stored(&z);
    let d = decode(&br, &[b"gzip, identity", b"deflate, br"], Limits::default()).unwrap();
    assert_eq!(d.bytes, plain);
    assert_eq!(d.encoded_len, br.len());
    assert_eq!(d.decoded_len, plain.len());
    assert_eq!(
        d.layers.iter().map(|l| l.coding).collect::<Vec<_>>(),
        [
            Coding::Brotli,
            Coding::Deflate,
            Coding::Identity,
            Coding::Gzip
        ]
    );
    for l in &d.layers {
        assert_eq!(l.consumed, l.input_len);
    }
    assert_eq!(d.layers[0].output_len, z.len());
    assert_eq!(d.layers[3].input_len, gz.len());
}
#[test]
fn http_deflate_is_zlib_and_no_raw_retry_or_preset_dictionary() {
    let raw = packed(b"synthetic", Format::Deflate);
    assert!(decode(&raw, &[b"deflate"], Limits::default()).is_err());
    let mut z = packed(b"synthetic", Format::Zlib);
    let cmf = 0x78u16;
    let mut flg = 0x20u16;
    flg += (31 - ((cmf * 256 + flg) % 31)) % 31;
    z[0] = cmf as u8;
    z[1] = flg as u8;
    z.splice(2..2, [0, 0, 0, 1]);
    assert!(matches!(
        decode(&z, &[b"deflate"], Limits::default())
            .unwrap_err()
            .kind,
        ErrorKind::Compression(_)
    ));
}
#[test]
fn trailing_concat_and_checksum_failures_withhold_results() {
    let plain = b"synthetic secret-free body";
    for (coding, format) in [
        (b"gzip".as_slice(), Format::Gzip),
        (b"deflate", Format::Zlib),
    ] {
        let mut p = packed(plain, format);
        p.push(0);
        assert_eq!(
            decode(&p, &[coding], Limits::default()).unwrap_err().kind,
            ErrorKind::Compression(compression::ErrorKind::TrailingData)
        );
        p.pop();
        let last = p.len() - 1;
        p[last] ^= 1;
        assert!(decode(&p, &[coding], Limits::default()).is_err());
    }
    let mut concat = packed(b"first", Format::Gzip);
    concat.extend_from_slice(&packed(b"second", Format::Gzip));
    assert_eq!(
        decode(&concat, &[b"gzip"], Limits::default())
            .unwrap()
            .bytes,
        b"firstsecond"
    );
    let mut br = br_stored(plain);
    br.extend_from_slice(&[6]);
    assert_eq!(
        decode(&br, &[b"br"], Limits::default()).unwrap_err().kind,
        ErrorKind::Brotli(brotli::ErrorKind::TrailingData)
    );
}
#[test]
fn corrupted_inner_layer_reports_actual_failing_layer() {
    let mut gz = packed(b"synthetic", Format::Gzip);
    let last = gz.len() - 8;
    gz[last] ^= 1;
    let err = decode(&br_stored(&gz), &[b"gzip, br"], Limits::default()).unwrap_err();
    assert_eq!(err.layer, 1);
    assert!(matches!(
        err.kind,
        ErrorKind::Compression(compression::ErrorKind::ChecksumMismatch { .. })
    ));
    assert!(err.output_produced > 0);
}
#[test]
fn input_output_and_identity_caps() {
    let plain = vec![b'A'; 1000];
    let br = br_repeat(plain.len());
    let mut l = Limits::default();
    l.codec.max_input_bytes = br.len() as u64 - 1;
    assert_eq!(
        decode(&br, &[b"br"], l).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Input)
    );
    l = Limits::default();
    l.codec.max_output_bytes = 999;
    assert_eq!(
        decode(&br, &[b"br"], l).unwrap_err().kind,
        ErrorKind::Brotli(brotli::ErrorKind::LimitExceeded(Limit::Output))
    );
    l.codec.max_output_bytes = 1000;
    assert_eq!(decode(&br, &[b"br"], l).unwrap().bytes, plain);
    for fields in [vec![], vec![b"identity".as_slice()]] {
        l.codec.max_output_bytes = 999;
        assert!(decode(&plain, &fields, l).is_err());
        l.codec.max_output_bytes = 1000;
        assert_eq!(decode(&plain, &fields, l).unwrap().bytes, plain);
    }
}
#[test]
fn aggregate_expansion_bound_cannot_reset_between_layers() {
    let plain = vec![b'A'; 50000];
    let gz = packed(&plain, Format::Gzip);
    let z = packed(&gz, Format::Zlib);
    let mut limits = Limits::default();
    limits.codec.max_expansion_ratio = 2;
    limits.codec.expansion_slack_bytes = 0;
    assert!(decode(&z, &[b"gzip, deflate"], limits).is_err());
    limits.codec.expansion_slack_bytes = plain.len() as u64;
    assert_eq!(
        decode(&z, &[b"gzip, deflate"], limits).unwrap().bytes,
        plain
    );
}
#[test]
fn zero_expansion_policy_is_consistent_for_identity() {
    let mut limits = Limits::default();
    limits.codec.max_expansion_ratio = 0;
    limits.codec.expansion_slack_bytes = 0;
    for fields in [vec![], vec![b"identity".as_slice()]] {
        assert!(decode(b"a", &fields, limits).is_err());
        assert!(decode(b"", &fields, limits).is_ok());
    }
}
#[test]
fn allocation_and_work_caps_include_layer_buffers() {
    let br = br_repeat(12000);
    let mut limits = Limits::default();
    limits.codec.max_allocation_bytes = 0;
    assert_eq!(
        decode(&br, &[b"br"], limits).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Allocation)
    );
    limits = Limits::default();
    limits.codec.max_allocation_bytes = 32768;
    assert!(decode(&br, &[b"br"], limits).is_err());
    limits = Limits::default();
    limits.codec.max_work = 0;
    assert_eq!(
        decode(&br, &[b"br"], limits).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Work)
    );
    let d = decode(&br, &[b"br"], Limits::default()).unwrap();
    limits = Limits::default();
    limits.codec.max_work = d.layers[0].work + 2;
    assert!(decode(&br, &[b"br"], limits).is_ok());
    limits.codec.max_work -= 1;
    assert!(decode(&br, &[b"br"], limits).is_err());
    let gz = packed(&vec![0; 40000], Format::Gzip);
    let z = packed(&gz, Format::Zlib);
    let spent = decode(&z, &[b"gzip,deflate"], Limits::default())
        .unwrap()
        .layers
        .iter()
        .map(|l| l.work)
        .sum::<u64>();
    limits = Limits::default();
    limits.codec.max_work = spent;
    assert!(decode(&z, &[b"gzip,deflate"], limits).is_err());
}
#[test]
fn cancellation_and_deadlines_reach_inner_work_loops() {
    let br = br_repeat(20000);
    let gz = packed(&vec![b'A'; 20000], Format::Gzip);
    for (data, coding) in [(&br, b"br".as_slice()), (&gz, b"gzip".as_slice())] {
        for abort in [Abort::Cancelled, Abort::DeadlineExceeded] {
            let mut calls = 0;
            let e = content_service::decode(data, &[coding], Limits::default(), &mut || {
                calls += 1;
                if calls >= 5 { Err(abort) } else { Ok(()) }
            })
            .unwrap_err();
            assert_eq!(e.kind, ErrorKind::Aborted(abort));
            assert!(calls >= 5);
        }
    }
    assert_eq!(
        content_service::decode(b"", &[], Limits::default(), &mut || Err(Abort::Cancelled))
            .unwrap_err()
            .kind,
        ErrorKind::Aborted(Abort::Cancelled)
    );
}
#[test]
fn brotli_stored_metadata_and_empty_streams() {
    let plain = vec![42; 100000];
    let d = br_decode(&br_stored(&plain), compression::Limits::default()).unwrap();
    assert_eq!(d.bytes, plain);
    assert_eq!(d.meta_blocks, 3);
    assert_eq!(
        br_decode(&[6], compression::Limits::default())
            .unwrap()
            .bytes,
        b""
    );
    let mut b = Bits::new();
    b.raw(0, 1);
    b.raw(0, 1);
    b.raw(3, 2);
    b.raw(0, 1);
    b.raw(1, 2);
    b.raw(2, 8);
    b.align();
    for v in b"xyz" {
        b.raw(u32::from(*v), 8);
    }
    b.stored(b"abc");
    b.final_empty();
    let d = br_decode(&b.finish(), compression::Limits::default()).unwrap();
    assert_eq!(d.bytes, b"abc");
    assert_eq!(d.meta_blocks, 3);
}
#[test]
fn brotli_invalid_window_padding_and_meta_block_encodings() {
    assert_eq!(
        br_decode(&[0x11], compression::Limits::default())
            .unwrap_err()
            .kind,
        brotli::ErrorKind::InvalidWindow
    );
    assert_eq!(
        br_decode(&[0x86], compression::Limits::default())
            .unwrap_err()
            .kind,
        brotli::ErrorKind::InvalidPadding
    );
    let mut b = Bits::new();
    b.raw(0, 1);
    b.raw(1, 1);
    b.raw(0, 1);
    b.raw(1, 2);
    b.raw(0, 20);
    assert_eq!(
        br_decode(&b.finish(), compression::Limits::default())
            .unwrap_err()
            .kind,
        brotli::ErrorKind::InvalidMetaBlock
    );
    let mut b = Bits::new();
    b.raw(0, 1);
    b.raw(0, 1);
    b.raw(3, 2);
    b.raw(1, 1);
    assert_eq!(
        br_decode(&b.finish(), compression::Limits::default())
            .unwrap_err()
            .kind,
        brotli::ErrorKind::InvalidMetaBlock
    );
    let mut b = Bits::new();
    b.raw(0, 1);
    b.raw(0, 1);
    b.raw(3, 2);
    b.raw(0, 1);
    b.raw(2, 2);
    b.raw(1, 16);
    assert_eq!(
        br_decode(&b.finish(), compression::Limits::default())
            .unwrap_err()
            .kind,
        brotli::ErrorKind::InvalidMetaBlock
    );
}
#[test]
fn brotli_duplicate_huffman_symbols_are_invalid() {
    let mut b = Bits::new();
    literal_prefix(&mut b, 1);
    b.simple(&[65, 65], 8);
    assert_eq!(
        br_decode(&b.finish(), compression::Limits::default())
            .unwrap_err()
            .kind,
        brotli::ErrorKind::InvalidSymbol
    );
}
#[test]
fn brotli_metablock_limit_has_its_own_classification() {
    let p = br_stored(b"abc");
    assert_eq!(
        brotli::decode(
            &p,
            compression::Limits::default(),
            1,
            TrailingData::Reject,
            &mut || Ok(())
        )
        .unwrap_err()
        .kind,
        brotli::ErrorKind::MetaBlockLimit
    );
    assert!(
        brotli::decode(
            &p,
            compression::Limits::default(),
            2,
            TrailingData::Reject,
            &mut || Ok(())
        )
        .is_ok()
    );
}
#[test]
fn brotli_limits_and_trailing_allow_use_consumed_input_only() {
    let mut p = br_repeat(20000);
    let used = p.len();
    p.extend_from_slice(&vec![0; 10000]);
    let mut limits = compression::Limits::default();
    limits.max_input_bytes = used as u64;
    assert_eq!(
        brotli::decode(&p, limits, 10, TrailingData::Allow, &mut || Ok(()))
            .unwrap()
            .consumed,
        used
    );
    limits.max_expansion_ratio = 1;
    limits.expansion_slack_bytes = 0;
    assert_eq!(
        brotli::decode(&p, limits, 10, TrailingData::Allow, &mut || Ok(()))
            .unwrap_err()
            .kind,
        brotli::ErrorKind::LimitExceeded(Limit::Expansion)
    );
    limits = compression::Limits::default();
    limits.max_work = 0;
    assert_eq!(
        br_decode(&p[..used], limits).unwrap_err().kind,
        brotli::ErrorKind::LimitExceeded(Limit::Work)
    );
}
#[test]
fn brotli_all_byte_truncations_fail_and_random_input_never_panics() {
    let fixtures = [br_repeat(20000), br_stored(b"synthetic HTTP body")];
    for p in fixtures {
        for n in 0..p.len() {
            assert_eq!(
                br_decode(&p[..n], compression::Limits::default())
                    .unwrap_err()
                    .kind,
                brotli::ErrorKind::Truncated
            );
        }
    }
    let mut state = 0x7932u64;
    for i in 0..3000 {
        let mut p = vec![0; (i % 97) as usize];
        for v in &mut p {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *v = state as u8;
        }
        let l = compression::Limits {
            max_input_bytes: 128,
            max_output_bytes: 4096,
            max_work: 40000,
            max_allocation_bytes: 256000,
            ..compression::Limits::default()
        };
        if let Ok(d) = br_decode(&p, l) {
            assert_eq!(d.consumed, p.len());
            assert!(d.bytes.len() <= 4096);
            assert!(d.work <= 40000);
        }
    }
}

fn wire_response(body: &[u8], coding: &str, chunked: bool) -> Vec<u8> {
    let mut wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: {coding}\r\n"
    )
    .into_bytes();
    if chunked {
        wire.extend_from_slice(b"Transfer-Encoding: chunked\r\nTrailer: X-Synthetic\r\n\r\n");
        for chunk in body.chunks(7) {
            wire.extend_from_slice(format!("{:x};test=1\r\n", chunk.len()).as_bytes());
            wire.extend_from_slice(chunk);
            wire.extend_from_slice(b"\r\n");
        }
        wire.extend_from_slice(b"0\r\nX-Synthetic: complete\r\n\r\n");
    } else {
        wire.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        wire.extend_from_slice(body);
    }
    wire
}
fn parser() -> http1::ResponseDecoder {
    let limits = http1::Limits::default();
    let headers = [http1::Header {
        name: b"Host".to_vec(),
        value: b"127.0.0.1".to_vec(),
    }];
    let context =
        http1::RequestContext::new(b"GET", http1::Version::Http11, &headers, limits).unwrap();
    http1::ResponseDecoder::new(context, limits).unwrap()
}
fn parse_wire(wire: &[u8], chunk: usize) -> Result<http1::Response, http1::Error> {
    let mut p = parser();
    for part in wire.chunks(chunk) {
        let mut pos = 0;
        while pos < part.len() {
            let r = p.feed(&part[pos..])?;
            pos += r.consumed;
            if r.status == http1::DecodeStatus::Complete {
                assert_eq!(pos, part.len());
                break;
            }
            assert!(r.consumed > 0);
        }
    }
    p.finish()?;
    p.into_response()
}
fn decode_response(
    response: &http1::Response,
    limits: Limits,
) -> Result<content_service::DecodedBody, content_service::Error> {
    let fields = response
        .head
        .headers
        .iter()
        .filter(|h| h.is(b"content-encoding"))
        .map(|h| h.value.as_slice())
        .collect::<Vec<_>>();
    decode(&response.body, &fields, limits)
}
#[test]
fn real_http_response_fixtures_decode_after_framing_at_all_split_sizes() {
    let plain = b"{\"synthetic\":true,\"amount\":123456789}";
    let gz = packed(plain, Format::Gzip);
    let br = br_stored(&gz);
    for chunked in [false, true] {
        for size in [1, 2, 3, 7, 31, 4096] {
            let wire = wire_response(&br, "gzip, br", chunked);
            let r = parse_wire(&wire, size).unwrap();
            assert_eq!(r.body, br);
            assert_eq!(r.wire_bytes, wire.len() as u64);
            let d = decode_response(&r, Limits::default()).unwrap();
            assert_eq!(d.bytes, plain);
            if chunked {
                assert_eq!(r.trailers.len(), 1);
                assert_eq!(r.framing, http1::Framing::Chunked);
            } else {
                assert_eq!(r.framing, http1::Framing::ContentLength(br.len() as u64));
            }
        }
    }
}
#[test]
fn encoded_length_is_never_compared_to_decoded_length() {
    let plain = vec![b'A'; 20000];
    let body = packed(&plain, Format::Gzip);
    assert!(body.len() < plain.len());
    let r = parse_wire(&wire_response(&body, "gzip", false), 1).unwrap();
    assert_eq!(r.framing, http1::Framing::ContentLength(body.len() as u64));
    assert_eq!(
        decode_response(&r, Limits::default()).unwrap().decoded_len,
        plain.len()
    );
    let mut wire = wire_response(&body, "gzip", false);
    wire.pop();
    assert_eq!(parse_wire(&wire, 11).unwrap_err(), http1::Error::Truncated);
}
#[test]
fn synthetic_tcp_server_exercises_actual_http_and_content_codecs() {
    let plain = b"{\"synthetic\":true,\"coins\":42}";
    let gz = packed(plain, Format::Gzip);
    let z = packed(plain, Format::Zlib);
    let br = br_stored(plain);
    let mut corrupt = gz.clone();
    let index = corrupt.len() - 8;
    corrupt[index] ^= 1;
    for (body, coding, chunked, success) in [
        (&gz, "gzip", false, true),
        (&z, "deflate", true, true),
        (&br, "br", true, true),
        (&corrupt, "gzip", false, false),
    ] {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let wire = wire_response(body, coding, chunked);
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 128];
            while !request.windows(4).any(|v| v == b"\r\n\r\n") {
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
                assert!(request.len() < 4096);
            }
            assert!(request.starts_with(b"GET /synthetic HTTP/1.1\r\n"));
            for part in wire.chunks(11) {
                socket.write_all(part).unwrap();
            }
        });
        let mut client = TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        client.write_all(b"GET /synthetic HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept-Encoding: gzip, deflate, br\r\n\r\n").unwrap();
        let mut p = parser();
        let mut buf = [0; 13];
        loop {
            let n = client.read(&mut buf).unwrap();
            if n == 0 {
                p.finish().unwrap();
                break;
            }
            let mut pos = 0;
            while pos < n {
                let r = p.feed(&buf[pos..n]).unwrap();
                pos += r.consumed;
                assert!(r.consumed > 0);
            }
            if p.response().is_some() {
                break;
            }
        }
        server.join().unwrap();
        let response = p.into_response().unwrap();
        let result = decode_response(&response, Limits::default());
        if success {
            assert_eq!(result.unwrap().bytes, plain);
        } else {
            assert!(matches!(
                result.unwrap_err().kind,
                ErrorKind::Compression(compression::ErrorKind::ChecksumMismatch { .. })
            ));
        }
    }
}
