//! Tor-specific primitives. SHA-256/HMAC reuse the wallet hash owner.
//! Clearing in safe Rust is best effort; compiler/CPU timing needs review.

pub mod aes;
pub mod hash;

pub use aes::AesCtr;
pub use hash::{Sha3_256, sha3_256, shake256};

pub(crate) fn clear(bytes: &mut [u8]) {
    bytes.fill(0);
    std::hint::black_box(bytes);
}
