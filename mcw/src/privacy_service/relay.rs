//! Relay message envelopes and authenticated circuit/stream flow control.
//! Envelopes must only be acted on after running-digest verification.

use super::{Error, cell::BODY_LEN};
use crate::wallet_hashes::constant_time_eq;
use std::{collections::VecDeque, fmt};

pub const MAX_DATA: usize = BODY_LEN - 11;

#[derive(Clone, Eq, PartialEq)]
pub struct Message<'a> {
    pub command: u8,
    pub stream_id: u16,
    pub data: &'a [u8],
}
impl fmt::Debug for Message<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RelayMessage([redacted])")
    }
}
fn validate_stream(command: u8, stream: u16) -> Result<(), Error> {
    match command {
        1..=4 | 11..=13 | 43..=44 if stream == 0 => Err(Error::InvalidStream),
        6..=9 | 14..=15 | 19..=22 | 32..=42 if stream != 0 => Err(Error::InvalidStream),
        _ => Ok(()),
    }
}

/// Random padding is mandatory at the caller boundary. Do not use a predictable
/// fallback if native entropy is unavailable. Four leading padding bytes are 0.
pub fn encode(message: &Message<'_>, padding: &[u8]) -> Result<[u8; BODY_LEN], Error> {
    if message.data.len() > MAX_DATA {
        return Err(Error::LengthLimit);
    }
    validate_stream(message.command, message.stream_id)?;
    let unused = MAX_DATA - message.data.len();
    let zero_len = unused.min(4);
    if padding.len() != unused - zero_len {
        return Err(Error::LengthLimit);
    }
    let mut result = [0; BODY_LEN];
    result[0] = message.command;
    result[3..5].copy_from_slice(&message.stream_id.to_be_bytes());
    result[9..11].copy_from_slice(&(message.data.len() as u16).to_be_bytes());
    result[11..11 + message.data.len()].copy_from_slice(message.data);
    result[11 + message.data.len() + zero_len..].copy_from_slice(padding);
    Ok(result)
}
pub fn decode(body: &[u8; BODY_LEN]) -> Result<Message<'_>, Error> {
    if body[1..3] != [0, 0] {
        return Err(Error::InvalidRelay);
    }
    let len = u16::from_be_bytes(body[9..11].try_into().unwrap()) as usize;
    if len > MAX_DATA {
        return Err(Error::InvalidRelay);
    }
    let stream_id = u16::from_be_bytes(body[3..5].try_into().unwrap());
    validate_stream(body[0], stream_id)?;
    Ok(Message {
        command: body[0],
        stream_id,
        data: &body[11..11 + len],
    })
}

pub fn begin(host: &[u8], port: u16, flags: u32) -> Result<Vec<u8>, Error> {
    if host.is_empty() || host.len() > 255 || port == 0 || flags & !7 != 0 {
        return Err(Error::InvalidRelay);
    }
    // ASCII A-labels or already-parsed numeric literals are sent unchanged.
    // No local lookup, Unicode/IDNA conversion or shell-style interpolation.
    if host
        .iter()
        .any(|b| !b.is_ascii_graphic() || matches!(b, b':' | b'[' | b']' | b'\0'))
    {
        return Err(Error::InvalidRelay);
    }
    let mut data = host.to_vec();
    data.push(b':');
    data.extend_from_slice(port.to_string().as_bytes());
    data.push(0);
    if flags != 0 {
        data.extend_from_slice(&flags.to_be_bytes());
    }
    Ok(data)
}

pub struct CircuitWindow {
    package: u16,
    deliver: u16,
    initial: u16,
    expected: VecDeque<[u8; 20]>,
    closed: bool,
}
impl CircuitWindow {
    /// This checkpoint emits/accepts authenticated SENDME v1 only. An unsupported
    /// consensus policy must prevent circuit construction, never downgrade.
    pub fn new(window: u16, emit_min: u8, accept_min: u8) -> Result<Self, Error> {
        if !(100..=1000).contains(&window)
            || !window.is_multiple_of(100)
            || emit_min > 1
            || accept_min > 1
        {
            return Err(Error::FlowControl);
        }
        Ok(Self {
            package: window,
            deliver: window,
            initial: window,
            expected: VecDeque::with_capacity(10),
            closed: false,
        })
    }
    pub fn package_available(&self) -> u16 {
        if self.closed { 0 } else { self.package }
    }
    pub fn on_sent_data(&mut self, digest: [u8; 20]) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.package == 0 {
            return Err(Error::FlowControl);
        }
        self.package -= 1;
        if self.package.is_multiple_of(100) {
            self.expected.push_back(digest);
        }
        Ok(())
    }
    pub fn on_sendme(&mut self, body: &[u8]) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let valid = body.len() >= 23
            && body[0] == 1
            && u16::from_be_bytes([body[1], body[2]]) >= 20
            && body.len() >= 3 + u16::from_be_bytes([body[1], body[2]]) as usize
            && self
                .expected
                .front()
                .is_some_and(|expected| constant_time_eq(expected, &body[3..23]))
            && self.package <= self.initial - 100;
        if !valid {
            self.close();
            return Err(Error::AuthenticationFailed);
        }
        self.expected.pop_front();
        self.package += 100;
        Ok(())
    }
    /// The caller emits this SENDME only after accepting/flushing received data.
    pub fn on_received_data(&mut self, digest: [u8; 20]) -> Result<Option<[u8; 23]>, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.deliver == 0 {
            self.close();
            return Err(Error::FlowControl);
        }
        self.deliver -= 1;
        if self.deliver == self.initial - 100 {
            self.deliver += 100;
            let mut body = [0; 23];
            body[..3].copy_from_slice(&[1, 0, 20]);
            body[3..].copy_from_slice(&digest);
            Ok(Some(body))
        } else {
            Ok(None)
        }
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.package = 0;
        for digest in &mut self.expected {
            super::crypto::clear(digest);
        }
        self.expected.clear();
    }
}
impl Drop for CircuitWindow {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct StreamWindow {
    package: u16,
    deliver: u16,
    since_ack: u16,
}
impl Default for StreamWindow {
    fn default() -> Self {
        Self::new()
    }
}
impl StreamWindow {
    pub fn new() -> Self {
        Self {
            package: 500,
            deliver: 500,
            since_ack: 0,
        }
    }
    pub fn on_sent_data(&mut self) -> Result<(), Error> {
        if self.package == 0 {
            return Err(Error::FlowControl);
        }
        self.package -= 1;
        self.since_ack += 1;
        Ok(())
    }
    pub fn on_sendme(&mut self) -> Result<(), Error> {
        if self.since_ack < 50 || self.package > 450 {
            return Err(Error::FlowControl);
        }
        self.package += 50;
        self.since_ack -= 50;
        Ok(())
    }
    pub fn on_received_data(&mut self) -> Result<(), Error> {
        if self.deliver == 0 {
            return Err(Error::FlowControl);
        }
        self.deliver -= 1;
        Ok(())
    }
    /// Do not acknowledge while the user's receive queue contains ten cells.
    pub fn on_flushed(&mut self, queued_cells: usize) -> bool {
        if self.deliver <= 450 && queued_cells < 10 {
            self.deliver += 50;
            true
        } else {
            false
        }
    }
}
