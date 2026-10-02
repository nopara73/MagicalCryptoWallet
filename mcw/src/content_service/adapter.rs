//! One bounded response-decode operation. No sockets, persistent body sessions,
//! compression fallback, runtime helper, or plaintext logging.
#![forbid(unsafe_code)]
use super::{Abort, Coding, ErrorKind, Limits};
use crate::compression::{self, Limit};
use std::time::{Duration, Instant};

pub const OPERATION: u16 = 0x0900;
pub const VERSION: u16 = 1;
pub const MAX_BODY: usize = 512 * 1024;
pub const MAX_FIELDS: usize = 4;
pub const MAX_FIELD: usize = 2048;
pub const MAX_TIMEOUT_MS: u16 = 5000;
pub const MAX_REPLY: usize = 12 + MAX_FIELDS * 21 + MAX_BODY;

/// Safe wire classifications. Neither messages nor Debug contain body/header data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Failure {
    MalformedRequest = 1,
    InvalidEncoding = 2,
    UnsupportedEncoding = 3,
    TooManyEncodings = 4,
    Cancelled = 5,
    Deadline = 6,
    InputLimit = 7,
    OutputLimit = 8,
    ExpansionLimit = 9,
    WorkLimit = 10,
    AllocationLimit = 11,
    Truncated = 12,
    Trailing = 13,
    Checksum = 14,
    MalformedContent = 15,
    DictionaryRejected = 16,
    MetaBlockLimit = 17,
    AllocationFailed = 18,
    GzipHeaderLimit = 19,
    GzipMembersLimit = 20,
    DictionaryLimit = 21,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationFailed;

fn classify_limit(limit: Limit) -> Failure {
    match limit {
        Limit::Input => Failure::InputLimit,
        Limit::Output => Failure::OutputLimit,
        Limit::Expansion => Failure::ExpansionLimit,
        Limit::Work => Failure::WorkLimit,
        Limit::Allocation => Failure::AllocationLimit,
        Limit::Dictionary => Failure::DictionaryLimit,
        Limit::GzipHeader => Failure::GzipHeaderLimit,
        Limit::GzipMembers => Failure::GzipMembersLimit,
    }
}
fn classify(kind: ErrorKind) -> Failure {
    match kind {
        ErrorKind::InvalidEncoding => Failure::InvalidEncoding,
        ErrorKind::UnsupportedEncoding => Failure::UnsupportedEncoding,
        ErrorKind::TooManyEncodings => Failure::TooManyEncodings,
        ErrorKind::Aborted(Abort::Cancelled) => Failure::Cancelled,
        ErrorKind::Aborted(Abort::DeadlineExceeded) => Failure::Deadline,
        ErrorKind::LimitExceeded(l) => classify_limit(l),
        ErrorKind::AllocationFailed => Failure::AllocationFailed,
        ErrorKind::Compression(c) => match c {
            compression::ErrorKind::LimitExceeded(l) => classify_limit(l),
            compression::ErrorKind::Truncated => Failure::Truncated,
            compression::ErrorKind::TrailingData => Failure::Trailing,
            compression::ErrorKind::ChecksumMismatch { .. }
            | compression::ErrorKind::GzipSizeMismatch { .. } => Failure::Checksum,
            compression::ErrorKind::DictionaryRequired { .. }
            | compression::ErrorKind::DictionaryMismatch { .. }
            | compression::ErrorKind::InvalidDictionaryPolicy => Failure::DictionaryRejected,
            compression::ErrorKind::AllocationFailed => Failure::AllocationFailed,
            _ => Failure::MalformedContent,
        },
        ErrorKind::Brotli(c) => match c {
            super::brotli::ErrorKind::LimitExceeded(l) => classify_limit(l),
            super::brotli::ErrorKind::Truncated => Failure::Truncated,
            super::brotli::ErrorKind::TrailingData => Failure::Trailing,
            super::brotli::ErrorKind::MetaBlockLimit => Failure::MetaBlockLimit,
            super::brotli::ErrorKind::Aborted(Abort::Cancelled) => Failure::Cancelled,
            super::brotli::ErrorKind::Aborted(Abort::DeadlineExceeded) => Failure::Deadline,
            super::brotli::ErrorKind::AllocationFailed => Failure::AllocationFailed,
            _ => Failure::MalformedContent,
        },
    }
}
fn failed(
    code: Failure,
    layer: usize,
    input: u64,
    output: u64,
) -> Result<Vec<u8>, AllocationFailed> {
    let mut p = Vec::new();
    p.try_reserve_exact(22).map_err(|_| AllocationFailed)?;
    p.extend_from_slice(&VERSION.to_le_bytes());
    p.push(1);
    p.extend_from_slice(&(code as u16).to_le_bytes());
    p.push(layer.min(255) as u8);
    p.extend_from_slice(&input.to_le_bytes());
    p.extend_from_slice(&output.to_le_bytes());
    Ok(p)
}

/// Request v1: version:u16, timeout_ms:u16, fields:u8, reserved:u8=0,
/// body_len:u32; each field is length:u16 + bytes; then exactly body_len bytes.
/// Success: version:u16,status:u8=0,encoded:u32,decoded:u32,layers:u8;
/// per layer coding:u8,input:u32,consumed:u32,output:u32,work:u64; then body.
/// Failure: version:u16,status:u8=1,code:u16,layer:u8,input:u64,output:u64.
/// Every packet fits the existing 1 MiB frame. Only <=512 KiB responses use it.
/// The host owns cancellation and connection lifetime: its checkpoint queries
/// the bounded inbox for the active (request ID, operation), closure and shutdown.
/// The execution deadline starts here, after acquisition and queueing. Allocation
/// limits account for logical codec storage, not the caller, allocator or RSS.
pub fn execute(
    payload: &[u8],
    native_check: &mut impl FnMut() -> Result<(), Abort>,
) -> Result<Vec<u8>, AllocationFailed> {
    let malformed = || failed(Failure::MalformedRequest, 0, 0, 0);
    if payload.len() < 10
        || u16::from_le_bytes([payload[0], payload[1]]) != VERSION
        || payload[5] != 0
    {
        return malformed();
    }
    let timeout = u16::from_le_bytes([payload[2], payload[3]]);
    let count = usize::from(payload[4]);
    let size = u32::from_le_bytes(payload[6..10].try_into().unwrap()) as usize;
    if timeout == 0 || timeout > MAX_TIMEOUT_MS || count > MAX_FIELDS || size > MAX_BODY {
        return malformed();
    }
    let mut fields = [&[][..]; MAX_FIELDS];
    let mut remaining = &payload[10..];
    for field in &mut fields[..count] {
        if remaining.len() < 2 {
            return malformed();
        }
        let n = u16::from_le_bytes([remaining[0], remaining[1]]) as usize;
        remaining = &remaining[2..];
        if n > MAX_FIELD || n > remaining.len() {
            return malformed();
        }
        *field = &remaining[..n];
        remaining = &remaining[n..];
    }
    if remaining.len() != size {
        return malformed();
    }
    let started = Instant::now();
    let mut check = || {
        native_check()?;
        if started.elapsed() >= Duration::from_millis(u64::from(timeout)) {
            Err(Abort::DeadlineExceeded)
        } else {
            Ok(())
        }
    };
    let limits = Limits {
        codec: compression::Limits {
            max_input_bytes: MAX_BODY as u64,
            max_output_bytes: MAX_BODY as u64,
            max_expansion_ratio: 200,
            expansion_slack_bytes: 256 * 1024,
            max_work: 16_000_000,
            max_allocation_bytes: 4 * 1024 * 1024,
            max_dictionary_bytes: 0,
            max_gzip_header_bytes: 16 * 1024,
            max_gzip_members: 16,
        },
        max_codings: MAX_FIELDS,
        max_brotli_meta_blocks: 256,
    };
    let decoded = match super::decode(remaining, &fields[..count], limits, &mut check) {
        Ok(v) => v,
        Err(e) => {
            return failed(
                classify(e.kind),
                e.layer,
                e.input_consumed,
                e.output_produced,
            );
        }
    };
    if let Err(reason) = check() {
        return failed(
            if reason == Abort::Cancelled {
                Failure::Cancelled
            } else {
                Failure::Deadline
            },
            0,
            decoded.encoded_len as u64,
            decoded.decoded_len as u64,
        );
    }
    let length = 12 + decoded.layers.len() * 21 + decoded.bytes.len();
    if length > MAX_REPLY {
        return failed(Failure::OutputLimit, 0, 0, 0);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(length)
        .map_err(|_| AllocationFailed)?;
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&(decoded.encoded_len as u32).to_le_bytes());
    out.extend_from_slice(&(decoded.decoded_len as u32).to_le_bytes());
    out.push(decoded.layers.len() as u8);
    for proof in &decoded.layers {
        out.push(match proof.coding {
            Coding::Identity => 0,
            Coding::Gzip => 1,
            Coding::Deflate => 2,
            Coding::Brotli => 3,
        });
        out.extend_from_slice(&(proof.input_len as u32).to_le_bytes());
        out.extend_from_slice(&(proof.consumed as u32).to_le_bytes());
        out.extend_from_slice(&(proof.output_len as u32).to_le_bytes());
        out.extend_from_slice(&proof.work.to_le_bytes());
    }
    out.extend_from_slice(&decoded.bytes);
    Ok(out)
}
