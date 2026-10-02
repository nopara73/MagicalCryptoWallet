//! Bounded, first-party RFC 8259 JSON for the `mcw` application.
//!
//! No floating-point conversion, platform bindings, or external crates. Objects
//! retain member order, numbers retain their spelling, and parsed strings retain
//! their escapes. `Document::source` replays the original input exactly; fresh
//! serialization chooses whitespace explicitly. Extensions are opt-in and are
//! reported on the document. Dates, Bitcoin types, and schema defaults belong to
//! typed domain adapters, never to the syntax engine.

use std::collections::BTreeSet;
use std::fmt;

#[path = "json/compat.rs"]
pub mod compat;

/// Limits count UTF-8 bytes, JSON values plus object keys, and nested containers.
/// A scalar root has depth zero; an empty root container has depth one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub depth: usize,
    pub nodes: usize,
    pub container_entries: usize,
    pub string_bytes: usize,
    pub number_bytes: usize,
    pub total_decoded_bytes: usize,
}

/// Non-removable ceilings, including a stack-safe recursive descent depth cap.
pub const HARD_LIMITS: Limits = Limits {
    input_bytes: 64 * 1024 * 1024,
    output_bytes: 128 * 1024 * 1024,
    depth: 128,
    nodes: 1_000_000,
    container_entries: 1_000_000,
    string_bytes: 16 * 1024 * 1024,
    number_bytes: 64 * 1024,
    total_decoded_bytes: 64 * 1024 * 1024,
};

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 8 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            depth: 64,
            nodes: 100_000,
            container_entries: 50_000,
            string_bytes: 1024 * 1024,
            number_bytes: 4096,
            total_decoded_bytes: 8 * 1024 * 1024,
        }
    }
}

impl Limits {
    fn validate(self) -> Result<(), ErrorKind> {
        if self.input_bytes > HARD_LIMITS.input_bytes
            || self.output_bytes > HARD_LIMITS.output_bytes
            || self.depth > HARD_LIMITS.depth
            || self.nodes > HARD_LIMITS.nodes
            || self.container_entries > HARD_LIMITS.container_entries
            || self.string_bytes > HARD_LIMITS.string_bytes
            || self.number_bytes > HARD_LIMITS.number_bytes
            || self.total_decoded_bytes > HARD_LIMITS.total_decoded_bytes
        {
            Err(ErrorKind::InvalidOptions)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DuplicatePolicy {
    /// Compare decoded keys exactly, without case folding or Unicode normalization.
    #[default]
    Reject,
    /// Keep every member in order. Lookup still refuses ambiguous keys.
    Preserve,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extensions {
    pub comments: bool,
    pub trailing_commas: bool,
    pub utf8_bom: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtensionsUsed {
    pub comments: bool,
    pub trailing_commas: bool,
    pub utf8_bom: bool,
    pub duplicate_keys: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParseOptions {
    pub limits: Limits,
    pub duplicates: DuplicatePolicy,
    pub extensions: Extensions,
}

impl ParseOptions {
    /// Current managed JsonDecoder skips comments but rejects trailing commas.
    /// This syntax profile does not implement its schema coercions or defaults.
    pub fn legacy_config() -> Self {
        Self {
            extensions: Extensions {
                comments: true,
                ..Extensions::default()
            },
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    InputBytes,
    OutputBytes,
    Depth,
    Nodes,
    ContainerEntries,
    StringBytes,
    NumberBytes,
    TotalDecodedBytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidUtf8,
    BomNotAllowed,
    UnexpectedEof,
    ExpectedValue,
    ExpectedObjectKey,
    ExpectedColon,
    ExpectedCommaOrEnd,
    InvalidNumber,
    InvalidEscape,
    InvalidUnicodeEscape,
    UnpairedSurrogate,
    UnescapedControl,
    UnterminatedString,
    UnterminatedComment,
    TrailingCharacters,
    DuplicateKey,
    LimitExceeded(LimitKind),
    InvalidOptions,
    AllocationFailed,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded(limit) => write!(f, "JSON resource limit exceeded: {limit:?}"),
            other => write!(f, "JSON {other:?}"),
        }
    }
}

/// Byte offset and one-based line/byte column (LF starts a new line).
/// Errors never include source text, keys, or wallet secrets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl Error {
    fn at(input: &[u8], offset: usize, kind: ErrorKind) -> Self {
        let offset = offset.min(input.len());
        let prefix = &input[..offset];
        let line = 1 + prefix.iter().filter(|b| **b == b'\n').count();
        let column = match prefix.iter().rposition(|b| *b == b'\n') {
            Some(last_newline) => offset - last_newline,
            None => offset + 1,
        };
        Self {
            kind,
            offset,
            line,
            column,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at byte {} ({}:{})",
            self.kind, self.offset, self.line, self.column
        )
    }
}

impl std::error::Error for Error {}

/// Decoded Unicode scalars plus the original validated token, when parsed.
/// Fields are private so changing text cannot leave a stale cached token.
#[derive(Clone, Debug)]
pub struct JsonString {
    decoded: String,
    token: Option<String>,
}

impl JsonString {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            decoded: value.into(),
            token: None,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.decoded
    }
    pub fn original_token(&self) -> Option<&str> {
        self.token.as_deref()
    }
    pub fn into_string(self) -> String {
        self.decoded
    }
}

impl PartialEq for JsonString {
    fn eq(&self, other: &Self) -> bool {
        self.decoded == other.decoded
    }
}
impl Eq for JsonString {}

/// A validated JSON number token, not a machine floating-point approximation.
/// Equality is lexical: `1`, `1.0`, `1e0`, and `-0` stay distinguishable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Number {
    token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberConversionError {
    NonIntegral,
    OutOfRange,
    ExponentOutOfRange,
}

impl fmt::Display for NumberConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exact JSON number conversion: {self:?}")
    }
}
impl std::error::Error for NumberConversionError {}

impl Number {
    /// Validate a single token under the default number byte limit.
    pub fn parse(token: &str) -> Result<Self, Error> {
        let mut end = 0;
        scan_number(token.as_bytes(), &mut end, Limits::default().number_bytes)
            .map_err(|(kind, at)| Error::at(token.as_bytes(), at, kind))?;
        if end != token.len() {
            return Err(Error::at(token.as_bytes(), end, ErrorKind::InvalidNumber));
        }
        Ok(Self {
            token: copy_string(token).map_err(|kind| Error::at(token.as_bytes(), 0, kind))?,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.token
    }

    pub fn to_i128_exact(&self) -> Result<i128, NumberConversionError> {
        self.to_scaled_i128(0)
    }

    pub fn to_i64_exact(&self) -> Result<i64, NumberConversionError> {
        self.to_i128_exact()?
            .try_into()
            .map_err(|_| NumberConversionError::OutOfRange)
    }

    pub fn to_u64_exact(&self) -> Result<u64, NumberConversionError> {
        self.to_i128_exact()?
            .try_into()
            .map_err(|_| NumberConversionError::OutOfRange)
    }

    /// Convert exactly to integer units of 10^-scale. No rounding or saturation.
    /// Bitcoin amounts use scale 8, then an independently checked domain range.
    pub fn to_scaled_i128(&self, scale: u32) -> Result<i128, NumberConversionError> {
        use NumberConversionError::{ExponentOutOfRange, NonIntegral, OutOfRange};
        let negative = self.token.starts_with('-');
        let unsigned = self.token.strip_prefix('-').unwrap_or(&self.token);
        let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
            Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
            None => (unsigned, None),
        };
        // Zero is exact even with an arbitrarily large exponent spelling.
        if mantissa.bytes().all(|b| b == b'0' || b == b'.') {
            return Ok(0);
        }
        let exponent = exponent
            .map(|e| e.parse::<i64>().map_err(|_| ExponentOutOfRange))
            .transpose()?
            .unwrap_or(0);
        let fraction = mantissa.find('.').map_or(0, |at| mantissa.len() - at - 1);
        let power = exponent
            .checked_add(i64::from(scale))
            .and_then(|e| e.checked_sub(fraction as i64))
            .ok_or(ExponentOutOfRange)?;
        let digits = mantissa.bytes().filter(|b| *b != b'.').count();
        let retained = if power < 0 {
            let discarded = power.unsigned_abs();
            if discarded >= digits as u64 {
                return Err(NonIntegral);
            }
            if mantissa
                .bytes()
                .rev()
                .filter(|b| *b != b'.')
                .take(discarded as usize)
                .any(|b| b != b'0')
            {
                return Err(NonIntegral);
            }
            digits - discarded as usize
        } else {
            digits
        };
        let ceiling = if negative {
            1_u128 << 127
        } else {
            i128::MAX as u128
        };
        let mut magnitude = 0_u128;
        for b in mantissa.bytes().filter(|b| *b != b'.').take(retained) {
            magnitude = magnitude
                .checked_mul(10)
                .and_then(|v| v.checked_add(u128::from(b - b'0')))
                .ok_or(OutOfRange)?;
            if magnitude > ceiling {
                return Err(OutOfRange);
            }
        }
        if power > 0 {
            // A nonzero i128 cannot survive 39 additional decimal zeroes.
            if power > 38 {
                return Err(OutOfRange);
            }
            for _ in 0..power {
                magnitude = magnitude.checked_mul(10).ok_or(OutOfRange)?;
                if magnitude > ceiling {
                    return Err(OutOfRange);
                }
            }
        }
        if negative {
            Ok(-((magnitude - 1) as i128) - 1)
        } else {
            Ok(magnitude as i128)
        }
    }

    /// Construct a plain decimal token from exact integer units, retaining scale.
    pub fn from_scaled_i128(units: i128, scale: u32) -> Result<Self, ErrorKind> {
        let digits = units.unsigned_abs().to_string();
        let scale = scale as usize;
        let negative = usize::from(units < 0);
        let length = if scale == 0 {
            negative + digits.len()
        } else {
            negative + digits.len().max(scale + 1) + 1
        };
        if length > Limits::default().number_bytes {
            return Err(ErrorKind::LimitExceeded(LimitKind::NumberBytes));
        }
        let mut token = String::new();
        token
            .try_reserve(length)
            .map_err(|_| ErrorKind::AllocationFailed)?;
        if units < 0 {
            token.push('-');
        }
        if scale == 0 {
            token.push_str(&digits);
        } else if digits.len() > scale {
            let at = digits.len() - scale;
            token.push_str(&digits[..at]);
            token.push('.');
            token.push_str(&digits[at..]);
        } else {
            token.push_str("0.");
            for _ in digits.len()..scale {
                token.push('0');
            }
            token.push_str(&digits);
        }
        Ok(Self { token })
    }
}

impl From<i128> for Number {
    fn from(value: i128) -> Self {
        Self {
            token: value.to_string(),
        }
    }
}
impl From<i64> for Number {
    fn from(value: i64) -> Self {
        Self {
            token: value.to_string(),
        }
    }
}
impl From<u64> for Number {
    fn from(value: u64) -> Self {
        Self {
            token: value.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(JsonString),
    Array(Vec<Value>),
    Object(Object),
}

impl Value {
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(JsonString::new(value))
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
    pub fn as_number(&self) -> Option<&Number> {
        match self {
            Self::Number(n) => Some(n),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }
}

/// Members are retained, including duplicates when explicitly allowed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Object {
    members: Vec<(JsonString, Value)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbiguousKey;

impl Object {
    /// Serialization validates size and duplicate policy on constructed objects.
    pub fn new(members: Vec<(JsonString, Value)>) -> Self {
        Self { members }
    }
    pub fn members(&self) -> &[(JsonString, Value)] {
        &self.members
    }
    pub fn into_members(self) -> Vec<(JsonString, Value)> {
        self.members
    }

    pub fn get_unique(&self, key: &str) -> Result<Option<&Value>, AmbiguousKey> {
        let mut found = None;
        for (name, value) in &self.members {
            if name.as_str() == key {
                if found.is_some() {
                    return Err(AmbiguousKey);
                }
                found = Some(value);
            }
        }
        Ok(found)
    }
}

#[derive(Debug)]
pub struct Document {
    source: String,
    root: Value,
    used: ExtensionsUsed,
}

impl Document {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn root(&self) -> &Value {
        &self.root
    }
    pub fn into_root(self) -> Value {
        self.root
    }
    pub fn extensions_used(&self) -> ExtensionsUsed {
        self.used
    }
}

/// Parse exactly one UTF-8 JSON value. Invalid UTF-8 is never repaired.
pub fn parse(input: &[u8], options: &ParseOptions) -> Result<Document, Error> {
    options
        .limits
        .validate()
        .map_err(|kind| Error::at(input, 0, kind))?;
    if input.len() > options.limits.input_bytes {
        return Err(Error::at(
            input,
            0,
            ErrorKind::LimitExceeded(LimitKind::InputBytes),
        ));
    }
    let text = std::str::from_utf8(input)
        .map_err(|e| Error::at(input, e.valid_up_to(), ErrorKind::InvalidUtf8))?;
    let mut parser = Parser {
        text,
        bytes: input,
        pos: 0,
        options: *options,
        nodes: 0,
        decoded: 0,
        used: ExtensionsUsed::default(),
    };
    if input.starts_with(&[0xef, 0xbb, 0xbf]) {
        if !options.extensions.utf8_bom {
            return Err(parser.error(ErrorKind::BomNotAllowed));
        }
        parser.pos = 3;
        parser.used.utf8_bom = true;
    }
    parser.whitespace()?;
    let root = parser.value(0)?;
    parser.whitespace()?;
    if parser.pos != input.len() {
        return Err(parser.error(ErrorKind::TrailingCharacters));
    }
    let source = copy_string(text).map_err(|kind| parser.error(kind))?;
    Ok(Document {
        source,
        root,
        used: parser.used,
    })
}

fn copy_string(text: &str) -> Result<String, ErrorKind> {
    let mut result = String::new();
    result
        .try_reserve(text.len())
        .map_err(|_| ErrorKind::AllocationFailed)?;
    result.push_str(text);
    Ok(result)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    options: ParseOptions,
    nodes: usize,
    decoded: usize,
    used: ExtensionsUsed,
}

impl Parser<'_> {
    fn error(&self, kind: ErrorKind) -> Error {
        Error::at(self.bytes, self.pos, kind)
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn node(&mut self) -> Result<(), Error> {
        if self.nodes == self.options.limits.nodes {
            return Err(self.error(ErrorKind::LimitExceeded(LimitKind::Nodes)));
        }
        self.nodes += 1;
        Ok(())
    }

    fn whitespace(&mut self) -> Result<(), Error> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.pos += 1,
                Some(b'/') if self.options.extensions.comments => {
                    match self.bytes.get(self.pos + 1) {
                        Some(b'/') => {
                            self.used.comments = true;
                            self.pos += 2;
                            while !matches!(self.peek(), None | Some(b'\r' | b'\n')) {
                                self.pos += 1;
                            }
                        }
                        Some(b'*') => {
                            self.used.comments = true;
                            self.pos += 2;
                            loop {
                                if self.bytes.get(self.pos..self.pos + 2) == Some(b"*/") {
                                    self.pos += 2;
                                    break;
                                }
                                if self.peek().is_none() {
                                    return Err(self.error(ErrorKind::UnterminatedComment));
                                }
                                self.pos += 1;
                            }
                        }
                        _ => return Ok(()),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        self.node()?;
        match self.peek() {
            Some(b'n') => {
                self.literal(b"null")?;
                Ok(Value::Null)
            }
            Some(b't') => {
                self.literal(b"true")?;
                Ok(Value::Bool(true))
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Ok(Value::Bool(false))
            }
            Some(b'"') => self.string().map(Value::String),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                scan_number(self.bytes, &mut self.pos, self.options.limits.number_bytes)
                    .map_err(|(kind, at)| Error::at(self.bytes, at, kind))?;
                let token =
                    copy_string(&self.text[start..self.pos]).map_err(|kind| self.error(kind))?;
                Ok(Value::Number(Number { token }))
            }
            Some(b'[' | b'{') => {
                if depth == self.options.limits.depth {
                    return Err(self.error(ErrorKind::LimitExceeded(LimitKind::Depth)));
                }
                if self.peek() == Some(b'[') {
                    self.array(depth + 1)
                } else {
                    self.object(depth + 1)
                }
            }
            None => Err(self.error(ErrorKind::UnexpectedEof)),
            _ => Err(self.error(ErrorKind::ExpectedValue)),
        }
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), Error> {
        for expected in literal {
            match self.peek() {
                Some(actual) if actual == *expected => self.pos += 1,
                None => return Err(self.error(ErrorKind::UnexpectedEof)),
                _ => return Err(self.error(ErrorKind::ExpectedValue)),
            }
        }
        Ok(())
    }

    fn append_decoded(&mut self, value: &mut String, text: &str) -> Result<(), Error> {
        if text.len() > self.options.limits.string_bytes.saturating_sub(value.len()) {
            return Err(self.error(ErrorKind::LimitExceeded(LimitKind::StringBytes)));
        }
        if text.len()
            > self
                .options
                .limits
                .total_decoded_bytes
                .saturating_sub(self.decoded)
        {
            return Err(self.error(ErrorKind::LimitExceeded(LimitKind::TotalDecodedBytes)));
        }
        value
            .try_reserve(text.len())
            .map_err(|_| self.error(ErrorKind::AllocationFailed))?;
        value.push_str(text);
        self.decoded += text.len();
        Ok(())
    }

    fn hex4(&mut self) -> Result<u16, Error> {
        let mut result = 0_u16;
        for _ in 0..4 {
            let digit = match self.peek() {
                Some(b'0'..=b'9') => self.bytes[self.pos] - b'0',
                Some(b'a'..=b'f') => self.bytes[self.pos] - b'a' + 10,
                Some(b'A'..=b'F') => self.bytes[self.pos] - b'A' + 10,
                _ => return Err(self.error(ErrorKind::InvalidUnicodeEscape)),
            };
            result = result * 16 + u16::from(digit);
            self.pos += 1;
        }
        Ok(result)
    }

    fn string(&mut self) -> Result<JsonString, Error> {
        let start = self.pos;
        self.pos += 1;
        let mut decoded = String::new();
        let mut chunk = self.pos;
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.append_decoded(&mut decoded, &self.text[chunk..self.pos])?;
                    self.pos += 1;
                    let token = copy_string(&self.text[start..self.pos])
                        .map_err(|kind| self.error(kind))?;
                    return Ok(JsonString {
                        decoded,
                        token: Some(token),
                    });
                }
                Some(b'\\') => {
                    self.append_decoded(&mut decoded, &self.text[chunk..self.pos])?;
                    let escape_offset = self.pos;
                    self.pos += 1;
                    let escaped = match self.peek() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{0008}',
                        Some(b'f') => '\u{000c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.pos += 1;
                            let first = self.hex4()?;
                            let scalar = match first {
                                0xd800..=0xdbff => {
                                    if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                                        return Err(Error::at(
                                            self.bytes,
                                            escape_offset,
                                            ErrorKind::UnpairedSurrogate,
                                        ));
                                    }
                                    self.pos += 2;
                                    let second = self.hex4()?;
                                    if !(0xdc00..=0xdfff).contains(&second) {
                                        return Err(Error::at(
                                            self.bytes,
                                            escape_offset,
                                            ErrorKind::UnpairedSurrogate,
                                        ));
                                    }
                                    0x10000
                                        + ((u32::from(first) - 0xd800) << 10)
                                        + u32::from(second)
                                        - 0xdc00
                                }
                                0xdc00..=0xdfff => {
                                    return Err(Error::at(
                                        self.bytes,
                                        escape_offset,
                                        ErrorKind::UnpairedSurrogate,
                                    ));
                                }
                                _ => u32::from(first),
                            };
                            let ch = char::from_u32(scalar)
                                .ok_or_else(|| self.error(ErrorKind::InvalidUnicodeEscape))?;
                            let mut utf8 = [0_u8; 4];
                            self.append_decoded(&mut decoded, ch.encode_utf8(&mut utf8))?;
                            chunk = self.pos;
                            continue;
                        }
                        _ => return Err(self.error(ErrorKind::InvalidEscape)),
                    };
                    self.pos += 1;
                    let mut utf8 = [0_u8; 4];
                    self.append_decoded(&mut decoded, escaped.encode_utf8(&mut utf8))?;
                    chunk = self.pos;
                }
                Some(0..=0x1f) => return Err(self.error(ErrorKind::UnescapedControl)),
                Some(_) => self.pos += 1,
                None => return Err(self.error(ErrorKind::UnterminatedString)),
            }
        }
    }

    fn entry(&self, count: usize) -> Result<(), Error> {
        if count >= self.options.limits.container_entries {
            Err(self.error(ErrorKind::LimitExceeded(LimitKind::ContainerEntries)))
        } else {
            Ok(())
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, Error> {
        self.pos += 1;
        self.whitespace()?;
        let mut values = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(values));
        }
        loop {
            self.entry(values.len())?;
            let value = self.value(depth)?;
            values
                .try_reserve(1)
                .map_err(|_| self.error(ErrorKind::AllocationFailed))?;
            values.push(value);
            if self.container_end(b']')? {
                break;
            }
        }
        Ok(Value::Array(values))
    }

    fn object(&mut self, depth: usize) -> Result<Value, Error> {
        self.pos += 1;
        self.whitespace()?;
        let mut members = Vec::new();
        let mut seen = BTreeSet::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(Object::new(members)));
        }
        loop {
            self.entry(members.len())?;
            if self.peek() != Some(b'"') {
                return Err(self.error(ErrorKind::ExpectedObjectKey));
            }
            self.node()?;
            let key_offset = self.pos;
            let key = self.string()?;
            if !seen.insert(copy_string(key.as_str()).map_err(|kind| self.error(kind))?) {
                self.used.duplicate_keys = true;
                if self.options.duplicates == DuplicatePolicy::Reject {
                    return Err(Error::at(self.bytes, key_offset, ErrorKind::DuplicateKey));
                }
            }
            self.whitespace()?;
            if self.peek() != Some(b':') {
                return Err(self.error(ErrorKind::ExpectedColon));
            }
            self.pos += 1;
            self.whitespace()?;
            let value = self.value(depth)?;
            members
                .try_reserve(1)
                .map_err(|_| self.error(ErrorKind::AllocationFailed))?;
            members.push((key, value));
            if self.container_end(b'}')? {
                break;
            }
        }
        Ok(Value::Object(Object::new(members)))
    }

    fn container_end(&mut self, end: u8) -> Result<bool, Error> {
        self.whitespace()?;
        if self.peek() == Some(end) {
            self.pos += 1;
            return Ok(true);
        }
        if self.peek() != Some(b',') {
            return Err(self.error(if self.peek().is_none() {
                ErrorKind::UnexpectedEof
            } else {
                ErrorKind::ExpectedCommaOrEnd
            }));
        }
        self.pos += 1;
        self.whitespace()?;
        if self.peek() == Some(end) && self.options.extensions.trailing_commas {
            self.used.trailing_commas = true;
            self.pos += 1;
            return Ok(true);
        }
        Ok(false)
    }
}

fn scan_number(bytes: &[u8], pos: &mut usize, limit: usize) -> Result<(), (ErrorKind, usize)> {
    let start = *pos;
    if bytes.get(*pos) == Some(&b'-') {
        *pos += 1;
    }
    match bytes.get(*pos) {
        Some(b'0') => {
            *pos += 1;
            if matches!(bytes.get(*pos), Some(b'0'..=b'9')) {
                return Err((ErrorKind::InvalidNumber, *pos));
            }
        }
        Some(b'1'..=b'9') => {
            *pos += 1;
            while matches!(bytes.get(*pos), Some(b'0'..=b'9')) {
                *pos += 1;
            }
        }
        _ => return Err((ErrorKind::InvalidNumber, *pos)),
    }
    if bytes.get(*pos) == Some(&b'.') {
        *pos += 1;
        let digits = *pos;
        while matches!(bytes.get(*pos), Some(b'0'..=b'9')) {
            *pos += 1;
        }
        if *pos == digits {
            return Err((ErrorKind::InvalidNumber, *pos));
        }
    }
    if matches!(bytes.get(*pos), Some(b'e' | b'E')) {
        *pos += 1;
        if matches!(bytes.get(*pos), Some(b'+' | b'-')) {
            *pos += 1;
        }
        let digits = *pos;
        while matches!(bytes.get(*pos), Some(b'0'..=b'9')) {
            *pos += 1;
        }
        if *pos == digits {
            return Err((ErrorKind::InvalidNumber, *pos));
        }
    }
    if *pos - start > limit {
        Err((
            ErrorKind::LimitExceeded(LimitKind::NumberBytes),
            start + limit,
        ))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Compact,
    /// One to eight ASCII spaces per level, LF newlines, no terminal newline.
    Pretty { indent: u8 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StringEncoding {
    #[default]
    Preserve,
    Minimal,
    Ascii,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SerializeOptions {
    pub limits: Limits,
    pub duplicates: DuplicatePolicy,
    pub layout: Layout,
    pub strings: StringEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeError {
    pub kind: ErrorKind,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}
impl std::error::Error for EncodeError {}
impl From<ErrorKind> for EncodeError {
    fn from(kind: ErrorKind) -> Self {
        Self { kind }
    }
}

/// Fresh deterministic JSON, never a sorted/canonical signing representation.
/// The engine does not omit nulls, rename keys, or normalize numeric spelling.
/// Keep this returned buffer separate until complete; an error returns no prefix.
pub fn serialize(value: &Value, options: &SerializeOptions) -> Result<String, EncodeError> {
    options.limits.validate()?;
    if matches!(
        options.layout,
        Layout::Pretty {
            indent: 0 | 9..=u8::MAX
        }
    ) {
        return Err(ErrorKind::InvalidOptions.into());
    }
    let mut encoder = Encoder {
        out: String::new(),
        options: *options,
        nodes: 0,
        decoded: 0,
    };
    encoder.value(value, 0)?;
    Ok(encoder.out)
}

struct Encoder {
    out: String,
    options: SerializeOptions,
    nodes: usize,
    decoded: usize,
}

impl Encoder {
    fn push(&mut self, text: &str) -> Result<(), EncodeError> {
        if text.len()
            > self
                .options
                .limits
                .output_bytes
                .saturating_sub(self.out.len())
        {
            return Err(ErrorKind::LimitExceeded(LimitKind::OutputBytes).into());
        }
        self.out
            .try_reserve(text.len())
            .map_err(|_| ErrorKind::AllocationFailed)?;
        self.out.push_str(text);
        Ok(())
    }

    fn node(&mut self) -> Result<(), EncodeError> {
        if self.nodes == self.options.limits.nodes {
            return Err(ErrorKind::LimitExceeded(LimitKind::Nodes).into());
        }
        self.nodes += 1;
        Ok(())
    }

    fn value(&mut self, value: &Value, depth: usize) -> Result<(), EncodeError> {
        self.node()?;
        match value {
            Value::Null => self.push("null"),
            Value::Bool(true) => self.push("true"),
            Value::Bool(false) => self.push("false"),
            Value::String(s) => self.string(s),
            Value::Number(n) => {
                if n.as_str().len() > self.options.limits.number_bytes {
                    return Err(ErrorKind::LimitExceeded(LimitKind::NumberBytes).into());
                }
                self.push(n.as_str())
            }
            Value::Array(values) => {
                self.container(depth, values.len())?;
                self.push("[")?;
                for (index, item) in values.iter().enumerate() {
                    if index != 0 {
                        self.push(",")?;
                    }
                    self.newline(depth + 1)?;
                    self.value(item, depth + 1)?;
                }
                if !values.is_empty() {
                    self.newline(depth)?;
                }
                self.push("]")
            }
            Value::Object(object) => {
                self.container(depth, object.members.len())?;
                let mut seen = BTreeSet::new();
                self.push("{")?;
                for (index, (key, item)) in object.members.iter().enumerate() {
                    if self.options.duplicates == DuplicatePolicy::Reject
                        && !seen.insert(key.as_str())
                    {
                        return Err(ErrorKind::DuplicateKey.into());
                    }
                    if index != 0 {
                        self.push(",")?;
                    }
                    self.newline(depth + 1)?;
                    self.node()?;
                    self.string(key)?;
                    self.push(if self.options.layout == Layout::Compact {
                        ":"
                    } else {
                        ": "
                    })?;
                    self.value(item, depth + 1)?;
                }
                if !object.members.is_empty() {
                    self.newline(depth)?;
                }
                self.push("}")
            }
        }
    }

    fn container(&self, depth: usize, entries: usize) -> Result<(), EncodeError> {
        if depth == self.options.limits.depth {
            return Err(ErrorKind::LimitExceeded(LimitKind::Depth).into());
        }
        if entries > self.options.limits.container_entries {
            return Err(ErrorKind::LimitExceeded(LimitKind::ContainerEntries).into());
        }
        Ok(())
    }

    fn newline(&mut self, depth: usize) -> Result<(), EncodeError> {
        if let Layout::Pretty { indent } = self.options.layout {
            self.push("\n")?;
            for _ in 0..depth {
                self.push(&"        "[..usize::from(indent)])?;
            }
        }
        Ok(())
    }

    fn string(&mut self, string: &JsonString) -> Result<(), EncodeError> {
        let text = string.as_str();
        if text.len() > self.options.limits.string_bytes {
            return Err(ErrorKind::LimitExceeded(LimitKind::StringBytes).into());
        }
        if text.len()
            > self
                .options
                .limits
                .total_decoded_bytes
                .saturating_sub(self.decoded)
        {
            return Err(ErrorKind::LimitExceeded(LimitKind::TotalDecodedBytes).into());
        }
        self.decoded += text.len();
        if self.options.strings == StringEncoding::Preserve
            && let Some(token) = string.original_token()
        {
            return self.push(token);
        }
        self.push("\"")?;
        let mut chunk = 0;
        for (at, ch) in text.char_indices() {
            let short = match ch {
                '"' => Some("\\\""),
                '\\' => Some("\\\\"),
                '\u{0008}' => Some("\\b"),
                '\u{000c}' => Some("\\f"),
                '\n' => Some("\\n"),
                '\r' => Some("\\r"),
                '\t' => Some("\\t"),
                _ => None,
            };
            let hex = ch < '\u{0020}'
                || (self.options.strings == StringEncoding::Ascii && !ch.is_ascii());
            if short.is_none() && !hex {
                continue;
            }
            self.push(&text[chunk..at])?;
            if let Some(escaped) = short {
                self.push(escaped)?;
            } else {
                let mut utf16 = [0_u16; 2];
                for unit in ch.encode_utf16(&mut utf16) {
                    self.hex_escape(*unit)?;
                }
            }
            chunk = at + ch.len_utf8();
        }
        self.push(&text[chunk..])?;
        self.push("\"")
    }

    fn hex_escape(&mut self, unit: u16) -> Result<(), EncodeError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bytes = [
            b'\\',
            b'u',
            HEX[usize::from(unit >> 12)],
            HEX[usize::from((unit >> 8) & 15)],
            HEX[usize::from((unit >> 4) & 15)],
            HEX[usize::from(unit & 15)],
        ];
        // All six bytes come from ASCII constants, so this cannot fail.
        let escaped = std::str::from_utf8(&bytes).map_err(|_| ErrorKind::InvalidUtf8)?;
        self.push(escaped)
    }
}
