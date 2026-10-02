//! Portable, deterministic PNG export for an unpadded row-major QR matrix.
//!
//! A module value of 1 is black; 0 is white. Rows run from top to bottom and
//! columns from left to right. The encoder adds exactly four white modules on
//! every side and uses a single integral scale for both axes. It neither
//! transposes nor resamples the input. Only Rust's standard library is used.
//!
//! PNG color type 0, bit depth 1 or 8, filter None and non-interlaced scanlines
//! are emitted. A first-party RFC 1951 stored-block DEFLATE writer is wrapped
//! in RFC 1950 zlib with Adler-32; PNG chunks use CRC-32. Stored blocks trade
//! compression ratio for a small, bounded implementation. No zlib library,
//! image crate, OS image service, metadata or filesystem access is involved.

#![forbid(unsafe_code)]

use std::fmt;

/// The quiet zone cannot be disabled or changed by a caller.
pub const QUIET_ZONE_MODULES: u32 = 4;
/// Arbitrary rectangular binary matrices are accepted within this bound.
/// All standard QR symbol sizes (21 through 177) fit inside it.
pub const MAX_MODULE_SIDE: u32 = 1_024;
pub const MAX_PIXEL_SIDE: u32 = 16_384;
pub const MAX_PIXELS: u64 = 64 * 1_024 * 1_024;
/// The existing receive-address exporter has a 512-pixel minimum.
pub const DEFAULT_EXPORT_MINIMUM: u32 = 512;

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const STORED_BLOCK_LIMIT: usize = u16::MAX as usize;
const PNG_FIXED_BYTES: usize = 57; // Signature, IHDR, IDAT envelope, IEND.

/// A validated borrowed matrix. The input excludes the quiet zone.
#[derive(Clone, Copy, Debug)]
pub struct QrMatrix<'a> {
    width: u32,
    height: u32,
    modules: &'a [u8],
}

impl<'a> QrMatrix<'a> {
    /// Accepts exactly `width * height` bytes at index `y * width + x`.
    /// Values other than 0 and 1 are rejected, rather than coerced.
    pub fn new(width: u32, height: u32, modules: &'a [u8]) -> Result<Self, PngError> {
        if width == 0 || height == 0 {
            return Err(PngError::EmptyMatrix);
        }
        if width > MAX_MODULE_SIDE || height > MAX_MODULE_SIDE {
            return Err(PngError::MatrixTooLarge { width, height });
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .ok_or(PngError::ArithmeticOverflow)?;
        if modules.len() != expected {
            return Err(PngError::LengthMismatch {
                expected,
                actual: modules.len(),
            });
        }
        if let Some((index, &value)) = modules.iter().enumerate().find(|(_, value)| **value > 1) {
            return Err(PngError::InvalidModule { index, value });
        }
        Ok(Self {
            width,
            height,
            modules,
        })
    }

    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }

    pub fn modules(self) -> &'a [u8] {
        self.modules
    }
}

/// No layout introduces extra canvas pixels outside the four-module quiet zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Exactly this positive number of pixels per module.
    Scale(u32),
    /// Largest integral scale fitting inside both requested dimensions.
    /// Unused space is omitted from the output, rather than padded.
    FitWithin { width: u32, height: u32 },
    /// Smallest integral scale meeting both minimum dimensions.
    /// Output can exceed the minimum by less than one module in the governing
    /// dimension. For a square QR, a 512-pixel minimum generally snaps upward.
    AtLeast { width: u32, height: u32 },
}

impl Default for Layout {
    fn default() -> Self {
        Self::AtLeast {
            width: DEFAULT_EXPORT_MINIMUM,
            height: DEFAULT_EXPORT_MINIMUM,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BitDepth {
    /// Packed grayscale: white is 1 and black is 0, MSB first in each byte.
    #[default]
    Monochrome1,
    /// Grayscale bytes: white is 255 and black is 0. No intermediate shades.
    Grayscale8,
}

impl BitDepth {
    pub fn bits(self) -> u8 {
        match self {
            Self::Monochrome1 => 1,
            Self::Grayscale8 => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
    pub scale: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngError {
    EmptyMatrix,
    MatrixTooLarge {
        width: u32,
        height: u32,
    },
    LengthMismatch {
        expected: usize,
        actual: usize,
    },
    InvalidModule {
        index: usize,
        value: u8,
    },
    InvalidScale,
    InvalidCanvas {
        width: u32,
        height: u32,
    },
    CanvasTooSmall {
        width: u32,
        height: u32,
        minimum_width: u32,
        minimum_height: u32,
    },
    ImageTooLarge {
        width: u32,
        height: u32,
    },
    PixelLimitExceeded {
        pixels: u64,
    },
    ArithmeticOverflow,
    AllocationFailed,
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMatrix => f.write_str("PNG matrix dimensions must be positive"),
            Self::MatrixTooLarge { width, height } => write!(
                f,
                "PNG matrix {width}x{height} exceeds the {MAX_MODULE_SIDE}-module side limit"
            ),
            Self::LengthMismatch { expected, actual } => {
                write!(f, "PNG matrix needs {expected} modules, received {actual}")
            }
            Self::InvalidModule { index, value } => {
                write!(f, "PNG module {index} has invalid binary value {value}")
            }
            Self::InvalidScale => f.write_str("PNG module scale must be positive"),
            Self::InvalidCanvas { width, height } => write!(
                f,
                "PNG requested dimensions {width}x{height} must be positive"
            ),
            Self::CanvasTooSmall {
                width,
                height,
                minimum_width,
                minimum_height,
            } => write!(
                f,
                "PNG canvas {width}x{height} cannot fit {minimum_width}x{minimum_height} modules including the quiet zone"
            ),
            Self::ImageTooLarge { width, height } => write!(
                f,
                "PNG dimensions {width}x{height} exceed the {MAX_PIXEL_SIDE}-pixel side limit"
            ),
            Self::PixelLimitExceeded { pixels } => write!(
                f,
                "PNG has {pixels} pixels, exceeding the {MAX_PIXELS}-pixel limit"
            ),
            Self::ArithmeticOverflow => f.write_str("PNG layout or encoded length overflow"),
            Self::AllocationFailed => f.write_str("Cannot allocate bounded PNG export buffers"),
        }
    }
}

impl std::error::Error for PngError {}

/// Validates and resolves a layout without allocating or touching module bytes.
pub fn dimensions(matrix: QrMatrix<'_>, requested: Layout) -> Result<Dimensions, PngError> {
    let padded_width = matrix
        .width
        .checked_add(QUIET_ZONE_MODULES * 2)
        .ok_or(PngError::ArithmeticOverflow)?;
    let padded_height = matrix
        .height
        .checked_add(QUIET_ZONE_MODULES * 2)
        .ok_or(PngError::ArithmeticOverflow)?;
    let scale = match requested {
        Layout::Scale(0) => return Err(PngError::InvalidScale),
        Layout::Scale(scale) => scale,
        Layout::FitWithin { width, height } | Layout::AtLeast { width, height }
            if width == 0 || height == 0 =>
        {
            return Err(PngError::InvalidCanvas { width, height });
        }
        Layout::FitWithin { width, height } => {
            let scale = (width / padded_width).min(height / padded_height);
            if scale == 0 {
                return Err(PngError::CanvasTooSmall {
                    width,
                    height,
                    minimum_width: padded_width,
                    minimum_height: padded_height,
                });
            }
            scale
        }
        Layout::AtLeast { width, height } => width
            .div_ceil(padded_width)
            .max(height.div_ceil(padded_height)),
    };
    let width = padded_width
        .checked_mul(scale)
        .ok_or(PngError::ArithmeticOverflow)?;
    let height = padded_height
        .checked_mul(scale)
        .ok_or(PngError::ArithmeticOverflow)?;
    if width > MAX_PIXEL_SIDE || height > MAX_PIXEL_SIDE {
        return Err(PngError::ImageTooLarge { width, height });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(PngError::PixelLimitExceeded { pixels });
    }
    Ok(Dimensions {
        width,
        height,
        scale,
    })
}

/// Encodes opaque black/white PNG bytes without a full decoded pixel buffer.
/// Only one scanline plus the exact encoded output are allocated; allocation,
/// size and integer-overflow failures are returned before any output is exposed.
pub fn encode(
    matrix: QrMatrix<'_>,
    requested: Layout,
    depth: BitDepth,
) -> Result<Vec<u8>, PngError> {
    let image = dimensions(matrix, requested)?;
    let row_bytes = match depth {
        BitDepth::Monochrome1 => (image.width as usize).div_ceil(8),
        BitDepth::Grayscale8 => image.width as usize,
    };
    let row_length = row_bytes
        .checked_add(1)
        .ok_or(PngError::ArithmeticOverflow)?;
    let raw_length = row_length
        .checked_mul(image.height as usize)
        .ok_or(PngError::ArithmeticOverflow)?;
    let block_bytes = raw_length
        .div_ceil(STORED_BLOCK_LIMIT)
        .checked_mul(5)
        .ok_or(PngError::ArithmeticOverflow)?;
    let zlib_length = raw_length
        .checked_add(block_bytes)
        .and_then(|len| len.checked_add(6))
        .ok_or(PngError::ArithmeticOverflow)?;
    let output_length = zlib_length
        .checked_add(PNG_FIXED_BYTES)
        .ok_or(PngError::ArithmeticOverflow)?;
    // PNG chunk lengths must fit a signed 31-bit integer, even on 64-bit hosts.
    let idat_length = u32::try_from(zlib_length).map_err(|_| PngError::ArithmeticOverflow)?;
    if idat_length > i32::MAX as u32 {
        return Err(PngError::ArithmeticOverflow);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_length)
        .map_err(|_| PngError::AllocationFailed)?;
    let mut row = Vec::new();
    row.try_reserve_exact(row_length)
        .map_err(|_| PngError::AllocationFailed)?;
    row.resize(row_length, 0);

    output.extend_from_slice(&SIGNATURE);
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&image.width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&image.height.to_be_bytes());
    ihdr[8] = depth.bits(); // Color type, compression, filter, interlace are 0.
    append_chunk(&mut output, *b"IHDR", &ihdr);
    output.extend_from_slice(&idat_length.to_be_bytes());
    let idat_type_offset = output.len();
    output.extend_from_slice(b"IDAT");

    let quiet_pixels = (QUIET_ZONE_MODULES * image.scale) as usize;
    let scale = image.scale as usize;
    let mut deflate = StoredDeflate::new(&mut output, raw_length);
    white_scanline(&mut row, image.width as usize, depth);
    for _ in 0..quiet_pixels {
        deflate.write(&row);
    }
    for module_y in 0..matrix.height as usize {
        white_scanline(&mut row, image.width as usize, depth);
        let modules = &matrix.modules
            [module_y * matrix.width as usize..(module_y + 1) * matrix.width as usize];
        for (module_x, &module) in modules.iter().enumerate() {
            if module == 1 {
                let start = quiet_pixels + module_x * scale;
                match depth {
                    BitDepth::Grayscale8 => row[1 + start..1 + start + scale].fill(0),
                    BitDepth::Monochrome1 => {
                        for pixel in start..start + scale {
                            row[1 + pixel / 8] &= !(0x80 >> (pixel % 8));
                        }
                    }
                }
            }
        }
        for _ in 0..scale {
            deflate.write(&row);
        }
    }
    white_scanline(&mut row, image.width as usize, depth);
    for _ in 0..quiet_pixels {
        deflate.write(&row);
    }
    deflate.finish();
    let checksum = crc32(&output[idat_type_offset..]);
    output.extend_from_slice(&checksum.to_be_bytes());
    append_chunk(&mut output, *b"IEND", &[]);
    debug_assert_eq!(output.len(), output_length);
    Ok(output)
}

fn white_scanline(row: &mut [u8], width: usize, depth: BitDepth) {
    row[0] = 0; // PNG filter None.
    row[1..].fill(255);
    if depth == BitDepth::Monochrome1 && !width.is_multiple_of(8) {
        // Unused low bits in a packed scanline are deterministic zeroes.
        let last = row.len() - 1;
        row[last] &= 0xff << (8 - width % 8);
    }
}

fn append_chunk(output: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    // Only the fixed-size IHDR and empty IEND use this helper.
    debug_assert!(data.len() <= 13);
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let type_offset = output.len();
    output.extend_from_slice(&kind);
    output.extend_from_slice(data);
    let checksum = crc32(&output[type_offset..]);
    output.extend_from_slice(&checksum.to_be_bytes());
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ 0xedb8_8320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = crc_table();

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ u32::from(byte)) & 255) as usize];
    }
    !crc
}

struct Adler32 {
    s1: u32,
    s2: u32,
}

impl Adler32 {
    fn new() -> Self {
        Self { s1: 1, s2: 0 }
    }

    fn update(&mut self, bytes: &[u8]) {
        // At most 5552 maximal bytes may be accumulated before modulo reduction
        // without overflowing a u32, including maximal incoming modulo sums.
        for part in bytes.chunks(5552) {
            for &byte in part {
                self.s1 += u32::from(byte);
                self.s2 += self.s1;
            }
            self.s1 %= 65521;
            self.s2 %= 65521;
        }
    }

    fn value(&self) -> u32 {
        (self.s2 << 16) | self.s1
    }
}

/// Streams byte-aligned DEFLATE BTYPE=00 blocks of at most 65535 bytes.
/// A block can cross row boundaries; BFINAL is set on exactly the last block.
struct StoredDeflate<'a> {
    output: &'a mut Vec<u8>,
    remaining: usize,
    block_remaining: usize,
    adler: Adler32,
}

impl<'a> StoredDeflate<'a> {
    fn new(output: &'a mut Vec<u8>, raw_length: usize) -> Self {
        output.extend_from_slice(&[0x78, 0x01]); // CM=8, CINFO=7, no dictionary, FCHECK valid.
        if raw_length == 0 {
            output.extend_from_slice(&[1, 0, 0, 255, 255]);
        }
        Self {
            output,
            remaining: raw_length,
            block_remaining: 0,
            adler: Adler32::new(),
        }
    }

    fn write(&mut self, mut bytes: &[u8]) {
        debug_assert!(bytes.len() <= self.remaining);
        while !bytes.is_empty() {
            if self.block_remaining == 0 {
                let length = self.remaining.min(STORED_BLOCK_LIMIT) as u16;
                self.output
                    .push(u8::from(self.remaining <= STORED_BLOCK_LIMIT));
                self.output.extend_from_slice(&length.to_le_bytes());
                self.output.extend_from_slice(&(!length).to_le_bytes());
                self.block_remaining = usize::from(length);
            }
            let count = bytes.len().min(self.block_remaining);
            self.output.extend_from_slice(&bytes[..count]);
            self.adler.update(&bytes[..count]);
            self.block_remaining -= count;
            self.remaining -= count;
            bytes = &bytes[count..];
        }
    }

    fn finish(self) {
        debug_assert_eq!(self.remaining, 0);
        debug_assert_eq!(self.block_remaining, 0);
        self.output
            .extend_from_slice(&self.adler.value().to_be_bytes());
    }
}

#[cfg(test)]
mod checksum_tests {
    use super::*;

    #[test]
    fn crc_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn adler_known_vectors_and_incremental_updates() {
        assert_eq!(Adler32::new().value(), 1);
        let mut adler = Adler32::new();
        adler.update(b"Wiki");
        adler.update(b"pedia");
        assert_eq!(adler.value(), 0x11e6_0398);
        let input = vec![255; 65536];
        let mut whole = Adler32::new();
        whole.update(&input);
        let mut split = Adler32::new();
        for part in input.chunks(7) {
            split.update(part);
        }
        assert_eq!(whole.value(), split.value());
        // Independent slow modulo-per-byte reference.
        let (mut a, mut b) = (1u64, 0u64);
        for value in input {
            a = (a + u64::from(value)) % 65521;
            b = (b + a) % 65521;
        }
        assert_eq!(whole.value(), ((b << 16) | a) as u32);
    }

    #[test]
    fn stored_blocks_handle_empty_and_exact_boundary_lengths() {
        for length in [0, 1, 65534, 65535, 65536, 131070, 131071] {
            let input: Vec<u8> = (0..length).map(|index| (index % 251) as u8).collect();
            let mut stream = Vec::new();
            let mut writer = StoredDeflate::new(&mut stream, length);
            for part in input.chunks(97) {
                writer.write(part);
            }
            writer.finish();
            let mut cursor = 2;
            let mut decoded = Vec::new();
            loop {
                let final_block = stream[cursor];
                assert!(final_block <= 1);
                let count = u16::from_le_bytes(stream[cursor + 1..cursor + 3].try_into().unwrap());
                let complement =
                    u16::from_le_bytes(stream[cursor + 3..cursor + 5].try_into().unwrap());
                assert_eq!(count, !complement);
                cursor += 5;
                decoded.extend_from_slice(&stream[cursor..cursor + usize::from(count)]);
                cursor += usize::from(count);
                if final_block == 1 {
                    break;
                }
            }
            assert_eq!(decoded, input);
            assert_eq!(cursor + 4, stream.len());
        }
    }
}
