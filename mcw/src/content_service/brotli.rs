//! Independently implemented RFC7932 decoder. Buffered, bounded application
//! bodies, cancellable within work slices; no output result escapes on failure.
//! Static dictionary/context/transform assets are normative RFC format data.
#![forbid(unsafe_code)]
use super::Abort;
use crate::compression::{Limit, Limits, TrailingData};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Truncated,
    TrailingData,
    InvalidWindow,
    InvalidPadding,
    InvalidMetaBlock,
    InvalidHuffman,
    InvalidSymbol,
    InvalidRepeat,
    InvalidBlockType,
    InvalidContextMap,
    InvalidDistance,
    InvalidDictionary,
    MetaBlockLimit,
    LimitExceeded(Limit),
    AllocationFailed,
    Aborted(Abort),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub input_consumed: u64,
    pub output_produced: u64,
    pub byte_offset: usize,
    pub bit_offset: u8,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Brotli {:?} at {}:{} ({} input, {} output)",
            self.kind, self.byte_offset, self.bit_offset, self.input_consumed, self.output_produced
        )
    }
}
impl std::error::Error for Error {}
pub struct Decoded {
    pub bytes: Vec<u8>,
    pub consumed: usize,
    pub work: u64,
    pub meta_blocks: u32,
}
impl fmt::Debug for Decoded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoded")
            .field("output_len", &self.bytes.len())
            .field("consumed", &self.consumed)
            .field("work", &self.work)
            .field("meta_blocks", &self.meta_blocks)
            .finish()
    }
}

struct Transform {
    prefix: &'static [u8],
    kind: u8,
    suffix: &'static [u8],
}
mod tables {
    include!("data/tables.rs");
}
const DICTIONARY: &[u8; 122784] = include_bytes!("data/dictionary.bin");
const CODE_ORDER: [usize; 18] = [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const INSERT_BASE: [usize; 24] = [
    0, 1, 2, 3, 4, 5, 6, 8, 10, 14, 18, 26, 34, 50, 66, 98, 130, 194, 322, 578, 1090, 2114, 6210,
    22594,
];
const INSERT_EXTRA: [u8; 24] = [
    0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 8, 9, 10, 12, 14, 24,
];
const COPY_BASE: [usize; 24] = [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 18, 22, 30, 38, 54, 70, 102, 134, 198, 326, 582, 1094, 2118,
];
const COPY_EXTRA: [u8; 24] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 8, 9, 10, 24,
];
const COUNT_BASE: [usize; 26] = [
    1, 5, 9, 13, 17, 25, 33, 41, 49, 65, 81, 97, 113, 145, 177, 209, 241, 305, 369, 497, 753, 1265,
    2289, 4337, 8433, 16625,
];
const COUNT_EXTRA: [u8; 26] = [
    2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 7, 8, 9, 10, 11, 12, 13, 24,
];
// Conservative fixed parser/stack allowance, separate from fallible Vec capacity.
const FIXED_MEMORY: usize = 32768;

struct Reader<'a, 'c> {
    input: &'a [u8],
    bit: usize,
    work: u64,
    checked: u64,
    output: usize,
    output_capacity: usize,
    limits: Limits,
    tables: usize,
    check: &'c mut dyn FnMut() -> Result<(), Abort>,
}
impl Reader<'_, '_> {
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            input_consumed: self.bit.div_ceil(8) as u64,
            output_produced: self.output as u64,
            byte_offset: self.bit / 8,
            bit_offset: (self.bit % 8) as u8,
        }
    }
    fn charge(&mut self, n: u64) -> Result<(), Error> {
        self.work = self
            .work
            .checked_add(n)
            .filter(|&v| v <= self.limits.max_work)
            .ok_or_else(|| self.error(ErrorKind::LimitExceeded(Limit::Work)))?;
        if self.work - self.checked >= 4096 {
            (self.check)().map_err(|reason| self.error(ErrorKind::Aborted(reason)))?;
            self.checked = self.work;
        }
        Ok(())
    }
    fn read(&mut self, n: u8) -> Result<u32, Error> {
        let end = self
            .bit
            .checked_add(usize::from(n))
            .ok_or_else(|| self.error(ErrorKind::Truncated))?;
        let bytes = end.div_ceil(8);
        if bytes > self.input.len() {
            return Err(self.error(ErrorKind::Truncated));
        }
        if bytes as u64 > self.limits.max_input_bytes {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Input)));
        }
        self.charge(u64::from(n) + 1)?;
        let mut value = 0;
        for p in 0..n {
            value |= u32::from(
                (self.input[(self.bit + usize::from(p)) / 8] >> ((self.bit + usize::from(p)) % 8))
                    & 1,
            ) << p;
        }
        self.bit = end;
        Ok(value)
    }
    fn align(&mut self) -> Result<(), Error> {
        let n = ((8 - self.bit % 8) % 8) as u8;
        if self.read(n)? != 0 {
            return Err(self.error(ErrorKind::InvalidPadding));
        }
        Ok(())
    }
    fn variable(&mut self) -> Result<usize, Error> {
        if self.read(1)? == 0 {
            return Ok(0);
        }
        let n = self.read(3)? as u8;
        if n == 0 {
            Ok(1)
        } else {
            Ok((1usize << n) + self.read(n)? as usize)
        }
    }
    fn reserve_table<T>(&mut self, bytes: &mut Vec<T>, size: usize) -> Result<(), Error> {
        let extra = size
            .saturating_sub(bytes.capacity())
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.error(ErrorKind::LimitExceeded(Limit::Allocation)))?;
        if self
            .tables
            .checked_add(extra)
            .and_then(|n| n.checked_add(self.output_capacity))
            .is_none_or(|n| n > self.limits.max_allocation_bytes)
        {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Allocation)));
        }
        self.charge(size as u64)?;
        let old = bytes.capacity();
        if size > old {
            bytes
                .try_reserve_exact(size - bytes.len())
                .map_err(|_| self.error(ErrorKind::AllocationFailed))?;
        }
        self.tables += (bytes.capacity() - old) * std::mem::size_of::<T>();
        if self.tables.saturating_add(self.output_capacity) > self.limits.max_allocation_bytes {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Allocation)));
        }
        Ok(())
    }
    fn emit(&mut self, bytes: &mut Vec<u8>, value: u8) -> Result<(), Error> {
        if bytes.len() as u64 >= self.limits.max_output_bytes {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Output)));
        }
        let allowed = (self.bit.div_ceil(8) as u64)
            .saturating_mul(self.limits.max_expansion_ratio)
            .saturating_add(self.limits.expansion_slack_bytes);
        if bytes.len() as u64 >= allowed {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Expansion)));
        }
        self.charge(2)?;
        if bytes.len() == bytes.capacity() {
            let budget = self.limits.max_allocation_bytes.saturating_sub(self.tables);
            if bytes.len() >= budget {
                return Err(self.error(ErrorKind::LimitExceeded(Limit::Allocation)));
            }
            let size = (bytes.len() + 1)
                .max(bytes.capacity().saturating_mul(2))
                .max(256)
                .min(budget);
            self.charge(bytes.len() as u64)?;
            bytes
                .try_reserve_exact(size - bytes.len())
                .map_err(|_| self.error(ErrorKind::AllocationFailed))?;
            self.output_capacity = bytes.capacity();
            if bytes.capacity() > budget {
                return Err(self.error(ErrorKind::LimitExceeded(Limit::Allocation)));
            }
        }
        bytes.push(value);
        self.output = bytes.len();
        Ok(())
    }
}

struct Huffman {
    counts: [u16; 16],
    first: [u16; 16],
    start: [u16; 16],
    symbols: [u16; 704],
    max: u8,
    single: Option<u16>,
}
impl Huffman {
    fn empty() -> Self {
        Self {
            counts: [0; 16],
            first: [0; 16],
            start: [0; 16],
            symbols: [0; 704],
            max: 0,
            single: None,
        }
    }
    fn single(value: u16) -> Self {
        let mut h = Self::empty();
        h.single = Some(value);
        h
    }
    fn build(lengths: &[u8], allow_single: bool, r: &Reader<'_, '_>) -> Result<Self, Error> {
        let mut h = Self::empty();
        let mut used = 0;
        let mut last = 0;
        for (s, &n) in lengths.iter().enumerate() {
            if n > 15 {
                return Err(r.error(ErrorKind::InvalidHuffman));
            }
            if n != 0 {
                h.counts[n as usize] += 1;
                h.max = h.max.max(n);
                used += 1;
                last = s;
            }
        }
        if used == 1 && allow_single {
            return Ok(Self::single(last as u16));
        }
        if used < 2 {
            return Err(r.error(ErrorKind::InvalidHuffman));
        }
        let mut space = 1i32;
        for n in 1..=15 {
            space = (space << 1) - i32::from(h.counts[n]);
            if space < 0 {
                return Err(r.error(ErrorKind::InvalidHuffman));
            }
        }
        if space != 0 {
            return Err(r.error(ErrorKind::InvalidHuffman));
        }
        let mut code = 0;
        let mut start = 0;
        for n in 1..=15 {
            code = (code + h.counts[n - 1]) << 1;
            h.first[n] = code;
            h.start[n] = start;
            start += h.counts[n];
        }
        let mut next = h.start;
        for (s, &n) in lengths.iter().enumerate() {
            if n > 0 {
                h.symbols[next[n as usize] as usize] = s as u16;
                next[n as usize] += 1;
            }
        }
        Ok(h)
    }
    fn decode(&self, r: &mut Reader<'_, '_>) -> Result<usize, Error> {
        r.charge(1)?;
        if let Some(s) = self.single {
            return Ok(usize::from(s));
        }
        let mut code = 0u16;
        for n in 1..=self.max {
            code = (code << 1) | r.read(1)? as u16;
            if let Some(delta) = code.checked_sub(self.first[n as usize])
                && delta < self.counts[n as usize]
            {
                return Ok(self.symbols[(self.start[n as usize] + delta) as usize] as usize);
            }
        }
        Err(r.error(ErrorKind::InvalidHuffman))
    }
}

fn prefix(r: &mut Reader<'_, '_>, alphabet: usize) -> Result<Huffman, Error> {
    r.charge(704)?;
    if !(2..=704).contains(&alphabet) {
        return Err(r.error(ErrorKind::InvalidHuffman));
    }
    let mode = r.read(2)? as usize;
    let mut lengths = [0; 704];
    if mode == 1 {
        let n = r.read(2)? as usize + 1;
        let width = (usize::BITS - (alphabet - 1).leading_zeros()) as u8;
        let mut values = [0; 4];
        for i in 0..n {
            let symbol = r.read(width)? as usize;
            if symbol >= alphabet || values[..i].contains(&symbol) {
                return Err(r.error(ErrorKind::InvalidSymbol));
            }
            values[i] = symbol;
        }
        if n == 1 {
            return Ok(Huffman::single(values[0] as u16));
        }
        let shape = match n {
            2 => [1, 1, 0, 0],
            3 => [1, 2, 2, 0],
            _ => {
                if r.read(1)? == 0 {
                    [2, 2, 2, 2]
                } else {
                    [1, 2, 3, 3]
                }
            }
        };
        for i in 0..n {
            lengths[values[i]] = shape[i];
        }
        return Huffman::build(&lengths[..alphabet], false, r);
    }
    let mut code_lengths = [0; 18];
    let mut space = 32i32;
    for &symbol in &CODE_ORDER[mode..] {
        let length = match r.read(2)? {
            0 => 0,
            1 => 4,
            2 => 3,
            _ => {
                if r.read(1)? == 0 {
                    2
                } else if r.read(1)? == 0 {
                    1
                } else {
                    5
                }
            }
        };
        code_lengths[symbol] = length;
        if length > 0 {
            space -= 32 >> length;
            if space < 0 {
                return Err(r.error(ErrorKind::InvalidHuffman));
            }
        }
        if space == 0 {
            break;
        }
    }
    if space != 0 && code_lengths.iter().filter(|&&n| n > 0).count() != 1 {
        return Err(r.error(ErrorKind::InvalidHuffman));
    }
    let code = Huffman::build(&code_lengths, true, r)?;
    let mut at = 0usize;
    let mut space = 32768usize;
    let mut previous = 8u8;
    let mut repeat = 0usize;
    let mut last_repeat = usize::MAX;
    while at < alphabet && space > 0 {
        let symbol = code.decode(r)?;
        if symbol <= 15 {
            lengths[at] = symbol as u8;
            at += 1;
            repeat = 0;
            last_repeat = usize::MAX;
            if symbol != 0 {
                previous = symbol as u8;
                let weight = 32768 >> symbol;
                space = space
                    .checked_sub(weight)
                    .ok_or_else(|| r.error(ErrorKind::InvalidHuffman))?;
            }
        } else {
            let (value, bits) = match symbol {
                16 => (previous, 2),
                17 => (0, 3),
                _ => return Err(r.error(ErrorKind::InvalidRepeat)),
            };
            let addition = r.read(bits)? as usize + 3;
            let old = if last_repeat == symbol { repeat } else { 0 };
            repeat = if old == 0 {
                addition
            } else {
                old.checked_sub(2)
                    .and_then(|n| n.checked_shl(bits as u32))
                    .and_then(|n| n.checked_add(addition))
                    .ok_or_else(|| r.error(ErrorKind::InvalidRepeat))?
            };
            let delta = repeat
                .checked_sub(old)
                .ok_or_else(|| r.error(ErrorKind::InvalidRepeat))?;
            if delta > alphabet - at {
                return Err(r.error(ErrorKind::InvalidRepeat));
            }
            r.charge(delta as u64)?;
            lengths[at..at + delta].fill(value);
            at += delta;
            last_repeat = symbol;
            if value != 0 {
                space = space
                    .checked_sub(delta * (32768 >> value))
                    .ok_or_else(|| r.error(ErrorKind::InvalidHuffman))?;
            }
        }
    }
    if space != 0 {
        return Err(r.error(ErrorKind::InvalidHuffman));
    }
    Huffman::build(&lengths[..alphabet], false, r)
}

struct Blocks {
    types: usize,
    current: usize,
    previous: usize,
    count: usize,
    type_code: Huffman,
    count_code: Huffman,
}
fn block_count(code: &Huffman, r: &mut Reader<'_, '_>) -> Result<usize, Error> {
    let value = code.decode(r)?;
    if value >= 26 {
        return Err(r.error(ErrorKind::InvalidSymbol));
    }
    Ok(COUNT_BASE[value] + r.read(COUNT_EXTRA[value])? as usize)
}
impl Blocks {
    fn new(r: &mut Reader<'_, '_>) -> Result<Self, Error> {
        let types = r.variable()? + 1;
        let (type_code, count_code, count) = if types == 1 {
            (Huffman::empty(), Huffman::empty(), usize::MAX)
        } else {
            let t = prefix(r, types + 2)?;
            let c = prefix(r, 26)?;
            let n = block_count(&c, r)?;
            (t, c, n)
        };
        Ok(Self {
            types,
            current: 0,
            previous: 1,
            count,
            type_code,
            count_code,
        })
    }
    fn next(&mut self, r: &mut Reader<'_, '_>) -> Result<usize, Error> {
        if self.count == 0 {
            let s = self.type_code.decode(r)?;
            let next = match s {
                0 => self.previous,
                1 => (self.current + 1) % self.types,
                _ => s - 2,
            };
            if next >= self.types || next == self.current {
                return Err(r.error(ErrorKind::InvalidBlockType));
            }
            self.previous = self.current;
            self.current = next;
            self.count = block_count(&self.count_code, r)?;
        }
        if self.types != 1 {
            self.count -= 1;
        }
        Ok(self.current)
    }
}

fn context_map(r: &mut Reader<'_, '_>, size: usize, trees: usize) -> Result<Vec<u8>, Error> {
    let mut map = Vec::new();
    r.reserve_table(&mut map, size)?;
    map.resize(size, 0);
    if trees == 1 {
        return Ok(map);
    }
    let rle = if r.read(1)? == 0 {
        0
    } else {
        r.read(4)? as usize + 1
    };
    let code = prefix(r, trees + rle)?;
    let mut at = 0;
    while at < size {
        let s = code.decode(r)?;
        if s > 0 && s <= rle {
            let n = (1usize << s) + r.read(s as u8)? as usize;
            if n > size - at {
                return Err(r.error(ErrorKind::InvalidContextMap));
            }
            r.charge(n as u64)?;
            at += n;
        } else {
            let value = if s == 0 { 0 } else { s - rle };
            if value >= trees {
                return Err(r.error(ErrorKind::InvalidContextMap));
            }
            map[at] = value as u8;
            at += 1;
        }
    }
    if r.read(1)? != 0 {
        let mut list = [0u8; 256];
        for (n, v) in list.iter_mut().enumerate() {
            *v = n as u8;
        }
        for v in &mut map {
            let index = usize::from(*v);
            r.charge(index as u64 + 1)?;
            let value = list[index];
            list.copy_within(..index, 1);
            list[0] = value;
            *v = value;
        }
    }
    let mut used = [false; 256];
    for &v in &map {
        used[v as usize] = true;
    }
    if used[..trees].iter().any(|&b| !b) {
        return Err(r.error(ErrorKind::InvalidContextMap));
    }
    Ok(map)
}
fn prefix_set(
    r: &mut Reader<'_, '_>,
    count: usize,
    alphabet: usize,
) -> Result<Vec<Huffman>, Error> {
    let mut set = Vec::new();
    r.reserve_table(&mut set, count)?;
    for _ in 0..count {
        set.push(prefix(r, alphabet)?);
    }
    Ok(set)
}

fn transform_word(
    length: usize,
    word_id: usize,
    r: &mut Reader<'_, '_>,
) -> Result<([u8; 38], usize), Error> {
    if !(4..=24).contains(&length) {
        return Err(r.error(ErrorKind::InvalidDictionary));
    }
    let bits = tables::NDBITS[length];
    let transform = word_id >> bits;
    let Some(t) = tables::TRANSFORMS.get(transform) else {
        return Err(r.error(ErrorKind::InvalidDictionary));
    };
    let index = word_id & ((1usize << bits) - 1);
    let offset = tables::DOFFSET[length] + length * index;
    let word = &DICTIONARY[offset..offset + length];
    let (first, last) = match t.kind {
        3..=11 => ((t.kind as usize - 2).min(length), length),
        12..=20 => (0, length.saturating_sub(t.kind as usize - 11)),
        _ => (0, length),
    };
    let mut result = [0; 38];
    let mut size = t.prefix.len();
    result[..size].copy_from_slice(t.prefix);
    result[size..size + last - first].copy_from_slice(&word[first..last]);
    let word_start = size;
    size += last - first;
    if t.kind == 1 || t.kind == 2 {
        let mut at = word_start;
        while at < size {
            let step = if result[at] < 192 {
                if result[at].is_ascii_lowercase() {
                    result[at] ^= 32;
                }
                1
            } else if result[at] < 224 {
                if at + 1 < size {
                    result[at + 1] ^= 32;
                }
                2
            } else {
                if at + 2 < size {
                    result[at + 2] ^= 5;
                }
                3
            };
            at += step;
            if t.kind == 1 {
                break;
            }
        }
    }
    result[size..size + t.suffix.len()].copy_from_slice(t.suffix);
    size += t.suffix.len();
    r.charge(size as u64 + 1)?;
    Ok((result, size))
}

fn compressed_block(
    r: &mut Reader<'_, '_>,
    output: &mut Vec<u8>,
    size: usize,
    window: usize,
    last: &mut [usize; 4],
) -> Result<(), Error> {
    let mut literal = Blocks::new(r)?;
    let mut command = Blocks::new(r)?;
    let mut distance = Blocks::new(r)?;
    let postfix = r.read(2)? as u8;
    let direct = (r.read(4)? as usize) << postfix;
    let mut modes = [0; 256];
    for mode in &mut modes[..literal.types] {
        *mode = r.read(2)? as u8;
    }
    let literal_trees = r.variable()? + 1;
    let literal_map = context_map(r, literal.types * 64, literal_trees)?;
    let distance_trees = r.variable()? + 1;
    let distance_map = context_map(r, distance.types * 4, distance_trees)?;
    let literals = prefix_set(r, literal_trees, 256)?;
    let commands = prefix_set(r, command.types, 704)?;
    let distances = prefix_set(r, distance_trees, 16 + direct + (48usize << postfix))?;
    if output
        .capacity()
        .checked_add(r.tables)
        .is_none_or(|n| n > r.limits.max_allocation_bytes)
    {
        return Err(r.error(ErrorKind::LimitExceeded(Limit::Allocation)));
    }
    let goal = output
        .len()
        .checked_add(size)
        .ok_or_else(|| r.error(ErrorKind::InvalidMetaBlock))?;
    while output.len() < goal {
        let cmd = commands[command.next(r)?].decode(r)?;
        let group = cmd / 64;
        let (insert_group, copy_group) = match group {
            0 => (0, 0),
            1 => (0, 8),
            2 => (0, 0),
            3 => (0, 8),
            4 => (8, 0),
            5 => (8, 8),
            6 => (0, 16),
            7 => (16, 0),
            8 => (8, 16),
            9 => (16, 8),
            10 => (16, 16),
            _ => return Err(r.error(ErrorKind::InvalidSymbol)),
        };
        let insert_code = insert_group + ((cmd >> 3) & 7);
        let copy_code = copy_group + (cmd & 7);
        let insert = INSERT_BASE[insert_code] + r.read(INSERT_EXTRA[insert_code])? as usize;
        let copy = COPY_BASE[copy_code] + r.read(COPY_EXTRA[copy_code])? as usize;
        if insert > goal - output.len() {
            return Err(r.error(ErrorKind::InvalidMetaBlock));
        }
        for _ in 0..insert {
            let typ = literal.next(r)?;
            let p1 = output.last().copied().unwrap_or(0) as usize;
            let p2 = output
                .get(output.len().wrapping_sub(2))
                .copied()
                .unwrap_or(0) as usize;
            let context = match modes[typ] {
                0 => p1 & 63,
                1 => p1 >> 2,
                2 => usize::from(tables::LUT0[p1] | tables::LUT1[p2]),
                _ => usize::from((tables::LUT2[p1] << 3) | tables::LUT2[p2]),
            };
            let tree = literal_map[typ * 64 + context] as usize;
            let value = literals[tree].decode(r)? as u8;
            r.emit(output, value)?;
        }
        if output.len() == goal {
            break;
        }
        let dcode = if cmd < 128 {
            0
        } else {
            let typ = distance.next(r)?;
            let context = (copy - 2).min(3);
            distances[distance_map[typ * 4 + context] as usize].decode(r)?
        };
        let dist = if dcode < 16 {
            match dcode {
                0..=3 => last[dcode],
                4..=9 => {
                    let delta = (dcode - 4) / 2 + 1;
                    if dcode % 2 == 0 {
                        last[0].checked_sub(delta)
                    } else {
                        last[0].checked_add(delta)
                    }
                    .ok_or_else(|| r.error(ErrorKind::InvalidDistance))?
                }
                _ => {
                    let delta = (dcode - 10) / 2 + 1;
                    if dcode % 2 == 0 {
                        last[1].checked_sub(delta)
                    } else {
                        last[1].checked_add(delta)
                    }
                    .ok_or_else(|| r.error(ErrorKind::InvalidDistance))?
                }
            }
        } else if dcode < 16 + direct {
            dcode - 15
        } else {
            let base = dcode - direct - 16;
            let bits = 1 + (base >> (postfix + 1));
            if bits > 24 {
                return Err(r.error(ErrorKind::InvalidDistance));
            }
            let extra = r.read(bits as u8)? as usize;
            let high = base >> postfix;
            let low = base & ((1usize << postfix) - 1);
            let offset = ((2 + (high & 1)) << bits) - 4;
            ((offset + extra) << postfix) + low + direct + 1
        };
        if dist == 0 {
            return Err(r.error(ErrorKind::InvalidDistance));
        }
        let max = window.min(output.len());
        if dist > max {
            let (word, count) = transform_word(copy, dist - max - 1, r)?;
            if count > goal - output.len() {
                return Err(r.error(ErrorKind::InvalidMetaBlock));
            }
            for &value in &word[..count] {
                r.emit(output, value)?;
            }
        } else {
            if copy > goal - output.len() {
                return Err(r.error(ErrorKind::InvalidMetaBlock));
            }
            if dcode != 0 {
                last.copy_within(..3, 1);
                last[0] = dist;
            }
            for _ in 0..copy {
                let value = output[output.len() - dist];
                r.emit(output, value)?;
            }
        }
    }
    Ok(())
}

/// Decode one RFC7932 stream using first-party code and embedded normative data.
/// No RFC9841 shared-dictionary/dcb or nonstandard large-window extension is
/// silently accepted. HTTP `br` uses this RFC7932 format.
pub fn decode(
    input: &[u8],
    limits: Limits,
    max_meta_blocks: u32,
    trailing: TrailingData,
    check: &mut impl FnMut() -> Result<(), Abort>,
) -> Result<Decoded, Error> {
    let mut r = Reader {
        input,
        bit: 0,
        work: 0,
        checked: 0,
        output: 0,
        output_capacity: 0,
        limits,
        tables: FIXED_MEMORY,
        check,
    };
    (r.check)().map_err(|reason| r.error(ErrorKind::Aborted(reason)))?;
    if FIXED_MEMORY > limits.max_allocation_bytes {
        return Err(r.error(ErrorKind::LimitExceeded(Limit::Allocation)));
    }
    let wbits = if r.read(1)? == 0 {
        16
    } else {
        let n = r.read(3)?;
        if n > 0 {
            17 + n
        } else {
            match r.read(3)? {
                0 => 17,
                1 => return Err(r.error(ErrorKind::InvalidWindow)),
                n => 8 + n,
            }
        }
    };
    let window = (1usize << wbits) - 16;
    let mut output = Vec::new();
    let mut last = [4, 11, 15, 16];
    let mut meta_blocks = 0;
    loop {
        if meta_blocks >= max_meta_blocks {
            return Err(r.error(ErrorKind::MetaBlockLimit));
        }
        meta_blocks += 1;
        r.tables = FIXED_MEMORY;
        let is_last = r.read(1)? != 0;
        if is_last && r.read(1)? != 0 {
            r.align()?;
            break;
        }
        let n = r.read(2)?;
        if n == 3 {
            if r.read(1)? != 0 {
                return Err(r.error(ErrorKind::InvalidMetaBlock));
            }
            let bytes = r.read(2)? as u8;
            let count = if bytes == 0 {
                0
            } else {
                let n = r.read(bytes * 8)? as usize;
                if bytes > 1 && n >> (8 * (bytes - 1)) == 0 {
                    return Err(r.error(ErrorKind::InvalidMetaBlock));
                }
                n + 1
            };
            r.align()?;
            for _ in 0..count {
                r.read(8)?;
            }
        } else {
            let nibbles = n as u8 + 4;
            let encoded_size = r.read(nibbles * 4)? as usize;
            if nibbles > 4 && encoded_size >> (4 * (nibbles - 1)) == 0 {
                return Err(r.error(ErrorKind::InvalidMetaBlock));
            }
            let size = encoded_size + 1;
            if !is_last && r.read(1)? != 0 {
                r.align()?;
                for _ in 0..size {
                    let value = r.read(8)? as u8;
                    r.emit(&mut output, value)?;
                }
            } else {
                compressed_block(&mut r, &mut output, size, window, &mut last)?;
            }
        }
        if is_last {
            r.align()?;
            break;
        }
    }
    let consumed = r.bit / 8;
    if trailing == TrailingData::Reject && consumed != input.len() {
        return Err(r.error(ErrorKind::TrailingData));
    }
    (r.check)().map_err(|reason| r.error(ErrorKind::Aborted(reason)))?;
    Ok(Decoded {
        bytes: output,
        consumed,
        work: r.work,
        meta_blocks,
    })
}
