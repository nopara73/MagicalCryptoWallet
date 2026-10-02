//! The retained application's Tor control reply and CRLF-line codec.
//! Safe Rust/std only; no daemon, control connection, DNS, wallet state or TLS.
//!
//! `Reply.lines` deliberately preserves the existing application representation:
//! the first/continuation prefixes are stripped, multiline terminal lines and
//! data-block dot lines are retained, blank continuation lines are skipped, and
//! backslashes/dot stuffing are not interpreted. High bytes map to ASCII '?',
//! matching the retired reader. This is a compatibility codec, not a validator
//! of the semantic correctness or authentication of Tor replies.
#![forbid(unsafe_code)]

pub mod service;
pub mod stream;

use std::fmt;

pub const MAX_INPUT: usize = 524_288;
pub const MAX_LINE: usize = 65_536;
pub const MAX_LINES: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NoMoreData,
    IncompleteLine,
    NoReplyLine { incomplete: bool },
    MissingStatus,
    InvalidStatus([u8; 3]),
    Limit,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoMoreData => "No more data.",
            Self::IncompleteLine => "Incomplete message.",
            Self::NoReplyLine { .. } => "No reply line was received.",
            Self::MissingStatus => "Status code requires at least 3 characters.",
            Self::InvalidStatus(_) => "Unknown Tor control status code.",
            Self::Limit => "Tor control parsing limit exceeded.",
        })
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Eq, PartialEq)]
pub struct Reply {
    pub status: i32,
    pub lines: Vec<String>,
}
impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TorControlReply")
            .field("status", &self.status)
            .field("lines", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum Scan<T> {
    NeedMore,
    Complete { consumed: usize, value: T },
}
impl<T> fmt::Debug for Scan<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedMore => f.write_str("NeedMore"),
            Self::Complete { consumed, .. } => f
                .debug_struct("Complete")
                .field("consumed", consumed)
                .field("value", &"[redacted]")
                .finish(),
        }
    }
}

fn ascii(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for &byte in bytes {
        text.push(if byte.is_ascii() { byte as char } else { '?' });
    }
    text
}

pub fn line(bytes: &[u8], eof: bool) -> Result<Scan<String>, Error> {
    stream::Decoder::new(stream::Kind::Line)
        .feed(bytes, eof)
        .map(|scan| match scan {
            Scan::NeedMore => Scan::NeedMore,
            Scan::Complete {
                consumed,
                mut value,
            } => Scan::Complete {
                consumed,
                value: value.lines.remove(0),
            },
        })
}

/// Mirrors .NET Integer status parsing on the first three ASCII octets, including
/// its historically accepted sign/ASCII whitespace/trailing NULs. Unknown enum
/// values survive. The independent .NET fixture includes these unusual inputs.
fn status(bytes: &[u8; 3]) -> Option<i32> {
    fn space(byte: u8) -> bool {
        matches!(byte, b'\t'..=b'\r' | b' ')
    }
    let mut text = bytes.as_slice();
    while text.last() == Some(&0) {
        text = &text[..text.len() - 1];
    }
    while text.first().is_some_and(|&b| space(b)) {
        text = &text[1..];
    }
    while text.last().is_some_and(|&b| space(b)) {
        text = &text[..text.len() - 1];
    }
    let sign = match text.first() {
        Some(b'-') => {
            text = &text[1..];
            -1
        }
        Some(b'+') => {
            text = &text[1..];
            1
        }
        _ => 1,
    };
    if text.is_empty() || text.iter().any(|b| !b.is_ascii_digit()) {
        return None;
    }
    Some(sign * text.iter().fold(0, |n, &b| n * 10 + i32::from(b - b'0')))
}

/// Single-shot convenience over the same incremental grammar. Managed streams
/// use a retained Decoder and feed only new chunks through the bounded service.
pub fn reply(bytes: &[u8], eof: bool) -> Result<Scan<Reply>, Error> {
    stream::Decoder::new(stream::Kind::Reply).feed(bytes, eof)
}
