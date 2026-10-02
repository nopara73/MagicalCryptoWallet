//! QR Model 2 (ISO/IEC 18004): numeric, alphanumeric and UTF-8 byte segments.
//! Coordinate access is (x,y); the service representation is row-major bytes.
#![forbid(unsafe_code)]
mod tables;
use tables::{BLOCKS, ECC};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ecc {
    L,
    M,
    Q,
    H,
}
impl Ecc {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "L" => Some(Self::L),
            "M" => Some(Self::M),
            "Q" => Some(Self::Q),
            "H" => Some(Self::H),
            _ => None,
        }
    }
    pub fn from_byte(value: u8) -> Option<Self> {
        [Self::L, Self::M, Self::Q, Self::H]
            .get(value as usize)
            .copied()
    }
    fn format(self) -> u32 {
        [1, 0, 3, 2][self as usize]
    }
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub version: u8,
    pub ecc: Ecc,
    pub width: usize,
    pub modules: Vec<u8>,
}
#[derive(Clone, Copy)]
enum Mode {
    Numeric,
    Alphanumeric,
    Byte,
}
const ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
impl Mode {
    fn for_text(text: &str) -> Self {
        if text.bytes().all(|byte| byte.is_ascii_digit()) {
            Self::Numeric
        } else if text.bytes().all(|byte| ALPHANUMERIC.contains(&byte)) {
            Self::Alphanumeric
        } else {
            Self::Byte
        }
    }
    fn header(self, version: usize) -> usize {
        let group = usize::from(version >= 10) + usize::from(version >= 27);
        match self {
            Self::Numeric => [10, 12, 14][group],
            Self::Alphanumeric => [9, 11, 13][group],
            Self::Byte => [8, 16, 16][group],
        }
    }
    fn bits(self, length: usize) -> usize {
        match self {
            Self::Numeric => length / 3 * 10 + [0, 4, 7][length % 3],
            Self::Alphanumeric => length / 2 * 11 + length % 2 * 6,
            Self::Byte => length * 8,
        }
    }
    fn tag(self) -> u32 {
        match self {
            Self::Numeric => 1,
            Self::Alphanumeric => 2,
            Self::Byte => 4,
        }
    }
}

pub fn encode(text: &str, ecc: Ecc) -> Result<Symbol, &'static str> {
    if text.is_empty() {
        return Err("QR content is empty");
    }
    // The largest supported numeric symbol has 7089 digits. Bound work before allocation.
    if text.len() > 7089 {
        return Err("QR content exceeds version 40 capacity");
    }
    let mode = Mode::for_text(text);
    let eci = !text.is_ascii();
    let version = (1..=40)
        .find(|&v| {
            text.len() < (1 << mode.header(v))
                && 4 + mode.header(v) + mode.bits(text.len()) + if eci { 12 } else { 0 }
                    <= capacity(v, ecc) * 8
        })
        .ok_or("QR content exceeds version 40 capacity")?;
    let mut bits = Vec::with_capacity(capacity(version, ecc) * 8);
    if eci {
        push(&mut bits, 7, 4);
        push(&mut bits, 26, 8);
    }
    push(&mut bits, mode.tag(), 4);
    push(&mut bits, text.len() as u32, mode.header(version));
    match mode {
        Mode::Numeric => {
            for chunk in text.as_bytes().chunks(3) {
                let number = chunk.iter().fold(0u32, |v, b| v * 10 + u32::from(b - b'0'));
                push(&mut bits, number, [0, 4, 7, 10][chunk.len()]);
            }
        }
        Mode::Alphanumeric => {
            for chunk in text.as_bytes().chunks(2) {
                let number = chunk.iter().fold(0u32, |v, b| {
                    v * 45 + ALPHANUMERIC.iter().position(|c| c == b).unwrap() as u32
                });
                push(&mut bits, number, if chunk.len() == 2 { 11 } else { 6 });
            }
        }
        Mode::Byte => {
            for byte in text.bytes() {
                push(&mut bits, u32::from(byte), 8);
            }
        }
    }
    let remaining = capacity(version, ecc) * 8 - bits.len();
    push(&mut bits, 0, remaining.min(4));
    while !bits.len().is_multiple_of(8) {
        bits.push(false);
    }
    let mut data: Vec<u8> = bits
        .chunks(8)
        .map(|chunk| chunk.iter().fold(0, |v, b| v * 2 + u8::from(*b)))
        .collect();
    let mut pad = 0xec;
    while data.len() < capacity(version, ecc) {
        data.push(pad);
        pad ^= 0xec ^ 0x11;
    }
    let interleaved = interleave(&data, version, ecc);
    let width = version * 4 + 17;
    let mut matrix = Matrix {
        symbol: Symbol {
            version: version as u8,
            ecc,
            width,
            modules: vec![0; width * width],
        },
        functions: vec![false; width * width],
    };
    matrix.patterns(version);
    matrix.place(&interleaved);
    let original = matrix.symbol.modules.clone();
    let mut best = usize::MAX;
    let mut modules = Vec::new();
    for mask in 0..8 {
        matrix.symbol.modules.clone_from(&original);
        matrix.mask(mask);
        matrix.format(mask);
        let score = matrix.penalty();
        if score < best {
            best = score;
            modules.clone_from(&matrix.symbol.modules);
        }
    }
    matrix.symbol.modules = modules;
    Ok(matrix.symbol)
}

fn raw_modules(v: usize) -> usize {
    let mut count = (16 * v + 128) * v + 64;
    if v >= 2 {
        let align = v / 7 + 2;
        count -= (25 * align - 10) * align - 55;
        if v >= 7 {
            count -= 36;
        }
    }
    count
}
fn capacity(v: usize, ecc: Ecc) -> usize {
    raw_modules(v) / 8 - usize::from(ECC[ecc as usize][v]) * usize::from(BLOCKS[ecc as usize][v])
}
fn push(bits: &mut Vec<bool>, value: u32, count: usize) {
    for i in (0..count).rev() {
        bits.push((value >> i) & 1 != 0);
    }
}
fn multiply(mut a: u8, mut b: u8) -> u8 {
    let mut result = 0;
    for _ in 0..8 {
        result ^= a.wrapping_mul(b & 1);
        a = (a << 1) ^ if a & 0x80 != 0 { 0x1d } else { 0 };
        b >>= 1;
    }
    result
}
fn parity(data: &[u8], degree: usize) -> Vec<u8> {
    let mut divisor = vec![0; degree];
    divisor[degree - 1] = 1;
    let mut root = 1;
    for _ in 0..degree {
        for i in 0..degree {
            divisor[i] = multiply(divisor[i], root);
            if i + 1 < degree {
                divisor[i] ^= divisor[i + 1];
            }
        }
        root = multiply(root, 2);
    }
    let mut result = vec![0; degree];
    for byte in data {
        let factor = byte ^ result[0];
        result.rotate_left(1);
        result[degree - 1] = 0;
        for (item, coefficient) in result.iter_mut().zip(&divisor) {
            *item ^= multiply(*coefficient, factor);
        }
    }
    result
}
fn interleave(data: &[u8], version: usize, level: Ecc) -> Vec<u8> {
    let raw = raw_modules(version) / 8;
    let blocks = usize::from(BLOCKS[level as usize][version]);
    let ecc = usize::from(ECC[level as usize][version]);
    let short_count = blocks - raw % blocks;
    let short_data = raw / blocks - ecc;
    let mut groups = Vec::new();
    let mut checks = Vec::new();
    let mut offset = 0;
    for i in 0..blocks {
        let length = short_data + usize::from(i >= short_count);
        let group = &data[offset..offset + length];
        groups.push(group);
        checks.push(parity(group, ecc));
        offset += length;
    }
    let mut result = Vec::with_capacity(raw);
    for i in 0..=short_data {
        for group in &groups {
            if let Some(byte) = group.get(i) {
                result.push(*byte);
            }
        }
    }
    for i in 0..ecc {
        for check in &checks {
            result.push(check[i]);
        }
    }
    debug_assert_eq!(result.len(), raw);
    result
}
struct Matrix {
    symbol: Symbol,
    functions: Vec<bool>,
}
impl Matrix {
    fn dark(&self, x: usize, y: usize) -> bool {
        self.symbol.modules[y * self.symbol.width + x] != 0
    }
    fn function(&mut self, x: usize, y: usize, dark: bool) {
        let index = y * self.symbol.width + x;
        self.symbol.modules[index] = u8::from(dark);
        self.functions[index] = true;
    }
    fn patterns(&mut self, version: usize) {
        let size = self.symbol.width;
        for i in 0..size {
            self.function(6, i, i % 2 == 0);
            self.function(i, 6, i % 2 == 0);
        }
        for (cx, cy) in [(3, 3), (size - 4, 3), (3, size - 4)] {
            for dy in -4isize..=4 {
                for dx in -4isize..=4 {
                    let (x, y) = (cx as isize + dx, cy as isize + dy);
                    if x >= 0 && y >= 0 && x < size as isize && y < size as isize {
                        let distance = dx.abs().max(dy.abs());
                        self.function(x as usize, y as usize, distance != 2 && distance != 4);
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
                            self.function(
                                (x as isize + dx) as usize,
                                (y as isize + dy) as usize,
                                dx.abs().max(dy.abs()) != 1,
                            );
                        }
                    }
                }
            }
        }
        self.format(0);
        if version >= 7 {
            let mut remainder = version as u32;
            for _ in 0..12 {
                remainder = (remainder << 1) ^ if remainder >> 11 != 0 { 0x1f25 } else { 0 };
            }
            let bits = ((version as u32) << 12) | remainder;
            for i in 0..18 {
                let dark = (bits >> i) & 1 != 0;
                let (a, b) = (size - 11 + i % 3, i / 3);
                self.function(a, b, dark);
                self.function(b, a, dark);
            }
        }
    }
    fn format(&mut self, mask: usize) {
        let size = self.symbol.width;
        let data = (self.symbol.ecc.format() << 3) | mask as u32;
        let mut remainder = data;
        for _ in 0..10 {
            remainder = (remainder << 1) ^ if remainder >> 9 != 0 { 0x537 } else { 0 };
        }
        let bits = ((data << 10) | remainder) ^ 0x5412;
        let bit = |i| (bits >> i) & 1 != 0;
        for i in 0..6 {
            self.function(8, i, bit(i));
        }
        self.function(8, 7, bit(6));
        self.function(8, 8, bit(7));
        self.function(7, 8, bit(8));
        for i in 9..15 {
            self.function(14 - i, 8, bit(i));
        }
        for i in 0..8 {
            self.function(size - 1 - i, 8, bit(i));
        }
        for i in 8..15 {
            self.function(8, size - 15 + i, bit(i));
        }
        self.function(8, size - 8, true);
    }
    fn place(&mut self, data: &[u8]) {
        let size = self.symbol.width;
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
                for x in [right, right - 1] {
                    let index = y * size + x;
                    if !self.functions[index] && bit < data.len() * 8 {
                        self.symbol.modules[index] = (data[bit / 8] >> (7 - bit % 8)) & 1;
                        bit += 1;
                    }
                }
            }
            if right < 2 {
                break;
            }
            right -= 2;
        }
        debug_assert_eq!(bit, data.len() * 8);
    }
    fn mask(&mut self, mask: usize) {
        let size = self.symbol.width;
        for y in 0..size {
            for x in 0..size {
                let invert = match mask {
                    0 => (x + y) % 2 == 0,
                    1 => y % 2 == 0,
                    2 => x % 3 == 0,
                    3 => (x + y) % 3 == 0,
                    4 => (x / 3 + y / 2) % 2 == 0,
                    5 => x * y % 2 + x * y % 3 == 0,
                    6 => (x * y % 2 + x * y % 3) % 2 == 0,
                    7 => ((x + y) % 2 + x * y % 3) % 2 == 0,
                    _ => unreachable!(),
                };
                let index = y * size + x;
                if !self.functions[index] && invert {
                    self.symbol.modules[index] ^= 1;
                }
            }
        }
    }
    fn penalty(&self) -> usize {
        let size = self.symbol.width;
        let mut penalty = 0;
        // N1 and N3. Count scaled 1:1:3:1:1 finder-like runs with four light
        // modules on either side (the off-symbol quiet zone is also light).
        for axis in 0..2 {
            for line in 0..size {
                let mut runs: Vec<(bool, usize)> = vec![(false, size)];
                let mut color = false;
                let mut length = 0;
                for offset in 0..size {
                    let dark = if axis == 0 {
                        self.dark(offset, line)
                    } else {
                        self.dark(line, offset)
                    };
                    if offset == 0 {
                        color = dark;
                        length = 1;
                    } else if dark == color {
                        length += 1;
                        if length == 5 {
                            penalty += 3;
                        } else if length > 5 {
                            penalty += 1;
                        }
                    } else {
                        runs.push((color, length));
                        color = dark;
                        length = 1;
                    }
                }
                runs.push((color, length));
                runs.push((false, size));
                if runs[1].0 == runs[0].0 {
                    let first = runs.remove(0);
                    runs[0].1 += first.1;
                }
                let last = runs.len() - 1;
                if runs[last].0 == runs[last - 1].0 {
                    let end = runs.pop().unwrap();
                    let last = runs.len() - 1;
                    runs[last].1 += end.1;
                }
                for window in runs.windows(7) {
                    let unit = window[1].1;
                    if !window[0].0
                        && window[1].0
                        && window[2].1 == unit
                        && window[3].1 == 3 * unit
                        && window[4].1 == unit
                        && window[5].1 == unit
                    {
                        if window[0].1 >= 4 * unit && window[6].1 >= unit {
                            penalty += 40;
                        }
                        if window[6].1 >= 4 * unit && window[0].1 >= unit {
                            penalty += 40;
                        }
                    }
                }
            }
        }
        // N2: 2x2 same-color squares.
        for y in 0..size - 1 {
            for x in 0..size - 1 {
                let dark = self.dark(x, y);
                if dark == self.dark(x + 1, y)
                    && dark == self.dark(x, y + 1)
                    && dark == self.dark(x + 1, y + 1)
                {
                    penalty += 3;
                }
            }
        }
        // N4: each complete five-percent departure from 50 percent dark.
        let dark: usize = self.symbol.modules.iter().map(|b| usize::from(*b)).sum();
        penalty + (dark * 20).abs_diff(size * size * 10) / (size * size) * 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacities_and_boundaries() {
        for level in [Ecc::L, Ecc::M, Ecc::Q, Ecc::H] {
            for version in 1..=40 {
                let length = capacity(version, level) - if version < 10 { 2 } else { 3 };
                assert_eq!(
                    encode(&"a".repeat(length), level).unwrap().version as usize,
                    version
                );
                if version < 40 {
                    assert_eq!(
                        encode(&"a".repeat(length + 1), level).unwrap().version as usize,
                        version + 1
                    );
                }
            }
        }
        assert_eq!(encode(&"1".repeat(7089), Ecc::L).unwrap().version, 40);
        assert!(encode(&"1".repeat(7090), Ecc::L).is_err());
        assert!(encode("", Ecc::M).is_err());
    }
    #[test]
    fn exact_text_and_determinism() {
        for text in [
            "1234567890",
            "HELLO WORLD",
            "mixed Case\n",
            "你好 🦀",
            "bitcoin:bc1qtest?label=Hello%20World",
        ] {
            let a = encode(text, Ecc::M).unwrap();
            let b = encode(text, Ecc::M).unwrap();
            assert_eq!(a.modules, b.modules);
            assert!(a.modules.iter().all(|b| *b <= 1));
        }
    }
}
