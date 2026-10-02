//! Standalone std-only verification until the application host declares png.
//! Run `rustc --edition=2024 --test mcw/tests/png_contract.rs -o <artifact>`.
//! This is a test harness, never a second shipping executable or Cargo package.

#[allow(dead_code)]
#[path = "../src/compression.rs"]
mod compression;

#[allow(dead_code)]
#[path = "../src/png.rs"]
mod png;

use png::{BitDepth, Dimensions, Layout, PngError, QrMatrix};

struct Decoded {
    width: usize,
    height: usize,
    depth: u8,
    raw: Vec<u8>,
    blocks: usize,
}

// A deliberately simple, independent bit-at-a-time CRC reference.
fn crc_reference(input: &[u8]) -> u32 {
    let mut value = !0u32;
    for byte in input {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = if value & 1 == 0 {
                value >> 1
            } else {
                (value >> 1) ^ 0xedb8_8320
            };
        }
    }
    !value
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

fn decode_contract(bytes: &[u8]) -> Decoded {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut cursor = 8;
    let mut chunks = Vec::new();
    let mut header = Vec::new();
    let mut idat = Vec::new();
    while cursor < bytes.len() {
        let length = read_u32(&bytes[cursor..cursor + 4]) as usize;
        assert!(length <= i32::MAX as usize);
        let kind = &bytes[cursor + 4..cursor + 8];
        let end = cursor + 8 + length;
        let data = &bytes[cursor + 8..end];
        assert_eq!(
            read_u32(&bytes[end..end + 4]),
            crc_reference(&bytes[cursor + 4..end])
        );
        chunks.push(kind.to_vec());
        match kind {
            b"IHDR" => header.extend_from_slice(data),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => assert_eq!(length, 0),
            _ => panic!("unexpected chunk"),
        }
        cursor = end + 4;
    }
    assert_eq!(cursor, bytes.len());
    assert_eq!(
        chunks,
        [b"IHDR".to_vec(), b"IDAT".to_vec(), b"IEND".to_vec()]
    );
    assert_eq!(header.len(), 13);
    assert_eq!(&header[9..], &[0, 0, 0, 0]);
    assert!([1, 8].contains(&header[8]));
    assert_eq!(&idat[..2], &[0x78, 0x01]);
    assert_eq!((u32::from(idat[0]) * 256 + u32::from(idat[1])) % 31, 0);
    let mut offset = 2;
    let mut raw = Vec::new();
    let mut blocks = 0;
    loop {
        let final_bit = idat[offset];
        assert!(
            final_bit <= 1,
            "only stored blocks, zero byte-alignment padding"
        );
        let length = u16::from_le_bytes(idat[offset + 1..offset + 3].try_into().unwrap());
        let inverse = u16::from_le_bytes(idat[offset + 3..offset + 5].try_into().unwrap());
        assert_eq!(length ^ inverse, u16::MAX);
        offset += 5;
        raw.extend_from_slice(&idat[offset..offset + usize::from(length)]);
        offset += usize::from(length);
        blocks += 1;
        if final_bit == 1 {
            break;
        }
        assert_eq!(usize::from(length), 65535);
    }
    assert_eq!(
        offset + 4,
        idat.len(),
        "one complete zlib stream, no trailing bytes"
    );
    let (mut a, mut b) = (1u64, 0u64);
    for byte in &raw {
        a = (a + u64::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    assert_eq!(read_u32(&idat[offset..]), ((b << 16) | a) as u32);
    Decoded {
        width: read_u32(&header[0..4]) as usize,
        height: read_u32(&header[4..8]) as usize,
        depth: header[8],
        raw,
        blocks,
    }
}

fn assert_pixels(image: &Decoded, modules: &[u8], width: usize, height: usize, scale: usize) {
    assert_eq!(
        (image.width, image.height),
        ((width + 8) * scale, (height + 8) * scale)
    );
    let row_bytes = if image.depth == 1 {
        image.width.div_ceil(8)
    } else {
        image.width
    };
    let row_length = row_bytes + 1;
    assert_eq!(image.raw.len(), row_length * image.height);
    let quiet = 4 * scale;
    for y in 0..image.height {
        let row = &image.raw[y * row_length..(y + 1) * row_length];
        assert_eq!(row[0], 0);
        for x in 0..image.width {
            let expected = if x < quiet
                || y < quiet
                || x >= image.width - quiet
                || y >= image.height - quiet
            {
                255
            } else if modules[(y - quiet) / scale * width + (x - quiet) / scale] == 1 {
                0
            } else {
                255
            };
            let value = if image.depth == 8 {
                row[x + 1]
            } else if row[x / 8 + 1] & (128 >> (x % 8)) == 0 {
                0
            } else {
                255
            };
            assert_eq!(value, expected, "pixel ({x}, {y})");
        }
        if image.depth == 1 && !image.width.is_multiple_of(8) {
            let unused = 8 - image.width % 8;
            assert_eq!(row[row.len() - 1] & ((1 << unused) - 1), 0);
        }
    }
}

#[test]
fn matrix_validation_is_strict_and_bounded() {
    assert_eq!(QrMatrix::new(0, 1, &[]).unwrap_err(), PngError::EmptyMatrix);
    assert_eq!(QrMatrix::new(1, 0, &[]).unwrap_err(), PngError::EmptyMatrix);
    assert_eq!(
        QrMatrix::new(2, 3, &[0; 5]).unwrap_err(),
        PngError::LengthMismatch {
            expected: 6,
            actual: 5
        }
    );
    assert_eq!(
        QrMatrix::new(1, 1, &[0, 1]).unwrap_err(),
        PngError::LengthMismatch {
            expected: 1,
            actual: 2
        }
    );
    assert_eq!(
        QrMatrix::new(2, 2, &[0, 1, 2, 0]).unwrap_err(),
        PngError::InvalidModule { index: 2, value: 2 }
    );
    assert_eq!(
        QrMatrix::new(1, 1, &[255]).unwrap_err(),
        PngError::InvalidModule {
            index: 0,
            value: 255
        }
    );
    assert!(matches!(
        QrMatrix::new(png::MAX_MODULE_SIDE + 1, 1, &[]),
        Err(PngError::MatrixTooLarge { .. })
    ));
    assert!(matches!(
        QrMatrix::new(1, u32::MAX, &[]),
        Err(PngError::MatrixTooLarge { .. })
    ));
    let modules = vec![0; png::MAX_MODULE_SIDE as usize * png::MAX_MODULE_SIDE as usize];
    let matrix = QrMatrix::new(png::MAX_MODULE_SIDE, png::MAX_MODULE_SIDE, &modules).unwrap();
    assert_eq!(matrix.width(), png::MAX_MODULE_SIDE);
    assert_eq!(matrix.height(), png::MAX_MODULE_SIDE);
    assert_eq!(matrix.modules().len(), modules.len());
}

#[test]
fn exact_scale_and_rectangular_fit_preserve_aspect_ratio() {
    let matrix = QrMatrix::new(3, 2, &[0; 6]).unwrap();
    assert_eq!(
        png::dimensions(matrix, Layout::Scale(3)).unwrap(),
        Dimensions {
            width: 33,
            height: 30,
            scale: 3
        }
    );
    assert_eq!(
        png::dimensions(
            matrix,
            Layout::FitWithin {
                width: 512,
                height: 300
            }
        )
        .unwrap(),
        Dimensions {
            width: 330,
            height: 300,
            scale: 30
        }
    );
    assert_eq!(
        png::dimensions(
            matrix,
            Layout::AtLeast {
                width: 512,
                height: 300
            }
        )
        .unwrap(),
        Dimensions {
            width: 517,
            height: 470,
            scale: 47
        }
    );
    assert_eq!(
        png::dimensions(
            matrix,
            Layout::AtLeast {
                width: 1,
                height: 1
            }
        )
        .unwrap()
        .scale,
        1
    );
}

#[test]
fn default_export_keeps_512_minimum_and_four_module_margins() {
    for (side, pixels, scale) in [(21, 522, 18), (177, 555, 3)] {
        let modules = vec![1; side * side];
        let matrix = QrMatrix::new(side as u32, side as u32, &modules).unwrap();
        assert_eq!(
            png::dimensions(matrix, Layout::default()).unwrap(),
            Dimensions {
                width: pixels,
                height: pixels,
                scale
            }
        );
        let encoded = png::encode(matrix, Layout::default(), BitDepth::default()).unwrap();
        assert_pixels(
            &decode_contract(&encoded),
            &modules,
            side,
            side,
            scale as usize,
        );
    }
}

#[test]
fn invalid_layouts_return_specific_errors() {
    let matrix = QrMatrix::new(1, 1, &[1]).unwrap();
    assert_eq!(
        png::dimensions(matrix, Layout::Scale(0)).unwrap_err(),
        PngError::InvalidScale
    );
    for layout in [
        Layout::FitWithin {
            width: 0,
            height: 9,
        },
        Layout::AtLeast {
            width: 9,
            height: 0,
        },
    ] {
        assert!(matches!(
            png::dimensions(matrix, layout),
            Err(PngError::InvalidCanvas { .. })
        ));
    }
    assert_eq!(
        png::dimensions(
            matrix,
            Layout::FitWithin {
                width: 8,
                height: 9
            }
        )
        .unwrap_err(),
        PngError::CanvasTooSmall {
            width: 8,
            height: 9,
            minimum_width: 9,
            minimum_height: 9
        }
    );
    assert_eq!(
        png::dimensions(matrix, Layout::Scale(u32::MAX)).unwrap_err(),
        PngError::ArithmeticOverflow
    );
    assert_eq!(
        png::dimensions(
            matrix,
            Layout::AtLeast {
                width: u32::MAX,
                height: 1
            }
        )
        .unwrap_err(),
        PngError::ArithmeticOverflow
    );
    assert!(matches!(
        png::encode(matrix, Layout::Scale(2000), BitDepth::Grayscale8),
        Err(PngError::ImageTooLarge { .. })
    ));
    assert!(matches!(
        png::dimensions(matrix, Layout::Scale(1000)),
        Err(PngError::PixelLimitExceeded { .. })
    ));
}

#[test]
fn output_bounds_are_inclusive_and_checked_before_encoding() {
    let modules = vec![0; 1016 * 1016];
    let square = QrMatrix::new(1016, 1016, &modules).unwrap();
    assert_eq!(
        png::dimensions(square, Layout::Scale(8)).unwrap(),
        Dimensions {
            width: 8192,
            height: 8192,
            scale: 8
        }
    );
    assert_eq!(
        png::dimensions(square, Layout::Scale(16)).unwrap_err(),
        PngError::PixelLimitExceeded { pixels: 268435456 }
    );
    let wide = QrMatrix::new(1016, 1, &modules[..1016]).unwrap();
    assert_eq!(
        png::dimensions(wide, Layout::Scale(16)).unwrap().width,
        png::MAX_PIXEL_SIDE
    );
    assert!(matches!(
        png::dimensions(wide, Layout::Scale(17)),
        Err(PngError::ImageTooLarge { .. })
    ));
}

#[test]
fn asymmetric_matrix_orientation_is_top_left_row_major() {
    let modules = [1, 0, 0, 1, 1, 0];
    let matrix = QrMatrix::new(3, 2, &modules).unwrap();
    for depth in [BitDepth::Monochrome1, BitDepth::Grayscale8] {
        for scale in [1, 2, 3, 7, 8, 17] {
            let bytes = png::encode(matrix, Layout::Scale(scale), depth).unwrap();
            assert_pixels(&decode_contract(&bytes), &modules, 3, 2, scale as usize);
        }
    }
}

#[test]
fn all_black_and_white_inputs_have_opaque_exact_quiet_zones() {
    for value in [0, 1] {
        let modules = [value; 35];
        for depth in [BitDepth::Monochrome1, BitDepth::Grayscale8] {
            let bytes = png::encode(
                QrMatrix::new(7, 5, &modules).unwrap(),
                Layout::Scale(3),
                depth,
            )
            .unwrap();
            assert_pixels(&decode_contract(&bytes), &modules, 7, 5, 3);
        }
    }
}

#[test]
fn packed_pixel_widths_cover_every_final_byte_remainder() {
    for width in 1..=16 {
        let modules = vec![1; width as usize];
        let encoded = png::encode(
            QrMatrix::new(width, 1, &modules).unwrap(),
            Layout::Scale(1),
            BitDepth::Monochrome1,
        )
        .unwrap();
        assert_pixels(&decode_contract(&encoded), &modules, width as usize, 1, 1);
    }
}

#[test]
fn streams_cross_rows_and_exact_deflate_block_boundaries() {
    for (width, height, raw_length) in [(248, 247, 65535), (247, 248, 65536), (505, 247, 131070)] {
        let modules: Vec<u8> = (0..width * height)
            .map(|index| u8::from(index % 3 == 0))
            .collect();
        let bytes = png::encode(
            QrMatrix::new(width, height, &modules).unwrap(),
            Layout::Scale(1),
            BitDepth::Grayscale8,
        )
        .unwrap();
        let decoded = decode_contract(&bytes);
        assert_eq!(decoded.raw.len(), raw_length);
        assert_eq!(decoded.blocks, raw_length.div_ceil(65535));
        assert_pixels(&decoded, &modules, width as usize, height as usize, 1);
    }
}

#[test]
fn all_standard_qr_sizes_are_lossless_at_integral_scales() {
    for version in 1..=40 {
        let side = 17 + 4 * version;
        let modules: Vec<u8> = (0..side * side)
            .map(|index| u8::from((index * 17 + index / side * 7) % 11 < 5))
            .collect();
        let matrix = QrMatrix::new(side, side, &modules).unwrap();
        for depth in [BitDepth::Monochrome1, BitDepth::Grayscale8] {
            let bytes = png::encode(matrix, Layout::Scale(2), depth).unwrap();
            assert_pixels(
                &decode_contract(&bytes),
                &modules,
                side as usize,
                side as usize,
                2,
            );
        }
    }
}

#[test]
fn encoding_is_deterministic_without_ancillary_metadata() {
    let matrix = QrMatrix::new(3, 2, &[1, 0, 0, 1, 1, 0]).unwrap();
    let first = png::encode(matrix, Layout::default(), BitDepth::default()).unwrap();
    let second = png::encode(matrix, Layout::default(), BitDepth::default()).unwrap();
    assert_eq!(first, second);
    decode_contract(&first); // Checks exact chunk set/order and no trailing bytes.
    assert!(format!("{}", PngError::ArithmeticOverflow).contains("overflow"));
}

#[test]
#[ignore = "writes synthetic PNG fixtures for png_verify.py independent decoding"]
fn emit_oracle_fixtures() {
    use std::fmt::Write as _;
    use std::fs;
    use std::path::PathBuf;

    let root =
        PathBuf::from(std::env::var_os("MCW_PNG_EVIDENCE_DIR").expect("set MCW_PNG_EVIDENCE_DIR"));
    fs::create_dir_all(&root).unwrap();
    let mut manifest = String::from(
        "name\tmodule_width\tmodule_height\tscale\tdepth\tpixel_width\tpixel_height\n",
    );
    let mut sequence = 0;
    let mut emit = |width: u32, height: u32, modules: &[u8], layout: Layout, depth: BitDepth| {
        let matrix = QrMatrix::new(width, height, modules).unwrap();
        let dimensions = png::dimensions(matrix, layout).unwrap();
        let name = format!("fixture-{sequence:04}");
        sequence += 1;
        fs::write(
            root.join(format!("{name}.png")),
            png::encode(matrix, layout, depth).unwrap(),
        )
        .unwrap();
        fs::write(root.join(format!("{name}.modules")), modules).unwrap();
        writeln!(
            manifest,
            "{name}\t{width}\t{height}\t{}\t{}\t{}\t{}",
            dimensions.scale,
            depth.bits(),
            dimensions.width,
            dimensions.height
        )
        .unwrap();
    };
    for value in [0, 1] {
        for depth in [BitDepth::Monochrome1, BitDepth::Grayscale8] {
            for scale in [1, 7, 8] {
                emit(1, 1, &[value], Layout::Scale(scale), depth);
            }
        }
    }
    for depth in [BitDepth::Monochrome1, BitDepth::Grayscale8] {
        for scale in [1, 3, 17] {
            emit(3, 2, &[1, 0, 0, 1, 1, 0], Layout::Scale(scale), depth);
        }
    }
    for width in 1..=8 {
        emit(
            width,
            1,
            &vec![1; width as usize],
            Layout::Scale(1),
            BitDepth::Monochrome1,
        );
    }
    for version in 1..=40 {
        let side = 17 + 4 * version;
        let modules: Vec<u8> = (0..side * side)
            .map(|index| u8::from((index * 17 + index / side * 7) % 11 < 5))
            .collect();
        for depth in [BitDepth::Monochrome1, BitDepth::Grayscale8] {
            for layout in [
                Layout::Scale(1),
                Layout::default(),
                Layout::FitWithin {
                    width: 512,
                    height: 600,
                },
            ] {
                emit(side, side, &modules, layout, depth);
            }
        }
    }
    for (width, height) in [(248, 247), (247, 248), (505, 247), (1024, 3)] {
        let modules: Vec<u8> = (0..width * height)
            .map(|index| u8::from(index % 3 == 0))
            .collect();
        emit(
            width,
            height,
            &modules,
            Layout::Scale(1),
            BitDepth::Grayscale8,
        );
    }
    fs::write(root.join("fixtures.tsv"), manifest).unwrap();
    println!(
        "emitted {sequence} synthetic PNG fixtures to {}",
        root.display()
    );
}
