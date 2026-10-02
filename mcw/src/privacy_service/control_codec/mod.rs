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

fn raw_line(bytes: &[u8], eof: bool) -> Result<Scan<&[u8]>, Error> {
    let bounded = &bytes[..bytes.len().min(MAX_LINE + 2)];
    if let Some(end) = bounded.windows(2).position(|pair| pair == b"\r\n") {
        return Ok(Scan::Complete {
            consumed: end + 2,
            value: &bounded[..end],
        });
    }
    if bytes.len() >= MAX_LINE + 2 {
        return Err(Error::Limit);
    }
    if eof {
        return Err(if bytes.is_empty() {
            Error::NoMoreData
        } else {
            Error::IncompleteLine
        });
    }
    Ok(Scan::NeedMore)
}
fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| if byte.is_ascii() { byte as char } else { '?' })
        .collect()
}

pub fn line(bytes: &[u8], eof: bool) -> Result<Scan<String>, Error> {
    match raw_line(bytes, eof)? {
        Scan::NeedMore => Ok(Scan::NeedMore),
        Scan::Complete { consumed, value } => Ok(Scan::Complete {
            consumed,
            value: ascii(value),
        }),
    }
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

fn push(lines: &mut Vec<String>, line: String) -> Result<(), Error> {
    if lines.len() == MAX_LINES {
        return Err(Error::Limit);
    }
    lines.push(line);
    Ok(())
}

/// Stateless prefix scan. The owner stages incomplete raw bytes in a bounded
/// buffer; consumed bytes never include a coalesced subsequent reply.
pub fn reply(bytes: &[u8], eof: bool) -> Result<Scan<Reply>, Error> {
    let prefix = &bytes[..bytes.len().min(MAX_INPUT)];
    let prefix_eof = eof && bytes.len() <= MAX_INPUT;
    let (mut consumed, first) = match raw_line(prefix, prefix_eof) {
        Ok(Scan::NeedMore) => {
            return if prefix.len() == MAX_INPUT {
                Err(Error::Limit)
            } else {
                Ok(Scan::NeedMore)
            };
        }
        Ok(Scan::Complete { consumed, value }) => (consumed, ascii(value)),
        Err(Error::NoMoreData) => return Err(Error::NoReplyLine { incomplete: false }),
        Err(Error::IncompleteLine) => return Err(Error::NoReplyLine { incomplete: true }),
        Err(error) => return Err(error),
    };
    if first.len() < 3 {
        return Err(Error::MissingStatus);
    }
    let status_bytes = first.as_bytes()[..3].try_into().unwrap();
    let status = status(&status_bytes).ok_or(Error::InvalidStatus(status_bytes))?;
    let tail = &first[3..];
    let mut lines = Vec::new();
    let Some(&separator) = tail.as_bytes().first() else {
        lines.push(String::new());
        return Ok(Scan::Complete {
            consumed,
            value: Reply { status, lines },
        });
    };
    if !matches!(separator, b'+' | b'-') {
        lines.push(if separator == b' ' {
            tail[1..].to_owned()
        } else {
            tail.to_owned()
        });
        return Ok(Scan::Complete {
            consumed,
            value: Reply { status, lines },
        });
    }
    push(&mut lines, tail[1..].to_owned())?;
    loop {
        let current = match raw_line(&prefix[consumed..], prefix_eof)? {
            Scan::NeedMore => {
                return if prefix.len() == MAX_INPUT {
                    Err(Error::Limit)
                } else {
                    Ok(Scan::NeedMore)
                };
            }
            Scan::Complete {
                consumed: used,
                value,
            } => {
                consumed += used;
                ascii(value)
            }
        };
        if current.is_empty() {
            continue;
        }
        if separator == b'-' && current.len() > 3 && current.as_bytes()[3] == b' ' {
            push(&mut lines, current)?;
            return Ok(Scan::Complete {
                consumed,
                value: Reply { status, lines },
            });
        }
        let current = if separator != b'+' && current.len() > 3 {
            current[4..].to_owned()
        } else {
            current
        };
        let dot = separator == b'+' && current == ".";
        push(&mut lines, current)?;
        if dot {
            match raw_line(&prefix[consumed..], prefix_eof)? {
                Scan::NeedMore => {
                    return if prefix.len() == MAX_INPUT {
                        Err(Error::Limit)
                    } else {
                        Ok(Scan::NeedMore)
                    };
                }
                Scan::Complete {
                    consumed: used,
                    value,
                } => {
                    consumed += used;
                    push(&mut lines, ascii(value))?;
                }
            }
            return Ok(Scan::Complete {
                consumed,
                value: Reply { status, lines },
            });
        }
    }
}
