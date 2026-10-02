//! Bounded NIP-01 event-ID serialization and SHA-256 for the retained update caller.
//! Relays, subscriptions, NIP-19 and signature verification stay with their owners.
#![forbid(unsafe_code)]

use crate::{bitcoin_encoding, json};
use std::fmt;

pub const OPERATION: u16 = 0x0c00;
pub const PAYLOAD_VERSION: u8 = 1;
/// The existing application bridge's one-MiB frame minus its sixteen-byte header.
pub const MAX_REQUEST_BYTES: usize = 1_048_560;
pub const MAX_CANONICAL_BYTES: usize = MAX_REQUEST_BYTES * 6 + 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    TooLarge,
    Truncated,
    UnsupportedVersion,
    InvalidUtf8,
    InvalidCount,
    TrailingData,
    Encoding,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "Nostr event exceeds the application request limit",
            Self::Truncated => "truncated Nostr event request",
            Self::UnsupportedVersion => "unsupported Nostr event request version",
            Self::InvalidUtf8 => "Nostr event request contains invalid UTF-8",
            Self::InvalidCount => "Nostr event request has an invalid element count",
            Self::TrailingData => "trailing Nostr event request bytes",
            Self::Encoding => "could not encode Nostr event ID",
        })
    }
}

impl std::error::Error for Error {}

struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<&'a [u8; N], Error> {
        let (head, tail) = self.remaining.split_at_checked(N).ok_or(Error::Truncated)?;
        self.remaining = tail;
        head.try_into().map_err(|_| Error::Truncated)
    }

    fn number(&mut self) -> Result<usize, Error> {
        usize::try_from(u32::from_le_bytes(*self.take()?)).map_err(|_| Error::InvalidCount)
    }

    fn count(&mut self) -> Result<usize, Error> {
        let count = self.number()?;
        // Every following tag or string requires at least one four-byte length.
        if count > self.remaining.len() / 4 {
            return Err(Error::InvalidCount);
        }
        Ok(count)
    }

    fn text(&mut self) -> Result<json::Value, Error> {
        let length = self.number()?;
        let (head, tail) = self
            .remaining
            .split_at_checked(length)
            .ok_or(Error::Truncated)?;
        let text = std::str::from_utf8(head).map_err(|_| Error::InvalidUtf8)?;
        self.remaining = tail;
        Ok(json::Value::string(text))
    }
}

/// The typed request is version:u8, pubkey:32 bytes, timestamp:i64 LE,
/// kind:i32 LE, tag_count:u32 LE, then each tag's string_count:u32 LE and
/// each string's byte_length:u32 LE + UTF-8 bytes, followed by the content string.
/// The managed adapter supplies the retained caller's nullable-field defaults.
pub fn canonical_preimage(payload: &[u8]) -> Result<String, Error> {
    if payload.len() > MAX_REQUEST_BYTES {
        return Err(Error::TooLarge);
    }
    let mut reader = Reader { remaining: payload };
    if reader.take::<1>()?[0] != PAYLOAD_VERSION {
        return Err(Error::UnsupportedVersion);
    }
    let public_key =
        bitcoin_encoding::hex_encode(reader.take::<32>()?).map_err(|_| Error::Encoding)?;
    let timestamp = i64::from_le_bytes(*reader.take()?);
    let kind = i32::from_le_bytes(*reader.take()?);
    let count = reader.count()?;
    let mut tags = Vec::with_capacity(count);
    for _ in 0..count {
        let count = reader.count()?;
        let mut tag = Vec::with_capacity(count);
        for _ in 0..count {
            tag.push(reader.text()?);
        }
        tags.push(json::Value::Array(tag));
    }
    let content = reader.text()?;
    if !reader.remaining.is_empty() {
        return Err(Error::TrailingData);
    }
    let value = json::Value::Array(vec![
        json::Value::Number(0_u64.into()),
        json::Value::string(public_key),
        json::Value::Number(timestamp.into()),
        json::Value::Number(i64::from(kind).into()),
        json::Value::Array(tags),
        content,
    ]);
    let options = json::SerializeOptions {
        strings: json::StringEncoding::Minimal,
        layout: json::Layout::Compact,
        limits: json::Limits {
            input_bytes: MAX_REQUEST_BYTES,
            output_bytes: MAX_CANONICAL_BYTES,
            depth: 3,
            nodes: MAX_REQUEST_BYTES / 4 + 8,
            container_entries: MAX_REQUEST_BYTES / 4,
            string_bytes: MAX_REQUEST_BYTES,
            total_decoded_bytes: MAX_REQUEST_BYTES + 64,
            number_bytes: 32,
        },
        ..json::SerializeOptions::default()
    };
    json::serialize(&value, &options).map_err(|_| Error::Encoding)
}

pub fn digest(payload: &[u8]) -> Result<[u8; 32], Error> {
    let preimage = canonical_preimage(payload)?;
    bitcoin_encoding::sha256(preimage.as_bytes()).map_err(|_| Error::Encoding)
}
