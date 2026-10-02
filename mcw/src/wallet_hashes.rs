//! Portable wallet hash primitives; first-party Rust and the standard library only.
//!
//! SHA-256 is the actual sibling `bitcoin_encoding` implementation, not a copy.
//! RIPEMD-160 follows the designers' specification; SHA-512 follows FIPS 180-4;
//! HMAC and PBKDF2 follow RFC 2104 and RFC 8018. See the conformance fixture
//! manifest for source/vector provenance. Inputs are bytes, with no text encoding,
//! normalization, key derivation, signing, wallet state, bridge, or OS policy.
//!
//! Compression schedules and control flow do not depend on secret byte values.
//! Lengths are public. MAC comparison visits every byte of equal-length inputs
//! with optimization barriers, but Rust/compiler/CPU timing is not a formal
//! constant-time guarantee. Safe-Rust clearing is best effort: copies, registers,
//! caller buffers, and the reused SHA-256 state are not guaranteed erased.
#![forbid(unsafe_code)]

use crate::bitcoin_encoding::Sha256;
use std::{fmt, hint::black_box};

pub const MAX_RIPEMD160_BYTES: u64 = u64::MAX / 8;
pub const MAX_SHA512_BYTES: u128 = u128::MAX / 8;
/// Admission limits, not recommended password-hardening parameters.
pub const MAX_PBKDF2_INPUT_BYTES: usize = 1_048_576;
pub const MAX_PBKDF2_OUTPUT_BYTES: usize = 65_536;
pub const MAX_PBKDF2_ITERATIONS: u32 = 1_000_000;
/// Total hash compression blocks, including long keys/salts and all output blocks.
pub const MAX_PBKDF2_WORK_BLOCKS: u64 = 16_000_000;

/// Errors contain no supplied bytes, lengths, keys, salts, or computed digests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Ripemd160MessageTooLong,
    Sha256MessageTooLong,
    Sha512MessageTooLong,
    InvalidIterations,
    IterationLimit,
    InvalidOutputLength,
    OutputLimit,
    InputLimit,
    WorkLimit,
    AllocationFailed,
    InvalidMacLength,
    MacMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ripemd160MessageTooLong => "RIPEMD-160 message length limit exceeded",
            Self::Sha256MessageTooLong => "SHA-256 message length limit exceeded",
            Self::Sha512MessageTooLong => "SHA-512 message length limit exceeded",
            Self::InvalidIterations => "PBKDF2 requires a positive iteration count",
            Self::IterationLimit => "PBKDF2 iteration limit exceeded",
            Self::InvalidOutputLength => "PBKDF2 requires a positive output length",
            Self::OutputLimit => "PBKDF2 output limit exceeded",
            Self::InputLimit => "PBKDF2 input limit exceeded",
            Self::WorkLimit => "PBKDF2 work limit exceeded",
            Self::AllocationFailed => "PBKDF2 output allocation failed",
            Self::InvalidMacLength => "full-length MAC required",
            Self::MacMismatch => "MAC verification failed",
        })
    }
}
impl std::error::Error for Error {}

fn sha256_update(hash: &mut Sha256, bytes: &[u8]) -> Result<(), Error> {
    // This upstream operation can return only its message-length error. Never
    // forward codec diagnostics which might carry bytes from another operation.
    hash.update(bytes).map_err(|_| Error::Sha256MessageTooLong)
}

fn clear(bytes: &mut [u8]) {
    bytes.fill(0);
    black_box(bytes);
}

/// Equal-length comparison has no early exit or secret-indexed memory access.
/// Length mismatch returns immediately; callers must treat lengths as public.
#[inline(never)]
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let (left, right) = (black_box(left), black_box(right));
    let mut difference = 0u8;
    for (&a, &b) in left.iter().zip(right) {
        difference |= black_box(a) ^ black_box(b);
    }
    black_box(difference) == 0
}

#[derive(Clone)]
pub struct Ripemd160 {
    state: [u32; 5],
    block: [u8; 64],
    used: usize,
    bytes: u64,
}
impl fmt::Debug for Ripemd160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ripemd160([REDACTED])")
    }
}
impl Default for Ripemd160 {
    fn default() -> Self {
        Self::new()
    }
}
impl Ripemd160 {
    pub fn new() -> Self {
        Self {
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0],
            block: [0; 64],
            used: 0,
            bytes: 0,
        }
    }
    /// An excessive length rejects the whole update without changing the state.
    pub fn update(&mut self, mut input: &[u8]) -> Result<(), Error> {
        let length = u64::try_from(input.len()).map_err(|_| Error::Ripemd160MessageTooLong)?;
        let total = self
            .bytes
            .checked_add(length)
            .filter(|&n| n <= MAX_RIPEMD160_BYTES)
            .ok_or(Error::Ripemd160MessageTooLong)?;
        self.bytes = total;
        if self.used != 0 {
            let count = (64 - self.used).min(input.len());
            self.block[self.used..self.used + count].copy_from_slice(&input[..count]);
            self.used += count;
            input = &input[count..];
            if self.used < 64 {
                return Ok(());
            }
            compress_ripemd160(&mut self.state, &self.block);
            self.used = 0;
        }
        let (blocks, tail) = input.as_chunks::<64>();
        for block in blocks {
            compress_ripemd160(&mut self.state, block);
        }
        self.block[..tail.len()].copy_from_slice(tail);
        self.used = tail.len();
        Ok(())
    }
    /// Consumes the state; digest is the specification's little-endian bytes.
    pub fn finalize(mut self) -> [u8; 20] {
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 56 {
            self.block[self.used..].fill(0);
            compress_ripemd160(&mut self.state, &self.block);
            self.block.fill(0);
        } else {
            self.block[self.used..56].fill(0);
        }
        self.block[56..].copy_from_slice(&(self.bytes * 8).to_le_bytes());
        compress_ripemd160(&mut self.state, &self.block);
        let mut digest = [0; 20];
        for (word, out) in self.state.iter().zip(digest.as_chunks_mut::<4>().0) {
            *out = word.to_le_bytes();
        }
        digest
    }
}
impl Drop for Ripemd160 {
    fn drop(&mut self) {
        self.state.fill(0);
        black_box(&mut self.state);
        clear(&mut self.block);
        self.bytes = 0;
        self.used = 0;
    }
}

fn ripemd_function(round: usize, x: u32, y: u32, z: u32) -> u32 {
    match round {
        0 => x ^ y ^ z,
        1 => (x & y) | (!x & z),
        2 => (x | !y) ^ z,
        3 => (x & z) | (y & !z),
        _ => x ^ (y | !z),
    }
}
fn compress_ripemd160(state: &mut [u32; 5], block: &[u8; 64]) {
    // Public round-indexed permutations and rotations from the specification.
    const R: [usize; 80] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9,
        5, 2, 14, 11, 8, 3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12, 1, 9, 11, 10, 0, 8,
        12, 4, 13, 3, 7, 15, 14, 5, 6, 2, 4, 0, 5, 9, 7, 12, 2, 10, 14, 1, 3, 8, 11, 6, 15, 13,
    ];
    const RR: [usize; 80] = [
        5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12, 6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8,
        12, 4, 9, 1, 2, 15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13, 8, 6, 4, 1, 3, 11,
        15, 0, 5, 12, 2, 13, 9, 7, 10, 14, 12, 15, 10, 4, 1, 5, 8, 7, 6, 2, 13, 14, 0, 3, 9, 11,
    ];
    const S: [u32; 80] = [
        11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8, 7, 6, 8, 13, 11, 9, 7, 15, 7, 12,
        15, 9, 11, 7, 13, 12, 11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5, 11, 12, 14,
        15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12, 9, 15, 5, 11, 6, 8, 13, 12, 5, 12, 13, 14, 11,
        8, 5, 6,
    ];
    const SS: [u32; 80] = [
        8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6, 9, 13, 15, 7, 12, 8, 9, 11, 7, 7,
        12, 7, 6, 15, 13, 11, 9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5, 15, 5, 8, 11,
        14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8, 8, 5, 12, 9, 12, 5, 14, 6, 8, 13, 6, 5, 15, 13,
        11, 11,
    ];
    const K: [u32; 5] = [0, 0x5a827999, 0x6ed9eba1, 0x8f1bbcdc, 0xa953fd4e];
    const KK: [u32; 5] = [0x50a28be6, 0x5c4dd124, 0x6d703ef3, 0x7a6d76e9, 0];
    let mut words = [0u32; 16];
    for (out, bytes) in words.iter_mut().zip(block.as_chunks::<4>().0) {
        *out = u32::from_le_bytes(*bytes);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    let [mut aa, mut bb, mut cc, mut dd, mut ee] = *state;
    for i in 0..80 {
        let round = i / 16;
        let t = a
            .wrapping_add(ripemd_function(round, b, c, d))
            .wrapping_add(words[R[i]])
            .wrapping_add(K[round])
            .rotate_left(S[i])
            .wrapping_add(e);
        a = e;
        e = d;
        d = c.rotate_left(10);
        c = b;
        b = t;
        let t = aa
            .wrapping_add(ripemd_function(4 - round, bb, cc, dd))
            .wrapping_add(words[RR[i]])
            .wrapping_add(KK[round])
            .rotate_left(SS[i])
            .wrapping_add(ee);
        aa = ee;
        ee = dd;
        dd = cc.rotate_left(10);
        cc = bb;
        bb = t;
    }
    let t = state[1].wrapping_add(c).wrapping_add(dd);
    state[1] = state[2].wrapping_add(d).wrapping_add(ee);
    state[2] = state[3].wrapping_add(e).wrapping_add(aa);
    state[3] = state[4].wrapping_add(a).wrapping_add(bb);
    state[4] = state[0].wrapping_add(b).wrapping_add(cc);
    state[0] = t;
    words.fill(0);
    black_box(&mut words);
}
pub fn ripemd160(bytes: &[u8]) -> Result<[u8; 20], Error> {
    let mut hash = Ripemd160::new();
    hash.update(bytes)?;
    Ok(hash.finalize())
}

#[derive(Clone)]
pub struct Sha512 {
    state: [u64; 8],
    block: [u8; 128],
    used: usize,
    bytes: u128,
}
impl fmt::Debug for Sha512 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Sha512([REDACTED])")
    }
}
impl Default for Sha512 {
    fn default() -> Self {
        Self::new()
    }
}
impl Sha512 {
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667f3bcc908,
                0xbb67ae8584caa73b,
                0x3c6ef372fe94f82b,
                0xa54ff53a5f1d36f1,
                0x510e527fade682d1,
                0x9b05688c2b3e6c1f,
                0x1f83d9abfb41bd6b,
                0x5be0cd19137e2179,
            ],
            block: [0; 128],
            used: 0,
            bytes: 0,
        }
    }
    /// Checks the full 128-bit bit-length bound before changing state.
    pub fn update(&mut self, mut input: &[u8]) -> Result<(), Error> {
        let total = self
            .bytes
            .checked_add(input.len() as u128)
            .filter(|&n| n <= MAX_SHA512_BYTES)
            .ok_or(Error::Sha512MessageTooLong)?;
        self.bytes = total;
        if self.used != 0 {
            let count = (128 - self.used).min(input.len());
            self.block[self.used..self.used + count].copy_from_slice(&input[..count]);
            self.used += count;
            input = &input[count..];
            if self.used < 128 {
                return Ok(());
            }
            compress_sha512(&mut self.state, &self.block);
            self.used = 0;
        }
        let (blocks, tail) = input.as_chunks::<128>();
        for block in blocks {
            compress_sha512(&mut self.state, block);
        }
        self.block[..tail.len()].copy_from_slice(tail);
        self.used = tail.len();
        Ok(())
    }
    pub fn finalize(mut self) -> [u8; 64] {
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 112 {
            self.block[self.used..].fill(0);
            compress_sha512(&mut self.state, &self.block);
            self.block.fill(0);
        } else {
            self.block[self.used..112].fill(0);
        }
        self.block[112..].copy_from_slice(&(self.bytes * 8).to_be_bytes());
        compress_sha512(&mut self.state, &self.block);
        let mut digest = [0; 64];
        for (word, out) in self.state.iter().zip(digest.as_chunks_mut::<8>().0) {
            *out = word.to_be_bytes();
        }
        digest
    }
}
impl Drop for Sha512 {
    fn drop(&mut self) {
        self.state.fill(0);
        black_box(&mut self.state);
        clear(&mut self.block);
        self.bytes = 0;
        self.used = 0;
    }
}
fn compress_sha512(state: &mut [u64; 8], block: &[u8; 128]) {
    const K: [u64; 80] = [
        0x428a2f98d728ae22,
        0x7137449123ef65cd,
        0xb5c0fbcfec4d3b2f,
        0xe9b5dba58189dbbc,
        0x3956c25bf348b538,
        0x59f111f1b605d019,
        0x923f82a4af194f9b,
        0xab1c5ed5da6d8118,
        0xd807aa98a3030242,
        0x12835b0145706fbe,
        0x243185be4ee4b28c,
        0x550c7dc3d5ffb4e2,
        0x72be5d74f27b896f,
        0x80deb1fe3b1696b1,
        0x9bdc06a725c71235,
        0xc19bf174cf692694,
        0xe49b69c19ef14ad2,
        0xefbe4786384f25e3,
        0x0fc19dc68b8cd5b5,
        0x240ca1cc77ac9c65,
        0x2de92c6f592b0275,
        0x4a7484aa6ea6e483,
        0x5cb0a9dcbd41fbd4,
        0x76f988da831153b5,
        0x983e5152ee66dfab,
        0xa831c66d2db43210,
        0xb00327c898fb213f,
        0xbf597fc7beef0ee4,
        0xc6e00bf33da88fc2,
        0xd5a79147930aa725,
        0x06ca6351e003826f,
        0x142929670a0e6e70,
        0x27b70a8546d22ffc,
        0x2e1b21385c26c926,
        0x4d2c6dfc5ac42aed,
        0x53380d139d95b3df,
        0x650a73548baf63de,
        0x766a0abb3c77b2a8,
        0x81c2c92e47edaee6,
        0x92722c851482353b,
        0xa2bfe8a14cf10364,
        0xa81a664bbc423001,
        0xc24b8b70d0f89791,
        0xc76c51a30654be30,
        0xd192e819d6ef5218,
        0xd69906245565a910,
        0xf40e35855771202a,
        0x106aa07032bbd1b8,
        0x19a4c116b8d2d0c8,
        0x1e376c085141ab53,
        0x2748774cdf8eeb99,
        0x34b0bcb5e19b48a8,
        0x391c0cb3c5c95a63,
        0x4ed8aa4ae3418acb,
        0x5b9cca4f7763e373,
        0x682e6ff3d6b2b8a3,
        0x748f82ee5defb2fc,
        0x78a5636f43172f60,
        0x84c87814a1f0ab72,
        0x8cc702081a6439ec,
        0x90befffa23631e28,
        0xa4506cebde82bde9,
        0xbef9a3f7b2c67915,
        0xc67178f2e372532b,
        0xca273eceea26619c,
        0xd186b8c721c0c207,
        0xeada7dd6cde0eb1e,
        0xf57d4f7fee6ed178,
        0x06f067aa72176fba,
        0x0a637dc5a2c898a6,
        0x113f9804bef90dae,
        0x1b710b35131c471b,
        0x28db77f523047d84,
        0x32caab7b40c72493,
        0x3c9ebe0a15c9bebc,
        0x431d67c49c100d4c,
        0x4cc5d4becb3e42b6,
        0x597f299cfc657e2a,
        0x5fcb6fab3ad6faec,
        0x6c44198c4a475817,
    ];
    let mut w = [0u64; 80];
    for (out, bytes) in w.iter_mut().zip(block.as_chunks::<8>().0) {
        *out = u64::from_be_bytes(*bytes);
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let choice = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choice)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (out, word) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *out = out.wrapping_add(word);
    }
    w.fill(0);
    black_box(&mut w);
}
pub fn sha512(bytes: &[u8]) -> Result<[u8; 64], Error> {
    let mut hash = Sha512::new();
    hash.update(bytes)?;
    Ok(hash.finalize())
}

/// Streaming RIPEMD160(SHA256(message)), using the committed SHA-256 sibling.
#[derive(Clone, Default)]
pub struct Hash160(Sha256);
impl fmt::Debug for Hash160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Hash160([REDACTED])")
    }
}
impl Hash160 {
    pub fn new() -> Self {
        Self(Sha256::new())
    }
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), Error> {
        sha256_update(&mut self.0, bytes)
    }
    pub fn finalize(self) -> [u8; 20] {
        let mut digest = self.0.finalize();
        let mut hash = Ripemd160::new();
        // A fixed 32-byte input cannot approach the RIPEMD-160 length limit.
        hash.block[..32].copy_from_slice(&digest);
        hash.used = 32;
        hash.bytes = 32;
        clear(&mut digest);
        hash.finalize()
    }
}
pub fn hash160(bytes: &[u8]) -> Result<[u8; 20], Error> {
    let mut hash = Hash160::new();
    hash.update(bytes)?;
    Ok(hash.finalize())
}

/// Prepared keyed state. Cloning duplicates sensitive state; Debug is redacted.
#[derive(Clone)]
pub struct HmacSha256 {
    inner: Sha256,
    outer: Sha256,
}
impl fmt::Debug for HmacSha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HmacSha256([REDACTED])")
    }
}
impl HmacSha256 {
    /// All key lengths (including zero and longer than the block) follow HMAC.
    pub fn new(key: &[u8]) -> Result<Self, Error> {
        let mut pad = [0u8; 64];
        if key.len() > 64 {
            let mut hash = Sha256::new();
            sha256_update(&mut hash, key)?;
            let mut digest = hash.finalize();
            pad[..32].copy_from_slice(&digest);
            clear(&mut digest);
        } else {
            pad[..key.len()].copy_from_slice(key);
        }
        for byte in &mut pad {
            *byte ^= 0x36;
        }
        let mut inner = Sha256::new();
        sha256_update(&mut inner, &pad)?;
        for byte in &mut pad {
            *byte ^= 0x36 ^ 0x5c;
        }
        let mut outer = Sha256::new();
        sha256_update(&mut outer, &pad)?;
        clear(&mut pad);
        Ok(Self { inner, outer })
    }
    /// Payload capacity is MAX_SHA256_BYTES minus the 64-byte inner prefix.
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), Error> {
        sha256_update(&mut self.inner, bytes)
    }
    pub fn finalize(mut self) -> Result<[u8; 32], Error> {
        let mut digest = self.inner.finalize();
        let result = sha256_update(&mut self.outer, &digest);
        clear(&mut digest);
        result?;
        Ok(self.outer.finalize())
    }
    /// Requires the entire 32-byte MAC; no implicit truncation or empty success.
    pub fn verify(self, expected: &[u8]) -> Result<(), Error> {
        if expected.len() != 32 {
            return Err(Error::InvalidMacLength);
        }
        let mut actual = self.finalize()?;
        let equal = constant_time_eq(&actual, expected);
        clear(&mut actual);
        if equal {
            Ok(())
        } else {
            Err(Error::MacMismatch)
        }
    }
}
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> Result<[u8; 32], Error> {
    let mut mac = HmacSha256::new(key)?;
    mac.update(message)?;
    mac.finalize()
}
pub fn verify_hmac_sha256(key: &[u8], message: &[u8], expected: &[u8]) -> Result<(), Error> {
    if expected.len() != 32 {
        return Err(Error::InvalidMacLength);
    }
    let mut mac = HmacSha256::new(key)?;
    mac.update(message)?;
    mac.verify(expected)
}

#[derive(Clone)]
pub struct HmacSha512 {
    inner: Sha512,
    outer: Sha512,
}
impl fmt::Debug for HmacSha512 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HmacSha512([REDACTED])")
    }
}
impl HmacSha512 {
    pub fn new(key: &[u8]) -> Result<Self, Error> {
        let mut pad = [0u8; 128];
        if key.len() > 128 {
            let mut digest = sha512(key)?;
            pad[..64].copy_from_slice(&digest);
            clear(&mut digest);
        } else {
            pad[..key.len()].copy_from_slice(key);
        }
        for byte in &mut pad {
            *byte ^= 0x36;
        }
        let mut inner = Sha512::new();
        inner.update(&pad)?;
        for byte in &mut pad {
            *byte ^= 0x36 ^ 0x5c;
        }
        let mut outer = Sha512::new();
        outer.update(&pad)?;
        clear(&mut pad);
        Ok(Self { inner, outer })
    }
    /// Payload capacity is MAX_SHA512_BYTES minus the 128-byte inner prefix.
    pub fn update(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.inner.update(bytes)
    }
    pub fn finalize(mut self) -> Result<[u8; 64], Error> {
        let mut digest = self.inner.finalize();
        let result = self.outer.update(&digest);
        clear(&mut digest);
        result?;
        Ok(self.outer.finalize())
    }
    pub fn verify(self, expected: &[u8]) -> Result<(), Error> {
        if expected.len() != 64 {
            return Err(Error::InvalidMacLength);
        }
        let mut actual = self.finalize()?;
        let equal = constant_time_eq(&actual, expected);
        clear(&mut actual);
        if equal {
            Ok(())
        } else {
            Err(Error::MacMismatch)
        }
    }
}
pub fn hmac_sha512(key: &[u8], message: &[u8]) -> Result<[u8; 64], Error> {
    let mut mac = HmacSha512::new(key)?;
    mac.update(message)?;
    mac.finalize()
}
pub fn verify_hmac_sha512(key: &[u8], message: &[u8], expected: &[u8]) -> Result<(), Error> {
    if expected.len() != 64 {
        return Err(Error::InvalidMacLength);
    }
    let mut mac = HmacSha512::new(key)?;
    mac.update(message)?;
    mac.verify(expected)
}

trait PbkdfMac<const N: usize>: Clone {
    const BLOCK: u64;
    const PADDING: u64;
    fn keyed(key: &[u8]) -> Result<Self, Error>;
    fn update(&mut self, bytes: &[u8]) -> Result<(), Error>;
    fn finish(self) -> Result<[u8; N], Error>;
}
impl PbkdfMac<32> for HmacSha256 {
    const BLOCK: u64 = 64;
    const PADDING: u64 = 9;
    fn keyed(key: &[u8]) -> Result<Self, Error> {
        Self::new(key)
    }
    fn update(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.update(bytes)
    }
    fn finish(self) -> Result<[u8; 32], Error> {
        self.finalize()
    }
}
impl PbkdfMac<64> for HmacSha512 {
    const BLOCK: u64 = 128;
    const PADDING: u64 = 17;
    fn keyed(key: &[u8]) -> Result<Self, Error> {
        Self::new(key)
    }
    fn update(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.update(bytes)
    }
    fn finish(self) -> Result<[u8; 64], Error> {
        self.finalize()
    }
}
fn validate_pbkdf2<const N: usize, M: PbkdfMac<N>>(
    password_len: usize,
    salt_len: usize,
    iterations: u32,
    output_len: usize,
) -> Result<(), Error> {
    if iterations == 0 {
        return Err(Error::InvalidIterations);
    }
    if iterations > MAX_PBKDF2_ITERATIONS {
        return Err(Error::IterationLimit);
    }
    if output_len == 0 {
        return Err(Error::InvalidOutputLength);
    }
    if output_len > MAX_PBKDF2_OUTPUT_BYTES {
        return Err(Error::OutputLimit);
    }
    if password_len > MAX_PBKDF2_INPUT_BYTES || salt_len > MAX_PBKDF2_INPUT_BYTES {
        return Err(Error::InputLimit);
    }
    let blocks = u64::try_from(output_len.div_ceil(N)).map_err(|_| Error::OutputLimit)?;
    if blocks > u64::from(u32::MAX) {
        return Err(Error::OutputLimit);
    }
    let password_len = password_len as u64;
    let salt_len = salt_len as u64;
    let key_work = if password_len > M::BLOCK {
        (password_len + M::PADDING).div_ceil(M::BLOCK)
    } else {
        0
    };
    // Precompute full salt blocks once. Each U1 finishes a cloned salt prefix;
    // remaining U values each use one inner and one outer compression block.
    let per_block = (salt_len % M::BLOCK + 4 + M::PADDING).div_ceil(M::BLOCK)
        + 1
        + 2 * (u64::from(iterations) - 1);
    let work = blocks
        .checked_mul(per_block)
        .and_then(|n| n.checked_add(key_work + 2 + salt_len / M::BLOCK))
        .ok_or(Error::WorkLimit)?;
    if work > MAX_PBKDF2_WORK_BLOCKS {
        return Err(Error::WorkLimit);
    }
    Ok(())
}
fn pbkdf2_into<const N: usize, M: PbkdfMac<N>>(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output: &mut [u8],
) -> Result<(), Error> {
    validate_pbkdf2::<N, M>(password.len(), salt.len(), iterations, output.len())?;
    let keyed = M::keyed(password)?;
    let mut salted = keyed.clone();
    salted.update(salt)?;
    for (index, chunk) in output.chunks_mut(N).enumerate() {
        // Validation bounds the block count to 2048 and prevents counter wrap.
        let counter = u32::try_from(index + 1).map_err(|_| Error::OutputLimit)?;
        let mut first = salted.clone();
        first.update(&counter.to_be_bytes())?;
        let mut u = first.finish()?;
        let mut total = u;
        for _ in 1..iterations {
            let mut next = keyed.clone();
            next.update(&u)?;
            clear(&mut u);
            u = next.finish()?;
            for (a, &b) in total.iter_mut().zip(&u) {
                *a ^= b;
            }
        }
        chunk.copy_from_slice(&total[..chunk.len()]);
        clear(&mut u);
        clear(&mut total);
    }
    Ok(())
}
fn pbkdf2_alloc<const N: usize, M: PbkdfMac<N>>(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output_len: usize,
) -> Result<Vec<u8>, Error> {
    validate_pbkdf2::<N, M>(password.len(), salt.len(), iterations, output_len)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_len)
        .map_err(|_| Error::AllocationFailed)?;
    output.resize(output_len, 0);
    if let Err(error) = pbkdf2_into::<N, M>(password, salt, iterations, &mut output) {
        clear(&mut output);
        return Err(error);
    }
    Ok(output)
}

/// RFC 8018 PBKDF2-HMAC-SHA256. Empty password/salt are valid octet strings.
/// Zero iterations/output are rejected. No string normalization is performed.
pub fn pbkdf2_hmac_sha256(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output_len: usize,
) -> Result<Vec<u8>, Error> {
    pbkdf2_alloc::<32, HmacSha256>(password, salt, iterations, output_len)
}
/// Parameter rejection leaves the caller's output unchanged; no allocation.
pub fn pbkdf2_hmac_sha256_into(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output: &mut [u8],
) -> Result<(), Error> {
    pbkdf2_into::<32, HmacSha256>(password, salt, iterations, output)
}
/// RFC 8018 PBKDF2-HMAC-SHA512 with the same admission policy as SHA256.
pub fn pbkdf2_hmac_sha512(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output_len: usize,
) -> Result<Vec<u8>, Error> {
    pbkdf2_alloc::<64, HmacSha512>(password, salt, iterations, output_len)
}
pub fn pbkdf2_hmac_sha512_into(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    output: &mut [u8],
) -> Result<(), Error> {
    pbkdf2_into::<64, HmacSha512>(password, salt, iterations, output)
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn checked_hash_lengths_reject_atomically() {
        let mut r = Ripemd160::new();
        r.update(b"synthetic").unwrap();
        r.bytes = MAX_RIPEMD160_BYTES;
        let before = (r.state, r.block, r.used, r.bytes);
        assert_eq!(r.update(b"x"), Err(Error::Ripemd160MessageTooLong));
        assert_eq!((r.state, r.block, r.used, r.bytes), before);
        assert!(r.update(b"").is_ok());
        r.bytes = u64::MAX;
        assert_eq!(r.update(b"x"), Err(Error::Ripemd160MessageTooLong));
        let mut s = Sha512::new();
        s.update(b"synthetic").unwrap();
        s.bytes = MAX_SHA512_BYTES;
        let before = (s.state, s.block, s.used, s.bytes);
        assert_eq!(s.update(b"x"), Err(Error::Sha512MessageTooLong));
        assert_eq!((s.state, s.block, s.used, s.bytes), before);
        assert!(s.update(b"").is_ok());
        s.bytes = u128::MAX;
        assert_eq!(s.update(b"x"), Err(Error::Sha512MessageTooLong));
    }
    #[test]
    fn sha512_hmac_accounts_for_inner_key_block() {
        let mut h = HmacSha512::new(b"synthetic").unwrap();
        assert_eq!(h.inner.bytes, 128);
        h.inner.bytes = MAX_SHA512_BYTES;
        let before = (h.inner.state, h.inner.block, h.inner.used, h.inner.bytes);
        assert_eq!(h.update(b"x"), Err(Error::Sha512MessageTooLong));
        assert_eq!(
            (h.inner.state, h.inner.block, h.inner.used, h.inner.bytes),
            before
        );
    }
    #[test]
    fn full_width_length_fields_are_used() {
        let mut s = Sha512::new();
        s.bytes = (1u128 << 64) + 3;
        s.used = 3;
        s.block[..3].copy_from_slice(b"abc");
        let mut block = s.block;
        block[3] = 0x80;
        block[112..].copy_from_slice(&(s.bytes * 8).to_be_bytes());
        let mut expected = s.state;
        compress_sha512(&mut expected, &block);
        let output = s.finalize();
        for (word, bytes) in expected.iter().zip(output.as_chunks::<8>().0) {
            assert_eq!(word.to_be_bytes(), *bytes);
        }
        let mut r = Ripemd160::new();
        r.bytes = (1u64 << 32) + 3;
        r.used = 3;
        r.block[..3].copy_from_slice(b"abc");
        let mut block = r.block;
        block[3] = 0x80;
        block[56..].copy_from_slice(&(r.bytes * 8).to_le_bytes());
        let mut expected = r.state;
        compress_ripemd160(&mut expected, &block);
        let output = r.finalize();
        for (word, bytes) in expected.iter().zip(output.as_chunks::<4>().0) {
            assert_eq!(word.to_le_bytes(), *bytes);
        }
    }
    #[test]
    fn admission_bounds_cover_combined_work_and_counter() {
        for n in [1, 31, 32, 33, 63, 64, 65, MAX_PBKDF2_OUTPUT_BYTES] {
            assert!(validate_pbkdf2::<32, HmacSha256>(0, 0, 1, n).is_ok());
            assert!(validate_pbkdf2::<64, HmacSha512>(0, 0, 1, n).is_ok());
        }
        assert!(
            validate_pbkdf2::<32, HmacSha256>(
                MAX_PBKDF2_INPUT_BYTES,
                MAX_PBKDF2_INPUT_BYTES,
                1,
                32
            )
            .is_ok()
        );
        assert!(
            validate_pbkdf2::<64, HmacSha512>(
                MAX_PBKDF2_INPUT_BYTES,
                MAX_PBKDF2_INPUT_BYTES,
                1,
                64
            )
            .is_ok()
        );
        assert!(validate_pbkdf2::<64, HmacSha512>(0, 0, MAX_PBKDF2_ITERATIONS, 64).is_ok());
        assert_eq!(
            validate_pbkdf2::<64, HmacSha512>(0, 0, MAX_PBKDF2_ITERATIONS, 513),
            Err(Error::WorkLimit)
        );
        assert_eq!(
            validate_pbkdf2::<32, HmacSha256>(0, 0, MAX_PBKDF2_ITERATIONS, 257),
            Err(Error::WorkLimit)
        );
        assert_eq!(
            validate_pbkdf2::<32, HmacSha256>(usize::MAX, 0, 1, 32),
            Err(Error::InputLimit)
        );
        assert_eq!(
            validate_pbkdf2::<64, HmacSha512>(0, 0, 1, usize::MAX),
            Err(Error::OutputLimit)
        );
    }
}
