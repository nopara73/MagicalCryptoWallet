//! STROBE-128 v1.0.2 operations used by the retained round fingerprint.
//! Byte order and the 166-byte duplex rate are protocol-defined. The shared
//! first-party Keccak permutation is supplied by the privacy hash leaf.
use super::Error;
use crate::privacy_service::crypto::hash::keccak_f1600;

const RATE: usize = 166;
const AD: u8 = 2;
const META_AD: u8 = 18;
const PRF: u8 = 7;

pub struct Strobe {
    state: [u8; 200],
    position: usize,
    begin: u8,
    flags: u8,
}

impl Strobe {
    pub fn new(protocol: &[u8]) -> Self {
        let mut state = [0; 200];
        state[..6].copy_from_slice(&[1, 168, 1, 0, 1, 96]);
        state[6..18].copy_from_slice(b"STROBEv1.0.2");
        permute(&mut state);
        let mut result = Self {
            state,
            position: 0,
            begin: 0,
            flags: 0,
        };
        result.meta_ad(protocol, false).expect("initial operation");
        result
    }

    fn run(&mut self) {
        self.state[self.position] ^= self.begin;
        self.state[self.position + 1] ^= 4;
        self.state[RATE + 1] ^= 128;
        permute(&mut self.state);
        self.position = 0;
        self.begin = 0;
    }

    fn absorb(&mut self, data: &[u8]) {
        for byte in data {
            self.state[self.position] ^= byte;
            self.position += 1;
            if self.position == RATE {
                self.run();
            }
        }
    }

    fn start(&mut self, flags: u8, more: bool) -> Result<(), Error> {
        if more {
            return if self.flags == flags {
                Ok(())
            } else {
                Err(Error::InvalidContinuation)
            };
        }
        let previous = self.begin;
        self.begin = (self.position + 1) as u8;
        self.flags = flags;
        self.absorb(&[previous, flags]);
        if flags & 4 != 0 && self.position != 0 {
            self.run();
        }
        Ok(())
    }

    pub fn meta_ad(&mut self, data: &[u8], more: bool) -> Result<(), Error> {
        self.start(META_AD, more)?;
        self.absorb(data);
        Ok(())
    }

    pub fn ad(&mut self, data: &[u8], more: bool) -> Result<(), Error> {
        self.start(AD, more)?;
        self.absorb(data);
        Ok(())
    }

    pub fn prf(&mut self, output: &mut [u8], more: bool) -> Result<(), Error> {
        self.start(PRF, more)?;
        for byte in output {
            *byte = self.state[self.position];
            self.state[self.position] = 0;
            self.position += 1;
            if self.position == RATE {
                self.run();
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn test_state(&self) -> &[u8; 200] {
        &self.state
    }

    // Published STROBEgo checkpoints let tests exercise only AD/meta-AD/PRF
    // after KEY without implementing or shipping an unused KEY operation.
    #[cfg(test)]
    pub fn from_test_checkpoint(state: [u8; 200], position: usize, begin: u8, flags: u8) -> Self {
        assert!(position < RATE && begin <= RATE as u8);
        Self {
            state,
            position,
            begin,
            flags,
        }
    }
}

impl Drop for Strobe {
    fn drop(&mut self) {
        self.state.fill(0);
        std::hint::black_box(&mut self.state);
    }
}

fn permute(bytes: &mut [u8; 200]) {
    let mut lanes = [0; 25];
    for (lane, bytes) in lanes.iter_mut().zip(bytes.as_chunks::<8>().0) {
        *lane = u64::from_le_bytes(*bytes);
    }
    keccak_f1600(&mut lanes);
    for (lane, bytes) in lanes.iter().zip(bytes.as_chunks_mut::<8>().0) {
        bytes.copy_from_slice(&lane.to_le_bytes());
    }
    lanes.fill(0);
    std::hint::black_box(&mut lanes);
}
