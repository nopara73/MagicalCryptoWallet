//! Bounded PNG-to-pixel decoding. Uses the first-party `compression` zlib codec.
//!
//! All standard PNG color/bit-depth combinations and Adam7 are accepted. Output
//! is tightly packed, top-left row-major Gray8/RGB8/straight RGBA8. Sixteen-bit
//! samples retain their high byte; packed grayscale expands across 0..255.
//! Transparency keys are compared before reducing sample depth. Color-profile,
//! gamma, EXIF orientation and other display metadata are not applied. Animated
//! PNG and unknown critical extensions are explicitly rejected.

#![forbid(unsafe_code)]

use crate::compression;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Gray8,
    Rgb8,
    /// Straight, unassociated alpha; no background compositing is performed.
    Rgba8,
}

impl PixelFormat {
    pub fn channels(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }
}

#[derive(Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub max_file_bytes: usize,
    pub max_dimension: u32,
    pub max_pixels: u64,
    pub max_inflated_bytes: usize,
    /// Requested live allocation capacities; caller-owned input and allocator
    /// overhead/transient reallocation storage are outside this accounting.
    pub max_allocation_bytes: usize,
    pub max_chunks: usize,
    pub max_compression_work: u64,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 32 * 1024 * 1024,
            max_dimension: 8192,
            max_pixels: 16 * 1024 * 1024,
            max_inflated_bytes: 160 * 1024 * 1024,
            max_allocation_bytes: 256 * 1024 * 1024,
            max_chunks: 16384,
            max_compression_work: 2_000_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeLimit {
    FileBytes,
    Dimension,
    Pixels,
    InflatedBytes,
    Allocation,
    Chunks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    InvalidLimits,
    InvalidSignature,
    Truncated,
    InvalidChunkLength,
    InvalidChunkType,
    ChecksumMismatch {
        chunk: [u8; 4],
    },
    InvalidChunkOrder {
        chunk: [u8; 4],
    },
    InvalidHeader,
    UnsupportedFormat {
        color_type: u8,
        bit_depth: u8,
        interlace: u8,
    },
    UnsupportedCriticalChunk {
        chunk: [u8; 4],
    },
    AnimationUnsupported,
    InvalidPalette,
    MissingPalette,
    InvalidTransparency,
    MissingImageData,
    MissingEnd,
    TrailingData,
    InvalidFilter {
        filter: u8,
    },
    InvalidPaletteIndex {
        index: u16,
    },
    InflatedSizeMismatch {
        expected: usize,
        actual: usize,
    },
    LimitExceeded(DecodeLimit),
    ArithmeticOverflow,
    AllocationFailed,
    Compression(compression::Error),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PNG decode: {self:?}")
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compression(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlace: u8,
}

impl Header {
    fn source_channels(self) -> usize {
        match self.color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            _ => unreachable!("validated PNG color type"),
        }
    }

    fn format(self, has_transparency: bool) -> PixelFormat {
        match self.color {
            0 if !has_transparency => PixelFormat::Gray8,
            2 | 3 if !has_transparency => PixelFormat::Rgb8,
            _ => PixelFormat::Rgba8,
        }
    }

    fn row_bytes(self, width: usize) -> Result<usize, DecodeError> {
        let bits = width
            .checked_mul(self.source_channels())
            .and_then(|n| n.checked_mul(usize::from(self.depth)))
            .ok_or(DecodeError::ArithmeticOverflow)?;
        Ok(bits.div_ceil(8))
    }

    fn sample_max(self) -> u16 {
        if self.depth == 16 {
            u16::MAX
        } else {
            (1u16 << self.depth) - 1
        }
    }
}

struct Parsed<'a> {
    header: Header,
    palette: Option<&'a [u8]>,
    transparency: Option<&'a [u8]>,
    idat_bytes: usize,
}

#[derive(Clone, Copy)]
struct Pass {
    x: usize,
    y: usize,
    dx: usize,
    dy: usize,
}

const SINGLE_PASS: [Pass; 1] = [Pass {
    x: 0,
    y: 0,
    dx: 1,
    dy: 1,
}];
const ADAM7: [Pass; 7] = [
    Pass {
        x: 0,
        y: 0,
        dx: 8,
        dy: 8,
    },
    Pass {
        x: 4,
        y: 0,
        dx: 8,
        dy: 8,
    },
    Pass {
        x: 0,
        y: 4,
        dx: 4,
        dy: 8,
    },
    Pass {
        x: 2,
        y: 0,
        dx: 4,
        dy: 4,
    },
    Pass {
        x: 0,
        y: 2,
        dx: 2,
        dy: 4,
    },
    Pass {
        x: 1,
        y: 0,
        dx: 2,
        dy: 2,
    },
    Pass {
        x: 0,
        y: 1,
        dx: 1,
        dy: 2,
    },
];

fn extent(total: usize, start: usize, step: usize) -> usize {
    total.saturating_sub(start).div_ceil(step)
}

fn passes(header: Header) -> &'static [Pass] {
    if header.interlace == 0 {
        &SINGLE_PASS
    } else {
        &ADAM7
    }
}

fn expected_inflated(header: Header) -> Result<usize, DecodeError> {
    let mut total = 0usize;
    for pass in passes(header) {
        let width = extent(header.width as usize, pass.x, pass.dx);
        let height = extent(header.height as usize, pass.y, pass.dy);
        if width == 0 || height == 0 {
            continue;
        }
        let bytes = header
            .row_bytes(width)?
            .checked_add(1)
            .and_then(|n| n.checked_mul(height))
            .ok_or(DecodeError::ArithmeticOverflow)?;
        total = total
            .checked_add(bytes)
            .ok_or(DecodeError::ArithmeticOverflow)?;
    }
    Ok(total)
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn validate_header(data: &[u8], limits: DecodeLimits) -> Result<Header, DecodeError> {
    if data.len() != 13 {
        return Err(DecodeError::InvalidHeader);
    }
    let header = Header {
        width: read_u32(data),
        height: read_u32(&data[4..]),
        depth: data[8],
        color: data[9],
        interlace: data[12],
    };
    if header.width == 0
        || header.height == 0
        || header.width > i32::MAX as u32
        || header.height > i32::MAX as u32
        || data[10] != 0
        || data[11] != 0
    {
        return Err(DecodeError::InvalidHeader);
    }
    let valid_depth = match header.color {
        0 => [1, 2, 4, 8, 16].contains(&header.depth),
        2 | 4 | 6 => [8, 16].contains(&header.depth),
        3 => [1, 2, 4, 8].contains(&header.depth),
        _ => false,
    };
    if !valid_depth || header.interlace > 1 {
        return Err(DecodeError::UnsupportedFormat {
            color_type: header.color,
            bit_depth: header.depth,
            interlace: header.interlace,
        });
    }
    if header.width > limits.max_dimension || header.height > limits.max_dimension {
        return Err(DecodeError::LimitExceeded(DecodeLimit::Dimension));
    }
    if u64::from(header.width) * u64::from(header.height) > limits.max_pixels {
        return Err(DecodeError::LimitExceeded(DecodeLimit::Pixels));
    }
    Ok(header)
}

fn parse(input: &[u8], limits: DecodeLimits) -> Result<Parsed<'_>, DecodeError> {
    if input.len() > limits.max_file_bytes {
        return Err(DecodeError::LimitExceeded(DecodeLimit::FileBytes));
    }
    if input.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return Err(DecodeError::InvalidSignature);
    }
    let (mut cursor, mut count) = (8usize, 0usize);
    let (mut header, mut palette, mut transparency) = (None, None, None);
    let (mut seen_idat, mut ended_idat, mut idat_bytes) = (false, false, 0usize);
    while cursor < input.len() {
        count += 1;
        if count > limits.max_chunks {
            return Err(DecodeError::LimitExceeded(DecodeLimit::Chunks));
        }
        let prefix_end = cursor
            .checked_add(8)
            .ok_or(DecodeError::ArithmeticOverflow)?;
        let prefix = input
            .get(cursor..prefix_end)
            .ok_or(DecodeError::Truncated)?;
        let length = read_u32(prefix) as usize;
        if length > i32::MAX as usize {
            return Err(DecodeError::InvalidChunkLength);
        }
        let kind: [u8; 4] = [prefix[4], prefix[5], prefix[6], prefix[7]];
        if !kind.iter().all(u8::is_ascii_alphabetic) || !kind[2].is_ascii_uppercase() {
            return Err(DecodeError::InvalidChunkType);
        }
        let data_end = prefix_end
            .checked_add(length)
            .ok_or(DecodeError::ArithmeticOverflow)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(DecodeError::ArithmeticOverflow)?;
        if chunk_end > input.len() {
            return Err(DecodeError::Truncated);
        }
        let data = &input[prefix_end..data_end];
        if super::crc32(&input[cursor + 4..data_end]) != read_u32(&input[data_end..chunk_end]) {
            return Err(DecodeError::ChecksumMismatch { chunk: kind });
        }
        if header.is_none() && kind != *b"IHDR" {
            return Err(DecodeError::InvalidChunkOrder { chunk: kind });
        }
        if seen_idat && kind != *b"IDAT" {
            ended_idat = true;
        }
        match &kind {
            b"IHDR" => {
                if header.is_some() {
                    return Err(DecodeError::InvalidChunkOrder { chunk: kind });
                }
                header = Some(validate_header(data, limits)?);
            }
            b"PLTE" => {
                let h = header.ok_or(DecodeError::InvalidHeader)?;
                if palette.is_some() || transparency.is_some() || seen_idat {
                    return Err(DecodeError::InvalidChunkOrder { chunk: kind });
                }
                if h.color == 0
                    || h.color == 4
                    || data.is_empty()
                    || data.len() > 768
                    || !data.len().is_multiple_of(3)
                    || (h.color == 3 && data.len() / 3 > 1usize << h.depth)
                {
                    return Err(DecodeError::InvalidPalette);
                }
                palette = Some(data);
            }
            b"tRNS" => {
                let h = header.ok_or(DecodeError::InvalidHeader)?;
                if transparency.is_some() || seen_idat {
                    return Err(DecodeError::InvalidChunkOrder { chunk: kind });
                }
                let valid = match h.color {
                    0 => data.len() == 2 && read_u16(data) <= h.sample_max(),
                    2 => {
                        data.len() == 6
                            && data
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .all(|sample| read_u16(sample) <= h.sample_max())
                    }
                    3 => match palette {
                        Some(p) => data.len() <= p.len() / 3,
                        None => return Err(DecodeError::MissingPalette),
                    },
                    _ => false,
                };
                if !valid {
                    return Err(DecodeError::InvalidTransparency);
                }
                transparency = Some(data);
            }
            b"IDAT" => {
                if ended_idat {
                    return Err(DecodeError::InvalidChunkOrder { chunk: kind });
                }
                if header.ok_or(DecodeError::InvalidHeader)?.color == 3 && palette.is_none() {
                    return Err(DecodeError::MissingPalette);
                }
                seen_idat = true;
                idat_bytes = idat_bytes
                    .checked_add(length)
                    .ok_or(DecodeError::ArithmeticOverflow)?;
            }
            b"IEND" => {
                if length != 0 {
                    return Err(DecodeError::InvalidChunkLength);
                }
                if !seen_idat || idat_bytes == 0 {
                    return Err(DecodeError::MissingImageData);
                }
                if chunk_end != input.len() {
                    return Err(DecodeError::TrailingData);
                }
                return Ok(Parsed {
                    header: header.ok_or(DecodeError::InvalidHeader)?,
                    palette,
                    transparency,
                    idat_bytes,
                });
            }
            b"acTL" | b"fcTL" | b"fdAT" => return Err(DecodeError::AnimationUnsupported),
            _ if kind[0].is_ascii_uppercase() => {
                return Err(DecodeError::UnsupportedCriticalChunk { chunk: kind });
            }
            _ => {} // Ancillary metadata is CRC checked, never expanded/executed.
        }
        cursor = chunk_end;
    }
    Err(DecodeError::MissingEnd)
}

fn collect_idat(input: &[u8], length: usize) -> Result<Vec<u8>, DecodeError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| DecodeError::AllocationFailed)?;
    let mut cursor = 8;
    // parse() has verified every length, chunk and terminal offset on this same
    // immutable borrowed input; no untrusted unchecked offset reaches this pass.
    while cursor < input.len() {
        let count = read_u32(&input[cursor..]) as usize;
        let end = cursor + 8 + count;
        if &input[cursor + 4..cursor + 8] == b"IDAT" {
            bytes.extend_from_slice(&input[cursor + 8..end]);
        }
        cursor = end + 4;
    }
    Ok(bytes)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a, b, c) = (i32::from(a), i32::from(b), i32::from(c));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

fn unfilter(
    raw: &mut [u8],
    start: usize,
    length: usize,
    previous: Option<usize>,
    bpp: usize,
) -> Result<(), DecodeError> {
    let filter = raw[start - 1];
    if filter > 4 {
        return Err(DecodeError::InvalidFilter { filter });
    }
    for i in 0..length {
        let a = if i >= bpp { raw[start + i - bpp] } else { 0 };
        let b = previous.map_or(0, |offset| raw[offset + i]);
        let c = if i >= bpp {
            previous.map_or(0, |offset| raw[offset + i - bpp])
        } else {
            0
        };
        let predictor = match filter {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            4 => paeth(a, b, c),
            _ => unreachable!("validated filter"),
        };
        raw[start + i] = raw[start + i].wrapping_add(predictor);
    }
    Ok(())
}

fn sample(row: &[u8], index: usize, depth: u8) -> u16 {
    match depth {
        16 => read_u16(&row[index * 2..]),
        8 => u16::from(row[index]),
        _ => {
            let bit = index * usize::from(depth);
            let shift = 8 - usize::from(depth) - bit % 8;
            u16::from((row[bit / 8] >> shift) & ((1u8 << depth) - 1))
        }
    }
}

fn byte_sample(value: u16, depth: u8) -> u8 {
    match depth {
        16 => (value >> 8) as u8,
        8 => value as u8,
        _ => (u32::from(value) * 255 / ((1u32 << depth) - 1)) as u8,
    }
}

fn pixel(parsed: &Parsed<'_>, row: &[u8], x: usize, target: &mut [u8]) -> Result<(), DecodeError> {
    let h = parsed.header;
    let offset = x * h.source_channels();
    let first = sample(row, offset, h.depth);
    match h.color {
        0 | 4 => {
            let gray = byte_sample(first, h.depth);
            if target.len() == 1 {
                target[0] = gray;
            } else {
                target[..3].fill(gray);
                target[3] = if h.color == 4 {
                    byte_sample(sample(row, offset + 1, h.depth), h.depth)
                } else if parsed
                    .transparency
                    .is_some_and(|key| read_u16(key) == first)
                {
                    0
                } else {
                    255
                };
            }
        }
        2 | 6 => {
            let green = sample(row, offset + 1, h.depth);
            let blue = sample(row, offset + 2, h.depth);
            target[0] = byte_sample(first, h.depth);
            target[1] = byte_sample(green, h.depth);
            target[2] = byte_sample(blue, h.depth);
            if target.len() == 4 {
                target[3] = if h.color == 6 {
                    byte_sample(sample(row, offset + 3, h.depth), h.depth)
                } else if parsed.transparency.is_some_and(|key| {
                    first == read_u16(key)
                        && green == read_u16(&key[2..])
                        && blue == read_u16(&key[4..])
                }) {
                    0
                } else {
                    255
                };
            }
        }
        3 => {
            let palette = parsed.palette.ok_or(DecodeError::MissingPalette)?;
            let offset = usize::from(first) * 3;
            let color = palette
                .get(offset..offset + 3)
                .ok_or(DecodeError::InvalidPaletteIndex { index: first })?;
            target[..3].copy_from_slice(color);
            if target.len() == 4 {
                target[3] = parsed
                    .transparency
                    .and_then(|alpha| alpha.get(usize::from(first)))
                    .copied()
                    .unwrap_or(255);
            }
        }
        _ => unreachable!("validated color type"),
    }
    Ok(())
}

/// Validates all chunks and zlib checksums before returning a complete image.
/// Errors never expose provisional pixels. Input length, dimensions, pixel count,
/// filtered output length, allocation capacities, chunk count and inflater work
/// are bounded; file IO, camera handles, compositing and color management are not
/// responsibilities of this portable module.
pub fn decode(input: &[u8], limits: DecodeLimits) -> Result<DecodedImage, DecodeError> {
    if limits.max_file_bytes == 0
        || limits.max_dimension == 0
        || limits.max_pixels == 0
        || limits.max_inflated_bytes == 0
        || limits.max_allocation_bytes == 0
        || limits.max_chunks == 0
        || limits.max_compression_work == 0
    {
        return Err(DecodeError::InvalidLimits);
    }
    let parsed = parse(input, limits)?;
    let h = parsed.header;
    let expected = expected_inflated(h)?;
    if expected > limits.max_inflated_bytes {
        return Err(DecodeError::LimitExceeded(DecodeLimit::InflatedBytes));
    }
    let format = h.format(parsed.transparency.is_some());
    let output_length = (h.width as usize)
        .checked_mul(h.height as usize)
        .and_then(|n| n.checked_mul(format.channels()))
        .ok_or(DecodeError::ArithmeticOverflow)?;
    let owned_minimum = parsed
        .idat_bytes
        .checked_add(expected)
        .and_then(|n| n.checked_add(output_length))
        .ok_or(DecodeError::ArithmeticOverflow)?;
    if owned_minimum > limits.max_allocation_bytes {
        return Err(DecodeError::LimitExceeded(DecodeLimit::Allocation));
    }
    let idat = collect_idat(input, parsed.idat_bytes)?;
    let reserved = idat
        .capacity()
        .checked_add(output_length)
        .ok_or(DecodeError::ArithmeticOverflow)?;
    let compression_budget = limits
        .max_allocation_bytes
        .checked_sub(reserved)
        .ok_or(DecodeError::LimitExceeded(DecodeLimit::Allocation))?;
    let mut options = compression::DecodeOptions::new(compression::Format::Zlib);
    options.limits.max_input_bytes = idat.len() as u64;
    options.limits.max_output_bytes = expected as u64;
    // Exact image-derived output/work limits already bound expansion. This avoids
    // rejecting legitimate uniformly colored PNGs with a compression ratio >200.
    options.limits.max_expansion_ratio = 1;
    options.limits.expansion_slack_bytes = expected as u64;
    options.limits.max_work = limits.max_compression_work;
    options.limits.max_allocation_bytes = compression_budget;
    let inflated = compression::decode(&idat, options).map_err(DecodeError::Compression)?;
    if inflated.consumed != idat.len() || inflated.members != 1 {
        return Err(DecodeError::TrailingData);
    }
    let mut raw = inflated.bytes;
    if raw.len() != expected {
        return Err(DecodeError::InflatedSizeMismatch {
            expected,
            actual: raw.len(),
        });
    }
    drop(idat);
    if raw
        .capacity()
        .checked_add(output_length)
        .ok_or(DecodeError::ArithmeticOverflow)?
        > limits.max_allocation_bytes
    {
        return Err(DecodeError::LimitExceeded(DecodeLimit::Allocation));
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(output_length)
        .map_err(|_| DecodeError::AllocationFailed)?;
    if raw
        .capacity()
        .checked_add(pixels.capacity())
        .ok_or(DecodeError::ArithmeticOverflow)?
        > limits.max_allocation_bytes
    {
        return Err(DecodeError::LimitExceeded(DecodeLimit::Allocation));
    }
    pixels.resize(output_length, 0);
    let bpp = (h.source_channels() * usize::from(h.depth)).div_ceil(8);
    let mut cursor = 0;
    for pass in passes(h) {
        let width = extent(h.width as usize, pass.x, pass.dx);
        let height = extent(h.height as usize, pass.y, pass.dy);
        if width == 0 || height == 0 {
            continue;
        }
        let row_bytes = h.row_bytes(width)?;
        let mut previous = None;
        for y in 0..height {
            let start = cursor + 1;
            unfilter(&mut raw, start, row_bytes, previous, bpp)?;
            let row = &raw[start..start + row_bytes];
            for x in 0..width {
                let target_x = pass.x + x * pass.dx;
                let target_y = pass.y + y * pass.dy;
                let target = (target_y * h.width as usize + target_x) * format.channels();
                pixel(
                    &parsed,
                    row,
                    x,
                    &mut pixels[target..target + format.channels()],
                )?;
            }
            previous = Some(start);
            cursor = start + row_bytes;
        }
    }
    debug_assert_eq!(cursor, expected);
    Ok(DecodedImage {
        width: h.width,
        height: h.height,
        format,
        pixels,
    })
}
