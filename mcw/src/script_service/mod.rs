//! First-party wallet Script/transaction services. This module's current concrete
//! API is signature hashing; execution/cryptographic validation is added only with
//! its actual first-party implementations, never a managed fallback.
#![forbid(unsafe_code)]

pub mod sighash;

pub use sighash::{
    Error as SighashError, SighashCache, TapScriptExtension, legacy_sighash, segwit_v0_sighash,
    tapbranch_hash, tapleaf_hash, taproot_sighash, taproot_sighash_message, taptweak_hash,
};
