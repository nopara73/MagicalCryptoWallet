//! Bounded RFC 6455 client protocol; no HTTP parser, sockets, TLS, or entropy source.
//!
//! The host supplies a fresh native-OS-random 16-byte handshake nonce and a fresh,
//! unpredictable four-byte mask for EVERY transmitted frame, including replies.
//! This module cannot verify entropy provenance. It never negotiates extensions.
//! HTTP/1.1 serialization, authority/Origin/authentication policy, deadlines, TLS
//! validation, transport shutdown and network cancellation belong to the host.
//! All errors are categories and all Debug implementations redact payloads.
//!
//! First-party implementation from RFC 6455 and RFC 3174, not copied source code.
//! SHA-1 is private and used only for the public WebSocket handshake digest.

#![forbid(unsafe_code)]

use std::fmt;

/// An error never includes peer-controlled data, addresses, or secrets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidLimits,
    Allocation,
    HandshakeTooLarge,
    InvalidHeader,
    DuplicateHeader,
    UnexpectedStatus,
    MissingUpgrade,
    MissingConnectionUpgrade,
    InvalidAccept,
    InvalidSubprotocol,
    UnsupportedExtension,
    ReservedBits,
    InvalidOpcode,
    WrongMask,
    NonCanonicalLength,
    LengthOverflow,
    FrameTooLarge,
    MessageTooLarge,
    TooManyFragments,
    InvalidControl,
    InvalidClose,
    InvalidUtf8,
    InvalidFragmentation,
    Backpressure,
    InvalidState,
    InvalidWriteCount,
    UnexpectedEof,
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocket {:?}", self)
    }
}
impl std::error::Error for Error {}

/// Already parsed HTTP fields. The HTTP owner validates HTTP/1.1 syntax/version.
#[derive(Clone, Copy)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}
impl fmt::Debug for Header<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("value_bytes", &self.value.len())
            .finish_non_exhaustive()
    }
}

const MAX_HEADERS: usize = 128;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_PROTOCOLS: usize = 32;
const MAX_PROTOCOL_BYTES: usize = 128;

fn token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

fn ows(value: &str) -> &str {
    value.trim_matches([' ', '\t'])
}

/// Holds only public handshake material and offered, case-sensitive protocols.
#[derive(Clone)]
pub struct ClientHandshake {
    key: String,
    accept: String,
    protocols: Vec<String>,
    protocol_field: String,
}

impl fmt::Debug for ClientHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientHandshake")
            .field("protocol_count", &self.protocols.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Negotiated {
    pub subprotocol: Option<String>,
}
impl fmt::Debug for Negotiated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Negotiated")
            .field("has_subprotocol", &self.subprotocol.is_some())
            .finish()
    }
}

impl ClientHandshake {
    /// `nonce` must be a fresh native-OS-random value, never a counter or RNG seed.
    pub fn new(nonce: [u8; 16], protocols: &[&str]) -> Result<Self, Error> {
        if protocols.len() > MAX_PROTOCOLS {
            return Err(Error::InvalidSubprotocol);
        }
        let mut offered = Vec::new();
        let mut field = String::new();
        for &protocol in protocols {
            if protocol.len() > MAX_PROTOCOL_BYTES
                || !token(protocol)
                || offered.iter().any(|p| p == protocol)
            {
                return Err(Error::InvalidSubprotocol);
            }
            if !field.is_empty() {
                field.push_str(", ");
            }
            field.push_str(protocol);
            offered.push(protocol.to_owned());
        }
        let key = base64(&nonce);
        let mut public_digest_input = [0_u8; 60];
        public_digest_input[..24].copy_from_slice(key.as_bytes());
        public_digest_input[24..].copy_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        let accept = base64(&sha1(&public_digest_input));
        Ok(Self {
            key,
            accept,
            protocols: offered,
            protocol_field: field,
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn expected_accept(&self) -> &str {
        &self.accept
    }

    /// WebSocket fields only. HTTP owner adds GET, target, Host and optional Origin.
    pub fn request_fields(&self) -> Vec<Header<'_>> {
        let mut fields = vec![
            Header {
                name: "Upgrade",
                value: "websocket",
            },
            Header {
                name: "Connection",
                value: "Upgrade",
            },
            Header {
                name: "Sec-WebSocket-Key",
                value: &self.key,
            },
            Header {
                name: "Sec-WebSocket-Version",
                value: "13",
            },
        ];
        if !self.protocols.is_empty() {
            fields.push(Header {
                name: "Sec-WebSocket-Protocol",
                value: &self.protocol_field,
            });
        }
        fields
    }

    /// Validate parsed HTTP/1.1 response fields. Never follows redirects/retries.
    /// Singleton Upgrade/Accept/Protocol duplicates and all extensions are rejected.
    /// Connection fields may repeat and are combined as strict token lists.
    pub fn validate_response(
        &self,
        status: u16,
        fields: &[Header<'_>],
    ) -> Result<Negotiated, Error> {
        if status != 101 {
            return Err(Error::UnexpectedStatus);
        }
        if fields.len() > MAX_HEADERS {
            return Err(Error::HandshakeTooLarge);
        }
        let mut bytes = 0_usize;
        let mut upgrade = None;
        let mut accept = None;
        let mut protocol = None;
        let mut connection_upgrade = false;
        for field in fields {
            bytes = bytes
                .checked_add(field.name.len())
                .and_then(|n| n.checked_add(field.value.len()))
                .ok_or(Error::HandshakeTooLarge)?;
            if bytes > MAX_HEADER_BYTES {
                return Err(Error::HandshakeTooLarge);
            }
            if field.name.len() > 256
                || !token(field.name)
                || field
                    .value
                    .bytes()
                    .any(|b| b == 127 || (b < 32 && b != b'\t'))
            {
                return Err(Error::InvalidHeader);
            }
            let value = ows(field.value);
            if field.name.eq_ignore_ascii_case("Upgrade") {
                if upgrade.replace(value).is_some() {
                    return Err(Error::DuplicateHeader);
                }
            } else if field.name.eq_ignore_ascii_case("Connection") {
                for part in value.split(',').map(ows) {
                    if !token(part) {
                        return Err(Error::InvalidHeader);
                    }
                    connection_upgrade |= part.eq_ignore_ascii_case("upgrade");
                }
            } else if field.name.eq_ignore_ascii_case("Sec-WebSocket-Accept") {
                if accept.replace(value).is_some() {
                    return Err(Error::DuplicateHeader);
                }
            } else if field.name.eq_ignore_ascii_case("Sec-WebSocket-Protocol") {
                if protocol.replace(value).is_some() {
                    return Err(Error::DuplicateHeader);
                }
            } else if field.name.eq_ignore_ascii_case("Sec-WebSocket-Extensions") {
                return Err(Error::UnsupportedExtension);
            }
        }
        if !upgrade.is_some_and(|v| v.eq_ignore_ascii_case("websocket")) {
            return Err(Error::MissingUpgrade);
        }
        if !connection_upgrade {
            return Err(Error::MissingConnectionUpgrade);
        }
        if accept != Some(self.accept.as_str()) {
            return Err(Error::InvalidAccept);
        }
        if let Some(p) = protocol
            && (!token(p) || !self.protocols.iter().any(|offered| offered == p))
        {
            return Err(Error::InvalidSubprotocol);
        }
        Ok(Negotiated {
            subprotocol: protocol.map(str::to_owned),
        })
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        result.push(ALPHABET[(b0 >> 2) as usize] as char);
        result.push(ALPHABET[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(b2 & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}

// Private SHA-1; public handshake input is ALWAYS exactly 60 bytes. Generic input
// exists only for RFC 3174 test vectors. No SHA-1 wallet/signature API is exported.
fn sha1(bytes: &[u8]) -> [u8; 20] {
    let mut state = [
        0x67452301_u32,
        0xefcdab89,
        0x98badcfe,
        0x10325476,
        0xc3d2e1f0,
    ];
    let (chunks, tail) = bytes.as_chunks::<64>();
    for block in chunks {
        sha1_block(&mut state, block);
    }
    let mut pad = [0_u8; 128];
    pad[..tail.len()].copy_from_slice(tail);
    pad[tail.len()] = 0x80;
    let used = if tail.len() < 56 { 64 } else { 128 };
    pad[used - 8..used].copy_from_slice(&((bytes.len() as u64) * 8).to_be_bytes());
    for block in pad[..used].as_chunks::<64>().0 {
        sha1_block(&mut state, block);
    }
    let mut result = [0_u8; 20];
    for (output, word) in result.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        output.copy_from_slice(&word.to_be_bytes());
    }
    result
}

fn sha1_block(state: &mut [u32; 5], block: &[u8]) {
    let mut words = [0_u32; 80];
    for (word, bytes) in words[..16].iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for i in 16..80 {
        words[i] = (words[i - 3] ^ words[i - 8] ^ words[i - 14] ^ words[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, word) in words.into_iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5a827999),
            20..=39 => (b ^ c ^ d, 0x6ed9eba1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
            _ => (b ^ c ^ d, 0xca62c1d6),
        };
        let next = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(word);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = next;
    }
    for (s, value) in state.iter_mut().zip([a, b, c, d, e]) {
        *s = s.wrapping_add(value);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Opcode {
    Continuation = 0,
    Text = 1,
    Binary = 2,
    Close = 8,
    Ping = 9,
    Pong = 10,
}
impl Opcode {
    pub fn from_byte(byte: u8) -> Result<Self, Error> {
        match byte {
            0 => Ok(Self::Continuation),
            1 => Ok(Self::Text),
            2 => Ok(Self::Binary),
            8 => Ok(Self::Close),
            9 => Ok(Self::Ping),
            10 => Ok(Self::Pong),
            _ => Err(Error::InvalidOpcode),
        }
    }
    pub fn is_control(self) -> bool {
        (self as u8) & 8 != 0
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct FrameHeader {
    pub final_fragment: bool,
    pub opcode: Opcode,
    pub payload_len: usize,
    pub header_len: usize,
    pub mask: Option<[u8; 4]>,
}
impl fmt::Debug for FrameHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameHeader")
            .field("final_fragment", &self.final_fragment)
            .field("opcode", &self.opcode)
            .field("payload_len", &self.payload_len)
            .field("masked", &self.mask.is_some())
            .finish()
    }
}

fn header_size(input: &[u8]) -> usize {
    if input.len() < 2 {
        return 2;
    }
    2 + match input[1] & 127 {
        126 => 2,
        127 => 8,
        _ => 0,
    } + if input[1] & 128 != 0 { 4 } else { 0 }
}

/// Incremental header inspection. Limits apply before payload allocation; `None`
/// means more bytes, errors can be reported before the entire header has arrived.
pub fn inspect_header(
    input: &[u8],
    direction: Direction,
    max_payload: usize,
) -> Result<Option<FrameHeader>, Error> {
    let Some(&first) = input.first() else {
        return Ok(None);
    };
    if first & 0x70 != 0 {
        return Err(Error::ReservedBits);
    }
    let opcode = Opcode::from_byte(first & 15)?;
    let fin = first & 128 != 0;
    if opcode.is_control() && !fin {
        return Err(Error::InvalidControl);
    }
    if input.len() < 2 {
        return Ok(None);
    }
    let masked = input[1] & 128 != 0;
    if masked != (direction == Direction::ClientToServer) {
        return Err(Error::WrongMask);
    }
    let marker = input[1] & 127;
    if opcode.is_control() && marker > 125 {
        return Err(Error::InvalidControl);
    }
    let length = match marker {
        126 => {
            if input.len() < 4 {
                return Ok(None);
            }
            let value = u16::from_be_bytes([input[2], input[3]]) as u64;
            if value < 126 {
                return Err(Error::NonCanonicalLength);
            }
            value
        }
        127 => {
            if input.len() < 10 {
                return Ok(None);
            }
            let value =
                u64::from_be_bytes(input[2..10].try_into().map_err(|_| Error::LengthOverflow)?);
            if value & (1_u64 << 63) != 0 {
                return Err(Error::LengthOverflow);
            }
            if value <= 65535 {
                return Err(Error::NonCanonicalLength);
            }
            value
        }
        value => value as u64,
    };
    let length = usize::try_from(length).map_err(|_| Error::LengthOverflow)?;
    if length > max_payload {
        return Err(Error::FrameTooLarge);
    }
    let size = header_size(input);
    size.checked_add(length).ok_or(Error::LengthOverflow)?;
    if input.len() < size {
        return Ok(None);
    }
    let mask = if masked {
        Some(
            input[size - 4..size]
                .try_into()
                .map_err(|_| Error::LengthOverflow)?,
        )
    } else {
        None
    };
    Ok(Some(FrameHeader {
        final_fragment: fin,
        opcode,
        payload_len: length,
        header_len: size,
        mask,
    }))
}

/// Borrowed wire payload. Bytes remain masked for client-to-server frames.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub header: FrameHeader,
    wire_payload: &'a [u8],
}
impl fmt::Debug for Frame<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.header.fmt(f)
    }
}
impl Frame<'_> {
    pub fn wire_payload(&self) -> &[u8] {
        self.wire_payload
    }
    pub fn payload_byte(&self, index: usize) -> Option<u8> {
        self.wire_payload
            .get(index)
            .map(|b| b ^ self.header.mask.map_or(0, |m| m[index % 4]))
    }
    pub fn payload(&self) -> Result<Vec<u8>, Error> {
        let mut result = Vec::new();
        result
            .try_reserve_exact(self.header.payload_len)
            .map_err(|_| Error::Allocation)?;
        result.extend((0..self.header.payload_len).map(|i| self.payload_byte(i).unwrap_or(0)));
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParsedFrame<'a> {
    pub frame: Frame<'a>,
    pub consumed: usize,
}

#[derive(Clone, Copy, Default)]
struct Utf8 {
    remaining: u8,
    low: u8,
    high: u8,
}
impl Utf8 {
    fn push(&mut self, bytes: impl IntoIterator<Item = u8>, fin: bool) -> Result<(), Error> {
        for byte in bytes {
            if self.remaining != 0 {
                if byte < self.low || byte > self.high {
                    return Err(Error::InvalidUtf8);
                }
                self.remaining -= 1;
                self.low = 0x80;
                self.high = 0xbf;
            } else {
                match byte {
                    0..=0x7f => {}
                    0xc2..=0xdf => {
                        self.remaining = 1;
                        self.low = 0x80;
                        self.high = 0xbf;
                    }
                    0xe0 => {
                        self.remaining = 2;
                        self.low = 0xa0;
                        self.high = 0xbf;
                    }
                    0xe1..=0xec | 0xee..=0xef => {
                        self.remaining = 2;
                        self.low = 0x80;
                        self.high = 0xbf;
                    }
                    0xed => {
                        self.remaining = 2;
                        self.low = 0x80;
                        self.high = 0x9f;
                    }
                    0xf0 => {
                        self.remaining = 3;
                        self.low = 0x90;
                        self.high = 0xbf;
                    }
                    0xf1..=0xf3 => {
                        self.remaining = 3;
                        self.low = 0x80;
                        self.high = 0xbf;
                    }
                    0xf4 => {
                        self.remaining = 3;
                        self.low = 0x80;
                        self.high = 0x8f;
                    }
                    _ => return Err(Error::InvalidUtf8),
                }
            }
        }
        if fin && self.remaining != 0 {
            return Err(Error::InvalidUtf8);
        }
        Ok(())
    }
}

/// Base protocol + assigned 1012..1014 and application/private 3000..4999.
/// Reserved/no-wire 1004/1005/1006/1015 and unknown standard codes are rejected.
pub fn valid_close_code(code: u16, direction: Direction) -> bool {
    matches!(code, 1000..=1003 | 1007..=1009 | 1011..=1014 | 3000..=4999)
        || (code == 1010 && direction == Direction::ClientToServer)
}

fn validate_payload(
    opcode: Opcode,
    fin: bool,
    length: usize,
    byte: impl Fn(usize) -> u8,
    direction: Direction,
) -> Result<(), Error> {
    if opcode == Opcode::Text {
        Utf8::default().push((0..length).map(&byte), fin)?;
    } else if opcode == Opcode::Close {
        if length == 1 {
            return Err(Error::InvalidClose);
        }
        if length > 0 {
            let code = u16::from_be_bytes([byte(0), byte(1)]);
            if !valid_close_code(code, direction) {
                return Err(Error::InvalidClose);
            }
            Utf8::default().push((2..length).map(byte), true)?;
        }
    }
    Ok(())
}

/// Parses exactly one frame; trailing bytes remain unconsumed. Fragmentation and
/// continuation UTF-8 context are enforced by `Client`, not this stateless view.
pub fn parse_frame(
    input: &[u8],
    direction: Direction,
    max_payload: usize,
) -> Result<Option<ParsedFrame<'_>>, Error> {
    let Some(header) = inspect_header(input, direction, max_payload)? else {
        return Ok(None);
    };
    let consumed = header.header_len + header.payload_len;
    if input.len() < consumed {
        return Ok(None);
    }
    let frame = Frame {
        header,
        wire_payload: &input[header.header_len..consumed],
    };
    validate_payload(
        header.opcode,
        header.final_fragment,
        header.payload_len,
        |i| frame.payload_byte(i).unwrap_or(0),
        direction,
    )?;
    Ok(Some(ParsedFrame { frame, consumed }))
}

/// Canonical frame serialization. The explicit mask is required for client
/// direction and forbidden for server direction; no randomness is invented.
pub fn encode_frame(
    direction: Direction,
    opcode: Opcode,
    fin: bool,
    payload: &[u8],
    mask: Option<[u8; 4]>,
    max_payload: usize,
) -> Result<Vec<u8>, Error> {
    if mask.is_some() != (direction == Direction::ClientToServer) {
        return Err(Error::WrongMask);
    }
    if opcode.is_control() && (!fin || payload.len() > 125) {
        return Err(Error::InvalidControl);
    }
    if payload.len() > max_payload {
        return Err(Error::FrameTooLarge);
    }
    if (payload.len() as u128) >= (1_u128 << 63) {
        return Err(Error::LengthOverflow);
    }
    validate_payload(opcode, fin, payload.len(), |i| payload[i], direction)?;
    let length_bytes = if payload.len() < 126 {
        0
    } else if payload.len() <= 65535 {
        2
    } else {
        8
    };
    let total = payload
        .len()
        .checked_add(2 + length_bytes + if mask.is_some() { 4 } else { 0 })
        .ok_or(Error::LengthOverflow)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(total)
        .map_err(|_| Error::Allocation)?;
    output.push((if fin { 128 } else { 0 }) | opcode as u8);
    let mask_bit = if mask.is_some() { 128 } else { 0 };
    match length_bytes {
        0 => output.push(mask_bit | payload.len() as u8),
        2 => {
            output.push(mask_bit | 126);
            output.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        _ => {
            output.push(mask_bit | 127);
            output.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
    }
    if let Some(key) = mask {
        output.extend_from_slice(&key);
    }
    output.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask.map_or(0, |m| m[i % 4])),
    );
    Ok(output)
}

#[derive(Clone, Eq, PartialEq)]
pub struct CloseData {
    pub code: Option<u16>,
    pub reason: String,
}
impl fmt::Debug for CloseData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseData")
            .field("code", &self.code)
            .field("reason_bytes", &self.reason.len())
            .finish()
    }
}

pub fn close_payload(
    code: Option<u16>,
    reason: &str,
    direction: Direction,
) -> Result<Vec<u8>, Error> {
    if code.is_none() && !reason.is_empty() {
        return Err(Error::InvalidClose);
    }
    if reason.len() > 123 {
        return Err(Error::InvalidControl);
    }
    if let Some(code) = code {
        if !valid_close_code(code, direction) {
            return Err(Error::InvalidClose);
        }
        let mut bytes = Vec::with_capacity(2 + reason.len());
        bytes.extend_from_slice(&code.to_be_bytes());
        bytes.extend_from_slice(reason.as_bytes());
        Ok(bytes)
    } else {
        Ok(Vec::new())
    }
}

/// Hard ceilings constrain malicious peers even when caller settings are faulty.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    frame: usize,
    message: usize,
    fragments: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            frame: 64 * 1024,
            message: 1024 * 1024,
            fragments: 1024,
        }
    }
}
impl Limits {
    pub fn new(frame: usize, message: usize, fragments: usize) -> Result<Self, Error> {
        if frame == 0
            || frame > 16 * 1024 * 1024
            || message == 0
            || message > 64 * 1024 * 1024
            || fragments == 0
            || fragments > 65536
        {
            return Err(Error::InvalidLimits);
        }
        Ok(Self {
            frame,
            message,
            fragments,
        })
    }
    pub fn frame_payload(self) -> usize {
        self.frame
    }
    pub fn message_payload(self) -> usize {
        self.message
    }
    pub fn fragments(self) -> usize {
        self.fragments
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Connecting,
    Open,
    Closing,
    Closed,
    Failed,
    Aborted,
}

#[derive(Eq, PartialEq)]
pub enum Event {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(CloseData),
}
impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(v) => f.debug_tuple("TextBytes").field(&v.len()).finish(),
            Self::Binary(v) => f.debug_tuple("BinaryBytes").field(&v.len()).finish(),
            Self::Ping(v) => f.debug_tuple("PingBytes").field(&v.len()).finish(),
            Self::Pong(v) => f.debug_tuple("PongBytes").field(&v.len()).finish(),
            Self::Close(v) => v.fmt(f),
        }
    }
}

#[derive(Debug)]
pub struct Received {
    pub consumed: usize,
    pub event: Option<Event>,
    pub backpressured: bool,
}

#[derive(Clone, Copy, Default)]
struct Sequence {
    opcode: Option<Opcode>,
    length: usize,
    count: usize,
    utf8: Utf8,
}
impl Sequence {
    fn preview(self, opcode: Opcode, length: usize, limits: Limits) -> Result<Self, Error> {
        if opcode.is_control() {
            return Ok(self);
        }
        let mut next = match opcode {
            Opcode::Text | Opcode::Binary if self.opcode.is_none() => Self {
                opcode: Some(opcode),
                ..Self::default()
            },
            Opcode::Continuation if self.opcode.is_some() => self,
            _ => return Err(Error::InvalidFragmentation),
        };
        next.length = next
            .length
            .checked_add(length)
            .ok_or(Error::MessageTooLarge)?;
        if next.length > limits.message {
            return Err(Error::MessageTooLarge);
        }
        next.count += 1;
        if next.count > limits.fragments {
            return Err(Error::TooManyFragments);
        }
        Ok(next)
    }
}

struct Outbound {
    bytes: Vec<u8>,
    offset: usize,
    opcode: Opcode,
}
struct Reply {
    opcode: Opcode,
    payload: Vec<u8>,
}

/// One read owner + one write owner serialized by the host. Retains at most one
/// partial incoming frame, one fragmented message, one outgoing frame, one
/// control reply, and no event queue. Returns exactly one frame's event at a time.
pub struct Client {
    handshake: ClientHandshake,
    negotiated: Option<Negotiated>,
    limits: Limits,
    state: State,
    input: Vec<u8>,
    incoming: Sequence,
    message: Vec<u8>,
    outgoing_sequence: Sequence,
    outgoing: Option<Outbound>,
    reply: Option<Reply>,
    close_started: bool,
    close_sent: bool,
    close_received: Option<CloseData>,
}
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("state", &self.state)
            .field("buffered_frame_bytes", &self.input.len())
            .field("buffered_message_bytes", &self.message.len())
            .field("outgoing_bytes", &self.outgoing().len())
            .field("pending_control", &self.reply.is_some())
            .finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(handshake: ClientHandshake, limits: Limits) -> Self {
        Self {
            handshake,
            negotiated: None,
            limits,
            state: State::Connecting,
            input: Vec::new(),
            incoming: Sequence::default(),
            message: Vec::new(),
            outgoing_sequence: Sequence::default(),
            outgoing: None,
            reply: None,
            close_started: false,
            close_sent: false,
            close_received: None,
        }
    }
    pub fn handshake(&self) -> &ClientHandshake {
        &self.handshake
    }
    pub fn negotiated(&self) -> Option<&Negotiated> {
        self.negotiated.as_ref()
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn close_sent(&self) -> bool {
        self.close_sent
    }
    pub fn close_received(&self) -> Option<&CloseData> {
        self.close_received.as_ref()
    }
    pub fn pending_control(&self) -> Option<Opcode> {
        self.reply.as_ref().map(|r| r.opcode)
    }

    /// Cancellation is a host-owned snapshot checked before this bounded call.
    /// Cancellation aborts; it is never a resumable/replayable transport operation.
    fn check_cancel(&mut self, cancelled: bool) -> Result<(), Error> {
        if cancelled {
            self.abort();
            return Err(Error::Cancelled);
        }
        Ok(())
    }
    pub fn accept_upgrade(
        &mut self,
        status: u16,
        fields: &[Header<'_>],
        cancelled: bool,
    ) -> Result<(), Error> {
        self.check_cancel(cancelled)?;
        if self.state != State::Connecting {
            return Err(Error::InvalidState);
        }
        match self.handshake.validate_response(status, fields) {
            Ok(negotiated) => {
                self.negotiated = Some(negotiated);
                self.state = State::Open;
                Ok(())
            }
            Err(error) => {
                self.fail();
                Err(error)
            }
        }
    }
    fn discard_buffers(&mut self) {
        self.input = Vec::new();
        self.message = Vec::new();
        self.outgoing = None;
        self.reply = None;
        self.incoming = Sequence::default();
        self.outgoing_sequence = Sequence::default();
    }
    fn fail(&mut self) {
        self.discard_buffers();
        self.state = State::Failed;
    }
    /// The host must close the transport, including partially written frames.
    pub fn abort(&mut self) {
        self.discard_buffers();
        self.state = State::Aborted;
    }

    /// Consume at most one frame. If a control reply awaits queuing, returns
    /// `backpressured=true, consumed=0`; drain output then supply a new reply mask.
    /// On protocol error the connection fails terminally; host closes transport.
    pub fn receive(&mut self, input: &[u8], cancelled: bool) -> Result<Received, Error> {
        self.check_cancel(cancelled)?;
        if !matches!(self.state, State::Open | State::Closing) {
            return Err(Error::InvalidState);
        }
        if self.reply.is_some() || self.close_received.is_some() {
            return Ok(Received {
                consumed: 0,
                event: None,
                backpressured: true,
            });
        }
        match self.receive_inner(input) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.fail();
                Err(error)
            }
        }
    }
    fn append_input(&mut self, input: &[u8]) -> Result<(), Error> {
        self.input
            .try_reserve_exact(input.len())
            .map_err(|_| Error::Allocation)?;
        self.input.extend_from_slice(input);
        Ok(())
    }
    fn receive_inner(&mut self, input: &[u8]) -> Result<Received, Error> {
        let mut consumed = 0;
        loop {
            // Inspect even incomplete headers to reject opcode/masking errors early.
            let header = inspect_header(&self.input, Direction::ServerToClient, self.limits.frame)?;
            let target = if let Some(header) = header {
                self.incoming
                    .preview(header.opcode, header.payload_len, self.limits)?;
                header.header_len + header.payload_len
            } else {
                header_size(&self.input)
            };
            let take = (target - self.input.len()).min(input.len() - consumed);
            self.append_input(&input[consumed..consumed + take])?;
            consumed += take;
            if self.input.len() < target {
                return Ok(Received {
                    consumed,
                    event: None,
                    backpressured: false,
                });
            }
            if header.is_none() {
                continue;
            }
            let Some(parsed) =
                parse_frame(&self.input, Direction::ServerToClient, self.limits.frame)?
            else {
                return Err(Error::InvalidState);
            };
            let header = parsed.frame.header;
            let payload = parsed.frame.payload()?;
            self.input.clear();
            let event = self.process(header, payload)?;
            return Ok(Received {
                consumed,
                event,
                backpressured: false,
            });
        }
    }

    fn process(&mut self, header: FrameHeader, payload: Vec<u8>) -> Result<Option<Event>, Error> {
        match header.opcode {
            Opcode::Ping => {
                self.reply = Some(Reply {
                    opcode: Opcode::Pong,
                    payload: payload.clone(),
                });
                Ok(Some(Event::Ping(payload)))
            }
            Opcode::Pong => Ok(Some(Event::Pong(payload))),
            Opcode::Close => {
                let close = if payload.is_empty() {
                    CloseData {
                        code: None,
                        reason: String::new(),
                    }
                } else {
                    CloseData {
                        code: Some(u16::from_be_bytes([payload[0], payload[1]])),
                        reason: String::from_utf8(payload[2..].to_vec())
                            .map_err(|_| Error::InvalidUtf8)?,
                    }
                };
                self.close_received = Some(close.clone());
                self.state = State::Closing;
                self.message = Vec::new();
                self.incoming = Sequence::default();
                self.outgoing_sequence = Sequence::default();
                // An unstarted frame can be discarded. A partially written frame
                // MUST complete before a Close response can be sent on that stream.
                if self
                    .outgoing
                    .as_ref()
                    .is_some_and(|o| o.offset == 0 && o.opcode != Opcode::Close)
                {
                    self.outgoing = None;
                }
                if !self.close_started {
                    self.reply = Some(Reply {
                        opcode: Opcode::Close,
                        payload,
                    });
                    self.close_started = true;
                }
                if self.close_sent {
                    self.state = State::Closed;
                }
                Ok(Some(Event::Close(close)))
            }
            opcode => {
                let mut next = self.incoming.preview(opcode, payload.len(), self.limits)?;
                if next.opcode == Some(Opcode::Text) {
                    next.utf8
                        .push(payload.iter().copied(), header.final_fragment)?;
                }
                self.message
                    .try_reserve_exact(payload.len())
                    .map_err(|_| Error::Allocation)?;
                self.message.extend_from_slice(&payload);
                if header.final_fragment {
                    let message = std::mem::take(&mut self.message);
                    self.incoming = Sequence::default();
                    if self.close_started {
                        return Ok(None);
                    }
                    if next.opcode == Some(Opcode::Text) {
                        Ok(Some(Event::Text(
                            String::from_utf8(message).map_err(|_| Error::InvalidUtf8)?,
                        )))
                    } else {
                        Ok(Some(Event::Binary(message)))
                    }
                } else {
                    self.incoming = next;
                    Ok(None)
                }
            }
        }
    }

    /// Queue exactly one immutable frame. Returns Backpressure without mutation
    /// if output/reply is occupied. Local validation errors do not fail a session.
    /// Data fragment UTF-8 is validated incrementally; no full outgoing message copy.
    pub fn queue_frame(
        &mut self,
        opcode: Opcode,
        fin: bool,
        payload: &[u8],
        mask: [u8; 4],
        cancelled: bool,
    ) -> Result<(), Error> {
        self.check_cancel(cancelled)?;
        if !matches!(self.state, State::Open | State::Closing) {
            return Err(Error::InvalidState);
        }
        if self.outgoing.is_some() || self.reply.is_some() {
            return Err(Error::Backpressure);
        }
        if self.close_received.is_some()
            || (self.close_started && !matches!(opcode, Opcode::Ping | Opcode::Pong))
        {
            return Err(Error::InvalidState);
        }
        let mut next = self
            .outgoing_sequence
            .preview(opcode, payload.len(), self.limits)?;
        if !opcode.is_control() && next.opcode == Some(Opcode::Text) {
            next.utf8.push(payload.iter().copied(), fin)?;
        }
        let bytes = encode_frame(
            Direction::ClientToServer,
            opcode,
            fin,
            payload,
            Some(mask),
            self.limits.frame,
        )?;
        if !opcode.is_control() {
            self.outgoing_sequence = if fin { Sequence::default() } else { next };
        }
        if opcode == Opcode::Close {
            self.close_started = true;
            self.state = State::Closing;
            self.outgoing_sequence = Sequence::default();
        }
        self.outgoing = Some(Outbound {
            bytes,
            offset: 0,
            opcode,
        });
        Ok(())
    }
    pub fn queue_close(
        &mut self,
        code: Option<u16>,
        reason: &str,
        mask: [u8; 4],
        cancelled: bool,
    ) -> Result<(), Error> {
        self.check_cancel(cancelled)?;
        let payload = close_payload(code, reason, Direction::ClientToServer)?;
        self.queue_frame(Opcode::Close, true, &payload, mask, false)
    }
    /// Serialize required Pong/Close reply using a fresh host entropy mask.
    /// Reply stays pending if outgoing is occupied. Nothing is silently dropped.
    pub fn queue_pending_control(&mut self, mask: [u8; 4], cancelled: bool) -> Result<(), Error> {
        self.check_cancel(cancelled)?;
        if !matches!(self.state, State::Open | State::Closing) {
            return Err(Error::InvalidState);
        }
        if self.outgoing.is_some() {
            return Err(Error::Backpressure);
        }
        let reply = self.reply.as_ref().ok_or(Error::InvalidState)?;
        let bytes = encode_frame(
            Direction::ClientToServer,
            reply.opcode,
            true,
            &reply.payload,
            Some(mask),
            self.limits.frame,
        )?;
        self.outgoing = Some(Outbound {
            bytes,
            offset: 0,
            opcode: reply.opcode,
        });
        self.reply = None;
        Ok(())
    }
    /// Immutable bytes still to write. Retry ONLY the unwritten suffix after a
    /// successful partial write; an ambiguous transport error requires abort.
    pub fn outgoing(&self) -> &[u8] {
        self.outgoing.as_ref().map_or(&[], |o| &o.bytes[o.offset..])
    }
    /// Acknowledge a successful transport write. No close is "sent" until all its
    /// bytes have been acknowledged; state handles simultaneous close races.
    pub fn advance_written(&mut self, bytes: usize, cancelled: bool) -> Result<(), Error> {
        self.check_cancel(cancelled)?;
        let outgoing = self.outgoing.as_mut().ok_or(Error::InvalidState)?;
        if bytes > outgoing.bytes.len() - outgoing.offset {
            self.fail();
            return Err(Error::InvalidWriteCount);
        }
        outgoing.offset += bytes;
        if outgoing.offset == outgoing.bytes.len() {
            if outgoing.opcode == Opcode::Close {
                self.close_sent = true;
                if self.close_received.is_some() {
                    self.state = State::Closed;
                }
            }
            self.outgoing = None;
        }
        Ok(())
    }
    /// A clean EOF requires BOTH Close frames. A closing deadline/transport
    /// failure should instead call abort; no timeout/reconnect policy is inferred.
    pub fn transport_eof(&mut self) -> Result<(), Error> {
        if self.close_sent && self.close_received.is_some() && self.state == State::Closed {
            self.discard_buffers();
            Ok(())
        } else {
            self.fail();
            Err(Error::UnexpectedEof)
        }
    }
}

#[cfg(test)]
mod private_tests {
    use super::*;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    #[test]
    fn rfc3174_sha1_vectors() {
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(&sha1(&vec![b'a'; 1_000_000])),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }
    #[test]
    fn incremental_utf8_matches_standard_library() {
        for a in 0_u8..=255 {
            for b in 0_u8..=255 {
                let pair = [a, b];
                assert_eq!(
                    Utf8::default().push(pair, true).is_ok(),
                    std::str::from_utf8(&pair).is_ok()
                );
            }
        }
        for bytes in [
            "\u{80}",
            "\u{800}",
            "\u{d7ff}",
            "\u{e000}",
            "\u{10000}",
            "\u{10ffff}",
        ]
        .map(str::as_bytes)
        {
            for split in 0..=bytes.len() {
                let mut validator = Utf8::default();
                validator
                    .push(bytes[..split].iter().copied(), false)
                    .unwrap();
                validator
                    .push(bytes[split..].iter().copied(), true)
                    .unwrap();
            }
        }
    }
}
