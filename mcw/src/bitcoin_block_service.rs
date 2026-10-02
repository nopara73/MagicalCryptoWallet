//! Bounded application service for the existing block-cache header identity.
//!
//! Digests retain Bitcoin wire order. This operation does not validate proof of
//! work, transactions, witness commitments or any block/chain consensus rule.
#![forbid(unsafe_code)]

use crate::bitcoin_block::{BlockHeader, Error};

/// Exact 80-byte serialized header in; exact 32-byte SHA256d digest out.
pub const HASH_HEADER: u16 = 0x0e00;

pub fn hash_header(header: &[u8]) -> Result<[u8; 32], Error> {
    Ok(BlockHeader::decode(header)?.hash()?.0)
}
