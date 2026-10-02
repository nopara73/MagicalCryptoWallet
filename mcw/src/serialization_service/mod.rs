//! Application JSON operations over the host's existing bounded binary bridge.
//! Buffers belong to one managed-child lifetime; this service never opens files.
#![forbid(unsafe_code)]

mod rpc;
pub mod tokens;

use crate::json::{self, Number, Value};
use std::{
    collections::BTreeMap,
    fmt,
    time::{Duration, Instant},
};

pub const OPEN: u16 = 0x0100;
pub const APPEND: u16 = 0x0101;
pub const FINISH: u16 = 0x0102;
pub const READ: u16 = 0x0103;
pub const CLOSE: u16 = 0x0104;
pub const NUMERIC: u16 = 0x0105;
pub const MAX_JSON: usize = 8 * 1024 * 1024;
pub const MAX_TRANSFER: usize = 16 * 1024 * 1024;
pub const MAX_CHUNK: usize = 512 * 1024;
pub const MAX_READ: usize = 256 * 1024;
pub const MAX_SESSIONS: usize = 8;
pub const MAX_RETAINED: usize = 64 * 1024 * 1024;
const EXPIRY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Action {
    ParseRpc = 0,
    WriteRpcResponse = 1,
    WriteRpcBatch = 2,
}
impl Action {
    fn read(byte: u8) -> Result<Self, ServiceError> {
        match byte {
            0 => Ok(Self::ParseRpc),
            1 => Ok(Self::WriteRpcResponse),
            2 => Ok(Self::WriteRpcBatch),
            _ => Err(ServiceError::protocol("unknown RPC JSON action")),
        }
    }
    fn max_input(self) -> usize {
        if self == Self::ParseRpc {
            MAX_JSON
        } else {
            MAX_TRANSFER
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    pub code: u16,
    pub message: String,
}
impl ServiceError {
    pub fn protocol(message: &str) -> Self {
        Self {
            code: 10,
            message: message.into(),
        }
    }
    pub fn limit() -> Self {
        Self {
            code: 12,
            message: "JSON service resource limit exceeded".into(),
        }
    }
    fn json(error: impl fmt::Display) -> Self {
        Self {
            code: 11,
            message: error.to_string(),
        }
    }
}
impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ServiceError {}

struct Session {
    action: Action,
    expected: usize,
    input: Vec<u8>,
    output: Option<Vec<u8>>,
    read: usize,
    touched: Instant,
}
impl Session {
    fn retained(&self) -> usize {
        self.output.as_ref().map_or(self.expected, Vec::len)
    }
}

#[derive(Default)]
pub struct SerializationService {
    next_id: u64,
    sessions: BTreeMap<u64, Session>,
}
impl SerializationService {
    pub fn supports(operation: u16) -> bool {
        (OPEN..=NUMERIC).contains(&operation)
    }
    pub fn handle(&mut self, operation: u16, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
        self.expire(Instant::now());
        match operation {
            OPEN => self.open(payload),
            APPEND => self.append(payload),
            FINISH => self.finish(payload),
            READ => self.read(payload),
            CLOSE => self.close(payload),
            NUMERIC => numeric(payload),
            _ => Err(ServiceError::protocol("unknown JSON operation")),
        }
    }
    fn retained(&self) -> usize {
        self.sessions.values().map(Session::retained).sum()
    }
    fn expire(&mut self, now: Instant) {
        self.sessions
            .retain(|_, session| now.duration_since(session.touched) < EXPIRY);
    }
    fn open(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
        if payload.len() != 5 {
            return Err(ServiceError::protocol("invalid JSON open payload"));
        }
        let action = Action::read(payload[0])?;
        let expected = u32_at(payload, 1)? as usize;
        if expected > action.max_input()
            || self.sessions.len() >= MAX_SESSIONS
            || self.retained().saturating_add(expected) > MAX_RETAINED
        {
            return Err(ServiceError::limit());
        }
        let id = self
            .next_id
            .checked_add(1)
            .ok_or_else(ServiceError::limit)?;
        let mut input = Vec::new();
        input
            .try_reserve_exact(expected)
            .map_err(|_| ServiceError::limit())?;
        self.next_id = id;
        self.sessions.insert(
            id,
            Session {
                action,
                expected,
                input,
                output: None,
                read: 0,
                touched: Instant::now(),
            },
        );
        Ok(id.to_le_bytes().to_vec())
    }
    fn append(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
        if payload.len() < 12 || payload.len() - 12 > MAX_CHUNK {
            return Err(ServiceError::protocol("invalid JSON append payload"));
        }
        let id = u64_at(payload, 0)?;
        let offset = u32_at(payload, 8)? as usize;
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| ServiceError::protocol("unknown JSON transfer"))?;
        let bytes = &payload[12..];
        if session.output.is_some()
            || offset != session.input.len()
            || bytes.len() > session.expected - session.input.len()
        {
            return Err(ServiceError::protocol("invalid JSON transfer offset"));
        }
        session.input.extend_from_slice(bytes);
        session.touched = Instant::now();
        Ok((session.input.len() as u32).to_le_bytes().to_vec())
    }
    fn finish(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
        if payload.len() != 8 {
            return Err(ServiceError::protocol("invalid JSON finish payload"));
        }
        let id = u64_at(payload, 0)?;
        // Remove before transformation: every failure drops the complete input.
        let mut session = self
            .sessions
            .remove(&id)
            .ok_or_else(|| ServiceError::protocol("unknown JSON transfer"))?;
        if session.output.is_some() || session.input.len() != session.expected {
            return Err(ServiceError::protocol("incomplete JSON transfer"));
        }
        let output = transform(session.action, &session.input)?;
        if output.len() > MAX_TRANSFER
            || self.retained().saturating_add(output.len()) > MAX_RETAINED
        {
            return Err(ServiceError::limit());
        }
        let length = output.len();
        session.input = Vec::new();
        session.output = Some(output);
        session.touched = Instant::now();
        // Zero-length responses need no read and must not leak their session.
        if length != 0 {
            self.sessions.insert(id, session);
        }
        Ok((length as u32).to_le_bytes().to_vec())
    }
    fn read(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
        if payload.len() != 16 {
            return Err(ServiceError::protocol("invalid JSON read payload"));
        }
        let id = u64_at(payload, 0)?;
        let offset = u32_at(payload, 8)? as usize;
        let maximum = u32_at(payload, 12)? as usize;
        if maximum == 0 || maximum > MAX_READ {
            return Err(ServiceError::protocol("invalid JSON read size"));
        }
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| ServiceError::protocol("unknown JSON transfer"))?;
        let output = session
            .output
            .as_ref()
            .ok_or_else(|| ServiceError::protocol("JSON transfer is unfinished"))?;
        if offset != session.read {
            return Err(ServiceError::protocol("invalid JSON read offset"));
        }
        let end = offset.saturating_add(maximum).min(output.len());
        let chunk = output[offset..end].to_vec();
        session.read = end;
        session.touched = Instant::now();
        if end == output.len() {
            self.sessions.remove(&id);
        }
        Ok(chunk)
    }
    fn close(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
        if payload.len() != 8 {
            return Err(ServiceError::protocol("invalid JSON close payload"));
        }
        self.sessions.remove(&u64_at(payload, 0)?);
        Ok(Vec::new())
    }
}

pub fn transform(action: Action, input: &[u8]) -> Result<Vec<u8>, ServiceError> {
    if input.len() > action.max_input() {
        return Err(ServiceError::limit());
    }
    match action {
        Action::ParseRpc => tokens::encode_rpc(&rpc::parse_requests(input)?),
        Action::WriteRpcResponse => rpc::write_response(&tokens::decode(input)?),
        Action::WriteRpcBatch => rpc::write_batch(&tokens::decode(input)?),
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, ServiceError> {
    let part = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| ServiceError::protocol("truncated JSON service payload"))?;
    Ok(u32::from_le_bytes(part.try_into().unwrap()))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, ServiceError> {
    let part = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| ServiceError::protocol("truncated JSON service payload"))?;
    Ok(u64::from_le_bytes(part.try_into().unwrap()))
}

// Primitive conversions used only by the retained RPC parameter contract.
// Numeric work stays in Rust; managed code maps these bounded results to domain types.
// Success is 0 + 16 bytes, failure is 1. Modes: integer=0, decimal96=1,
// boolean=2. Every conversion preserves the old RPC coercion policy.
fn numeric(payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
    let (&mode, token) = payload
        .split_first()
        .ok_or_else(|| ServiceError::protocol("missing numeric operation"))?;
    if payload.len() > 16 * 1024 {
        return Err(ServiceError::limit());
    }
    let value = tokens::decode(token)?;
    let result = match mode {
        0 => integer(&value).map(i128::to_le_bytes),
        1 => decimal(&value),
        2 => boolean(&value).map(|value| i128::from(value).to_le_bytes()),
        _ => return Err(ServiceError::protocol("unknown numeric conversion")),
    };
    Ok(match result {
        Some(bytes) => {
            let mut output = vec![0];
            output.extend_from_slice(&bytes);
            output
        }
        None => vec![1],
    })
}
fn integer(value: &Value) -> Option<i128> {
    match value {
        Value::Number(number) if number.as_str().contains(['.', 'e', 'E']) => {
            let value = number.as_str().parse::<f64>().ok()?.round_ties_even();
            // A float is coerced exactly as the old RPC JToken reader was; never
            // route integer tokens through floating point (large IDs/amounts).
            if value.is_finite() && value >= i128::MIN as f64 && value < -(i128::MIN as f64) {
                Some(value as i128)
            } else {
                None
            }
        }
        Value::Number(number) => number.to_i128_exact().ok(),
        Value::String(_) => {
            json::compat::integer_i128(value, json::compat::IntegerInput::NumberOrDecimalString)
                .ok()
        }
        _ => None,
    }
}
fn boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(number) => Some(number.as_str().parse::<f64>().ok()? != 0.0),
        Value::String(text) => match text.as_str().trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}
fn decimal(value: &Value) -> Option<[u8; 16]> {
    let token = match value {
        Value::Number(number) if number.as_str().contains(['.', 'e', 'E']) => {
            let float = number.as_str().parse::<f64>().ok()?;
            if !float.is_finite() {
                return None;
            }
            // Convert.ToDecimal(Double), the retained JToken behavior, rounds to
            // fifteen significant decimal digits. Strings stay exact decimal.
            format!("{float:.14e}")
        }
        Value::Number(number) => number.as_str().to_owned(),
        Value::String(text) => {
            let text = text.as_str().trim();
            if text.is_empty() {
                return None;
            }
            text.strip_prefix('+').unwrap_or(text).to_owned()
        }
        _ => return None,
    };
    let number = Number::parse(&token).ok()?;
    let lexical_scale = token
        .split(['e', 'E'])
        .next()?
        .split('.')
        .nth(1)
        .map_or(0, str::len)
        .min(28) as u32;
    let preferred = if matches!(value, Value::String(text) if !text.as_str().contains(['e','E'])) {
        lexical_scale
    } else {
        0
    };
    decimal_bits(&number, preferred)
}
fn decimal_bits(number: &Number, preferred: u32) -> Option<[u8; 16]> {
    const MAX: u128 = (1u128 << 96) - 1;
    for scale in preferred..=28 {
        if let Ok(integer) = number.to_scaled_i128(scale) {
            let magnitude = integer.unsigned_abs();
            if magnitude > MAX {
                return None;
            }
            let mut bits = [0; 16];
            bits[..12].copy_from_slice(&magnitude.to_le_bytes()[..12]);
            let flags = (scale << 16) | if integer < 0 { 1 << 31 } else { 0 };
            bits[12..].copy_from_slice(&flags.to_le_bytes());
            return Some(bits);
        }
    }
    None
}
