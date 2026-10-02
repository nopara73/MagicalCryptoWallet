//! First-party QR Model 2 decoder. Only Rust std is used. The block-size tables
//! are QR format metadata; no external decoder implementation is included.
//! A decoded QR is untrusted text, never a payment authorization.
use super::StructuredAppend;
use super::payload;
use super::{Control, Error, Result};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

// Rows: L/M/Q/H. Columns: version 0 (unused), then 1 through 40.
const ECC: [[u8; 41]; 4] = [
    [
        0, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28,
        30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
    [
        0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28,
        28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
    ],
    [
        0, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30,
        30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
    [
        0, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24,
        30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
];
const BLOCKS: [[u8; 41]; 4] = [
    [
        0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13,
        14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25,
    ],
    [
        0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21,
        23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49,
    ],
    [
        0, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29,
        34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68,
    ],
    [
        0, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32,
        35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81,
    ],
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    pub version: u8,
    /// 0=L, 1=M, 2=Q, 3=H.
    pub level: u8,
    pub corrected_symbols: usize,
    pub structured: Option<StructuredAppend>,
    /// XOR of decoded input bytes, for structured-append parity verification.
    pub parity: u8,
}

fn invalid() -> Error {
    Error::Invalid("QR code is invalid or unsupported")
}
fn multiply(mut a: u8, mut b: u8) -> u8 {
    let mut result = 0;
    while b != 0 {
        if b & 1 != 0 {
            result ^= a;
        }
        a = (a << 1) ^ if a & 0x80 != 0 { 0x1d } else { 0 };
        b >>= 1;
    }
    result
}
fn power(mut value: u8, mut exponent: usize) -> u8 {
    let mut result = 1;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result = multiply(result, value);
        }
        value = multiply(value, value);
        exponent >>= 1;
    }
    result
}
fn divide(a: u8, b: u8) -> Result<u8> {
    if b == 0 {
        return Err(invalid());
    }
    Ok(multiply(a, power(b, 254)))
}
fn evaluate_high(coefficients: &[u8], x: u8) -> u8 {
    coefficients
        .iter()
        .fold(0, |value, &coefficient| multiply(value, x) ^ coefficient)
}
fn evaluate_low(coefficients: &[u8], x: u8) -> u8 {
    coefficients
        .iter()
        .rev()
        .fold(0, |value, &coefficient| multiply(value, x) ^ coefficient)
}

/// Berlekamp–Massey locator, Chien roots and a GF(256) Vandermonde solve for
/// magnitudes. Every corrected block is rechecked against all parity equations.
fn correct(block: &mut [u8], ecc: usize) -> Result<usize> {
    if block.is_empty() || block.len() > 255 || ecc == 0 || ecc > 30 || ecc >= block.len() {
        return Err(invalid());
    }
    let syndromes: Vec<u8> = (0..ecc)
        .map(|i| evaluate_high(block, power(2, i)))
        .collect();
    if syndromes.iter().all(|&value| value == 0) {
        return Ok(0);
    }
    let mut locator = vec![1u8];
    let mut prior = vec![1u8];
    let mut degree = 0;
    let mut shift = 1;
    let mut last_discrepancy = 1;
    for n in 0..ecc {
        let mut discrepancy = syndromes[n];
        for i in 1..=degree {
            discrepancy ^= multiply(locator.get(i).copied().unwrap_or(0), syndromes[n - i]);
        }
        if discrepancy == 0 {
            shift += 1;
            continue;
        }
        let old = locator.clone();
        let coefficient = divide(discrepancy, last_discrepancy)?;
        locator.resize(locator.len().max(prior.len() + shift), 0);
        for (i, &value) in prior.iter().enumerate() {
            locator[i + shift] ^= multiply(coefficient, value);
        }
        if 2 * degree <= n {
            degree = n + 1 - degree;
            prior = old;
            last_discrepancy = discrepancy;
            shift = 1;
        } else {
            shift += 1;
        }
    }
    if degree == 0 || 2 * degree > ecc {
        return Err(invalid());
    }
    while locator.last() == Some(&0) {
        locator.pop();
    }
    if locator.len() != degree + 1 {
        return Err(invalid());
    }
    let mut positions = Vec::new();
    let mut locations = Vec::new();
    for position in 0..block.len() {
        let location = power(2, block.len() - 1 - position);
        if evaluate_low(&locator, divide(1, location)?) == 0 {
            positions.push(position);
            locations.push(location);
        }
    }
    if positions.len() != degree {
        return Err(invalid());
    }
    let mut equations = vec![vec![0u8; degree + 1]; degree];
    for row in 0..degree {
        for (column, &location) in locations.iter().enumerate() {
            equations[row][column] = power(location, row);
        }
        equations[row][degree] = syndromes[row];
    }
    for column in 0..degree {
        let pivot = (column..degree)
            .find(|&row| equations[row][column] != 0)
            .ok_or_else(invalid)?;
        equations.swap(column, pivot);
        let factor = divide(1, equations[column][column])?;
        for cell in &mut equations[column][column..] {
            *cell = multiply(*cell, factor);
        }
        let pivot_row = equations[column].clone();
        for (row, equation) in equations.iter_mut().enumerate() {
            if row == column {
                continue;
            }
            let factor = equation[column];
            for (cell, &pivot) in equation[column..].iter_mut().zip(&pivot_row[column..]) {
                *cell ^= multiply(factor, pivot);
            }
        }
    }
    for (row, &position) in positions.iter().enumerate() {
        block[position] ^= equations[row][degree];
    }
    if (0..ecc).any(|i| evaluate_high(block, power(2, i)) != 0) {
        return Err(invalid());
    }
    Ok(degree)
}

struct Matrix<'a> {
    size: usize,
    modules: &'a [bool],
    rotation: usize,
    mirror: bool,
}
impl Matrix<'_> {
    fn get(&self, mut x: usize, mut y: usize) -> bool {
        if self.mirror {
            std::mem::swap(&mut x, &mut y);
        }
        for _ in 0..self.rotation {
            (x, y) = (self.size - 1 - y, x);
        }
        self.modules[y * self.size + x]
    }
}

fn bch_format(value: u32) -> u32 {
    let mut remainder = value;
    for _ in 0..10 {
        remainder = (remainder << 1) ^ if remainder & 0x200 != 0 { 0x537 } else { 0 };
    }
    ((value << 10) | remainder) ^ 0x5412
}
fn format_value(bits: u32) -> Option<(usize, usize)> {
    (0..32)
        .find(|&candidate| (bch_format(candidate) ^ bits).count_ones() <= 3)
        .map(|candidate| {
            let level = match candidate >> 3 {
                1 => 0,
                0 => 1,
                3 => 2,
                _ => 3,
            };
            (level, (candidate & 7) as usize)
        })
}
fn mask_value(mask: usize, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y).is_multiple_of(2),
        1 => y.is_multiple_of(2),
        2 => x.is_multiple_of(3),
        3 => (x + y).is_multiple_of(3),
        4 => (y / 2 + x / 3).is_multiple_of(2),
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3).is_multiple_of(2),
        _ => ((x + y) % 2 + x * y % 3).is_multiple_of(2),
    }
}
fn raw_modules(version: usize) -> usize {
    let mut count = (16 * version + 128) * version + 64;
    if version >= 2 {
        let alignment = version / 7 + 2;
        count -= (25 * alignment - 10) * alignment - 55;
        if version >= 7 {
            count -= 36;
        }
    }
    count
}

fn unpack(matrix: &Matrix<'_>, version: usize) -> Result<(Vec<u8>, usize, usize)> {
    let size = matrix.size;
    // Reject impossible orientations before expensive parity work. The three
    // finder patterns are fixed format structure, with bounded sampling damage.
    for (cx, cy) in [(3, 3), (size - 4, 3), (3, size - 4)] {
        let mut errors = 0;
        for dy in -3isize..=3 {
            for dx in -3isize..=3 {
                let expected = dx.abs().max(dy.abs()) != 2;
                errors += usize::from(
                    matrix.get((cx as isize + dx) as usize, (cy as isize + dy) as usize)
                        != expected,
                );
            }
        }
        if errors > 8 {
            return Err(invalid());
        }
    }
    let mut reserved = vec![false; size * size];
    let mut mark = |x: usize, y: usize| {
        reserved[y * size + x] = true;
    };
    for i in 0..size {
        mark(6, i);
        mark(i, 6);
    }
    for (cx, cy) in [(3, 3), (size - 4, 3), (3, size - 4)] {
        for y in (cy as isize - 4)..=(cy as isize + 4) {
            for x in (cx as isize - 4)..=(cx as isize + 4) {
                if x >= 0 && y >= 0 && x < size as isize && y < size as isize {
                    mark(x as usize, y as usize);
                }
            }
        }
    }
    if version >= 2 {
        let count = version / 7 + 2;
        let step = if version == 32 {
            26
        } else {
            (version * 4 + count * 2 + 1) / (count * 2 - 2) * 2
        };
        let mut centers = vec![6];
        for i in (0..count - 1).rev() {
            centers.push(size - 7 - i * step);
        }
        for (i, &x) in centers.iter().enumerate() {
            for (j, &y) in centers.iter().enumerate() {
                if (i == 0 && (j == 0 || j == count - 1)) || (i == count - 1 && j == 0) {
                    continue;
                }
                for dy in -2isize..=2 {
                    for dx in -2isize..=2 {
                        mark((x as isize + dx) as usize, (y as isize + dy) as usize);
                    }
                }
            }
        }
    }
    let mut first = Vec::new();
    first.extend((0..6).map(|i| (8, i)));
    first.extend([(8, 7), (8, 8), (7, 8)]);
    first.extend((9..15).map(|i| (14 - i, 8)));
    let mut second = Vec::new();
    second.extend((0..8).map(|i| (size - 1 - i, 8)));
    second.extend((8..15).map(|i| (8, size - 15 + i)));
    let read_format = |coordinates: &[(usize, usize)]| {
        coordinates
            .iter()
            .enumerate()
            .fold(0, |value, (i, &(x, y))| {
                value | (u32::from(matrix.get(x, y)) << i)
            })
    };
    let a = format_value(read_format(&first));
    let b = format_value(read_format(&second));
    let (level, mask) = match (a, b) {
        (Some(a), Some(b)) if a == b => a,
        (Some(a), None) | (None, Some(a)) => a,
        _ => return Err(invalid()),
    };
    for &(x, y) in first.iter().chain(&second) {
        mark(x, y);
    }
    mark(8, size - 8);
    if version >= 7 {
        let mut remainder = version as u32;
        for _ in 0..12 {
            remainder = (remainder << 1) ^ if remainder & 0x800 != 0 { 0x1f25 } else { 0 };
        }
        let expected = ((version as u32) << 12) | remainder;
        let mut a = 0u32;
        let mut b = 0u32;
        for i in 0..18 {
            let x = size - 11 + i % 3;
            let y = i / 3;
            a |= u32::from(matrix.get(x, y)) << i;
            b |= u32::from(matrix.get(y, x)) << i;
            mark(x, y);
            mark(y, x);
        }
        if (a ^ expected).count_ones() > 3 && (b ^ expected).count_ones() > 3 {
            return Err(invalid());
        }
    }
    let raw = raw_modules(version) / 8;
    let mut bytes = vec![0u8; raw];
    let mut bit = 0;
    let mut right = size - 1;
    loop {
        if right == 6 {
            right = 5;
        }
        for vertical in 0..size {
            let y = if (right + 1) & 2 == 0 {
                size - 1 - vertical
            } else {
                vertical
            };
            for column in 0..2 {
                let x = right - column;
                if !reserved[y * size + x] {
                    if bit < raw * 8 {
                        bytes[bit / 8] |=
                            u8::from(matrix.get(x, y) ^ mask_value(mask, x, y)) << (7 - bit % 8);
                    }
                    bit += 1;
                }
            }
        }
        if right < 2 {
            break;
        }
        right -= 2;
    }
    if bit != raw_modules(version) {
        return Err(invalid());
    }
    let count = BLOCKS[level][version] as usize;
    let ecc = ECC[level][version] as usize;
    let short_count = count - raw % count;
    let short_data = raw / count - ecc;
    let lengths: Vec<_> = (0..count)
        .map(|i| short_data + usize::from(i >= short_count))
        .collect();
    let mut blocks: Vec<_> = lengths
        .iter()
        .map(|&length| vec![0u8; length + ecc])
        .collect();
    let mut offset = 0;
    for column in 0..=short_data {
        for (i, block) in blocks.iter_mut().enumerate() {
            if column < lengths[i] {
                block[column] = bytes[offset];
                offset += 1;
            }
        }
    }
    for column in 0..ecc {
        for (i, block) in blocks.iter_mut().enumerate() {
            block[lengths[i] + column] = bytes[offset];
            offset += 1;
        }
    }
    if offset != raw {
        return Err(invalid());
    }
    let mut data = Vec::with_capacity(raw - ecc * count);
    let mut corrected = 0;
    for (block, &length) in blocks.iter_mut().zip(&lengths) {
        corrected += correct(block, ecc)?;
        data.extend_from_slice(&block[..length]);
    }
    Ok((data, level, corrected))
}

/// The caller provides exactly one sampled, square QR matrix, excluding quiet
/// zone. Rotation/mirroring are recognized; conflicting valid payloads reject.
pub fn decode_modules(size: usize, modules: &[bool]) -> Result<Decoded> {
    let cancelled = AtomicBool::new(false);
    decode_modules_control(
        size,
        modules,
        Control::new(&cancelled, Instant::now() + Duration::from_secs(1)),
    )
}

pub fn decode_modules_control(
    size: usize,
    modules: &[bool],
    control: Control<'_>,
) -> Result<Decoded> {
    control.check()?;
    if !(21..=177).contains(&size) || !(size - 17).is_multiple_of(4) || modules.len() != size * size
    {
        return Err(invalid());
    }
    let version = (size - 17) / 4;
    let mut decoded: Option<Decoded> = None;
    for mirror in [false, true] {
        for rotation in 0..4 {
            control.check()?;
            let matrix = Matrix {
                size,
                modules,
                rotation,
                mirror,
            };
            if let Ok((data, level, corrected_symbols)) = unpack(&matrix, version)
                && let Ok(payload) = payload::decode(&data, version)
            {
                if decoded.as_ref().is_some_and(|prior| {
                    prior.text != payload.text || prior.structured != payload.structured
                }) {
                    return Err(Error::Ambiguous);
                }
                if decoded
                    .as_ref()
                    .is_none_or(|prior| prior.corrected_symbols > corrected_symbols)
                {
                    decoded = Some(Decoded {
                        text: payload.text,
                        structured: payload.structured,
                        parity: payload.parity,
                        version: version as u8,
                        level: level as u8,
                        corrected_symbols,
                    });
                }
            }
        }
    }
    control.check()?;
    decoded.ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parity(data: &[u8], ecc: usize) -> Vec<u8> {
        let mut generator = vec![1];
        for i in 0..ecc {
            let root = power(2, i);
            let mut next = vec![0; generator.len() + 1];
            for (j, &coefficient) in generator.iter().enumerate() {
                next[j] ^= coefficient;
                next[j + 1] ^= multiply(coefficient, root);
            }
            generator = next;
        }
        let mut block = data.to_vec();
        block.resize(data.len() + ecc, 0);
        for i in 0..data.len() {
            let coefficient = block[i];
            for j in 0..generator.len() {
                block[i + j] ^= multiply(coefficient, generator[j]);
            }
        }
        block[data.len()..].to_vec()
    }
    #[test]
    fn rs_corrects_symbols_at_each_supported_parity_degree() {
        for ecc in [7, 10, 13, 16, 18, 20, 22, 24, 26, 28, 30] {
            let data: Vec<_> = (0..80).map(|i| (i * 73 + 17) as u8).collect();
            let mut code = data.clone();
            code.extend(parity(&data, ecc));
            assert_eq!(correct(&mut code, ecc).unwrap(), 0);
            let expected = code.clone();
            for i in 0..ecc / 2 {
                code[i * 3] ^= (i + 1) as u8;
            }
            assert_eq!(correct(&mut code, ecc).unwrap(), ecc / 2);
            assert_eq!(code, expected);
        }
    }
    #[test]
    fn bounded_matrix_and_segment_failures_reject() {
        for size in [0, 20, 22, 178, usize::MAX] {
            assert!(decode_modules(size, &[]).is_err());
        }
        assert!(decode_modules(21, &[false; 441]).is_err());
        assert!(payload::decode(&[0x30, 0], 1).is_err()); // structured append must not become a partial invoice
        assert!(payload::decode(&[0x40, 0xff], 1).is_err());
        assert!(payload::decode(&[0x7f, 0xff], 1).is_err()); // malformed ECI
    }
}
