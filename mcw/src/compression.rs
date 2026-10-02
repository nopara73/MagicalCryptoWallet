//! First-party, bounded RFC 1950/1951/1952 codecs. No native compression library.
//!
//! `Decoder::process` retains its parser and 32 KiB history between calls. Pass
//! back the unconsumed suffix (including when `NeedInput` consumes zero bytes).
//! `end_of_input` means this slice contains all remaining input; preserve that
//! flag when retrying after `NeedOutput`. Output is provisional until `Finished`:
//! a subsequent trailer, checksum, limit, or trailing-data error can invalidate it.
//! Errors poison the decoder and are returned unchanged on subsequent calls.
//!
//! Limits are cumulative, independent of chunk sizes. Expansion is checked on
//! consumed input, never on an unconsumed suffix. The allocation ceiling includes
//! the decoder object and history; the collecting API also charges its result.
//! Caller-owned input/output buffers are outside this ceiling. No compressed input,
//! member metadata, or complete uncompressed stream is retained by the decoder.
//! Allocation limits charge requested live capacities; allocator bookkeeping and
//! transient reallocation storage are not measured. Work units charge input,
//! parsed bits, table construction, copies, checksums, and potential reallocations;
//! fixed-size object/history initialization and caller buffers are outside work.
//! Encoders produce stored blocks or fixed-Huffman blocks with bounded greedy LZ77;
//! they emit no preset dictionary. All three DEFLATE block types are decoded.
//!
//! Independently implemented from the RFCs, under the repository's MIT license.
//! Protocol constants are format data; no third-party implementation was copied.

#![forbid(unsafe_code)]

use std::fmt;

const WINDOW: usize = 32768;
const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Deflate,
    Zlib,
    Gzip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingData {
    Reject,
    /// Stop at the format boundary and report its exact consumption. With gzip
    /// concatenation enabled, a member magic starts another member, whose errors
    /// cannot be treated as trailing data. A lone 0x1f at EOF is truncation.
    Allow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GzipMembers {
    Concatenate,
    First,
}

#[derive(Clone, Copy, Debug)]
pub enum Dictionary<'a> {
    Reject,
    /// Raw DEFLATE uses the dictionary immediately. Zlib uses it only when
    /// FDICT is set and the full dictionary's Adler-32 matches DICTID. Only its
    /// final window is retained. Gzip rejects this policy (it has no DICTID).
    Use(&'a [u8]),
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_expansion_ratio: u64,
    pub expansion_slack_bytes: u64,
    pub max_work: u64,
    pub max_allocation_bytes: usize,
    pub max_dictionary_bytes: usize,
    pub max_gzip_header_bytes: u64,
    pub max_gzip_members: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            max_expansion_ratio: 200,
            expansion_slack_bytes: 1024 * 1024,
            max_work: 1_000_000_000,
            max_allocation_bytes: 65 * 1024 * 1024,
            max_dictionary_bytes: 1024 * 1024,
            max_gzip_header_bytes: 128 * 1024,
            max_gzip_members: 1024,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DecodeOptions<'a> {
    pub format: Format,
    pub limits: Limits,
    pub trailing_data: TrailingData,
    pub gzip_members: GzipMembers,
    pub dictionary: Dictionary<'a>,
}

impl DecodeOptions<'_> {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            limits: Limits::default(),
            trailing_data: TrailingData::Reject,
            gzip_members: GzipMembers::Concatenate,
            dictionary: Dictionary::Reject,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Input,
    Output,
    Expansion,
    Work,
    Allocation,
    Dictionary,
    GzipHeader,
    GzipMembers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Truncated,
    TrailingData,
    InvalidZlibHeader,
    InvalidGzipHeader,
    InvalidDictionaryPolicy,
    DictionaryRequired { id: u32 },
    DictionaryMismatch { expected: u32, actual: u32 },
    ReservedBlock,
    InvalidStoredLength,
    InvalidCodeLength,
    OversubscribedTree,
    IncompleteTree,
    MissingEndOfBlock,
    InvalidHuffmanCode,
    InvalidRepeat,
    ReservedSymbol,
    InvalidDistance,
    ChecksumMismatch { expected: u32, actual: u32 },
    GzipSizeMismatch { expected: u32, actual: u32 },
    LimitExceeded(Limit),
    AllocationFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// Bytes physically consumed, including any partially parsed bit-buffer bytes.
    pub input_consumed: u64,
    pub output_produced: u64,
    /// Location of the next logical bit. EOF is byte_offset == input_consumed.
    pub byte_offset: u64,
    pub bit_offset: u8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} at {}:{} ({} input, {} output bytes)",
            self.kind, self.byte_offset, self.bit_offset, self.input_consumed, self.output_produced
        )
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NeedInput,
    NeedOutput,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub consumed: usize,
    pub written: usize,
    pub status: Status,
}

#[derive(Debug)]
pub struct Decoded {
    pub bytes: Vec<u8>,
    pub consumed: usize,
    /// Gzip member count, or one for raw DEFLATE/zlib.
    pub members: u32,
}

/// Incremental IEEE CRC-32 (reflected polynomial 0xedb88320).
#[derive(Clone, Copy, Debug)]
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self(u32::MAX)
    }
    pub fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
    }
    fn byte(&mut self, byte: u8) {
        self.0 ^= u32::from(byte);
        for _ in 0..8 {
            self.0 = (self.0 >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(self.0 & 1));
        }
    }
    pub const fn value(self) -> u32 {
        !self.0
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.value()
}

#[derive(Clone, Copy, Debug)]
pub struct Adler32 {
    a: u32,
    b: u32,
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Adler32 {
    pub const fn new() -> Self {
        Self { a: 1, b: 0 }
    }
    pub fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
    }
    fn byte(&mut self, byte: u8) {
        self.a = (self.a + u32::from(byte)) % 65521;
        self.b = (self.b + self.a) % 65521;
    }
    pub const fn value(self) -> u32 {
        (self.b << 16) | self.a
    }
}

pub fn adler32(bytes: &[u8]) -> u32 {
    let mut adler = Adler32::new();
    adler.update(bytes);
    adler.value()
}

#[derive(Clone, Copy)]
enum TreeKind {
    CodeLengths,
    Literals,
    Distances,
}

// Canonical-code ranges, bounded independently of input lengths. No table heap.
struct Huffman {
    count: [u16; 16],
    first: [u16; 16],
    start: [u16; 16],
    symbols: [u16; 288],
    max: u8,
}

impl Huffman {
    fn empty() -> Self {
        Self {
            count: [0; 16],
            first: [0; 16],
            start: [0; 16],
            symbols: [0; 288],
            max: 0,
        }
    }
    fn build(&mut self, lengths: &[u8], kind: TreeKind) -> Result<(), ErrorKind> {
        self.count = [0; 16];
        self.max = 0;
        for &n in lengths {
            if n > 15 {
                return Err(ErrorKind::InvalidCodeLength);
            }
            if n != 0 {
                self.count[n as usize] += 1;
                self.max = self.max.max(n);
            }
        }
        if self.max == 0 {
            return if matches!(kind, TreeKind::Distances) {
                Ok(())
            } else {
                Err(ErrorKind::IncompleteTree)
            };
        }
        let mut left: i32 = 1;
        for n in 1..=15 {
            left = (left << 1) - i32::from(self.count[n]);
            if left < 0 {
                return Err(ErrorKind::OversubscribedTree);
            }
        }
        if left != 0 && (matches!(kind, TreeKind::CodeLengths) || self.max != 1) {
            return Err(ErrorKind::IncompleteTree);
        }
        if matches!(kind, TreeKind::Literals) && lengths.get(256).copied().unwrap_or(0) == 0 {
            return Err(ErrorKind::MissingEndOfBlock);
        }
        let mut code = 0;
        let mut offset = 0;
        for n in 1..=15 {
            code = (code + self.count[n - 1]) << 1;
            self.first[n] = code;
            self.start[n] = offset;
            offset += self.count[n];
        }
        let mut next = self.start;
        for (symbol, &n) in lengths.iter().enumerate() {
            if n != 0 {
                self.symbols[next[n as usize] as usize] = symbol as u16;
                next[n as usize] += 1;
            }
        }
        Ok(())
    }
    fn symbol(&self, code: u16, length: u8) -> Option<u16> {
        let n = length as usize;
        let delta = code.checked_sub(self.first[n])?;
        if delta < self.count[n] {
            Some(self.symbols[(self.start[n] + delta) as usize])
        } else {
            None
        }
    }
}

#[derive(Clone, Copy)]
enum Phase {
    ZlibHeader(u8),
    ZlibDictionary(u8),
    GzipHeader(u8),
    GzipExtraLength(u8),
    GzipExtra(u16),
    GzipName,
    GzipComment,
    GzipHeaderCrc(u8),
    Block,
    StoredHeader(u8),
    Stored(u16),
    DynamicCounts,
    CodeLengths(usize),
    DynamicLengths(usize),
    Repeat { index: usize, symbol: u16 },
    Literal,
    LiteralByte(u8),
    Length(usize),
    Distance(usize),
    DistanceExtra { length: usize, symbol: usize },
    Copy { length: usize, distance: usize },
    ZlibTrailer(u8),
    GzipTrailer(u8),
    BetweenMembers,
    End,
}

pub struct Decoder {
    format: Format,
    limits: Limits,
    trailing: TrailingData,
    gzip_members: GzipMembers,
    phase: Phase,
    failed: Option<Error>,
    finished: bool,
    input: u64,
    output: u64,
    work: u64,
    bits: u32,
    available: u8,
    prefix: u16,
    prefix_len: u8,
    final_block: bool,
    window: Vec<u8>,
    window_at: usize,
    history: usize,
    window_limit: usize,
    dictionary_id: Option<u32>,
    scratch: [u8; 10],
    flags: u8,
    header_bytes: u64,
    header_crc: Crc32,
    crc: Crc32,
    adler: Adler32,
    member_input_start: u64,
    member_output: u64,
    members: u32,
    lit_count: usize,
    dist_count: usize,
    code_count: usize,
    code_lengths: [u8; 19],
    lengths: [u8; 320],
    codes: Huffman,
    literals: Huffman,
    distances: Huffman,
}

impl Decoder {
    pub fn new(options: DecodeOptions<'_>) -> Result<Self, Error> {
        let allocation = std::mem::size_of::<Self>().checked_add(WINDOW);
        if allocation.is_none_or(|n| n > options.limits.max_allocation_bytes) {
            return Err(initial_error(ErrorKind::LimitExceeded(Limit::Allocation)));
        }
        if options.format == Format::Gzip && matches!(options.dictionary, Dictionary::Use(_)) {
            return Err(initial_error(ErrorKind::InvalidDictionaryPolicy));
        }
        let mut window = Vec::new();
        window
            .try_reserve_exact(WINDOW)
            .map_err(|_| initial_error(ErrorKind::AllocationFailed))?;
        window.resize(WINDOW, 0);
        let phase = match options.format {
            Format::Deflate => Phase::Block,
            Format::Zlib => Phase::ZlibHeader(0),
            Format::Gzip => Phase::GzipHeader(0),
        };
        let mut decoder = Self {
            format: options.format,
            limits: options.limits,
            trailing: options.trailing_data,
            gzip_members: options.gzip_members,
            phase,
            failed: None,
            finished: false,
            input: 0,
            output: 0,
            work: 0,
            bits: 0,
            available: 0,
            prefix: 0,
            prefix_len: 0,
            final_block: false,
            window,
            window_at: 0,
            history: 0,
            window_limit: WINDOW,
            dictionary_id: None,
            scratch: [0; 10],
            flags: 0,
            header_bytes: 0,
            header_crc: Crc32::new(),
            crc: Crc32::new(),
            adler: Adler32::new(),
            member_input_start: 0,
            member_output: 0,
            members: 0,
            lit_count: 0,
            dist_count: 0,
            code_count: 0,
            code_lengths: [0; 19],
            lengths: [0; 320],
            codes: Huffman::empty(),
            literals: Huffman::empty(),
            distances: Huffman::empty(),
        };
        if decoder.allocated_bytes() > options.limits.max_allocation_bytes {
            return Err(decoder.error(ErrorKind::LimitExceeded(Limit::Allocation)));
        }
        if let Dictionary::Use(bytes) = options.dictionary {
            if bytes.len() > decoder.limits.max_dictionary_bytes {
                return Err(decoder.error(ErrorKind::LimitExceeded(Limit::Dictionary)));
            }
            decoder.charge((bytes.len() as u64).saturating_mul(10))?;
            decoder.dictionary_id = Some(adler32(bytes));
            let tail = &bytes[bytes.len().saturating_sub(WINDOW)..];
            decoder.window[..tail.len()].copy_from_slice(tail);
            decoder.history = tail.len();
            decoder.window_at = tail.len() % WINDOW;
        }
        Ok(decoder)
    }

    pub fn total_input(&self) -> u64 {
        self.input
    }
    pub fn total_output(&self) -> u64 {
        self.output
    }
    pub fn total_work(&self) -> u64 {
        self.work
    }
    pub fn members(&self) -> u32 {
        self.members
    }
    pub fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.window.capacity()
    }

    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            input_consumed: self.input,
            output_produced: self.output,
            byte_offset: self.input - u64::from(self.available.div_ceil(8)),
            bit_offset: (8 - self.available % 8) % 8,
        }
    }
    fn charge(&mut self, n: u64) -> Result<(), Error> {
        self.work = self
            .work
            .checked_add(n)
            .filter(|&v| v <= self.limits.max_work)
            .ok_or_else(|| self.error(ErrorKind::LimitExceeded(Limit::Work)))?;
        Ok(())
    }
    fn byte(&mut self, input: &[u8], at: &mut usize) -> Result<Option<u8>, Error> {
        let Some(&b) = input.get(*at) else {
            return Ok(None);
        };
        if self.input >= self.limits.max_input_bytes {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Input)));
        }
        self.charge(1)?;
        self.input += 1;
        *at += 1;
        Ok(Some(b))
    }
    fn take_bits(&mut self, n: u8, input: &[u8], at: &mut usize) -> Result<Option<u32>, Error> {
        while self.available < n {
            let Some(b) = self.byte(input, at)? else {
                return Ok(None);
            };
            self.bits |= u32::from(b) << self.available;
            self.available += 8;
        }
        self.charge(u64::from(n))?;
        let v = self.bits & ((1u32 << n) - 1);
        self.bits >>= n;
        self.available -= n;
        Ok(Some(v))
    }
    fn symbol(
        &mut self,
        kind: TreeKind,
        input: &[u8],
        at: &mut usize,
    ) -> Result<Option<u16>, Error> {
        loop {
            let max = match kind {
                TreeKind::CodeLengths => self.codes.max,
                TreeKind::Literals => self.literals.max,
                TreeKind::Distances => self.distances.max,
            };
            if self.prefix_len >= max {
                return Err(self.error(ErrorKind::InvalidHuffmanCode));
            }
            let Some(bit) = self.take_bits(1, input, at)? else {
                return Ok(None);
            };
            self.prefix = (self.prefix << 1) | bit as u16;
            self.prefix_len += 1;
            let tree = match kind {
                TreeKind::CodeLengths => &self.codes,
                TreeKind::Literals => &self.literals,
                TreeKind::Distances => &self.distances,
            };
            if let Some(v) = tree.symbol(self.prefix, self.prefix_len) {
                self.prefix = 0;
                self.prefix_len = 0;
                return Ok(Some(v));
            }
        }
    }
    fn emit(&mut self, b: u8, output: &mut [u8], at: &mut usize) -> Result<bool, Error> {
        if *at == output.len() {
            return Ok(false);
        }
        if self.output >= self.limits.max_output_bytes {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Output)));
        }
        let ratio = self.limits.max_expansion_ratio;
        let slack = self.limits.expansion_slack_bytes;
        let global_allowed = self.input.saturating_mul(ratio).saturating_add(slack);
        let member_allowed = (self.input - self.member_input_start)
            .saturating_mul(ratio)
            .saturating_add(slack);
        if self.output >= global_allowed || self.member_output >= member_allowed {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Expansion)));
        }
        // Includes checksum arithmetic (CRC has eight bit steps per byte).
        self.charge(12)?;
        output[*at] = b;
        *at += 1;
        self.window[self.window_at] = b;
        self.window_at = (self.window_at + 1) % WINDOW;
        self.history = (self.history + 1).min(self.window_limit);
        self.output += 1;
        self.member_output += 1;
        self.crc.byte(b);
        self.adler.byte(b);
        Ok(true)
    }
    fn header_byte(&mut self, input: &[u8], at: &mut usize) -> Result<Option<u8>, Error> {
        if *at == input.len() {
            return Ok(None);
        }
        if self.header_bytes >= self.limits.max_gzip_header_bytes {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::GzipHeader)));
        }
        self.charge(8)?;
        let b = self.byte(input, at)?.expect("available header byte");
        self.header_bytes += 1;
        self.header_crc.byte(b);
        Ok(Some(b))
    }
    fn after_extra(&mut self) {
        self.phase = if self.flags & 8 != 0 {
            Phase::GzipName
        } else if self.flags & 16 != 0 {
            Phase::GzipComment
        } else if self.flags & 2 != 0 {
            Phase::GzipHeaderCrc(0)
        } else {
            Phase::Block
        };
    }
    fn after_name(&mut self) {
        self.phase = if self.flags & 16 != 0 {
            Phase::GzipComment
        } else if self.flags & 2 != 0 {
            Phase::GzipHeaderCrc(0)
        } else {
            Phase::Block
        };
    }
    fn end_block(&mut self) {
        if !self.final_block {
            self.phase = Phase::Block;
            return;
        }
        // RFC ignores residual bits through the end of this byte; never prefetch
        // the next byte. This makes raw and framed consumption exact.
        self.bits = 0;
        self.available = 0;
        self.phase = match self.format {
            Format::Deflate => {
                self.members = 1;
                Phase::End
            }
            Format::Zlib => Phase::ZlibTrailer(0),
            Format::Gzip => Phase::GzipTrailer(0),
        };
    }
    fn fixed(&mut self) -> Result<(), Error> {
        self.charge(320 * 16)?;
        let mut lengths = [8; 288];
        lengths[144..256].fill(9);
        lengths[256..280].fill(7);
        self.literals
            .build(&lengths, TreeKind::Literals)
            .map_err(|k| self.error(k))?;
        self.distances
            .build(&[5; 32], TreeKind::Distances)
            .map_err(|k| self.error(k))
    }

    pub fn process(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        end_of_input: bool,
    ) -> Result<Progress, Error> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        let result = self.process_inner(input, output, end_of_input);
        if let Err(e) = result {
            self.failed = Some(e);
        }
        result
    }

    fn process_inner(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        end: bool,
    ) -> Result<Progress, Error> {
        let mut i = 0;
        let mut o = 0;
        macro_rules! need_input {
            () => {{
                if end {
                    return Err(self.error(ErrorKind::Truncated));
                }
                return Ok(Progress {
                    consumed: i,
                    written: o,
                    status: Status::NeedInput,
                });
            }};
        }
        macro_rules! byte {
            () => {
                match self.byte(input, &mut i)? {
                    Some(v) => v,
                    None => {
                        need_input!();
                    }
                }
            };
        }
        macro_rules! header {
            () => {
                match self.header_byte(input, &mut i)? {
                    Some(v) => v,
                    None => {
                        need_input!();
                    }
                }
            };
        }
        macro_rules! bits {
            ($n:expr) => {
                match self.take_bits($n, input, &mut i)? {
                    Some(v) => v,
                    None => {
                        need_input!();
                    }
                }
            };
        }
        macro_rules! symbol {
            ($k:expr) => {
                match self.symbol($k, input, &mut i)? {
                    Some(v) => v,
                    None => {
                        need_input!();
                    }
                }
            };
        }
        macro_rules! need_output {
            () => {
                return Ok(Progress {
                    consumed: i,
                    written: o,
                    status: Status::NeedOutput,
                });
            };
        }
        loop {
            if self.finished {
                if !input[i..].is_empty() && self.trailing == TrailingData::Reject {
                    return Err(self.error(ErrorKind::TrailingData));
                }
                return Ok(Progress {
                    consumed: i,
                    written: o,
                    status: Status::Finished,
                });
            }
            match self.phase {
                Phase::ZlibHeader(n) => {
                    self.scratch[n as usize] = byte!();
                    if n == 0 {
                        self.phase = Phase::ZlibHeader(1);
                        continue;
                    }
                    let cmf = self.scratch[0];
                    let flg = self.scratch[1];
                    if cmf & 15 != 8 || cmf >> 4 > 7 || u16::from_be_bytes([cmf, flg]) % 31 != 0 {
                        return Err(self.error(ErrorKind::InvalidZlibHeader));
                    }
                    self.window_limit = 1usize << ((cmf >> 4) + 8);
                    self.history = self.history.min(self.window_limit);
                    if flg & 32 != 0 {
                        self.phase = Phase::ZlibDictionary(0);
                    } else {
                        self.history = 0;
                        self.window_at = 0;
                        self.phase = Phase::Block;
                    }
                }
                Phase::ZlibDictionary(n) => {
                    self.scratch[n as usize] = byte!();
                    if n < 3 {
                        self.phase = Phase::ZlibDictionary(n + 1);
                        continue;
                    }
                    let expected =
                        u32::from_be_bytes(self.scratch[..4].try_into().expect("four bytes"));
                    let Some(actual) = self.dictionary_id else {
                        return Err(self.error(ErrorKind::DictionaryRequired { id: expected }));
                    };
                    if expected != actual {
                        return Err(self.error(ErrorKind::DictionaryMismatch { expected, actual }));
                    }
                    self.phase = Phase::Block;
                }
                Phase::GzipHeader(n) => {
                    if n == 0
                        && self.header_bytes == 0
                        && self.members >= self.limits.max_gzip_members
                    {
                        return Err(self.error(ErrorKind::LimitExceeded(Limit::GzipMembers)));
                    }
                    let b = header!();
                    self.scratch[n as usize] = b;
                    if (n == 0 && b != 0x1f)
                        || (n == 1 && b != 0x8b)
                        || (n == 2 && b != 8)
                        || (n == 3 && b & 0xe0 != 0)
                    {
                        return Err(self.error(ErrorKind::InvalidGzipHeader));
                    }
                    if n < 9 {
                        self.phase = Phase::GzipHeader(n + 1);
                        continue;
                    }
                    self.flags = self.scratch[3];
                    if self.flags & 4 != 0 {
                        self.phase = Phase::GzipExtraLength(0);
                    } else {
                        self.after_extra();
                    }
                }
                Phase::GzipExtraLength(n) => {
                    self.scratch[n as usize] = header!();
                    if n == 0 {
                        self.phase = Phase::GzipExtraLength(1);
                    } else {
                        self.phase = Phase::GzipExtra(u16::from_le_bytes([
                            self.scratch[0],
                            self.scratch[1],
                        ]));
                    }
                }
                Phase::GzipExtra(n) => {
                    if n == 0 {
                        self.after_extra();
                    } else {
                        header!();
                        self.phase = Phase::GzipExtra(n - 1);
                    }
                }
                Phase::GzipName => {
                    if header!() == 0 {
                        self.after_name();
                    }
                }
                Phase::GzipComment => {
                    if header!() == 0 {
                        self.phase = if self.flags & 2 != 0 {
                            Phase::GzipHeaderCrc(0)
                        } else {
                            Phase::Block
                        };
                    }
                }
                Phase::GzipHeaderCrc(n) => {
                    if i == input.len() {
                        need_input!();
                    }
                    if self.header_bytes >= self.limits.max_gzip_header_bytes {
                        return Err(self.error(ErrorKind::LimitExceeded(Limit::GzipHeader)));
                    }
                    self.scratch[n as usize] = byte!();
                    self.header_bytes += 1;
                    if n == 0 {
                        self.phase = Phase::GzipHeaderCrc(1);
                        continue;
                    }
                    let expected =
                        u32::from(u16::from_le_bytes([self.scratch[0], self.scratch[1]]));
                    let actual = self.header_crc.value() & 0xffff;
                    if expected != actual {
                        return Err(self.error(ErrorKind::ChecksumMismatch { expected, actual }));
                    }
                    self.phase = Phase::Block;
                }
                Phase::Block => {
                    let b = bits!(3);
                    self.final_block = b & 1 != 0;
                    match b >> 1 {
                        0 => {
                            self.bits = 0;
                            self.available = 0;
                            self.phase = Phase::StoredHeader(0);
                        }
                        1 => {
                            self.fixed()?;
                            self.phase = Phase::Literal;
                        }
                        2 => {
                            self.phase = Phase::DynamicCounts;
                        }
                        _ => return Err(self.error(ErrorKind::ReservedBlock)),
                    }
                }
                Phase::StoredHeader(n) => {
                    self.scratch[n as usize] = byte!();
                    if n < 3 {
                        self.phase = Phase::StoredHeader(n + 1);
                        continue;
                    }
                    let len = u16::from_le_bytes([self.scratch[0], self.scratch[1]]);
                    let neg = u16::from_le_bytes([self.scratch[2], self.scratch[3]]);
                    if len != !neg {
                        return Err(self.error(ErrorKind::InvalidStoredLength));
                    }
                    self.phase = Phase::Stored(len);
                }
                Phase::Stored(n) => {
                    if n == 0 {
                        self.end_block();
                        continue;
                    }
                    if o == output.len() {
                        need_output!();
                    }
                    let b = byte!();
                    self.emit(b, output, &mut o)?;
                    self.phase = Phase::Stored(n - 1);
                }
                Phase::DynamicCounts => {
                    let n = bits!(14);
                    self.lit_count = (n as usize & 31) + 257;
                    self.dist_count = ((n as usize >> 5) & 31) + 1;
                    self.code_count = ((n as usize >> 10) & 15) + 4;
                    if self.lit_count > 286 {
                        return Err(self.error(ErrorKind::InvalidCodeLength));
                    }
                    self.code_lengths.fill(0);
                    self.phase = Phase::CodeLengths(0);
                }
                Phase::CodeLengths(n) => {
                    if n < self.code_count {
                        self.code_lengths[ORDER[n]] = bits!(3) as u8;
                        self.phase = Phase::CodeLengths(n + 1);
                    } else {
                        self.charge(19 * 16)?;
                        self.codes
                            .build(&self.code_lengths, TreeKind::CodeLengths)
                            .map_err(|k| self.error(k))?;
                        self.phase = Phase::DynamicLengths(0);
                    }
                }
                Phase::DynamicLengths(n) => {
                    if n == self.lit_count + self.dist_count {
                        self.charge(320 * 16)?;
                        self.literals
                            .build(&self.lengths[..self.lit_count], TreeKind::Literals)
                            .map_err(|k| self.error(k))?;
                        self.distances
                            .build(&self.lengths[self.lit_count..n], TreeKind::Distances)
                            .map_err(|k| self.error(k))?;
                        self.phase = Phase::Literal;
                    } else {
                        let symbol = symbol!(TreeKind::CodeLengths);
                        if symbol < 16 {
                            self.lengths[n] = symbol as u8;
                            self.phase = Phase::DynamicLengths(n + 1);
                        } else {
                            self.phase = Phase::Repeat { index: n, symbol };
                        }
                    }
                }
                Phase::Repeat { index, symbol } => {
                    if symbol == 16 && index == 0 {
                        return Err(self.error(ErrorKind::InvalidRepeat));
                    }
                    let (extra, base) = match symbol {
                        16 => (2, 3),
                        17 => (3, 3),
                        18 => (7, 11),
                        _ => return Err(self.error(ErrorKind::InvalidRepeat)),
                    };
                    let count = bits!(extra) as usize + base;
                    if index + count > self.lit_count + self.dist_count {
                        return Err(self.error(ErrorKind::InvalidRepeat));
                    }
                    self.charge(count as u64)?;
                    let v = if symbol == 16 {
                        self.lengths[index - 1]
                    } else {
                        0
                    };
                    self.lengths[index..index + count].fill(v);
                    self.phase = Phase::DynamicLengths(index + count);
                }
                Phase::Literal => match symbol!(TreeKind::Literals) {
                    n @ 0..=255 => self.phase = Phase::LiteralByte(n as u8),
                    256 => self.end_block(),
                    n @ 257..=285 => self.phase = Phase::Length((n - 257) as usize),
                    _ => return Err(self.error(ErrorKind::ReservedSymbol)),
                },
                Phase::LiteralByte(b) => {
                    if !self.emit(b, output, &mut o)? {
                        need_output!();
                    }
                    self.phase = Phase::Literal;
                }
                Phase::Length(n) => {
                    let length = LENGTH_BASE[n] + bits!(LENGTH_EXTRA[n]) as usize;
                    self.phase = Phase::Distance(length);
                }
                Phase::Distance(length) => {
                    let symbol = symbol!(TreeKind::Distances) as usize;
                    if symbol >= 30 {
                        return Err(self.error(ErrorKind::ReservedSymbol));
                    }
                    self.phase = Phase::DistanceExtra { length, symbol };
                }
                Phase::DistanceExtra { length, symbol } => {
                    let distance = DISTANCE_BASE[symbol] + bits!(DISTANCE_EXTRA[symbol]) as usize;
                    if distance > self.history || distance > self.window_limit {
                        return Err(self.error(ErrorKind::InvalidDistance));
                    }
                    self.phase = Phase::Copy { length, distance };
                }
                Phase::Copy { length, distance } => {
                    if length == 0 {
                        self.phase = Phase::Literal;
                        continue;
                    }
                    let from = (self.window_at + WINDOW - distance) % WINDOW;
                    if !self.emit(self.window[from], output, &mut o)? {
                        need_output!();
                    }
                    self.phase = Phase::Copy {
                        length: length - 1,
                        distance,
                    };
                }
                Phase::ZlibTrailer(n) => {
                    self.scratch[n as usize] = byte!();
                    if n < 3 {
                        self.phase = Phase::ZlibTrailer(n + 1);
                        continue;
                    }
                    let expected =
                        u32::from_be_bytes(self.scratch[..4].try_into().expect("four bytes"));
                    let actual = self.adler.value();
                    if expected != actual {
                        return Err(self.error(ErrorKind::ChecksumMismatch { expected, actual }));
                    }
                    self.members = 1;
                    self.phase = Phase::End;
                }
                Phase::GzipTrailer(n) => {
                    self.scratch[n as usize] = byte!();
                    if n < 7 {
                        self.phase = Phase::GzipTrailer(n + 1);
                        continue;
                    }
                    let expected =
                        u32::from_le_bytes(self.scratch[..4].try_into().expect("four bytes"));
                    let actual = self.crc.value();
                    if expected != actual {
                        return Err(self.error(ErrorKind::ChecksumMismatch { expected, actual }));
                    }
                    let expected =
                        u32::from_le_bytes(self.scratch[4..8].try_into().expect("four bytes"));
                    let actual = self.member_output as u32;
                    if expected != actual {
                        return Err(self.error(ErrorKind::GzipSizeMismatch { expected, actual }));
                    }
                    self.members += 1;
                    self.phase = if self.gzip_members == GzipMembers::First {
                        Phase::End
                    } else {
                        Phase::BetweenMembers
                    };
                }
                Phase::BetweenMembers => {
                    let rest = &input[i..];
                    if rest.is_empty() {
                        if end {
                            self.phase = Phase::End;
                        } else {
                            return Ok(Progress {
                                consumed: i,
                                written: o,
                                status: Status::NeedInput,
                            });
                        }
                    } else if rest[0] == 0x1f {
                        if rest.len() == 1 {
                            need_input!();
                        }
                        if rest[1] != 0x8b {
                            self.phase = Phase::End;
                            continue;
                        }
                        self.member_input_start = self.input;
                        self.member_output = 0;
                        self.history = 0;
                        self.window_at = 0;
                        self.header_bytes = 0;
                        self.header_crc = Crc32::new();
                        self.crc = Crc32::new();
                        self.adler = Adler32::new();
                        self.phase = Phase::GzipHeader(0);
                    } else {
                        self.phase = Phase::End;
                    }
                }
                Phase::End => {
                    if self.trailing == TrailingData::Reject {
                        if i < input.len() {
                            return Err(self.error(ErrorKind::TrailingData));
                        }
                        if !end {
                            return Ok(Progress {
                                consumed: i,
                                written: o,
                                status: Status::NeedInput,
                            });
                        }
                    }
                    self.finished = true;
                }
            }
        }
    }
}

fn initial_error(kind: ErrorKind) -> Error {
    Error {
        kind,
        input_consumed: 0,
        output_produced: 0,
        byte_offset: 0,
        bit_offset: 0,
    }
}

/// Collect a single bounded logical stream. No result escapes on failure.
pub fn decode(input: &[u8], options: DecodeOptions<'_>) -> Result<Decoded, Error> {
    let mut decoder = Decoder::new(options)?;
    let mut bytes = Vec::new();
    let mut consumed = 0;
    let mut scratch = [0; 8192];
    loop {
        // Do not produce bytes which this collector is unable to store.
        let budget = options
            .limits
            .max_allocation_bytes
            .saturating_sub(decoder.allocated_bytes());
        let remaining = budget.saturating_sub(bytes.len());
        let out_len = remaining.min(scratch.len());
        let progress = decoder.process(&input[consumed..], &mut scratch[..out_len], true)?;
        consumed += progress.consumed;
        let target = bytes
            .len()
            .checked_add(progress.written)
            .ok_or_else(|| decoder.error(ErrorKind::LimitExceeded(Limit::Allocation)))?;
        if target > bytes.capacity() {
            decoder.charge(bytes.len() as u64)?; // bound worst-case reallocation copying
            let capacity = target.max(bytes.capacity().saturating_mul(2)).min(budget);
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|_| decoder.error(ErrorKind::AllocationFailed))?;
            if bytes.capacity() > budget {
                return Err(decoder.error(ErrorKind::LimitExceeded(Limit::Allocation)));
            }
        }
        bytes.extend_from_slice(&scratch[..progress.written]);
        match progress.status {
            Status::Finished => {
                return Ok(Decoded {
                    bytes,
                    consumed,
                    members: decoder.members(),
                });
            }
            Status::NeedOutput if out_len == 0 => {
                return Err(decoder.error(ErrorKind::LimitExceeded(Limit::Allocation)));
            }
            Status::NeedOutput => {}
            Status::NeedInput => return Err(decoder.error(ErrorKind::Truncated)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeMethod {
    Stored,
    Fixed,
}

#[derive(Clone, Copy, Debug)]
pub struct EncodeLimits {
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_work: u64,
    /// Result capacity plus fixed encoder workspace (hash table for Fixed).
    pub max_allocation_bytes: usize,
}

impl Default for EncodeLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024 * 1024,
            max_output_bytes: 65 * 1024 * 1024,
            max_work: 1_000_000_000,
            max_allocation_bytes: 66 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EncodeOptions {
    pub format: Format,
    pub method: EncodeMethod,
    pub limits: EncodeLimits,
}

impl EncodeOptions {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            method: EncodeMethod::Fixed,
            limits: EncodeLimits::default(),
        }
    }
}

struct Writer {
    bytes: Vec<u8>,
    bits: u32,
    available: u8,
    limits: EncodeLimits,
    work: u64,
    input_at: usize,
    workspace: usize,
}

impl Writer {
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            input_consumed: self.input_at as u64,
            output_produced: self.bytes.len() as u64,
            byte_offset: self.input_at as u64,
            bit_offset: 0,
        }
    }
    fn charge(&mut self, n: u64) -> Result<(), Error> {
        self.work = self
            .work
            .checked_add(n)
            .filter(|&v| v <= self.limits.max_work)
            .ok_or_else(|| self.error(ErrorKind::LimitExceeded(Limit::Work)))?;
        Ok(())
    }
    fn append(&mut self, data: &[u8]) -> Result<(), Error> {
        let size = self
            .bytes
            .len()
            .checked_add(data.len())
            .ok_or_else(|| self.error(ErrorKind::LimitExceeded(Limit::Output)))?;
        if size > self.limits.max_output_bytes {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Output)));
        }
        let budget = self
            .limits
            .max_allocation_bytes
            .saturating_sub(self.workspace);
        if size > budget {
            return Err(self.error(ErrorKind::LimitExceeded(Limit::Allocation)));
        }
        self.charge(data.len() as u64)?;
        if size > self.bytes.capacity() {
            // Geometric growth amortizes allocation work; clamp before allocating.
            let capacity = size
                .max(self.bytes.capacity().saturating_mul(2))
                .max(64)
                .min(budget)
                .min(self.limits.max_output_bytes);
            self.charge(self.bytes.len() as u64)?;
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| self.error(ErrorKind::AllocationFailed))?;
            if self.bytes.capacity() > budget {
                return Err(self.error(ErrorKind::LimitExceeded(Limit::Allocation)));
            }
        }
        self.bytes.extend_from_slice(data);
        Ok(())
    }
    fn bits(&mut self, value: u32, n: u8) -> Result<(), Error> {
        self.charge(u64::from(n))?;
        self.bits |= value << self.available;
        self.available += n;
        while self.available >= 8 {
            self.append(&[self.bits as u8])?;
            self.bits >>= 8;
            self.available -= 8;
        }
        Ok(())
    }
    fn align(&mut self) -> Result<(), Error> {
        if self.available != 0 {
            self.append(&[self.bits as u8])?;
            self.bits = 0;
            self.available = 0;
        }
        Ok(())
    }
    fn fixed(&mut self, symbol: usize) -> Result<(), Error> {
        let (code, n) = match symbol {
            0..=143 => (symbol as u32 + 0x30, 8),
            144..=255 => (symbol as u32 - 144 + 0x190, 9),
            256..=279 => (symbol as u32 - 256, 7),
            _ => (symbol as u32 - 280 + 0xc0, 8),
        };
        self.bits(code.reverse_bits() >> (32 - n), n)
    }
}

/// Bounded deterministic encoding, including correct wrapper checksums. The
/// fixed encoder's single-candidate hash search bounds pathological match work.
pub fn encode(input: &[u8], options: EncodeOptions) -> Result<Vec<u8>, Error> {
    if input.len() > options.limits.max_input_bytes {
        return Err(initial_error(ErrorKind::LimitExceeded(Limit::Input)));
    }
    let workspace = std::mem::size_of::<Writer>()
        + if options.method == EncodeMethod::Fixed {
            std::mem::size_of::<[usize; 4096]>()
        } else {
            0
        };
    if workspace > options.limits.max_allocation_bytes {
        return Err(initial_error(ErrorKind::LimitExceeded(Limit::Allocation)));
    }
    let mut w = Writer {
        bytes: Vec::new(),
        bits: 0,
        available: 0,
        limits: options.limits,
        work: 0,
        input_at: 0,
        workspace,
    };
    match options.format {
        Format::Deflate => {}
        Format::Zlib => w.append(&[0x78, 0x01])?,
        Format::Gzip => w.append(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255])?,
    }
    match options.method {
        EncodeMethod::Stored => {
            if input.is_empty() {
                w.append(&[1, 0, 0, 255, 255])?;
            }
            for (n, chunk) in input.chunks(65535).enumerate() {
                w.input_at = n * 65535;
                w.append(&[u8::from(w.input_at + chunk.len() == input.len())])?;
                let len = chunk.len() as u16;
                w.append(&len.to_le_bytes())?;
                w.append(&(!len).to_le_bytes())?;
                w.append(chunk)?;
            }
        }
        EncodeMethod::Fixed => encode_fixed(input, &mut w)?,
    }
    w.input_at = input.len();
    // Account checksum traversal before performing it, including CRC's bit work.
    match options.format {
        Format::Deflate => {}
        Format::Zlib => {
            w.charge((input.len() as u64).saturating_mul(3))?;
            w.append(&adler32(input).to_be_bytes())?;
        }
        Format::Gzip => {
            w.charge((input.len() as u64).saturating_mul(10))?;
            w.append(&crc32(input).to_le_bytes())?;
            w.append(&(input.len() as u32).to_le_bytes())?;
        }
    }
    Ok(w.bytes)
}

fn hash3(data: &[u8], at: usize) -> usize {
    ((usize::from(data[at]) * 251 + usize::from(data[at + 1])) * 251 + usize::from(data[at + 2]))
        & 4095
}

fn encode_fixed(input: &[u8], w: &mut Writer) -> Result<(), Error> {
    w.charge(4096)?;
    let mut last = [usize::MAX; 4096];
    w.bits(3, 3)?; // BFINAL=1, BTYPE=01
    let mut at = 0;
    while at < input.len() {
        w.input_at = at;
        w.charge(1)?;
        let mut length = 0;
        let mut distance = 0;
        if input.len() - at >= 3 {
            w.charge(4)?;
            let hash = hash3(input, at);
            let candidate = last[hash];
            last[hash] = at;
            if candidate != usize::MAX && at - candidate <= WINDOW {
                let max = (input.len() - at).min(258);
                while length < max {
                    w.charge(1)?;
                    if input[candidate + length] != input[at + length] {
                        break;
                    }
                    length += 1;
                }
                distance = at - candidate;
            }
        }
        if length < 3 {
            w.fixed(usize::from(input[at]))?;
            at += 1;
            continue;
        }
        let li = if length == 258 {
            28
        } else {
            (0..28)
                .rev()
                .find(|&n| LENGTH_BASE[n] <= length)
                .expect("length >= 3")
        };
        w.fixed(li + 257)?;
        w.bits((length - LENGTH_BASE[li]) as u32, LENGTH_EXTRA[li])?;
        let di = (0..30)
            .rev()
            .find(|&n| DISTANCE_BASE[n] <= distance)
            .expect("distance >= 1");
        w.bits((di as u32).reverse_bits() >> 27, 5)?;
        w.bits((distance - DISTANCE_BASE[di]) as u32, DISTANCE_EXTRA[di])?;
        for n in at + 1..at + length {
            if input.len() - n >= 3 {
                w.charge(4)?;
                last[hash3(input, n)] = n;
            }
        }
        at += length;
    }
    w.fixed(256)?;
    w.align()
}
