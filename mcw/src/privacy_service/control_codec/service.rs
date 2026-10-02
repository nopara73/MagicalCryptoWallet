//! Binary operation payloads for the existing application bridge; no framing or
//! transport dependency. Rust/native callers use `reply` and `line` directly.

use super::{Error, MAX_INPUT, Scan, control, stream};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock, atomic::AtomicBool},
};

pub const PARSE_REPLY: u16 = 0x0f00;
pub const PARSE_LINE: u16 = 0x0f01;
pub const BEGIN: u16 = 0x0f02;
pub const FEED: u16 = 0x0f03;
pub const CLOSE: u16 = 0x0f04;
pub const MAX_CHUNK: usize = 16_384;
pub const MAX_READERS: usize = 16;
pub const MAX_RESPONSE: usize = MAX_INPUT + 4 * super::MAX_LINES + 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceError {
    UnsupportedOperation,
    InvalidRequest,
    ReaderQuota,
    Unavailable,
    Interrupted,
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
    dispatch_inner(operation, payload, None)
}

/// Host interruption never becomes a Tor parser-error packet. The host maps
/// `ServiceError::Interrupted` to its existing interrupted request behavior.
/// FEED cancellation removes that reader; CLOSE remains unconditional cleanup.
pub fn dispatch_control(
    operation: u16,
    payload: &[u8],
    interrupted: &AtomicBool,
) -> Result<Vec<u8>, ServiceError> {
    dispatch_inner(operation, payload, Some(interrupted))
}

fn dispatch_inner(
    operation: u16,
    payload: &[u8],
    interrupted: Option<&AtomicBool>,
) -> Result<Vec<u8>, ServiceError> {
    if matches!(operation, BEGIN | FEED | CLOSE) {
        return readers()
            .lock()
            .map_err(|_| ServiceError::Unavailable)?
            .dispatch_inner(operation, payload, interrupted);
    }
    if !matches!(operation, PARSE_REPLY | PARSE_LINE) {
        return Err(ServiceError::UnsupportedOperation);
    }
    let Some((&eof, bytes)) = payload.split_first() else {
        return Err(ServiceError::InvalidRequest);
    };
    if eof > 1 || bytes.len() > MAX_INPUT {
        return Err(ServiceError::InvalidRequest);
    }
    let kind = if operation == PARSE_REPLY {
        stream::Kind::Reply
    } else {
        stream::Kind::Line
    };
    let result = stream::Decoder::new(kind).feed_with_control(bytes, eof == 1, interrupted);
    encode(result, interrupted)
}

fn check(
    interrupted: Option<&AtomicBool>,
    point: control::Point,
    progress: usize,
) -> Result<(), ServiceError> {
    control::check(interrupted, point, progress).map_err(|_| ServiceError::Interrupted)
}

fn encode(
    result: Result<Scan<super::Reply>, Error>,
    interrupted: Option<&AtomicBool>,
) -> Result<Vec<u8>, ServiceError> {
    check(interrupted, control::Point::EncodeAllocate, 0)?;
    let encoded = match result {
        Ok(Scan::NeedMore) => vec![0],
        Ok(Scan::Complete { consumed, value }) => {
            let mut size = 13;
            for line in &value.lines {
                check(interrupted, control::Point::EncodeAllocate, size)?;
                size += line.len() + 4;
            }
            check(interrupted, control::Point::EncodeAllocate, size)?;
            let mut result = Vec::with_capacity(size);
            result.push(1);
            result.extend_from_slice(&(consumed as u32).to_le_bytes());
            result.extend_from_slice(&value.status.to_le_bytes());
            result.extend_from_slice(&(value.lines.len() as u32).to_le_bytes());
            for line in value.lines {
                check(interrupted, control::Point::EncodeLine, result.len())?;
                result.extend_from_slice(&(line.len() as u32).to_le_bytes());
                for bytes in line.as_bytes().chunks(control::CHECK_INTERVAL) {
                    check(interrupted, control::Point::EncodeBytes, result.len())?;
                    result.extend_from_slice(bytes);
                }
            }
            result
        }
        Err(Error::Interrupted) => return Err(ServiceError::Interrupted),
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
                    Error::Interrupted => unreachable!(),
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
    };
    check(interrupted, control::Point::EncodeFinished, encoded.len())?;
    Ok(encoded)
}

/// Per-read Rust state. IDs are chosen before the client's begin request, so a
/// canceled/late begin can always be closed. There is no replayed prefix payload.
pub struct Readers {
    active: BTreeMap<u64, stream::Decoder>,
}

impl Default for Readers {
    fn default() -> Self {
        Self::new()
    }
}

impl Readers {
    pub fn new() -> Self {
        Self {
            active: BTreeMap::new(),
        }
    }
    pub fn active_count(&self) -> usize {
        self.active.len()
    }
    pub fn clear(&mut self) {
        self.active.clear();
    }

    pub fn dispatch(&mut self, operation: u16, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
        self.dispatch_inner(operation, payload, None)
    }

    pub fn dispatch_control(
        &mut self,
        operation: u16,
        payload: &[u8],
        interrupted: &AtomicBool,
    ) -> Result<Vec<u8>, ServiceError> {
        self.dispatch_inner(operation, payload, Some(interrupted))
    }

    fn dispatch_inner(
        &mut self,
        operation: u16,
        payload: &[u8],
        interrupted: Option<&AtomicBool>,
    ) -> Result<Vec<u8>, ServiceError> {
        if !matches!(operation, BEGIN | FEED | CLOSE) {
            return Err(ServiceError::UnsupportedOperation);
        }
        if payload.len() < 8 {
            return Err(ServiceError::InvalidRequest);
        }
        let id = u64::from_le_bytes(payload[..8].try_into().unwrap());
        if id == 0 || id > i64::MAX as u64 {
            return Err(ServiceError::InvalidRequest);
        }
        match operation {
            BEGIN => {
                if payload.len() != 9 || payload[8] > 1 || self.active.contains_key(&id) {
                    return Err(ServiceError::InvalidRequest);
                }
                check(interrupted, control::Point::Entry, 0)?;
                if self.active.len() == MAX_READERS {
                    return Err(ServiceError::ReaderQuota);
                }
                let kind = if payload[8] == 0 {
                    stream::Kind::Reply
                } else {
                    stream::Kind::Line
                };
                check(interrupted, control::Point::BeginInsert, 0)?;
                self.active.insert(id, stream::Decoder::new(kind));
                let result = check(interrupted, control::Point::BeginCommitted, 0)
                    .and_then(|()| encode(Ok(Scan::NeedMore), interrupted));
                if matches!(result, Err(ServiceError::Interrupted)) {
                    self.active.remove(&id);
                }
                result
            }
            CLOSE => {
                if payload.len() != 8 {
                    return Err(ServiceError::InvalidRequest);
                }
                self.active.remove(&id);
                Ok(vec![0])
            }
            _ => {
                let result = self.feed(id, payload, interrupted);
                if matches!(result, Err(ServiceError::Interrupted)) {
                    self.active.remove(&id);
                }
                result
            }
        }
    }

    fn feed(
        &mut self,
        id: u64,
        payload: &[u8],
        interrupted: Option<&AtomicBool>,
    ) -> Result<Vec<u8>, ServiceError> {
        check(interrupted, control::Point::Entry, 0)?;
        if payload.len() < 9 || payload.len() > 9 + MAX_CHUNK || payload[8] > 1 {
            return Err(ServiceError::InvalidRequest);
        }
        let reader = self
            .active
            .get_mut(&id)
            .ok_or(ServiceError::InvalidRequest)?;
        let result = reader.feed_with_control(&payload[9..], payload[8] == 1, interrupted);
        if !matches!(result, Ok(Scan::NeedMore)) {
            self.active.remove(&id);
        }
        encode(result, interrupted)
    }
}

static READERS: OnceLock<Mutex<Readers>> = OnceLock::new();
fn readers() -> &'static Mutex<Readers> {
    READERS.get_or_init(|| Mutex::new(Readers::new()))
}
fn discard_readers() {
    readers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    readers().clear_poison();
}

/// The host owns this guard for one managed-child lifetime. Every exit/restart,
/// including pipe failure, drops all abandoned readers without another process.
pub struct ChildScope;
impl Default for ChildScope {
    fn default() -> Self {
        Self::new()
    }
}
impl ChildScope {
    pub fn new() -> Self {
        discard_readers();
        Self
    }
}
impl Drop for ChildScope {
    fn drop(&mut self) {
        discard_readers();
    }
}
