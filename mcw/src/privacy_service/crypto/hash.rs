//! Keccak-f1600, SHA3-256 and SHAKE256 per FIPS 202.

use super::super::Error;

/// Both SHA3-256 and SHAKE256 use the 1088-bit rate.
#[derive(Clone)]
struct Sponge {
    state: [u64; 25],
    position: usize,
}
impl Sponge {
    fn new() -> Self {
        Self {
            state: [0; 25],
            position: 0,
        }
    }
    fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.state[self.position / 8] ^= (byte as u64) << (8 * (self.position % 8));
            self.position += 1;
            if self.position == 136 {
                keccak_f1600(&mut self.state);
                self.position = 0;
            }
        }
    }
    fn squeeze(mut self, suffix: u8, output: &mut [u8]) {
        self.state[self.position / 8] ^= (suffix as u64) << (8 * (self.position % 8));
        self.state[16] ^= 0x8000000000000000;
        keccak_f1600(&mut self.state);
        self.position = 0;
        for byte in output {
            if self.position == 136 {
                keccak_f1600(&mut self.state);
                self.position = 0;
            }
            *byte = (self.state[self.position / 8] >> (8 * (self.position % 8))) as u8;
            self.position += 1;
        }
    }
}
impl Drop for Sponge {
    fn drop(&mut self) {
        self.state.fill(0);
        std::hint::black_box(&mut self.state);
    }
}

#[derive(Clone)]
pub struct Sha3_256(Sponge);
impl Default for Sha3_256 {
    fn default() -> Self {
        Self::new()
    }
}
impl Sha3_256 {
    pub fn new() -> Self {
        Self(Sponge::new())
    }
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    pub fn digest(&self) -> [u8; 32] {
        let mut result = [0; 32];
        self.0.clone().squeeze(0x06, &mut result);
        result
    }
}
pub fn sha3_256(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha3_256::new();
    hash.update(bytes);
    hash.digest()
}
pub fn shake256(bytes: &[u8], output: &mut [u8]) -> Result<(), Error> {
    if output.len() > 65_536 {
        return Err(Error::LengthLimit);
    }
    let mut sponge = Sponge::new();
    sponge.update(bytes);
    sponge.squeeze(0x1f, output);
    Ok(())
}

pub fn keccak_f1600(state: &mut [u64; 25]) {
    const RC: [u64; 24] = [
        0x0000000000000001,
        0x0000000000008082,
        0x800000000000808a,
        0x8000000080008000,
        0x000000000000808b,
        0x0000000080000001,
        0x8000000080008081,
        0x8000000000008009,
        0x000000000000008a,
        0x0000000000000088,
        0x0000000080008009,
        0x000000008000000a,
        0x000000008000808b,
        0x800000000000008b,
        0x8000000000008089,
        0x8000000000008003,
        0x8000000000008002,
        0x8000000000000080,
        0x000000000000800a,
        0x800000008000000a,
        0x8000000080008081,
        0x8000000000008080,
        0x0000000080000001,
        0x8000000080008008,
    ];
    const RHO: [[u32; 5]; 5] = [
        [0, 36, 3, 41, 18],
        [1, 44, 10, 45, 2],
        [62, 6, 43, 15, 61],
        [28, 55, 25, 21, 56],
        [27, 20, 39, 8, 14],
    ];
    for rc in RC {
        let mut columns = [0; 5];
        for x in 0..5 {
            columns[x] = state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20];
        }
        for x in 0..5 {
            let delta = columns[(x + 4) % 5] ^ columns[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                state[x + 5 * y] ^= delta;
            }
        }
        let mut rotated = [0; 25];
        for x in 0..5 {
            for y in 0..5 {
                rotated[y + 5 * ((2 * x + 3 * y) % 5)] = state[x + 5 * y].rotate_left(RHO[x][y]);
            }
        }
        for x in 0..5 {
            for y in 0..5 {
                state[x + 5 * y] = rotated[x + 5 * y]
                    ^ (!rotated[(x + 1) % 5 + 5 * y] & rotated[(x + 2) % 5 + 5 * y]);
            }
        }
        state[0] ^= rc;
    }
}
