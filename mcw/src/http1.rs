//! Bounded HTTP/1 client wire formatting and incremental response framing.
//!
//! First-party implementation from RFC 9110 and RFC 9112, not a port of a
//! third-party parser. This module performs no I/O, TLS, decompression, URI
//! resolution, retries, authentication, logging, or managed/IPC operations.
//! Content-Encoding bytes remain opaque. A framing error poisons the decoder:
//! discard the exchange and close its transport rather than trying to resync.
#![forbid(unsafe_code)]

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

impl Version {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Http10 => b"HTTP/1.0",
            Self::Http11 => b"HTTP/1.1",
        }
    }
}

/// All sizes are octets. Line limits exclude CRLF; head/trailer/wire limits
/// include it. Zero body/field/informational/chunk limits are useful policies.
/// Absolute ceilings prevent configuration from enabling unbounded resources.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_line_bytes: usize,
    pub max_head_bytes: usize,
    pub max_fields: usize,
    pub max_body_bytes: usize,
    pub max_trailer_bytes: usize,
    pub max_trailers: usize,
    pub max_informational: usize,
    pub max_chunk_line_bytes: usize,
    pub max_chunks: usize,
    pub max_wire_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_line_bytes: 8192,
            max_head_bytes: 65536,
            max_fields: 128,
            max_body_bytes: 16 * 1024 * 1024,
            max_trailer_bytes: 16384,
            max_trailers: 64,
            max_informational: 8,
            max_chunk_line_bytes: 1024,
            max_chunks: 65536,
            max_wire_bytes: 64 * 1024 * 1024,
        }
    }
}

impl Limits {
    pub fn validate(self) -> Result<Self, Error> {
        if self.max_line_bytes == 0
            || self.max_line_bytes > 65536
            || self.max_head_bytes == 0
            || self.max_head_bytes > 1024 * 1024
            || self.max_fields > 4096
            || self.max_body_bytes > 256 * 1024 * 1024
            || self.max_trailer_bytes > 1024 * 1024
            || self.max_trailers > 4096
            || self.max_informational > 64
            || self.max_chunk_line_bytes == 0
            || self.max_chunk_line_bytes > 65536
            || self.max_chunks > 1_000_000
            || self.max_wire_bytes == 0
            || self.max_wire_bytes > 1024 * 1024 * 1024
        {
            return Err(Error::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Line,
    Head,
    Fields,
    Body,
    Trailers,
    TrailerFields,
    Informational,
    ChunkLine,
    Chunks,
    Wire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidLimits,
    Limit(Resource),
    Allocation,
    InvalidMethod,
    InvalidTarget,
    InvalidHost,
    HostMismatch,
    InvalidStatus,
    UnsupportedVersion,
    InvalidCrlf,
    InvalidHeader,
    ObsoleteFold,
    InvalidContentLength,
    ConflictingContentLength,
    AmbiguousFraming,
    UnsupportedTransferEncoding,
    InvalidConnection,
    InvalidUpgrade,
    InvalidChunk,
    InvalidTrailer,
    BodyLengthMismatch,
    UnexpectedBody,
    Truncated,
    Poisoned,
    Incomplete,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Safe classifications only: never include peer bytes or wallet data.
        write!(f, "HTTP/1 wire error: {self:?}")
    }
}

impl std::error::Error for Error {}

/// Original name case and the exact bytes after ':' (including OWS) are kept.
/// Repeated fields remain separate and ordered; Set-Cookie is never combined.
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

impl Header {
    pub fn is(&self, name: &[u8]) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    pub fn trimmed_value(&self) -> &[u8] {
        trim_ows(&self.value)
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("name_octets", &self.name.len())
            .field("value_octets", &self.value.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ResponseHead {
    pub version: Version,
    pub status: u16,
    pub reason: Vec<u8>,
    pub headers: Vec<Header>,
}

impl fmt::Debug for ResponseHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseHead")
            .field("version", &self.version)
            .field("status", &self.status)
            .field("reason_octets", &self.reason.len())
            .field("fields", &self.headers.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    NoBody,
    ContentLength(u64),
    Chunked,
    UntilClose,
    Upgrade,
    Tunnel,
}

/// Reusable is a wire-level eligibility result, not proof that a transport is
/// alive, secure, fully written, or safe for another identity. Host policy may
/// always close it. Upgrade/Tunnel detach from HTTP at the empty header line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionUse {
    Reusable,
    MustClose,
    Upgrade,
    Tunnel,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    pub informational: Vec<ResponseHead>,
    pub head: ResponseHead,
    pub body: Vec<u8>,
    pub trailers: Vec<Header>,
    pub framing: Framing,
    pub connection: ConnectionUse,
    pub wire_bytes: u64,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.head.status)
            .field("body_octets", &self.body.len())
            .field("framing", &self.framing)
            .field("connection", &self.connection)
            .finish()
    }
}

/// Validated request semantics needed to associate a response with its request.
/// Methods are case-sensitive: "head" is an extension method, not HEAD.
#[derive(Debug, Clone)]
pub struct RequestContext {
    method: Method,
    version: Version,
    close: bool,
    keep_alive: bool,
    upgrades: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Head,
    Connect,
    Other,
}

impl RequestContext {
    /// For adapters that already own request serialization. Validate the whole
    /// request with serialize_request when using this module as the encoder.
    pub fn new(
        method: &[u8],
        version: Version,
        headers: &[Header],
        limits: Limits,
    ) -> Result<Self, Error> {
        let limits = limits.validate()?;
        validate_method(method, limits)?;
        validate_headers(headers, limits, false)?;
        let flags = connection_flags(headers)?;
        validate_request_te(headers, version)?;
        let upgrades = protocols(headers)?;
        if !upgrades.is_empty() && (!flags.upgrade || version != Version::Http11 || flags.close) {
            return Err(Error::InvalidUpgrade);
        }
        if flags.upgrade && upgrades.is_empty() {
            return Err(Error::InvalidUpgrade);
        }
        Ok(Self {
            method: match method {
                b"HEAD" => Method::Head,
                b"CONNECT" => Method::Connect,
                _ => Method::Other,
            },
            version,
            close: flags.close,
            keep_alive: flags.keep_alive,
            upgrades,
        })
    }
}

pub enum RequestBody<'a> {
    /// Omit a framing field unless the caller supplied Content-Length: 0.
    Empty,
    /// Emit exactly one canonical Content-Length, even for an empty slice.
    Bytes(&'a [u8]),
    /// Emit chunked framing and a generated Trailer declaration. Empty slices
    /// are skipped; only the generated last chunk terminates the body.
    Chunked {
        chunks: &'a [&'a [u8]],
        trailers: &'a [Header],
    },
}

pub struct Request<'a> {
    pub version: Version,
    pub method: &'a [u8],
    pub target: &'a [u8],
    pub headers: &'a [Header],
    pub body: RequestBody<'a>,
}

pub struct EncodedRequest {
    pub bytes: Vec<u8>,
    pub context: RequestContext,
    /// Split here for Expect: 100-continue; send body only after host policy
    /// permits it. The encoder itself never sends or automatically replays.
    pub head_bytes: usize,
}

impl fmt::Debug for EncodedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncodedRequest")
            .field("wire_octets", &self.bytes.len())
            .field("head_octets", &self.head_bytes)
            .finish()
    }
}

/// Complete, bounded serialization. All validation precedes returned bytes;
/// errors never return a partial request. User framing fields must agree with
/// the supplied body, and are emitted once in canonical form. Trailer is owned
/// by the encoder. Other header names/values and all body octets are preserved.
pub fn serialize_request(request: &Request<'_>, limits: Limits) -> Result<EncodedRequest, Error> {
    let limits = limits.validate()?;
    // CONNECT bytes after its head belong to the tunnel protocol, so emitting
    // even a zero-chunk body here would corrupt that boundary. TRACE forbids
    // content; reject chunked requests for both methods in this strict profile.
    if matches!(request.method, b"CONNECT" | b"TRACE")
        && !matches!(&request.body, RequestBody::Empty | RequestBody::Bytes([]))
    {
        return Err(Error::UnexpectedBody);
    }
    if request.method == b"TRACE"
        && request
            .headers
            .iter()
            .any(|h| h.is(b"authorization") || h.is(b"proxy-authorization") || h.is(b"cookie"))
    {
        return Err(Error::InvalidHeader);
    }
    let context = RequestContext::new(request.method, request.version, request.headers, limits)?;
    let host = request_host(request.headers, request.version)?;
    validate_target(request.method, request.target, host, limits)?;
    let cl_headers = request
        .headers
        .iter()
        .filter(|h| h.is(b"content-length"))
        .count();
    if cl_headers > 1 {
        return Err(Error::ConflictingContentLength);
    }
    let supplied_length = content_length(request.headers)?;
    if request.headers.iter().any(|h| h.is(b"trailer")) {
        return Err(Error::InvalidTrailer);
    }
    let supplied_chunked = transfer_encoding(request.headers, request.version)?;
    if supplied_length.is_some() && supplied_chunked {
        return Err(Error::AmbiguousFraming);
    }
    let mut out = Vec::new();
    let mut start = Vec::new();
    append(
        &mut start,
        request.method,
        limits.max_line_bytes,
        Resource::Line,
    )?;
    append(&mut start, b" ", limits.max_line_bytes, Resource::Line)?;
    append(
        &mut start,
        request.target,
        limits.max_line_bytes,
        Resource::Line,
    )?;
    append(&mut start, b" ", limits.max_line_bytes, Resource::Line)?;
    append(
        &mut start,
        request.version.as_bytes(),
        limits.max_line_bytes,
        Resource::Line,
    )?;
    append_wire(&mut out, &start, limits)?;
    append_wire(&mut out, b"\r\n", limits)?;
    let mut field_count = 0;
    for header in request.headers {
        if header.is(b"content-length") || header.is(b"transfer-encoding") {
            continue;
        }
        write_header(&mut out, header, limits, false)?;
        field_count += 1;
    }
    let (body_len, chunks, trailers) = match &request.body {
        RequestBody::Empty => {
            if supplied_chunked {
                return Err(Error::BodyLengthMismatch);
            }
            if supplied_length.is_some_and(|n| n != 0) {
                return Err(Error::BodyLengthMismatch);
            }
            if supplied_length.is_some() {
                write_generated_header(&mut out, b"Content-Length", b"0", limits)?;
                field_count += 1;
            }
            (0, None, &[][..])
        }
        RequestBody::Bytes(bytes) => {
            if supplied_chunked || supplied_length.is_some_and(|n| n != bytes.len() as u64) {
                return Err(Error::BodyLengthMismatch);
            }
            if bytes.len() > limits.max_body_bytes {
                return Err(Error::Limit(Resource::Body));
            }
            write_generated_header(
                &mut out,
                b"Content-Length",
                bytes.len().to_string().as_bytes(),
                limits,
            )?;
            field_count += 1;
            (bytes.len(), None, &[][..])
        }
        RequestBody::Chunked { chunks, trailers } => {
            if request.version != Version::Http11 {
                return Err(Error::UnsupportedTransferEncoding);
            }
            if supplied_length.is_some() {
                return Err(Error::AmbiguousFraming);
            }
            // Bound even skipped zero-length entries, which otherwise consume CPU.
            if chunks
                .len()
                .checked_add(1)
                .is_none_or(|n| n > limits.max_chunks)
            {
                return Err(Error::Limit(Resource::Chunks));
            }
            validate_headers(trailers, limits, true)?;
            let mut length = 0usize;
            for chunk in *chunks {
                length = length
                    .checked_add(chunk.len())
                    .ok_or(Error::Limit(Resource::Body))?;
                if length > limits.max_body_bytes {
                    return Err(Error::Limit(Resource::Body));
                }
            }
            for trailer in *trailers {
                if forbidden_trailer(&trailer.name)
                    || connection_nominates(request.headers, &trailer.name)
                {
                    return Err(Error::InvalidTrailer);
                }
            }
            write_generated_header(&mut out, b"Transfer-Encoding", b"chunked", limits)?;
            field_count += 1;
            if !trailers.is_empty() {
                let mut names = Vec::new();
                for (i, trailer) in trailers.iter().enumerate() {
                    if i != 0 {
                        append(&mut names, b", ", limits.max_line_bytes, Resource::Line)?;
                    }
                    append(
                        &mut names,
                        &trailer.name,
                        limits.max_line_bytes,
                        Resource::Line,
                    )?;
                }
                write_generated_header(&mut out, b"Trailer", &names, limits)?;
                field_count += 1;
            }
            (length, Some(*chunks), *trailers)
        }
    };
    if field_count > limits.max_fields {
        return Err(Error::Limit(Resource::Fields));
    }
    append_wire(&mut out, b"\r\n", limits)?;
    if out.len() > limits.max_head_bytes {
        return Err(Error::Limit(Resource::Head));
    }
    let head_bytes = out.len();
    if let Some(chunks) = chunks {
        for chunk in chunks {
            if chunk.is_empty() {
                continue;
            }
            let size = format!("{:x}", chunk.len());
            if size.len() > limits.max_chunk_line_bytes {
                return Err(Error::Limit(Resource::ChunkLine));
            }
            append_wire(&mut out, size.as_bytes(), limits)?;
            append_wire(&mut out, b"\r\n", limits)?;
            append_wire(&mut out, chunk, limits)?;
            append_wire(&mut out, b"\r\n", limits)?;
        }
        append_wire(&mut out, b"0\r\n", limits)?;
        let trailer_start = out.len();
        for trailer in trailers {
            write_header(&mut out, trailer, limits, true)?;
        }
        append_wire(&mut out, b"\r\n", limits)?;
        if out.len() - trailer_start > limits.max_trailer_bytes {
            return Err(Error::Limit(Resource::Trailers));
        }
    } else if let RequestBody::Bytes(bytes) = &request.body {
        append_wire(&mut out, bytes, limits)?;
    }
    debug_assert!(body_len <= limits.max_body_bytes);
    Ok(EncodedRequest {
        bytes: out,
        context,
        head_bytes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeStatus {
    NeedMore,
    /// Stops exactly after one informational head, enabling Expect/Continue.
    Informational,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedResult {
    pub consumed: usize,
    pub status: DecodeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Status,
    Headers,
    Length(u64),
    Close,
    ChunkSize,
    ChunkData(u64),
    ChunkCr,
    ChunkLf,
    Trailers,
    Complete,
    Failed,
}

/// One request's response sequence. No caller-owned input buffer is retained.
/// Partial bodies are never exposed as completed responses. Input following a
/// completed response (including tunnel/protocol bytes) is not consumed.
pub struct ResponseDecoder {
    limits: Limits,
    context: RequestContext,
    state: State,
    line: Vec<u8>,
    pending_cr: bool,
    head_bytes: usize,
    trailer_bytes: usize,
    chunks: usize,
    wire_bytes: u64,
    current_head: Option<ResponseHead>,
    informational: Vec<ResponseHead>,
    message: Option<Response>,
    saw_close: bool,
}

impl ResponseDecoder {
    pub fn new(context: RequestContext, limits: Limits) -> Result<Self, Error> {
        Ok(Self {
            limits: limits.validate()?,
            context,
            state: State::Status,
            line: Vec::new(),
            pending_cr: false,
            head_bytes: 0,
            trailer_bytes: 0,
            chunks: 0,
            wire_bytes: 0,
            current_head: None,
            informational: Vec::new(),
            message: None,
            saw_close: false,
        })
    }

    pub fn informational(&self) -> &[ResponseHead] {
        self.message
            .as_ref()
            .map_or(&self.informational, |m| &m.informational)
    }

    pub fn response(&self) -> Option<&Response> {
        if self.state == State::Complete {
            self.message.as_ref()
        } else {
            None
        }
    }

    pub fn into_response(self) -> Result<Response, Error> {
        if self.state == State::Failed {
            return Err(Error::Poisoned);
        }
        if self.state != State::Complete {
            return Err(Error::Incomplete);
        }
        self.message.ok_or(Error::Incomplete)
    }

    pub fn feed(&mut self, input: &[u8]) -> Result<FeedResult, Error> {
        if self.state == State::Failed {
            return Err(Error::Poisoned);
        }
        let result = self.feed_inner(input);
        if result.is_err() {
            self.poison();
        }
        result
    }

    /// Call ONLY for a verified orderly EOF from the transport. Timeouts,
    /// cancellation, resets and TLS truncation must call abort() instead.
    /// Until-close framing cannot itself prove representation integrity.
    pub fn finish(&mut self) -> Result<(), Error> {
        match self.state {
            State::Failed => Err(Error::Poisoned),
            State::Complete => {
                if let Some(message) = &mut self.message
                    && message.connection == ConnectionUse::Reusable
                {
                    message.connection = ConnectionUse::MustClose;
                }
                Ok(())
            }
            State::Close => {
                self.complete();
                Ok(())
            }
            _ => {
                self.poison();
                Err(Error::Truncated)
            }
        }
    }

    /// Fail an exchange on a non-orderly transport end; no response survives.
    pub fn abort(&mut self) {
        self.poison();
    }

    fn poison(&mut self) {
        self.state = State::Failed;
        self.message = None;
        self.current_head = None;
        self.informational.clear();
        self.line.clear();
    }

    fn complete(&mut self) {
        self.state = State::Complete;
        if let Some(message) = &mut self.message {
            message.wire_bytes = self.wire_bytes;
        }
    }

    fn charge(&mut self, n: usize) -> Result<(), Error> {
        self.wire_bytes = self
            .wire_bytes
            .checked_add(n as u64)
            .ok_or(Error::Limit(Resource::Wire))?;
        if self.wire_bytes > self.limits.max_wire_bytes {
            return Err(Error::Limit(Resource::Wire));
        }
        Ok(())
    }

    fn append_body(&mut self, data: &[u8]) -> Result<(), Error> {
        let message = self.message.as_mut().ok_or(Error::Incomplete)?;
        if message.head.status == 205 && !data.is_empty() {
            return Err(Error::UnexpectedBody);
        }
        append(
            &mut message.body,
            data,
            self.limits.max_body_bytes,
            Resource::Body,
        )
    }

    fn feed_inner(&mut self, input: &[u8]) -> Result<FeedResult, Error> {
        let mut consumed = 0;
        while consumed < input.len() && self.state != State::Complete {
            match self.state {
                State::Length(remaining) | State::ChunkData(remaining) => {
                    let n = (input.len() - consumed)
                        .min(usize::try_from(remaining).unwrap_or(usize::MAX));
                    self.charge(n)?;
                    self.append_body(&input[consumed..consumed + n])?;
                    consumed += n;
                    let remaining = remaining - n as u64;
                    match self.state {
                        State::Length(_) if remaining == 0 => self.complete(),
                        State::Length(_) => self.state = State::Length(remaining),
                        _ if remaining == 0 => self.state = State::ChunkCr,
                        _ => self.state = State::ChunkData(remaining),
                    }
                }
                State::Close => {
                    let data = &input[consumed..];
                    self.charge(data.len())?;
                    self.append_body(data)?;
                    consumed = input.len();
                }
                State::ChunkCr | State::ChunkLf => {
                    let expected = if self.state == State::ChunkCr {
                        b'\r'
                    } else {
                        b'\n'
                    };
                    self.charge(1)?;
                    if input[consumed] != expected {
                        return Err(Error::InvalidCrlf);
                    }
                    consumed += 1;
                    self.state = if self.state == State::ChunkCr {
                        State::ChunkLf
                    } else {
                        State::ChunkSize
                    };
                }
                State::Status | State::Headers | State::ChunkSize | State::Trailers => {
                    self.charge(1)?;
                    if matches!(self.state, State::Status | State::Headers) {
                        self.head_bytes += 1;
                        if self.head_bytes > self.limits.max_head_bytes {
                            return Err(Error::Limit(Resource::Head));
                        }
                    } else if self.state == State::Trailers {
                        self.trailer_bytes += 1;
                        if self.trailer_bytes > self.limits.max_trailer_bytes {
                            return Err(Error::Limit(Resource::Trailers));
                        }
                    }
                    let byte = input[consumed];
                    consumed += 1;
                    if self.line_byte(byte)? {
                        let line = std::mem::take(&mut self.line);
                        let info = self.process_line(&line)?;
                        if info {
                            return Ok(FeedResult {
                                consumed,
                                status: DecodeStatus::Informational,
                            });
                        }
                    }
                }
                State::Complete => break,
                State::Failed => return Err(Error::Poisoned),
            }
        }
        Ok(FeedResult {
            consumed,
            status: if self.state == State::Complete {
                DecodeStatus::Complete
            } else {
                DecodeStatus::NeedMore
            },
        })
    }

    fn line_byte(&mut self, byte: u8) -> Result<bool, Error> {
        if self.pending_cr {
            if byte != b'\n' {
                return Err(Error::InvalidCrlf);
            }
            self.pending_cr = false;
            return Ok(true);
        }
        match byte {
            b'\r' => self.pending_cr = true,
            b'\n' => return Err(Error::InvalidCrlf),
            _ => {
                let (limit, resource) = if self.state == State::ChunkSize {
                    (self.limits.max_chunk_line_bytes, Resource::ChunkLine)
                } else {
                    (self.limits.max_line_bytes, Resource::Line)
                };
                append(&mut self.line, &[byte], limit, resource)?;
            }
        }
        Ok(false)
    }

    fn process_line(&mut self, line: &[u8]) -> Result<bool, Error> {
        match self.state {
            State::Status => {
                self.current_head = Some(parse_status(line)?);
                self.state = State::Headers;
            }
            State::Headers if line.is_empty() => return self.finish_head(),
            State::Headers => {
                let head = self.current_head.as_mut().ok_or(Error::Incomplete)?;
                if head.headers.len() >= self.limits.max_fields {
                    return Err(Error::Limit(Resource::Fields));
                }
                let field = parse_header(line)?;
                head.headers.try_reserve(1).map_err(|_| Error::Allocation)?;
                head.headers.push(field);
            }
            State::ChunkSize => {
                self.chunks += 1;
                if self.chunks > self.limits.max_chunks {
                    return Err(Error::Limit(Resource::Chunks));
                }
                let size = chunk_size(line)?;
                let message = self.message.as_ref().ok_or(Error::Incomplete)?;
                if size
                    > self
                        .limits
                        .max_body_bytes
                        .saturating_sub(message.body.len()) as u64
                {
                    return Err(Error::Limit(Resource::Body));
                }
                if message.head.status == 205 && size != 0 {
                    return Err(Error::UnexpectedBody);
                }
                self.state = if size == 0 {
                    State::Trailers
                } else {
                    State::ChunkData(size)
                };
            }
            State::Trailers if line.is_empty() => self.complete(),
            State::Trailers => {
                let message = self.message.as_mut().ok_or(Error::Incomplete)?;
                if message.trailers.len() >= self.limits.max_trailers {
                    return Err(Error::Limit(Resource::TrailerFields));
                }
                let field = parse_header(line)?;
                if forbidden_trailer(&field.name)
                    || connection_nominates(&message.head.headers, &field.name)
                {
                    return Err(Error::InvalidTrailer);
                }
                message
                    .trailers
                    .try_reserve(1)
                    .map_err(|_| Error::Allocation)?;
                message.trailers.push(field);
            }
            _ => return Err(Error::Incomplete),
        }
        Ok(false)
    }

    fn finish_head(&mut self) -> Result<bool, Error> {
        let head = self.current_head.take().ok_or(Error::Incomplete)?;
        let flags = connection_flags(&head.headers)?;
        self.saw_close |= flags.close;
        // RFC 9112 6.3 precedence: successful CONNECT ignores framing fields,
        // even malformed values or TE+CL. These bytes describe no HTTP body.
        let tunnel = self.context.method == Method::Connect && (200..300).contains(&head.status);
        let (length, chunked) = if tunnel {
            (None, false)
        } else {
            let length = content_length(&head.headers)?;
            let chunked = transfer_encoding(&head.headers, head.version)?;
            if length.is_some() && chunked {
                return Err(Error::AmbiguousFraming);
            }
            if (head.status < 200 || head.status == 204) && (length.is_some() || chunked) {
                return Err(Error::AmbiguousFraming);
            }
            (length, chunked)
        };
        validate_trailer_declaration(&head.headers)?;
        if head.status < 200 && head.status != 101 {
            if head.version != Version::Http11 {
                return Err(Error::InvalidStatus);
            }
            if self.informational.len() >= self.limits.max_informational {
                return Err(Error::Limit(Resource::Informational));
            }
            self.informational
                .try_reserve(1)
                .map_err(|_| Error::Allocation)?;
            self.informational.push(head);
            self.head_bytes = 0;
            self.state = State::Status;
            return Ok(true);
        }
        let framing = if head.status == 101 {
            let selected = protocols(&head.headers)?;
            if head.version != Version::Http11
                || self.context.version != Version::Http11
                || !flags.upgrade
                || self.saw_close
                || self.context.close
                || selected.is_empty()
                || selected
                    .iter()
                    .any(|p| !self.context.upgrades.iter().any(|q| same_protocol(p, q)))
            {
                return Err(Error::InvalidUpgrade);
            }
            Framing::Upgrade
        } else if tunnel {
            Framing::Tunnel
        } else if self.context.method == Method::Head || head.status == 204 || head.status == 304 {
            Framing::NoBody
        } else if chunked {
            Framing::Chunked
        } else if let Some(n) = length {
            if n > self.limits.max_body_bytes as u64 {
                return Err(Error::Limit(Resource::Body));
            }
            if head.status == 205 && n != 0 {
                return Err(Error::UnexpectedBody);
            }
            Framing::ContentLength(n)
        } else {
            Framing::UntilClose
        };
        let connection = match framing {
            Framing::Upgrade => ConnectionUse::Upgrade,
            Framing::Tunnel => ConnectionUse::Tunnel,
            Framing::UntilClose => ConnectionUse::MustClose,
            _ if self.context.close
                || self.saw_close
                || (self.context.version == Version::Http10 && !self.context.keep_alive)
                || (head.version == Version::Http10 && !flags.keep_alive) =>
            {
                ConnectionUse::MustClose
            }
            _ => ConnectionUse::Reusable,
        };
        self.message = Some(Response {
            informational: std::mem::take(&mut self.informational),
            head,
            body: Vec::new(),
            trailers: Vec::new(),
            framing,
            connection,
            wire_bytes: 0,
        });
        self.state = match framing {
            Framing::ContentLength(0) | Framing::NoBody | Framing::Upgrade | Framing::Tunnel => {
                State::Complete
            }
            Framing::ContentLength(n) => State::Length(n),
            Framing::Chunked => State::ChunkSize,
            Framing::UntilClose => State::Close,
        };
        if self.state == State::Complete {
            self.complete();
        }
        Ok(false)
    }
}

fn token(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|b| tchar(*b))
}

fn tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

fn field_byte(b: u8) -> bool {
    b == b'\t' || b == b' ' || (0x21..=0x7e).contains(&b) || b >= 0x80
}

fn trim_ows(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn copy(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| Error::Allocation)?;
    out.extend_from_slice(bytes);
    Ok(out)
}

fn append(out: &mut Vec<u8>, bytes: &[u8], limit: usize, resource: Resource) -> Result<(), Error> {
    if bytes.len() > limit.saturating_sub(out.len()) {
        return Err(Error::Limit(resource));
    }
    out.try_reserve(bytes.len())
        .map_err(|_| Error::Allocation)?;
    out.extend_from_slice(bytes);
    Ok(())
}

fn append_wire(out: &mut Vec<u8>, bytes: &[u8], limits: Limits) -> Result<(), Error> {
    append(out, bytes, limits.max_wire_bytes as usize, Resource::Wire)
}

fn parse_status(line: &[u8]) -> Result<ResponseHead, Error> {
    if line.len() < 13
        || line[8] != b' '
        || line[12] != b' '
        || !line[9..12].iter().all(u8::is_ascii_digit)
    {
        return Err(Error::InvalidStatus);
    }
    let version = match &line[..8] {
        b"HTTP/1.0" => Version::Http10,
        b"HTTP/1.1" => Version::Http11,
        _ => return Err(Error::UnsupportedVersion),
    };
    let status = u16::from(line[9] - b'0') * 100
        + u16::from(line[10] - b'0') * 10
        + u16::from(line[11] - b'0');
    if !(100..600).contains(&status) || !line[13..].iter().all(|b| field_byte(*b)) {
        return Err(Error::InvalidStatus);
    }
    Ok(ResponseHead {
        version,
        status,
        reason: copy(&line[13..])?,
        headers: Vec::new(),
    })
}

fn parse_header(line: &[u8]) -> Result<Header, Error> {
    if line.first().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        return Err(Error::ObsoleteFold);
    }
    let colon = line
        .iter()
        .position(|b| *b == b':')
        .ok_or(Error::InvalidHeader)?;
    let (name, rest) = line.split_at(colon);
    if !token(name) || !rest[1..].iter().all(|b| field_byte(*b)) {
        return Err(Error::InvalidHeader);
    }
    Ok(Header {
        name: copy(name)?,
        value: copy(&rest[1..])?,
    })
}

fn validate_method(method: &[u8], limits: Limits) -> Result<(), Error> {
    if method.len() > limits.max_line_bytes {
        return Err(Error::Limit(Resource::Line));
    }
    if !token(method) {
        return Err(Error::InvalidMethod);
    }
    Ok(())
}

fn validate_headers(headers: &[Header], limits: Limits, trailers: bool) -> Result<(), Error> {
    if headers.len()
        > if trailers {
            limits.max_trailers
        } else {
            limits.max_fields
        }
    {
        return Err(Error::Limit(if trailers {
            Resource::TrailerFields
        } else {
            Resource::Fields
        }));
    }
    let mut size = 2usize;
    for header in headers {
        let n = header
            .name
            .len()
            .checked_add(1)
            .and_then(|n| n.checked_add(header.value.len()))
            .ok_or(Error::Limit(Resource::Line))?;
        if n > limits.max_line_bytes {
            return Err(Error::Limit(Resource::Line));
        }
        if !token(&header.name) || !header.value.iter().all(|b| field_byte(*b)) {
            return Err(Error::InvalidHeader);
        }
        size = size
            .checked_add(n + 2)
            .ok_or(Error::Limit(Resource::Head))?;
    }
    let (limit, resource) = if trailers {
        (limits.max_trailer_bytes, Resource::Trailers)
    } else {
        (limits.max_head_bytes, Resource::Head)
    };
    if size > limit {
        return Err(Error::Limit(resource));
    }
    Ok(())
}

fn write_header(
    out: &mut Vec<u8>,
    header: &Header,
    limits: Limits,
    _trailer: bool,
) -> Result<(), Error> {
    append_wire(out, &header.name, limits)?;
    append_wire(out, b":", limits)?;
    append_wire(out, &header.value, limits)?;
    append_wire(out, b"\r\n", limits)
}

fn write_generated_header(
    out: &mut Vec<u8>,
    name: &[u8],
    value: &[u8],
    limits: Limits,
) -> Result<(), Error> {
    if name.len() + value.len() + 2 > limits.max_line_bytes {
        return Err(Error::Limit(Resource::Line));
    }
    append_wire(out, name, limits)?;
    append_wire(out, b": ", limits)?;
    append_wire(out, value, limits)?;
    append_wire(out, b"\r\n", limits)
}

fn decimal(bytes: &[u8]) -> Result<u64, Error> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(Error::InvalidContentLength);
    }
    bytes.iter().try_fold(0u64, |n, b| {
        n.checked_mul(10)
            .and_then(|n| n.checked_add(u64::from(*b - b'0')))
            .ok_or(Error::InvalidContentLength)
    })
}

fn content_length(headers: &[Header]) -> Result<Option<u64>, Error> {
    let mut length = None;
    for header in headers.iter().filter(|h| h.is(b"content-length")) {
        for part in header.value.split(|b| *b == b',') {
            let n = decimal(trim_ows(part))?;
            if length.is_some_and(|old| old != n) {
                return Err(Error::ConflictingContentLength);
            }
            length = Some(n);
        }
    }
    Ok(length)
}

fn transfer_encoding(headers: &[Header], version: Version) -> Result<bool, Error> {
    let mut found = false;
    for header in headers.iter().filter(|h| h.is(b"transfer-encoding")) {
        // No hidden compression service: only the framing coding is supported.
        if found
            || version != Version::Http11
            || !header.trimmed_value().eq_ignore_ascii_case(b"chunked")
        {
            return Err(Error::UnsupportedTransferEncoding);
        }
        found = true;
    }
    Ok(found)
}

fn validate_request_te(headers: &[Header], version: Version) -> Result<(), Error> {
    let mut found = false;
    for header in headers.iter().filter(|h| h.is(b"te")) {
        if found
            || version != Version::Http11
            || (!header.trimmed_value().is_empty()
                && !header.trimmed_value().eq_ignore_ascii_case(b"trailers"))
        {
            return Err(Error::UnsupportedTransferEncoding);
        }
        if !connection_nominates(headers, b"te") {
            return Err(Error::InvalidConnection);
        }
        found = true;
    }
    Ok(())
}

#[derive(Default)]
struct ConnectionFlags {
    close: bool,
    keep_alive: bool,
    upgrade: bool,
}

fn connection_flags(headers: &[Header]) -> Result<ConnectionFlags, Error> {
    let mut flags = ConnectionFlags::default();
    for header in headers.iter().filter(|h| h.is(b"connection")) {
        for part in header.value.split(|b| *b == b',') {
            let part = trim_ows(part);
            if !token(part)
                || [
                    b"content-length".as_slice(),
                    b"transfer-encoding",
                    b"host",
                    b"trailer",
                ]
                .iter()
                .any(|n| part.eq_ignore_ascii_case(n))
            {
                return Err(Error::InvalidConnection);
            }
            flags.close |= part.eq_ignore_ascii_case(b"close");
            flags.keep_alive |= part.eq_ignore_ascii_case(b"keep-alive");
            flags.upgrade |= part.eq_ignore_ascii_case(b"upgrade");
        }
    }
    // Proxy-Connection is non-standard; honor close conservatively, never use
    // it to grant persistence or override the standard Connection field.
    for header in headers.iter().filter(|h| h.is(b"proxy-connection")) {
        for part in header.value.split(|b| *b == b',') {
            let part = trim_ows(part);
            if !token(part) {
                return Err(Error::InvalidConnection);
            }
            flags.close |= part.eq_ignore_ascii_case(b"close");
        }
    }
    Ok(flags)
}

fn connection_nominates(headers: &[Header], name: &[u8]) -> bool {
    headers.iter().filter(|h| h.is(b"connection")).any(|h| {
        h.value
            .split(|b| *b == b',')
            .any(|p| trim_ows(p).eq_ignore_ascii_case(name))
    })
}

fn protocols(headers: &[Header]) -> Result<Vec<Vec<u8>>, Error> {
    let mut result = Vec::new();
    for header in headers.iter().filter(|h| h.is(b"upgrade")) {
        for part in header.value.split(|b| *b == b',') {
            let part = trim_ows(part);
            let mut pieces = part.split(|b| *b == b'/');
            if !pieces.next().is_some_and(token)
                || pieces.next().is_some_and(|p| !token(p))
                || pieces.next().is_some()
            {
                return Err(Error::InvalidUpgrade);
            }
            result.try_reserve(1).map_err(|_| Error::Allocation)?;
            result.push(copy(part)?);
        }
    }
    Ok(result)
}

fn same_protocol(a: &[u8], b: &[u8]) -> bool {
    let mut a = a.splitn(2, |b| *b == b'/');
    let mut b = b.splitn(2, |b| *b == b'/');
    a.next()
        .unwrap_or_default()
        .eq_ignore_ascii_case(b.next().unwrap_or_default())
        && a.next() == b.next()
}

fn forbidden_trailer(name: &[u8]) -> bool {
    [
        b"content-length".as_slice(),
        b"transfer-encoding",
        b"host",
        b"connection",
        b"trailer",
        b"te",
        b"upgrade",
        b"keep-alive",
        b"proxy-connection",
        b"authorization",
        b"proxy-authorization",
        b"proxy-authenticate",
        b"www-authenticate",
        b"cookie",
        b"set-cookie",
        b"content-encoding",
        b"content-type",
        b"content-range",
        b"content-location",
        b"content-disposition",
        b"location",
        b"retry-after",
        b"cache-control",
        b"expires",
        b"vary",
        b"age",
    ]
    .iter()
    .any(|n| name.eq_ignore_ascii_case(n))
}

fn validate_trailer_declaration(headers: &[Header]) -> Result<(), Error> {
    for header in headers.iter().filter(|h| h.is(b"trailer")) {
        for part in header.value.split(|b| *b == b',') {
            let name = trim_ows(part);
            if !token(name) || forbidden_trailer(name) || connection_nominates(headers, name) {
                return Err(Error::InvalidTrailer);
            }
        }
    }
    Ok(())
}

fn chunk_size(line: &[u8]) -> Result<u64, Error> {
    let mut i = 0;
    let mut size = 0u64;
    while i < line.len() && line[i].is_ascii_hexdigit() {
        let digit = match line[i] {
            b'0'..=b'9' => line[i] - b'0',
            b'a'..=b'f' => line[i] - b'a' + 10,
            _ => line[i] - b'A' + 10,
        };
        size = size
            .checked_mul(16)
            .and_then(|n| n.checked_add(u64::from(digit)))
            .ok_or(Error::InvalidChunk)?;
        i += 1;
    }
    if i == 0 {
        return Err(Error::InvalidChunk);
    }
    while i < line.len() {
        skip_ows(line, &mut i);
        if line.get(i) != Some(&b';') {
            return Err(Error::InvalidChunk);
        }
        i += 1;
        skip_ows(line, &mut i);
        let start = i;
        while line.get(i).is_some_and(|b| tchar(*b)) {
            i += 1;
        }
        if i == start {
            return Err(Error::InvalidChunk);
        }
        let end = i;
        skip_ows(line, &mut i);
        if line.get(i) == Some(&b'=') {
            i += 1;
            skip_ows(line, &mut i);
            if line.get(i) == Some(&b'"') {
                i += 1;
                loop {
                    let b = *line.get(i).ok_or(Error::InvalidChunk)?;
                    i += 1;
                    if b == b'"' {
                        break;
                    }
                    if b == b'\\' {
                        let b = *line.get(i).ok_or(Error::InvalidChunk)?;
                        if !field_byte(b) {
                            return Err(Error::InvalidChunk);
                        }
                        i += 1;
                    } else if !field_byte(b) {
                        return Err(Error::InvalidChunk);
                    }
                }
            } else {
                let start = i;
                while line.get(i).is_some_and(|b| tchar(*b)) {
                    i += 1;
                }
                if i == start {
                    return Err(Error::InvalidChunk);
                }
            }
        } else {
            // BWS belongs to a following semicolon, not an absent '='.
            i = end;
        }
    }
    Ok(size)
}

fn skip_ows(bytes: &[u8], i: &mut usize) {
    while bytes.get(*i).is_some_and(|b| matches!(b, b' ' | b'\t')) {
        *i += 1;
    }
}

fn request_host(headers: &[Header], version: Version) -> Result<Option<&[u8]>, Error> {
    let mut result = None;
    for header in headers.iter().filter(|h| h.is(b"host")) {
        if result.is_some() {
            return Err(Error::InvalidHost);
        }
        let value = header.trimmed_value();
        authority(value, false).map_err(|_| Error::InvalidHost)?;
        result = Some(value);
    }
    if result.is_none() && version == Version::Http11 {
        return Err(Error::InvalidHost);
    }
    Ok(result)
}

struct Authority<'a> {
    host: &'a [u8],
    port: Option<u16>,
}

fn authority(bytes: &[u8], require_port: bool) -> Result<Authority<'_>, Error> {
    if bytes.is_empty() {
        return Err(Error::InvalidTarget);
    }
    let (host, rest) = if bytes[0] == b'[' {
        let close = bytes
            .iter()
            .position(|b| *b == b']')
            .ok_or(Error::InvalidTarget)?;
        let inside = &bytes[1..close];
        let valid_v6 = std::str::from_utf8(inside)
            .ok()
            .and_then(|s| s.parse::<std::net::Ipv6Addr>().ok())
            .is_some();
        let valid_future = inside.first().is_some_and(|b| matches!(b, b'v' | b'V')) && {
            let dot = inside.iter().position(|b| *b == b'.');
            dot.is_some_and(|d| {
                d > 1
                    && inside[1..d].iter().all(u8::is_ascii_hexdigit)
                    && d + 1 < inside.len()
                    && inside[d + 1..]
                        .iter()
                        .all(|b| unreserved(*b) || subdelim(*b) || *b == b':')
            })
        };
        if !valid_v6 && !valid_future {
            return Err(Error::InvalidTarget);
        }
        (&bytes[..=close], &bytes[close + 1..])
    } else {
        let colon = bytes.iter().position(|b| *b == b':').unwrap_or(bytes.len());
        let host = &bytes[..colon];
        if host.is_empty() || !uri_bytes(host, false, true) {
            return Err(Error::InvalidTarget);
        }
        (host, &bytes[colon..])
    };
    let port = if rest.is_empty() {
        None
    } else {
        if rest[0] != b':' {
            return Err(Error::InvalidTarget);
        }
        let n = decimal(&rest[1..]).map_err(|_| Error::InvalidTarget)?;
        if n == 0 || n > u64::from(u16::MAX) {
            return Err(Error::InvalidTarget);
        }
        Some(n as u16)
    };
    if require_port && port.is_none() {
        return Err(Error::InvalidTarget);
    }
    Ok(Authority { host, port })
}

fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~".contains(&b)
}
fn subdelim(b: u8) -> bool {
    b"!$&'()*+,;=".contains(&b)
}

fn uri_bytes(bytes: &[u8], query: bool, host: bool) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if !bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                || !bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
            {
                return false;
            }
            i += 3;
        } else if unreserved(b)
            || subdelim(b)
            || (!host && (b == b':' || b == b'@' || b == b'/' || (query && b == b'?')))
        {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

fn path_query(bytes: &[u8]) -> bool {
    let q = bytes.iter().position(|b| *b == b'?').unwrap_or(bytes.len());
    uri_bytes(&bytes[..q], false, false)
        && (q == bytes.len() || uri_bytes(&bytes[q + 1..], true, false))
}

fn validate_target(
    method: &[u8],
    target: &[u8],
    host: Option<&[u8]>,
    limits: Limits,
) -> Result<(), Error> {
    if target.len() > limits.max_line_bytes {
        return Err(Error::Limit(Resource::Line));
    }
    if target.is_empty() {
        return Err(Error::InvalidTarget);
    }
    if method == b"CONNECT" {
        let requested = authority(target, true)?;
        if let Some(host) = host {
            let actual = authority(host, false)?;
            if !requested.host.eq_ignore_ascii_case(actual.host)
                || actual.port.is_some_and(|p| Some(p) != requested.port)
            {
                return Err(Error::HostMismatch);
            }
        }
        return Ok(());
    }
    if target == b"*" {
        return if method == b"OPTIONS" {
            Ok(())
        } else {
            Err(Error::InvalidTarget)
        };
    }
    if target[0] == b'/' {
        return if path_query(target) {
            Ok(())
        } else {
            Err(Error::InvalidTarget)
        };
    }
    // Only HTTP(S) absolute-form; routing/security of that scheme is host-owned.
    let colon = target
        .iter()
        .position(|b| *b == b':')
        .ok_or(Error::InvalidTarget)?;
    let default_port = if target[..colon].eq_ignore_ascii_case(b"http") {
        80
    } else if target[..colon].eq_ignore_ascii_case(b"https") {
        443
    } else {
        return Err(Error::InvalidTarget);
    };
    if target.get(colon + 1..colon + 3) != Some(b"//") {
        return Err(Error::InvalidTarget);
    }
    let rest = &target[colon + 3..];
    let end = rest
        .iter()
        .position(|b| matches!(b, b'/' | b'?'))
        .unwrap_or(rest.len());
    let requested = authority(&rest[..end], false)?;
    if !path_query(&rest[end..]) {
        return Err(Error::InvalidTarget);
    }
    if let Some(host) = host {
        let actual = authority(host, false)?;
        if !requested.host.eq_ignore_ascii_case(actual.host)
            || requested.port.unwrap_or(default_port) != actual.port.unwrap_or(default_port)
        {
            return Err(Error::HostMismatch);
        }
    }
    Ok(())
}
