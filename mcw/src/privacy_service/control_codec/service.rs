//! Binary operation payloads for the existing application bridge; no framing or
//! transport dependency. Rust/native callers use `reply` and `line` directly.

use super::{Error, MAX_INPUT, Scan};

pub const PARSE_REPLY: u16 = 0x0f00;
pub const PARSE_LINE: u16 = 0x0f01;
pub const MAX_RESPONSE: usize = MAX_INPUT + 4 * super::MAX_LINES + 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceError {
    UnsupportedOperation,
    InvalidRequest,
}
impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Tor control codec request")
    }
}
impl std::error::Error for ServiceError {}

/// Request: EOF:u8 (0/1), raw buffered bytes. Response: 0=need more, or
/// 1 + consumed:u32LE + status:i32LE + count:u32LE + (length:u32LE + ASCII bytes).
/// Parser rejection: 2 + category:u8 (category2 adds incomplete:u8; category4
/// adds three status octets).
/// Parser errors are structured normal responses, not arbitrary diagnostic text.
pub fn dispatch(operation: u16, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
    if !matches!(operation, PARSE_REPLY | PARSE_LINE) {
        return Err(ServiceError::UnsupportedOperation);
    }
    let Some((&eof, bytes)) = payload.split_first() else {
        return Err(ServiceError::InvalidRequest);
    };
    if eof > 1 || bytes.len() > MAX_INPUT {
        return Err(ServiceError::InvalidRequest);
    }
    let result = if operation == PARSE_REPLY {
        super::reply(bytes, eof == 1)
    } else {
        super::line(bytes, eof == 1).map(|scan| match scan {
            Scan::NeedMore => Scan::NeedMore,
            Scan::Complete { consumed, value } => Scan::Complete {
                consumed,
                value: super::Reply {
                    status: 0,
                    lines: vec![value],
                },
            },
        })
    };
    Ok(match result {
        Ok(Scan::NeedMore) => vec![0],
        Ok(Scan::Complete { consumed, value }) => {
            let mut result =
                Vec::with_capacity(13 + value.lines.iter().map(|s| s.len() + 4).sum::<usize>());
            result.push(1);
            result.extend_from_slice(&(consumed as u32).to_le_bytes());
            result.extend_from_slice(&value.status.to_le_bytes());
            result.extend_from_slice(&(value.lines.len() as u32).to_le_bytes());
            for line in value.lines {
                result.extend_from_slice(&(line.len() as u32).to_le_bytes());
                result.extend_from_slice(line.as_bytes());
            }
            result
        }
        Err(error) => {
            let mut result = vec![
                2,
                match error {
                    Error::NoMoreData => 0,
                    Error::IncompleteLine => 1,
                    Error::NoReplyLine { .. } => 2,
                    Error::MissingStatus => 3,
                    Error::InvalidStatus(_) => 4,
                    Error::Limit => 5,
                },
            ];
            if let Error::InvalidStatus(prefix) = error {
                result.extend_from_slice(&prefix);
            }
            if let Error::NoReplyLine { incomplete } = error {
                result.push(u8::from(incomplete));
            }
            result
        }
    })
}
