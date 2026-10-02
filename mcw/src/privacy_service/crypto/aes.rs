//! CTR mode over the wallet crypto owner's AES block implementation.
//! No independent AES block/round/key-schedule implementation lives here.

use super::super::Error;

/// A block primitive binding is supplied by the wallet-crypto integration.
/// This trait is not an entropy or trust policy and does not expose a native handle.
pub trait BlockCipher {
    fn encrypt(&self, block: [u8; 16]) -> [u8; 16];
}

pub struct AesCtr<C: BlockCipher> {
    cipher: C,
    next: u128,
    block: [u8; 16],
    used: usize,
    exhausted: bool,
}
impl<C: BlockCipher> AesCtr<C> {
    pub fn new(cipher: C, counter: [u8; 16]) -> Self {
        Self {
            cipher,
            next: u128::from_be_bytes(counter),
            block: [0; 16],
            used: 16,
            exhausted: false,
        }
    }
    /// Length/counter rejection is atomic; existing buffered final-block bytes
    /// remain usable after the last 128-bit counter has been encrypted.
    pub fn apply(&mut self, bytes: &mut [u8]) -> Result<(), Error> {
        let available = 16 - self.used;
        let blocks = bytes.len().saturating_sub(available).div_ceil(16);
        if blocks != 0 && (self.exhausted || self.next.checked_add((blocks - 1) as u128).is_none())
        {
            return Err(Error::CipherExhausted);
        }
        for byte in bytes {
            if self.used == 16 {
                self.block = self.cipher.encrypt(self.next.to_be_bytes());
                match self.next.checked_add(1) {
                    Some(next) => self.next = next,
                    None => self.exhausted = true,
                }
                self.used = 0;
            }
            *byte ^= self.block[self.used];
            self.used += 1;
        }
        Ok(())
    }
}
impl<C: BlockCipher> Drop for AesCtr<C> {
    fn drop(&mut self) {
        super::clear(&mut self.block);
        self.next = 0;
        std::hint::black_box(&mut self.next);
    }
}
