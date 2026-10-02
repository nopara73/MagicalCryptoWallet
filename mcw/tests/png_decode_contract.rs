//! Std-only public API tests, compiled with rustc --test until host integration.
#[allow(dead_code)]
#[path = "../src/compression.rs"]
mod compression;
#[allow(dead_code)]
#[path = "../src/png.rs"]
mod png;

use png::decode::{DecodeError, DecodeLimit, DecodeLimits, PixelFormat, decode};

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = compression::crc32(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn header(width: u32, height: u32, depth: u8, color: u8, interlace: u8) -> [u8; 13] {
    let mut h = [0; 13];
    h[..4].copy_from_slice(&width.to_be_bytes());
    h[4..8].copy_from_slice(&height.to_be_bytes());
    h[8] = depth;
    h[9] = color;
    h[12] = interlace;
    h
}

fn zlib(raw: &[u8]) -> Vec<u8> {
    compression::encode(
        raw,
        compression::EncodeOptions::new(compression::Format::Zlib),
    )
    .unwrap()
}

fn png(h: [u8; 13], before: &[Vec<u8>], compressed: &[u8], after: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &h));
    for extra in before {
        out.extend_from_slice(extra);
    }
    out.extend(chunk(b"IDAT", compressed));
    for extra in after {
        out.extend_from_slice(extra);
    }
    out.extend(chunk(b"IEND", &[]));
    out
}

fn valid() -> Vec<u8> {
    png(
        header(2, 2, 8, 0, 0),
        &[],
        &zlib(&[0, 12, 250, 0, 19, 100]),
        &[],
    )
}

#[test]
fn decoder_roundtrips_published_encoder_with_exact_orientation() {
    let modules = [1, 0, 0, 1, 1, 0];
    let matrix = png::QrMatrix::new(3, 2, &modules).unwrap();
    for depth in [png::BitDepth::Monochrome1, png::BitDepth::Grayscale8] {
        let encoded = png::encode(matrix, png::Layout::Scale(3), depth).unwrap();
        let image = decode(&encoded, DecodeLimits::default()).unwrap();
        assert_eq!(
            (image.width, image.height, image.format),
            (33, 30, PixelFormat::Gray8)
        );
        for y in 0..30usize {
            for x in 0..33usize {
                let black = (12..21).contains(&x)
                    && (12..18).contains(&y)
                    && modules[(y - 12) / 3 * 3 + (x - 12) / 3] == 1;
                assert_eq!(image.pixels[y * 33 + x], if black { 0 } else { 255 });
            }
        }
    }
}

#[test]
fn packed_grayscale_depths_expand_exactly() {
    for (depth, source, expected) in [
        (1, 0b0100_0000, vec![0, 255]),
        (2, 0b0110_0000, vec![85, 170]),
        (4, 0x1e, vec![17, 238]),
    ] {
        let bytes = png(header(2, 1, depth, 0, 0), &[], &zlib(&[0, source]), &[]);
        assert_eq!(
            decode(&bytes, DecodeLimits::default()).unwrap().pixels,
            expected
        );
    }
}

#[test]
fn sixteen_bit_transparency_is_compared_before_reduction() {
    let key = chunk(b"tRNS", &[0x12, 0x34]);
    let bytes = png(
        header(2, 1, 16, 0, 0),
        &[key],
        &zlib(&[0, 0x12, 0x34, 0x12, 0x56]),
        &[],
    );
    let image = decode(&bytes, DecodeLimits::default()).unwrap();
    assert_eq!(image.format, PixelFormat::Rgba8);
    assert_eq!(image.pixels, [18, 18, 18, 0, 18, 18, 18, 255]);
    let key = chunk(b"tRNS", &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
    let raw = [
        0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0x12, 0x35, 0x56, 0x78, 0x9a, 0xbc,
    ];
    let bytes = png(header(2, 1, 16, 2, 0), &[key], &zlib(&raw), &[]);
    assert_eq!(
        decode(&bytes, DecodeLimits::default()).unwrap().pixels,
        [0x12, 0x56, 0x9a, 0, 0x12, 0x56, 0x9a, 255]
    );
}

#[test]
fn palette_alpha_and_straight_alpha_are_preserved() {
    let before = [chunk(b"PLTE", &[255, 1, 2, 3, 4, 5]), chunk(b"tRNS", &[0])];
    let bytes = png(
        header(2, 1, 1, 3, 0),
        &before,
        &zlib(&[0, 0b0100_0000]),
        &[],
    );
    let image = decode(&bytes, DecodeLimits::default()).unwrap();
    assert_eq!(image.format, PixelFormat::Rgba8);
    assert_eq!(image.pixels, [255, 1, 2, 0, 3, 4, 5, 255]);
    let bytes = png(
        header(1, 1, 8, 6, 0),
        &[],
        &zlib(&[0, 255, 128, 64, 0]),
        &[],
    );
    assert_eq!(
        decode(&bytes, DecodeLimits::default()).unwrap().pixels,
        [255, 128, 64, 0]
    );
    let bytes = png(
        header(1, 1, 16, 4, 0),
        &[],
        &zlib(&[0, 0x12, 0x34, 0x80, 0x00]),
        &[],
    );
    assert_eq!(
        decode(&bytes, DecodeLimits::default()).unwrap().pixels,
        [18, 18, 18, 128]
    );
}

#[test]
fn crc_signature_chunk_names_and_terminal_data_are_checked() {
    let good = valid();
    let mut bad = good.clone();
    bad[0] = 0;
    assert_eq!(
        decode(&bad, DecodeLimits::default()).unwrap_err(),
        DecodeError::InvalidSignature
    );
    let mut bad = good.clone();
    bad[29] ^= 1;
    assert!(matches!(
        decode(&bad, DecodeLimits::default()),
        Err(DecodeError::ChecksumMismatch { .. })
    ));
    let mut bad = good.clone();
    bad.extend_from_slice(b"extra");
    assert_eq!(
        decode(&bad, DecodeLimits::default()).unwrap_err(),
        DecodeError::TrailingData
    );
    let bad = png(
        header(2, 2, 8, 0, 0),
        &[chunk(b"abcc", &[])],
        &zlib(&[0; 6]),
        &[],
    );
    assert_eq!(
        decode(&bad, DecodeLimits::default()).unwrap_err(),
        DecodeError::InvalidChunkType
    );
    let bad = png(
        header(2, 2, 8, 0, 0),
        &[chunk(b"ABCd", &[])],
        &zlib(&[0; 6]),
        &[],
    );
    assert!(matches!(
        decode(&bad, DecodeLimits::default()),
        Err(DecodeError::UnsupportedCriticalChunk { .. })
    ));
    let bad = png(
        header(2, 2, 8, 0, 0),
        &[chunk(b"acTL", &[0; 8])],
        &zlib(&[0; 6]),
        &[],
    );
    assert_eq!(
        decode(&bad, DecodeLimits::default()).unwrap_err(),
        DecodeError::AnimationUnsupported
    );
    let good_metadata = png(
        header(2, 2, 8, 0, 0),
        &[chunk(b"tEXt", b"Comment\0ignored")],
        &zlib(&[0; 6]),
        &[],
    );
    assert!(decode(&good_metadata, DecodeLimits::default()).is_ok());
}

#[test]
fn chunk_order_and_palette_indices_are_checked() {
    let h = header(1, 1, 8, 3, 0);
    let missing = png(h, &[], &zlib(&[0, 0]), &[]);
    assert_eq!(
        decode(&missing, DecodeLimits::default()).unwrap_err(),
        DecodeError::MissingPalette
    );
    let indexed = png(h, &[chunk(b"PLTE", &[1, 2, 3])], &zlib(&[0, 1]), &[]);
    assert_eq!(
        decode(&indexed, DecodeLimits::default()).unwrap_err(),
        DecodeError::InvalidPaletteIndex { index: 1 }
    );
    let compressed = zlib(&[0, 12, 250, 0, 19, 100]);
    let mut split = b"\x89PNG\r\n\x1a\n".to_vec();
    split.extend(chunk(b"IHDR", &header(2, 2, 8, 0, 0)));
    split.extend(chunk(b"IDAT", &compressed[..1]));
    split.extend(chunk(b"tEXt", b"key\0value"));
    split.extend(chunk(b"IDAT", &compressed[1..]));
    split.extend(chunk(b"IEND", &[]));
    assert!(matches!(
        decode(&split, DecodeLimits::default()),
        Err(DecodeError::InvalidChunkOrder { .. })
    ));
    let duplicate = png(
        header(1, 1, 8, 0, 0),
        &[chunk(b"IHDR", &header(1, 1, 8, 0, 0))],
        &zlib(&[0, 1]),
        &[],
    );
    assert!(matches!(
        decode(&duplicate, DecodeLimits::default()),
        Err(DecodeError::InvalidChunkOrder { .. })
    ));
}

#[test]
fn consecutive_idat_chunks_can_split_any_zlib_byte() {
    let compressed = zlib(&[0, 12, 250, 0, 19, 100]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(chunk(b"IHDR", &header(2, 2, 8, 0, 0)));
    bytes.extend(chunk(b"IDAT", &[]));
    for byte in compressed {
        bytes.extend(chunk(b"IDAT", &[byte]));
    }
    bytes.extend(chunk(b"IEND", &[]));
    assert_eq!(
        decode(&bytes, DecodeLimits::default()).unwrap().pixels,
        [12, 250, 19, 100]
    );
}

#[test]
fn invalid_filters_and_zlib_size_or_checksum_fail_closed() {
    let bad_filter = png(header(1, 1, 8, 0, 0), &[], &zlib(&[5, 1]), &[]);
    assert_eq!(
        decode(&bad_filter, DecodeLimits::default()).unwrap_err(),
        DecodeError::InvalidFilter { filter: 5 }
    );
    let short = png(header(2, 2, 8, 0, 0), &[], &zlib(&[0, 12]), &[]);
    assert_eq!(
        decode(&short, DecodeLimits::default()).unwrap_err(),
        DecodeError::InflatedSizeMismatch {
            expected: 6,
            actual: 2
        }
    );
    let long = png(header(1, 1, 8, 0, 0), &[], &zlib(&[0, 1, 2]), &[]);
    assert!(matches!(
        decode(&long, DecodeLimits::default()),
        Err(DecodeError::Compression(_))
    ));
    let mut compressed = zlib(&[0, 1]);
    *compressed.last_mut().unwrap() ^= 1;
    let checksum = png(header(1, 1, 8, 0, 0), &[], &compressed, &[]);
    assert!(matches!(
        decode(&checksum, DecodeLimits::default()),
        Err(DecodeError::Compression(_))
    ));
    let mut compressed = zlib(&[0, 1]);
    compressed.push(0);
    let trailing = png(header(1, 1, 8, 0, 0), &[], &compressed, &[]);
    assert!(matches!(
        decode(&trailing, DecodeLimits::default()),
        Err(DecodeError::Compression(_))
    ));
}

#[test]
fn limits_are_applied_before_large_allocations_and_bound_inflate_work() {
    let bytes = valid();
    let cases = [
        (
            DecodeLimits {
                max_file_bytes: 1,
                ..DecodeLimits::default()
            },
            DecodeLimit::FileBytes,
        ),
        (
            DecodeLimits {
                max_dimension: 1,
                ..DecodeLimits::default()
            },
            DecodeLimit::Dimension,
        ),
        (
            DecodeLimits {
                max_pixels: 3,
                ..DecodeLimits::default()
            },
            DecodeLimit::Pixels,
        ),
        (
            DecodeLimits {
                max_inflated_bytes: 5,
                ..DecodeLimits::default()
            },
            DecodeLimit::InflatedBytes,
        ),
        (
            DecodeLimits {
                max_allocation_bytes: 1,
                ..DecodeLimits::default()
            },
            DecodeLimit::Allocation,
        ),
        (
            DecodeLimits {
                max_chunks: 2,
                ..DecodeLimits::default()
            },
            DecodeLimit::Chunks,
        ),
    ];
    for (limits, expected) in cases {
        assert_eq!(
            decode(&bytes, limits).unwrap_err(),
            DecodeError::LimitExceeded(expected)
        );
    }
    assert!(matches!(
        decode(
            &bytes,
            DecodeLimits {
                max_compression_work: 1,
                ..DecodeLimits::default()
            }
        ),
        Err(DecodeError::Compression(_))
    ));
    assert_eq!(
        decode(
            &bytes,
            DecodeLimits {
                max_dimension: 0,
                ..DecodeLimits::default()
            }
        )
        .unwrap_err(),
        DecodeError::InvalidLimits
    );
    let huge = png(header(u32::MAX, 1, 8, 0, 0), &[], &zlib(&[0, 0]), &[]);
    assert_eq!(
        decode(&huge, DecodeLimits::default()).unwrap_err(),
        DecodeError::InvalidHeader
    );
}

#[test]
fn every_truncation_and_mutated_prefix_returns_without_panicking() {
    let bytes = valid();
    for length in 0..bytes.len() {
        assert!(decode(&bytes[..length], DecodeLimits::default()).is_err());
    }
    for index in 0..bytes.len() {
        for value in [0, 1, 127, 255] {
            let mut mutated = bytes.clone();
            mutated[index] = value;
            let _ = decode(&mutated, DecodeLimits::default());
        }
    }
    let mut state = 0x1234_5678u32;
    for length in 0..128 {
        let input: Vec<u8> = (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        assert!(decode(&input, DecodeLimits::default()).is_err());
    }
}

#[test]
#[ignore = "decodes independent oracle fixtures supplied through MCW_PNG_DECODE_FIXTURES"]
fn decode_oracle_fixtures() {
    use std::fs;
    use std::path::PathBuf;
    let root =
        PathBuf::from(std::env::var_os("MCW_PNG_DECODE_FIXTURES").expect("set fixture root"));
    let manifest = fs::read_to_string(root.join("fixtures.tsv")).unwrap();
    let mut accepted = 0;
    let mut rejected = 0;
    for line in manifest.lines().skip(1) {
        let item: Vec<&str> = line.split('\t').collect();
        let bytes = fs::read(root.join(format!("{}.png", item[0]))).unwrap();
        let result = decode(&bytes, DecodeLimits::default());
        if item[1] == "reject" {
            assert!(result.is_err(), "{} should be rejected", item[0]);
            rejected += 1;
            continue;
        }
        let image = result.unwrap_or_else(|error| panic!("{}: {error}", item[0]));
        assert_eq!(image.width.to_string(), item[2], "{} width", item[0]);
        assert_eq!(image.height.to_string(), item[3], "{} height", item[0]);
        assert_eq!(
            image.format.channels().to_string(),
            item[4],
            "{} format",
            item[0]
        );
        let expected = fs::read(root.join(format!("{}.pixels", item[0]))).unwrap();
        assert_eq!(image.pixels, expected, "{} pixels", item[0]);
        accepted += 1;
    }
    println!(
        "PNG decoder oracle: {accepted} accepted with exact pixels, {rejected} invalid inputs rejected"
    );
    fs::write(
        root.join("rust-decoder-result.txt"),
        format!("passed\t{accepted}\t{rejected}\n"),
    )
    .unwrap();
}
