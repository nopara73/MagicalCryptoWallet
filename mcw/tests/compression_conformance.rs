//! Actual-source tests: usable with rustc --test before mcw module registration.
#![forbid(unsafe_code)]
#[path = "../src/compression.rs"]
mod compression;
use compression::*;

fn options(format: Format) -> DecodeOptions<'static> {
    DecodeOptions::new(format)
}
fn encoded(bytes: &[u8], format: Format, method: EncodeMethod) -> Vec<u8> {
    let mut o = EncodeOptions::new(format);
    o.method = method;
    encode(bytes, o).unwrap()
}
fn failure(bytes: &[u8], format: Format) -> ErrorKind {
    decode(bytes, options(format)).unwrap_err().kind
}

struct Bits {
    bytes: Vec<u8>,
    bits: u64,
    n: u8,
}
impl Bits {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bits: 0,
            n: 0,
        }
    }
    fn raw(&mut self, value: u32, n: u8) {
        self.bits |= u64::from(value) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.bytes.push(self.bits as u8);
            self.bits >>= 8;
            self.n -= 8;
        }
    }
    fn code(&mut self, code: u32, n: u8) {
        self.raw(code.reverse_bits() >> (32 - n), n);
    }
    fn fixed(&mut self, s: u32) {
        match s {
            0..=143 => self.code(s + 48, 8),
            144..=255 => self.code(s + 256, 9),
            256..=279 => self.code(s - 256, 7),
            _ => self.code(s - 88, 8),
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.bytes.push(self.bits as u8);
        }
        self.bytes
    }
}

fn literal_dynamic(lengths: &[u8], data: &[u8]) -> Vec<u8> {
    assert_eq!(lengths.len(), 258);
    let order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1];
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(14 << 10, 14);
    // Complete code-length tree {18:0, 0:10, 1:11}.
    for s in order {
        b.raw(
            match s {
                18 => 1,
                0 | 1 => 2,
                _ => 0,
            },
            3,
        );
    }
    for &len in lengths {
        b.code(if len == 0 { 2 } else { 3 }, 2);
    }
    for &v in data {
        assert_eq!(v, b'A');
        b.raw(0, 1);
    }
    b.raw(1, 1);
    b.finish()
}

fn drive(
    bytes: &[u8],
    options: DecodeOptions<'_>,
    input_chunk: usize,
    output_chunk: usize,
) -> Result<(Vec<u8>, usize, u64), Error> {
    let mut d = Decoder::new(options)?;
    let mut pos = 0;
    let mut fed = input_chunk.min(bytes.len());
    let mut result = Vec::new();
    for _ in 0..10_000_000 {
        let mut out = vec![0; output_chunk];
        let p = d.process(&bytes[pos..fed], &mut out, fed == bytes.len())?;
        pos += p.consumed;
        result.extend_from_slice(&out[..p.written]);
        match p.status {
            Status::Finished => return Ok((result, pos, d.total_work())),
            Status::NeedInput => {
                assert!(fed < bytes.len(), "unexpected final NeedInput");
                fed = (fed + input_chunk).min(bytes.len());
            }
            Status::NeedOutput => assert!(output_chunk > 0),
        }
    }
    panic!("decoder made no bounded progress")
}

#[test]
fn primary_checksum_vectors_and_incremental_checksums() {
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    assert_eq!(crc32(b""), 0);
    assert_eq!(adler32(b"Wikipedia"), 0x11e60398);
    assert_eq!(adler32(b""), 1);
    let mut crc = Crc32::default();
    let mut adler = Adler32::default();
    for c in b"123456789".chunks(2) {
        crc.update(c);
        adler.update(c);
    }
    assert_eq!(crc.value(), crc32(b"123456789"));
    assert_eq!(adler.value(), adler32(b"123456789"));
}

#[test]
fn stored_and_fixed_known_streams() {
    assert_eq!(
        decode(
            &[1, 3, 0, 252, 255, b'a', b'b', b'c'],
            options(Format::Deflate)
        )
        .unwrap()
        .bytes,
        b"abc"
    );
    assert_eq!(
        decode(&[3, 0], options(Format::Deflate)).unwrap().bytes,
        b""
    );
    let z = [
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0, 0x06, 0x2c, 0x02, 0x15,
    ];
    assert_eq!(decode(&z, options(Format::Zlib)).unwrap().bytes, b"hello");
}

#[test]
fn rfc_bit_order_and_overlapping_copy() {
    // RFC 1951 section 3.2.3: XY + length 5/distance 2 => XYXYXYX.
    let mut b = Bits::new();
    b.raw(3, 3);
    b.fixed(b'X' as u32);
    b.fixed(b'Y' as u32);
    b.fixed(259);
    b.code(1, 5);
    b.fixed(256);
    assert_eq!(
        decode(&b.finish(), options(Format::Deflate)).unwrap().bytes,
        b"XYXYXYX"
    );
}

#[test]
fn dynamic_literals_with_no_distance_tree() {
    let mut lengths = [0; 258];
    lengths[b'A' as usize] = 1;
    lengths[256] = 1;
    let bytes = literal_dynamic(&lengths, b"AAA");
    assert_eq!(
        decode(&bytes, options(Format::Deflate)).unwrap().bytes,
        b"AAA"
    );
    assert_eq!(
        drive(&bytes, options(Format::Deflate), 1, 1).unwrap().0,
        b"AAA"
    );
}

#[test]
fn dynamic_repeats_cross_literal_distance_boundary() {
    // HLIT=258, HDIST=2. Repeat 16 crosses from literal 257 to distances 0/1.
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(1 | (1 << 5) | (14 << 10), 14);
    // Code alphabet 0,1,16,18 all length 2; canonical codes 00,01,10,11.
    for s in [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1] {
        b.raw(if [0, 1, 16, 18].contains(&s) { 2 } else { 0 }, 3);
    }
    b.code(3, 2);
    b.raw(127, 7); // 138 zeros
    b.code(3, 2);
    b.raw(107, 7); // 118 zeros
    b.code(1, 2);
    b.code(2, 2);
    b.raw(0, 2);
    b.raw(0, 1); // length1, repeat3, EOB
    assert_eq!(
        decode(&b.finish(), options(Format::Deflate)).unwrap().bytes,
        b""
    );

    // Repeat 18 crosses unused literals 258/259 into nine zero distances.
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(3 | 8 << 5 | 14 << 10, 14);
    for s in [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1] {
        b.raw(
            match s {
                18 => 1,
                0 | 1 => 2,
                _ => 0,
            },
            3,
        );
    }
    b.code(0, 1);
    b.raw(127, 7); // 138
    b.code(0, 1);
    b.raw(107, 7); // 118, to index256
    b.code(3, 2);
    b.code(3, 2); // EOB and literal/length257 both length1
    b.code(0, 1);
    b.raw(0, 7); // eleven zeros
    b.raw(0, 1);
    assert!(
        decode(&b.finish(), options(Format::Deflate))
            .unwrap()
            .bytes
            .is_empty()
    );
}

#[test]
fn block_boundaries_retain_history() {
    let mut b = Bits::new();
    b.raw(2, 3);
    b.fixed(b'a' as u32);
    b.fixed(256);
    b.raw(3, 3);
    b.fixed(285);
    b.code(0, 5);
    b.fixed(256);
    assert_eq!(
        decode(&b.finish(), options(Format::Deflate)).unwrap().bytes,
        vec![b'a'; 259]
    );
}

#[test]
fn all_formats_methods_and_stored_block_boundaries() {
    for n in [0, 1, 258, 32768, 65535, 65536, 131071] {
        let data: Vec<_> = (0..n).map(|i| (i % 251) as u8).collect();
        for f in [Format::Deflate, Format::Zlib, Format::Gzip] {
            for m in [EncodeMethod::Stored, EncodeMethod::Fixed] {
                let enc = encoded(&data, f, m);
                let dec = decode(&enc, options(f)).unwrap();
                assert_eq!(dec.bytes, data);
                assert_eq!(dec.consumed, enc.len());
                assert_eq!(dec.members, 1);
            }
        }
    }
}

#[test]
fn fixed_encoder_compresses_runs_and_overlapping_patterns() {
    for data in [
        vec![b'X'; 90000],
        b"xy".repeat(40000),
        b"abcde".repeat(10000),
    ] {
        let enc = encoded(&data, Format::Deflate, EncodeMethod::Fixed);
        assert!(enc.len() < data.len() / 10);
        assert_eq!(decode(&enc, options(Format::Deflate)).unwrap().bytes, data);
    }
}

#[test]
fn truncation_at_every_byte_and_poisoned_decoder() {
    for f in [Format::Deflate, Format::Zlib, Format::Gzip] {
        for m in [EncodeMethod::Stored, EncodeMethod::Fixed] {
            let bytes = encoded(b"hello world hello world", f, m);
            for n in 0..bytes.len() {
                assert_eq!(
                    failure(&bytes[..n], f),
                    ErrorKind::Truncated,
                    "{f:?}/{m:?}/{n}"
                );
            }
        }
    }
    let mut d = Decoder::new(options(Format::Deflate)).unwrap();
    let e = d.process(&[7], &mut [0; 20], true).unwrap_err();
    assert_eq!(e.kind, ErrorKind::ReservedBlock);
    assert_eq!(d.process(&[3, 0], &mut [], true).unwrap_err(), e);
}

#[test]
fn exact_consumption_and_trailing_policies() {
    for f in [Format::Deflate, Format::Zlib, Format::Gzip] {
        let mut b = encoded(b"x", f, EncodeMethod::Fixed);
        let len = b.len();
        b.extend_from_slice(b"tail");
        let e = decode(&b, options(f)).unwrap_err();
        assert_eq!(e.kind, ErrorKind::TrailingData);
        assert_eq!(e.input_consumed, len as u64);
        let mut o = options(f);
        o.trailing_data = TrailingData::Allow;
        let result = decode(&b, o).unwrap();
        assert_eq!(result.consumed, len);
        assert_eq!(result.bytes, b"x");
    }
}

#[test]
fn fragmented_streaming_has_identical_work_and_consumption() {
    for f in [Format::Deflate, Format::Zlib, Format::Gzip] {
        let data = b"a repetitive phrase 123 ".repeat(300);
        let bytes = encoded(&data, f, EncodeMethod::Fixed);
        let baseline = drive(&bytes, options(f), bytes.len(), data.len()).unwrap();
        for ic in [1, 2, 3, 7, 31] {
            for oc in [1, 2, 17, 258] {
                let actual = drive(&bytes, options(f), ic, oc).unwrap();
                assert_eq!(actual, baseline);
            }
        }
    }
}

#[test]
fn zero_output_buffers_and_empty_final_call() {
    let bytes = encoded(b"xy", Format::Zlib, EncodeMethod::Fixed);
    let mut d = Decoder::new(options(Format::Zlib)).unwrap();
    let p = d.process(&bytes, &mut [], false).unwrap();
    assert_eq!(p.status, Status::NeedOutput);
    let mut out = [0; 8];
    let q = d.process(&bytes[p.consumed..], &mut out, false).unwrap();
    assert_eq!(&out[..q.written], b"xy");
    assert_eq!(q.status, Status::NeedInput);
    assert_eq!(
        d.process(&[], &mut [], true).unwrap().status,
        Status::Finished
    );
    assert_eq!(d.total_output(), 2);
    assert_eq!(d.total_input(), bytes.len() as u64);
    assert_eq!(
        d.process(b"tail", &mut [], true).unwrap_err().kind,
        ErrorKind::TrailingData
    );
}

#[test]
fn gzip_concatenation_empty_members_and_first_member_policy() {
    let mut bytes = encoded(b"one", Format::Gzip, EncodeMethod::Fixed);
    let len = bytes.len();
    bytes.extend(encoded(b"", Format::Gzip, EncodeMethod::Stored));
    bytes.extend(encoded(b"two", Format::Gzip, EncodeMethod::Fixed));
    let d = decode(&bytes, options(Format::Gzip)).unwrap();
    assert_eq!(d.bytes, b"onetwo");
    assert_eq!(d.members, 3);
    assert_eq!(
        drive(&bytes, options(Format::Gzip), 1, 1).unwrap().0,
        b"onetwo"
    );
    let mut o = options(Format::Gzip);
    o.gzip_members = GzipMembers::First;
    assert_eq!(decode(&bytes, o).unwrap_err().kind, ErrorKind::TrailingData);
    o.trailing_data = TrailingData::Allow;
    let d = decode(&bytes, o).unwrap();
    assert_eq!(d.bytes, b"one");
    assert_eq!(d.consumed, len);
    assert_eq!(d.members, 1);
}

fn metadata_gzip() -> Vec<u8> {
    let ordinary = encoded(b"metadata", Format::Gzip, EncodeMethod::Fixed);
    let mut header = vec![0x1f, 0x8b, 8, 31, 0, 0, 0, 0, 0, 255]; // FTEXT + all options
    header.extend_from_slice(&[5, 0, b'X', b'Y', 1, 0, b'z']);
    header.extend_from_slice(b"a-name\0a-comment\0");
    header.extend_from_slice(&(crc32(&header) as u16).to_le_bytes());
    header.extend_from_slice(&ordinary[10..]);
    header
}

#[test]
fn gzip_all_header_options_and_header_checksum() {
    let bytes = metadata_gzip();
    assert_eq!(
        drive(&bytes, options(Format::Gzip), 1, 1).unwrap().0,
        b"metadata"
    );
    let mut bad = bytes.clone();
    bad[18] ^= 1;
    assert!(matches!(
        failure(&bad, Format::Gzip),
        ErrorKind::ChecksumMismatch { .. }
    ));
}

#[test]
fn wrapper_checksum_size_method_and_reserved_flag_errors() {
    let mut z = encoded(b"abc", Format::Zlib, EncodeMethod::Fixed);
    *z.last_mut().unwrap() ^= 1;
    assert!(matches!(
        failure(&z, Format::Zlib),
        ErrorKind::ChecksumMismatch { .. }
    ));
    for index in [0, 2, 3] {
        let mut g = encoded(b"abc", Format::Gzip, EncodeMethod::Fixed);
        g[index] = if index == 3 { 32 } else { 0 };
        assert_eq!(failure(&g, Format::Gzip), ErrorKind::InvalidGzipHeader);
    }
    let mut g = encoded(b"abc", Format::Gzip, EncodeMethod::Fixed);
    let end = g.len();
    g[end - 8] ^= 1;
    assert!(matches!(
        failure(&g, Format::Gzip),
        ErrorKind::ChecksumMismatch { .. }
    ));
    let mut g = encoded(b"abc", Format::Gzip, EncodeMethod::Fixed);
    let end = g.len();
    g[end - 4] ^= 1;
    assert!(matches!(
        failure(&g, Format::Gzip),
        ErrorKind::GzipSizeMismatch { .. }
    ));
    for head in [[0x78, 0], [0x79, 0], [0x88, 0], [0x70, 0]] {
        assert_eq!(failure(&head, Format::Zlib), ErrorKind::InvalidZlibHeader);
    }
}

#[test]
fn malformed_blocks_trees_repeats_and_distances() {
    assert_eq!(failure(&[7], Format::Deflate), ErrorKind::ReservedBlock);
    assert_eq!(
        failure(&[1, 1, 0, 255, 255], Format::Deflate),
        ErrorKind::InvalidStoredLength
    );
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(31, 14);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::InvalidCodeLength
    );
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(0, 14);
    for n in [1, 1, 1, 0] {
        b.raw(n, 3);
    }
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::OversubscribedTree
    );
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(0, 14);
    for n in [0, 0, 0, 2] {
        b.raw(n, 3);
    }
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::IncompleteTree
    );
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(0, 14);
    for n in [1, 0, 0, 1] {
        b.raw(n, 3);
    }
    b.raw(1, 1);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::InvalidRepeat
    );
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(0, 14);
    for n in [0, 0, 1, 1] {
        b.raw(n, 3);
    }
    b.raw(1, 1);
    b.raw(127, 7);
    b.raw(1, 1);
    b.raw(127, 7);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::InvalidRepeat
    );
    let mut lengths = [0; 258];
    lengths[b'A' as usize] = 1;
    assert_eq!(
        failure(&literal_dynamic(&lengths, b""), Format::Deflate),
        ErrorKind::MissingEndOfBlock
    );
    lengths[b'B' as usize] = 1;
    lengths[256] = 1;
    assert_eq!(
        failure(&literal_dynamic(&lengths, b""), Format::Deflate),
        ErrorKind::OversubscribedTree
    );
    let mut b = Bits::new();
    b.raw(3, 3);
    b.fixed(286);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::ReservedSymbol
    );
    let mut b = Bits::new();
    b.raw(3, 3);
    b.fixed(257);
    b.code(30, 5);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::ReservedSymbol
    );
    let mut b = Bits::new();
    b.raw(3, 3);
    b.fixed(257);
    b.code(0, 5);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::InvalidDistance
    );
}

#[test]
fn preset_dictionary_raw_and_zlib_policies() {
    let mut b = Bits::new();
    b.raw(3, 3);
    b.fixed(257);
    b.code(2, 5);
    b.fixed(256);
    let raw = b.finish();
    let dict = b"abc";
    let mut o = options(Format::Deflate);
    o.dictionary = Dictionary::Use(dict);
    assert_eq!(decode(&raw, o).unwrap().bytes, dict);
    assert_eq!(failure(&raw, Format::Deflate), ErrorKind::InvalidDistance);
    let mut z = vec![0x78, 0x20];
    z.extend_from_slice(&adler32(dict).to_be_bytes());
    z.extend(&raw);
    z.extend_from_slice(&adler32(dict).to_be_bytes());
    assert!(matches!(
        failure(&z, Format::Zlib),
        ErrorKind::DictionaryRequired { .. }
    ));
    o.format = Format::Zlib;
    assert_eq!(drive(&z, o, 1, 1).unwrap().0, dict);
    o.dictionary = Dictionary::Use(b"xyz");
    assert!(matches!(
        decode(&z, o).unwrap_err().kind,
        ErrorKind::DictionaryMismatch { .. }
    ));
    o.format = Format::Gzip;
    assert_eq!(
        Decoder::new(o).err().unwrap().kind,
        ErrorKind::InvalidDictionaryPolicy
    );
    // Supplying a dictionary must not seed a zlib stream without FDICT.
    let mut z = vec![0x78, 0x01];
    z.extend(raw);
    z.extend_from_slice(&adler32(dict).to_be_bytes());
    o.format = Format::Zlib;
    o.dictionary = Dictionary::Use(dict);
    assert_eq!(decode(&z, o).unwrap_err().kind, ErrorKind::InvalidDistance);
}

#[test]
fn zlib_declared_window_restricts_backreferences() {
    let dict: Vec<u8> = (0..300).map(|i| i as u8).collect();
    let mut b = Bits::new();
    b.raw(3, 3);
    b.fixed(257);
    b.code(16, 5);
    b.raw(0, 7);
    b.fixed(256); // distance257
    let mut z = vec![0x08, 0x20];
    let rem = u16::from_be_bytes([z[0], z[1]]) % 31;
    z[1] += ((31 - rem) % 31) as u8;
    z.extend_from_slice(&adler32(&dict).to_be_bytes());
    z.extend(b.finish());
    z.extend_from_slice(&[0; 4]);
    let mut o = options(Format::Zlib);
    o.dictionary = Dictionary::Use(&dict);
    assert_eq!(decode(&z, o).unwrap_err().kind, ErrorKind::InvalidDistance);
}

#[test]
fn decompression_limits_and_no_ratio_credit_from_trailing_data() {
    let z = encoded(&vec![0; 20000], Format::Zlib, EncodeMethod::Fixed);
    let mut o = options(Format::Zlib);
    o.limits.max_input_bytes = (z.len() - 1) as u64;
    assert_eq!(
        decode(&z, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Input)
    );
    o = options(Format::Zlib);
    o.limits.max_output_bytes = 19999;
    assert_eq!(
        decode(&z, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Output)
    );
    o = options(Format::Zlib);
    o.limits.max_expansion_ratio = 1;
    o.limits.expansion_slack_bytes = 0;
    o.trailing_data = TrailingData::Allow;
    let mut padded = z.clone();
    padded.extend(vec![0; 100000]);
    assert_eq!(
        decode(&padded, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Expansion)
    );
    o = options(Format::Zlib);
    o.limits.max_work = 10;
    assert_eq!(
        decode(&z, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Work)
    );
    o = options(Format::Zlib);
    o.limits.max_allocation_bytes = 0;
    assert_eq!(
        Decoder::new(o).err().unwrap().kind,
        ErrorKind::LimitExceeded(Limit::Allocation)
    );
    o = options(Format::Zlib);
    let d = Decoder::new(o).unwrap();
    o.limits.max_allocation_bytes = d.allocated_bytes() + 19999;
    assert_eq!(
        decode(&z, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Allocation)
    );
    o = options(Format::Deflate);
    o.dictionary = Dictionary::Use(b"abcd");
    o.limits.max_dictionary_bytes = 3;
    assert_eq!(
        Decoder::new(o).err().unwrap().kind,
        ErrorKind::LimitExceeded(Limit::Dictionary)
    );
}

#[test]
fn gzip_header_and_member_limits_and_truncated_next_member() {
    let bytes = metadata_gzip();
    let mut o = options(Format::Gzip);
    o.limits.max_gzip_header_bytes = 10;
    assert_eq!(
        decode(&bytes, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::GzipHeader)
    );
    let mut bytes = encoded(b"", Format::Gzip, EncodeMethod::Fixed);
    let one = bytes.len();
    bytes.extend(bytes.clone());
    o = options(Format::Gzip);
    o.limits.max_gzip_members = 1;
    assert_eq!(
        decode(&bytes, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::GzipMembers)
    );
    bytes.truncate(one);
    bytes.push(0x1f);
    o.trailing_data = TrailingData::Allow;
    o.limits.max_gzip_members = 10;
    assert_eq!(decode(&bytes, o).unwrap_err().kind, ErrorKind::Truncated);
    bytes.push(0x8b);
    assert_eq!(decode(&bytes, o).unwrap_err().kind, ErrorKind::Truncated);
}

#[test]
fn encoder_limits_fail_before_unbounded_work() {
    let mut o = EncodeOptions::new(Format::Gzip);
    o.limits.max_input_bytes = 2;
    assert_eq!(
        encode(b"abc", o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Input)
    );
    o = EncodeOptions::new(Format::Gzip);
    o.limits.max_output_bytes = 10;
    assert_eq!(
        encode(b"abc", o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Output)
    );
    o = EncodeOptions::new(Format::Gzip);
    o.limits.max_work = 10;
    assert_eq!(
        encode(b"abc", o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Work)
    );
    o = EncodeOptions::new(Format::Gzip);
    o.limits.max_allocation_bytes = 0;
    assert_eq!(
        encode(b"abc", o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::Allocation)
    );
}

#[test]
fn bounded_arbitrary_bytes_never_panic_or_exceed_output_limit() {
    let mut state = 0x3210_9876u32;
    for n in 0..2500 {
        let mut bytes = vec![0; (n % 128) as usize];
        for b in &mut bytes {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *b = state as u8;
        }
        for f in [Format::Deflate, Format::Zlib, Format::Gzip] {
            let mut o = options(f);
            o.limits.max_output_bytes = 4096;
            o.limits.max_work = 100000;
            if let Ok(d) = decode(&bytes, o) {
                assert!(d.bytes.len() <= 4096);
                assert!(d.consumed <= bytes.len());
            }
        }
    }
}

fn general_dynamic(literals: &[u8], distances: &[u8]) -> Bits {
    let mut b = Bits::new();
    b.raw(5, 3);
    b.raw(
        ((literals.len() - 257) | ((distances.len() - 1) << 5) | (15 << 10)) as u32,
        14,
    );
    // Sixteen code-length symbols (0..15) all length4; 16..18 unused.
    for symbol in [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ] {
        b.raw(if symbol <= 15 { 4 } else { 0 }, 3);
    }
    for &n in literals.iter().chain(distances) {
        b.code(u32::from(n), 4);
    }
    b
}

#[test]
fn maximum_fifteen_bit_literal_and_distance_codes() {
    let mut literals = [0; 257];
    for n in 1..=15 {
        literals[64 + n] = n as u8;
    }
    literals[256] = 15;
    let mut b = general_dynamic(&literals, &[0]);
    for n in 1..=15 {
        b.code((1u32 << n) - 2, n);
    }
    b.code(32767, 15);
    let expected: Vec<u8> = (65..80).collect();
    let raw = b.finish();
    assert_eq!(
        drive(&raw, options(Format::Deflate), 1, 1).unwrap().0,
        expected
    );

    let mut literals = [0; 286];
    literals[256] = 1;
    literals[285] = 1;
    let mut distances = [0; 32];
    for n in 1..=14 {
        distances[n - 1] = n as u8;
    }
    distances[28] = 15;
    distances[29] = 15;
    let mut b = general_dynamic(&literals, &distances);
    b.code(1, 1);
    b.code(32767, 15);
    b.raw(8191, 13);
    b.code(0, 1);
    let raw = b.finish();
    let dict: Vec<u8> = (0..32768).map(|n| (n % 251) as u8).collect();
    let mut o = options(Format::Deflate);
    o.dictionary = Dictionary::Use(&dict);
    assert_eq!(drive(&raw, o, 1, 1).unwrap().0, dict[..258]);
}

#[test]
fn maximum_window_distance_and_wrapped_history() {
    let first: Vec<u8> = (0..32768).map(|n| (n % 251) as u8).collect();
    let mut raw = vec![0, 0, 128, 255, 127]; // nonfinal stored, 32768 bytes
    raw.extend(&first);
    let mut b = Bits::new();
    b.raw(3, 3);
    b.fixed(285);
    b.code(29, 5);
    b.raw(8191, 13);
    b.fixed(256);
    raw.extend(b.finish());
    let mut expected = first.clone();
    expected.extend_from_slice(&first[..258]);
    assert_eq!(
        drive(&raw, options(Format::Deflate), 13, 71).unwrap().0,
        expected
    );
}

#[test]
fn incomplete_literal_tree_and_unused_single_code_path_are_rejected() {
    let mut literals = [0; 257];
    literals[b'A' as usize] = 2;
    literals[256] = 2;
    assert_eq!(
        failure(&general_dynamic(&literals, &[0]).finish(), Format::Deflate),
        ErrorKind::IncompleteTree
    );
    literals[b'A' as usize] = 0;
    literals[256] = 1;
    let mut b = general_dynamic(&literals, &[0]);
    b.raw(1, 1);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::InvalidHuffmanCode
    );
    let mut literals = [0; 258];
    literals[256] = 1;
    literals[257] = 1;
    let mut b = general_dynamic(&literals, &[0]);
    b.raw(1, 1);
    assert_eq!(
        failure(&b.finish(), Format::Deflate),
        ErrorKind::InvalidHuffmanCode
    );
}

#[test]
fn gzip_members_cannot_borrow_expansion_credit_and_header_limit_is_exact() {
    let mut bytes = encoded(&vec![b'a'; 1000], Format::Gzip, EncodeMethod::Stored);
    bytes.extend(encoded(
        &vec![b'b'; 1000],
        Format::Gzip,
        EncodeMethod::Fixed,
    ));
    let mut o = options(Format::Gzip);
    o.limits.max_expansion_ratio = 2;
    o.limits.expansion_slack_bytes = 0;
    let error = decode(&bytes, o).unwrap_err();
    assert_eq!(error.kind, ErrorKind::LimitExceeded(Limit::Expansion));
    assert!(error.output_produced >= 1000 && error.output_produced < 2000);
    let bytes = metadata_gzip();
    let ordinary = encoded(b"metadata", Format::Gzip, EncodeMethod::Fixed);
    let header_size = bytes.len() - ordinary.len() + 10;
    o = options(Format::Gzip);
    o.limits.max_gzip_header_bytes = header_size as u64;
    assert_eq!(decode(&bytes, o).unwrap().bytes, b"metadata");
    o.limits.max_gzip_header_bytes -= 1;
    assert_eq!(
        decode(&bytes, o).unwrap_err().kind,
        ErrorKind::LimitExceeded(Limit::GzipHeader)
    );
}
