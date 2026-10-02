//! HTTP codecs independent of networking, managed types and OS handles; the
//! separately bounded payload adapter uses the existing host protocol.
//! All declared codings are decoded in reverse order; each layer must consume its
//! complete input. No plaintext result escapes before every layer validates.
#![forbid(unsafe_code)]
pub mod adapter;
pub mod brotli;
use crate::compression;
use std::fmt;

/// Only advertise codings for which this application owns a complete decoder.
pub const ACCEPT_ENCODING: &[u8] = b"gzip, deflate, br";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Abort {
    Cancelled,
    DeadlineExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
    Identity,
    Gzip,
    Deflate,
    Brotli,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub codec: compression::Limits,
    pub max_codings: usize,
    pub max_brotli_meta_blocks: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            codec: compression::Limits {
                max_allocation_bytes: 160 * 1024 * 1024,
                max_work: 256_000_000,
                ..compression::Limits::default()
            },
            max_codings: 8,
            max_brotli_meta_blocks: 4096,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidEncoding,
    UnsupportedEncoding,
    TooManyEncodings,
    Aborted(Abort),
    LimitExceeded(compression::Limit),
    Compression(compression::ErrorKind),
    Brotli(brotli::ErrorKind),
    AllocationFailed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// Zero-based decoding layer in reverse Content-Encoding order.
    pub layer: usize,
    pub input_consumed: u64,
    pub output_produced: u64,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "HTTP content decoding failed: {:?} (layer {}, {} input, {} output)",
            self.kind, self.layer, self.input_consumed, self.output_produced
        )
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerProof {
    pub coding: Coding,
    pub input_len: usize,
    pub consumed: usize,
    pub output_len: usize,
    pub work: u64,
}
pub struct DecodedBody {
    pub bytes: Vec<u8>,
    pub encoded_len: usize,
    pub decoded_len: usize,
    pub layers: Vec<LayerProof>,
}
impl fmt::Debug for DecodedBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecodedBody")
            .field("output_len", &self.bytes.len())
            .field("encoded_len", &self.encoded_len)
            .field("decoded_len", &self.decoded_len)
            .field("layers", &self.layers)
            .finish()
    }
}

fn error(kind: ErrorKind, layer: usize) -> Error {
    Error {
        kind,
        layer,
        input_consumed: 0,
        output_produced: 0,
    }
}
fn checkpoint(check: &mut impl FnMut() -> Result<(), Abort>, layer: usize) -> Result<(), Error> {
    check().map_err(|reason| error(ErrorKind::Aborted(reason), layer))
}

/// Parse ordered Content-Encoding field values (including comma lists), with
/// HTTP OWS and ASCII case folding. HTTP `deflate` is RFC1950 zlib, never a
/// heuristic raw-DEFLATE retry. Identity is explicit; unknown tokens fail closed.
pub fn parse_codings(values: &[&[u8]], max: usize) -> Result<Vec<Coding>, Error> {
    parse_codings_with_budget(values, max.min(32), usize::MAX)
}

fn parse_codings_with_budget(
    values: &[&[u8]],
    max: usize,
    budget: usize,
) -> Result<Vec<Coding>, Error> {
    let mut codings = Vec::new();
    for value in values {
        if value.len() > 2048 {
            return Err(error(ErrorKind::InvalidEncoding, 0));
        }
        for token in value.split(|&b| b == b',') {
            let start = token
                .iter()
                .position(|&b| b != b' ' && b != b'\t')
                .unwrap_or(token.len());
            let end = token
                .iter()
                .rposition(|&b| b != b' ' && b != b'\t')
                .map_or(start, |n| n + 1);
            let token = &token[start..end];
            if token.is_empty()
                || token.len() > 32
                || !token
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
            {
                return Err(error(ErrorKind::InvalidEncoding, 0));
            }
            let coding = if token.eq_ignore_ascii_case(b"identity") {
                Coding::Identity
            } else if token.eq_ignore_ascii_case(b"gzip") || token.eq_ignore_ascii_case(b"x-gzip") {
                Coding::Gzip
            } else if token.eq_ignore_ascii_case(b"deflate") {
                Coding::Deflate
            } else if token.eq_ignore_ascii_case(b"br") {
                Coding::Brotli
            } else {
                return Err(error(ErrorKind::UnsupportedEncoding, 0));
            };
            if codings.len() >= max {
                return Err(error(ErrorKind::TooManyEncodings, 0));
            }
            if (codings.len() + 1)
                * (std::mem::size_of::<Coding>() + std::mem::size_of::<LayerProof>())
                > budget
            {
                return Err(error(
                    ErrorKind::LimitExceeded(compression::Limit::Allocation),
                    0,
                ));
            }
            codings
                .try_reserve_exact(1)
                .map_err(|_| error(ErrorKind::AllocationFailed, 0))?;
            codings.push(coding);
        }
    }
    Ok(codings)
}

/// Stable application service boundary: transport owns encoded Content-Length,
/// de-framing, response headers, TLS, socket state and its cancellation/deadline.
/// This function owns decoding, exact per-layer proof, and independent aggregate
/// output/work/allocation limits. Buffers remain private on every failure.
pub fn decode(
    encoded_body: &[u8],
    content_encodings: &[&[u8]],
    limits: Limits,
    check: &mut impl FnMut() -> Result<(), Abort>,
) -> Result<DecodedBody, Error> {
    checkpoint(check, 0)?;
    if encoded_body.len() as u64 > limits.codec.max_input_bytes {
        return Err(error(
            ErrorKind::LimitExceeded(compression::Limit::Input),
            0,
        ));
    }
    // Header/control metadata has an independent hard ceiling even if the
    // caller supplies a much larger configured coding count.
    let codings = parse_codings_with_budget(
        content_encodings,
        limits.max_codings.min(32),
        limits.codec.max_allocation_bytes,
    )?;
    let metadata = codings.capacity() * std::mem::size_of::<Coding>()
        + codings.len() * std::mem::size_of::<LayerProof>();
    if metadata > limits.codec.max_allocation_bytes {
        return Err(error(
            ErrorKind::LimitExceeded(compression::Limit::Allocation),
            0,
        ));
    }
    let mut layers = Vec::new();
    layers
        .try_reserve_exact(codings.len())
        .map_err(|_| error(ErrorKind::AllocationFailed, 0))?;
    let mut current: Option<Vec<u8>> = None;
    let mut work = content_encodings
        .iter()
        .try_fold(0u64, |n, v| n.checked_add(v.len() as u64))
        .filter(|&n| n <= limits.codec.max_work)
        .ok_or_else(|| error(ErrorKind::LimitExceeded(compression::Limit::Work), 0))?;
    let original_bound = (encoded_body.len() as u64)
        .saturating_mul(limits.codec.max_expansion_ratio)
        .saturating_add(limits.codec.expansion_slack_bytes);
    for (layer, &coding) in codings.iter().rev().enumerate() {
        checkpoint(check, layer)?;
        let data = current.as_deref().unwrap_or(encoded_body);
        let mut codec_limits = limits.codec;
        codec_limits.max_input_bytes = limits.codec.max_output_bytes.max(encoded_body.len() as u64);
        codec_limits.max_output_bytes = limits.codec.max_output_bytes.min(original_bound);
        codec_limits.max_work = limits.codec.max_work.saturating_sub(work);
        let owned = current.as_ref().map_or(0, |v| v.capacity());
        codec_limits.max_allocation_bytes = limits
            .codec
            .max_allocation_bytes
            .saturating_sub(owned)
            .saturating_sub(metadata);
        let (next, consumed, spent) = match coding {
            Coding::Identity => {
                if data.len() as u64 > codec_limits.max_output_bytes {
                    return Err(error(
                        ErrorKind::LimitExceeded(compression::Limit::Output),
                        layer,
                    ));
                }
                layers.push(LayerProof {
                    coding,
                    input_len: data.len(),
                    consumed: data.len(),
                    output_len: data.len(),
                    work: 0,
                });
                continue;
            }
            Coding::Gzip | Coding::Deflate => {
                decode_deflate(data, coding, codec_limits, check, layer)?
            }
            Coding::Brotli => {
                let decoded = brotli::decode(
                    data,
                    codec_limits,
                    limits.max_brotli_meta_blocks,
                    compression::TrailingData::Reject,
                    check,
                )
                .map_err(|e| Error {
                    kind: match e.kind {
                        brotli::ErrorKind::Aborted(reason) => ErrorKind::Aborted(reason),
                        kind => ErrorKind::Brotli(kind),
                    },
                    layer,
                    input_consumed: e.input_consumed,
                    output_produced: e.output_produced,
                })?;
                (decoded.bytes, decoded.consumed, decoded.work)
            }
        };
        if consumed != data.len() {
            return Err(error(
                ErrorKind::Compression(compression::ErrorKind::TrailingData),
                layer,
            ));
        }
        work = work
            .checked_add(spent)
            .filter(|&n| n <= limits.codec.max_work)
            .ok_or_else(|| error(ErrorKind::LimitExceeded(compression::Limit::Work), layer))?;
        layers.push(LayerProof {
            coding,
            input_len: data.len(),
            consumed,
            output_len: next.len(),
            work: spent,
        });
        current = Some(next);
        checkpoint(check, layer)?;
    }
    let bytes = if let Some(bytes) = current {
        bytes
    } else {
        if encoded_body.len() as u64 > limits.codec.max_output_bytes {
            return Err(error(
                ErrorKind::LimitExceeded(compression::Limit::Output),
                0,
            ));
        }
        if encoded_body.len() as u64 > original_bound {
            return Err(error(
                ErrorKind::LimitExceeded(compression::Limit::Expansion),
                0,
            ));
        }
        if encoded_body.len() > limits.codec.max_allocation_bytes.saturating_sub(metadata) {
            return Err(error(
                ErrorKind::LimitExceeded(compression::Limit::Allocation),
                0,
            ));
        }
        let mut result = Vec::new();
        result
            .try_reserve_exact(encoded_body.len())
            .map_err(|_| error(ErrorKind::AllocationFailed, 0))?;
        for part in encoded_body.chunks(4096) {
            checkpoint(check, 0)?;
            work = work
                .checked_add(part.len() as u64)
                .filter(|&n| n <= limits.codec.max_work)
                .ok_or_else(|| error(ErrorKind::LimitExceeded(compression::Limit::Work), 0))?;
            result.extend_from_slice(part);
        }
        result
    };
    checkpoint(check, 0)?;
    Ok(DecodedBody {
        decoded_len: bytes.len(),
        bytes,
        encoded_len: encoded_body.len(),
        layers,
    })
}

fn decode_deflate(
    data: &[u8],
    coding: Coding,
    limits: compression::Limits,
    check: &mut impl FnMut() -> Result<(), Abort>,
    layer: usize,
) -> Result<(Vec<u8>, usize, u64), Error> {
    let mut options = compression::DecodeOptions::new(if coding == Coding::Gzip {
        compression::Format::Gzip
    } else {
        compression::Format::Zlib
    });
    options.limits = limits;
    let map = |e: compression::Error| Error {
        kind: ErrorKind::Compression(e.kind),
        layer,
        input_consumed: e.input_consumed,
        output_produced: e.output_produced,
    };
    let mut decoder = compression::Decoder::new(options).map_err(map)?;
    let budget = limits
        .max_allocation_bytes
        .saturating_sub(decoder.allocated_bytes())
        .saturating_sub(8192);
    let mut bytes = Vec::new();
    let mut pos = 0;
    let mut fed = data.len().min(4096);
    let mut storage = [0; 8192];
    let mut copies = 0u64;
    loop {
        checkpoint(check, layer)?;
        let available = budget.saturating_sub(bytes.len()).min(storage.len());
        let progress = decoder
            .process(
                &data[pos..fed],
                &mut storage[..available],
                fed == data.len(),
            )
            .map_err(map)?;
        pos += progress.consumed;
        let target = bytes.len() + progress.written;
        if target > bytes.capacity() {
            copies = copies.saturating_add(bytes.len() as u64);
            if copies.saturating_add(decoder.total_work()) > limits.max_work {
                return Err(error(
                    ErrorKind::LimitExceeded(compression::Limit::Work),
                    layer,
                ));
            }
            let capacity = target.max(bytes.capacity().saturating_mul(2)).min(budget);
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|_| error(ErrorKind::AllocationFailed, layer))?;
            if bytes.capacity() > budget {
                return Err(error(
                    ErrorKind::LimitExceeded(compression::Limit::Allocation),
                    layer,
                ));
            }
        }
        bytes.extend_from_slice(&storage[..progress.written]);
        if copies.saturating_add(decoder.total_work()) > limits.max_work {
            return Err(error(
                ErrorKind::LimitExceeded(compression::Limit::Work),
                layer,
            ));
        }
        match progress.status {
            compression::Status::Finished => {
                return Ok((bytes, pos, copies + decoder.total_work()));
            }
            compression::Status::NeedOutput if available == 0 => {
                return Err(error(
                    ErrorKind::LimitExceeded(compression::Limit::Allocation),
                    layer,
                ));
            }
            compression::Status::NeedOutput => {}
            compression::Status::NeedInput => {
                if fed == data.len() {
                    return Err(error(
                        ErrorKind::Compression(compression::ErrorKind::Truncated),
                        layer,
                    ));
                }
                fed = (fed + 4096).min(data.len());
            }
        }
    }
}
