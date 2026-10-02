//! First-party SOCKS5 CONNECT and Tor remote-name lookup.
//!
//! `wire` is portable domain code: bounded byte codecs with no sockets, OS
//! handles, logging, DNS, or unsafe code. `transport` owns a `std::net` tunnel
//! and its deadlines. A domain is always sent to the proxy unchanged. Neither
//! layer implements direct-connection fallback. Supplying credentials requires
//! method 0x02; a proxy cannot silently discard stream isolation.
//!
//! This is the RFC1928 TCP subset used by Tor, plus RFC1929 authentication and
//! Tor RESOLVE/RESOLVE_PTR. GSSAPI, BIND and UDP ASSOCIATE are not implemented.

pub mod wire {
    use std::fmt;

    pub const MAX_DOMAIN_LEN: usize = 255;
    pub const MAX_REQUEST_LEN: usize = 262;
    pub const MAX_REPLY_LEN: usize = 262;
    pub const MAX_AUTH_LEN: usize = 513;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum ProtocolError {
        EmptyDomain,
        DomainTooLong,
        DomainContainsNul,
        InvalidPort,
        EmptyUsername,
        UsernameTooLong,
        EmptyPassword,
        PasswordTooLong,
        InvalidVersion(u8),
        InvalidAuthVersion(u8),
        NoAcceptableMethod,
        UnofferedMethod(u8),
        AuthenticationRejected(u8),
        NonzeroReserved(u8),
        UnknownAddressType(u8),
    }

    impl fmt::Display for ProtocolError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            // Only static classifications and protocol bytes can be formatted.
            write!(f, "SOCKS5 protocol error: {self:?}")
        }
    }

    impl std::error::Error for ProtocolError {}

    /// An opaque DNS name, 1..=255 octets with no NUL. There is no IDNA,
    /// normalization, local lookup, UTF-8 requirement, or label rewriting.
    /// URI callers should supply the already-parsed host in its desired wire
    /// encoding (normally an ASCII A-label), without brackets or a port.
    #[derive(Clone, Eq, PartialEq)]
    pub struct DomainName(Vec<u8>);

    impl DomainName {
        pub fn new(bytes: &[u8]) -> Result<Self, ProtocolError> {
            if bytes.is_empty() {
                return Err(ProtocolError::EmptyDomain);
            }
            if bytes.len() > MAX_DOMAIN_LEN {
                return Err(ProtocolError::DomainTooLong);
            }
            if bytes.contains(&0) {
                return Err(ProtocolError::DomainContainsNul);
            }
            Ok(Self(bytes.to_vec()))
        }

        pub fn as_bytes(&self) -> &[u8] {
            &self.0
        }
    }

    impl fmt::Debug for DomainName {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("DomainName([redacted])")
        }
    }

    #[derive(Clone, Eq, PartialEq)]
    pub enum Address {
        Ipv4([u8; 4]),
        Ipv6([u8; 16]),
        Domain(DomainName),
    }

    impl fmt::Debug for Address {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(match self {
                Self::Ipv4(_) => "Ipv4([redacted])",
                Self::Ipv6(_) => "Ipv6([redacted])",
                Self::Domain(_) => "Domain([redacted])",
            })
        }
    }

    #[derive(Clone, Eq, PartialEq)]
    pub struct Destination {
        address: Address,
        port: u16,
    }

    impl Destination {
        pub fn new(address: Address, port: u16) -> Result<Self, ProtocolError> {
            if port == 0 {
                return Err(ProtocolError::InvalidPort);
            }
            Ok(Self { address, port })
        }

        pub fn address(&self) -> &Address {
            &self.address
        }

        pub fn port(&self) -> u16 {
            self.port
        }
    }

    impl fmt::Debug for Destination {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Destination([redacted])")
        }
    }

    /// RFC1929 octets, not text. In particular, Tor's modern `<torS0X>0`
    /// username and existing legacy isolation pairs are forwarded exactly.
    /// Empty fields are rejected to avoid accidentally disabling isolation.
    /// Drop overwrites these buffers using safe Rust; this is best effort, not
    /// a guarantee of compiler-proof erasure or erasure of caller copies.
    pub struct Credentials {
        username: [u8; MAX_DOMAIN_LEN],
        password: [u8; MAX_DOMAIN_LEN],
        username_len: u8,
        password_len: u8,
    }

    impl Credentials {
        pub fn new(username: &[u8], password: &[u8]) -> Result<Self, ProtocolError> {
            if username.is_empty() {
                return Err(ProtocolError::EmptyUsername);
            }
            if username.len() > MAX_DOMAIN_LEN {
                return Err(ProtocolError::UsernameTooLong);
            }
            if password.is_empty() {
                return Err(ProtocolError::EmptyPassword);
            }
            if password.len() > MAX_DOMAIN_LEN {
                return Err(ProtocolError::PasswordTooLong);
            }
            let mut result = Self {
                username: [0; MAX_DOMAIN_LEN],
                password: [0; MAX_DOMAIN_LEN],
                username_len: username.len() as u8,
                password_len: password.len() as u8,
            };
            result.username[..username.len()].copy_from_slice(username);
            result.password[..password.len()].copy_from_slice(password);
            Ok(result)
        }
    }

    impl Drop for Credentials {
        fn drop(&mut self) {
            self.username.fill(0);
            self.password.fill(0);
        }
    }

    impl fmt::Debug for Credentials {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Credentials([redacted])")
        }
    }

    #[derive(Clone, Copy)]
    pub enum Authentication<'a> {
        /// Explicit choice for callers that do not need SOCKS auth isolation.
        None,
        /// Offers only 0x02. Never falls back to unauthenticated negotiation.
        UsernamePassword(&'a Credentials),
    }

    impl fmt::Debug for Authentication<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(match self {
                Self::None => "Authentication::None",
                Self::UsernamePassword(_) => "Authentication::UsernamePassword([redacted])",
            })
        }
    }

    impl Authentication<'_> {
        pub fn greeting(self) -> [u8; 3] {
            [5, 1, self.method()]
        }

        fn method(self) -> u8 {
            match self {
                Self::None => 0,
                Self::UsernamePassword(_) => 2,
            }
        }

        pub fn accept_method(self, response: [u8; 2]) -> Result<(), ProtocolError> {
            if response[0] != 5 {
                return Err(ProtocolError::InvalidVersion(response[0]));
            }
            if response[1] == 255 {
                return Err(ProtocolError::NoAcceptableMethod);
            }
            if response[1] != self.method() {
                return Err(ProtocolError::UnofferedMethod(response[1]));
            }
            Ok(())
        }
    }

    /// A bounded outbound frame. Debug never reveals its payload.
    pub struct Packet {
        bytes: [u8; MAX_REQUEST_LEN],
        len: usize,
    }

    impl Packet {
        pub fn as_bytes(&self) -> &[u8] {
            &self.bytes[..self.len]
        }
    }

    impl fmt::Debug for Packet {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Packet([redacted])")
        }
    }

    pub struct AuthPacket {
        bytes: [u8; MAX_AUTH_LEN],
        len: usize,
    }

    impl AuthPacket {
        pub fn as_bytes(&self) -> &[u8] {
            &self.bytes[..self.len]
        }
    }

    impl Drop for AuthPacket {
        fn drop(&mut self) {
            self.bytes.fill(0);
        }
    }

    impl fmt::Debug for AuthPacket {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("AuthPacket([redacted])")
        }
    }

    pub fn encode_authentication(credentials: &Credentials) -> AuthPacket {
        let username_len = usize::from(credentials.username_len);
        let password_len = usize::from(credentials.password_len);
        let mut packet = AuthPacket {
            bytes: [0; MAX_AUTH_LEN],
            len: 3 + username_len + password_len,
        };
        packet.bytes[0] = 1;
        packet.bytes[1] = credentials.username_len;
        packet.bytes[2..2 + username_len].copy_from_slice(&credentials.username[..username_len]);
        packet.bytes[2 + username_len] = credentials.password_len;
        packet.bytes[3 + username_len..packet.len]
            .copy_from_slice(&credentials.password[..password_len]);
        packet
    }

    pub fn accept_authentication(response: [u8; 2]) -> Result<(), ProtocolError> {
        if response[0] != 1 {
            return Err(ProtocolError::InvalidAuthVersion(response[0]));
        }
        if response[1] != 0 {
            return Err(ProtocolError::AuthenticationRejected(response[1]));
        }
        Ok(())
    }

    fn request(command: u8, address: &Address, port: u16) -> Packet {
        let mut packet = Packet {
            bytes: [0; MAX_REQUEST_LEN],
            len: 4,
        };
        packet.bytes[..3].copy_from_slice(&[5, command, 0]);
        match address {
            Address::Ipv4(bytes) => {
                packet.bytes[3] = 1;
                packet.bytes[4..8].copy_from_slice(bytes);
                packet.len = 8;
            }
            Address::Ipv6(bytes) => {
                packet.bytes[3] = 4;
                packet.bytes[4..20].copy_from_slice(bytes);
                packet.len = 20;
            }
            Address::Domain(domain) => {
                let bytes = domain.as_bytes();
                packet.bytes[3] = 3;
                packet.bytes[4] = bytes.len() as u8;
                packet.bytes[5..5 + bytes.len()].copy_from_slice(bytes);
                packet.len = 5 + bytes.len();
            }
        }
        packet.bytes[packet.len..packet.len + 2].copy_from_slice(&port.to_be_bytes());
        packet.len += 2;
        packet
    }

    pub fn encode_connect(destination: &Destination) -> Packet {
        request(1, destination.address(), destination.port())
    }

    pub fn encode_resolve(domain: &DomainName) -> Packet {
        request(0xf0, &Address::Domain(domain.clone()), 0)
    }

    /// Tor documents RESOLVE_PTR with an IPv4 target; do not silently convert
    /// an IPv6 target into a hostname or perform a local reverse lookup.
    pub fn encode_resolve_ptr(ipv4: [u8; 4]) -> Packet {
        request(0xf1, &Address::Ipv4(ipv4), 0)
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum ReplyCode {
        Succeeded,
        GeneralFailure,
        RulesetDenied,
        NetworkUnreachable,
        HostUnreachable,
        ConnectionRefused,
        TtlExpired,
        CommandNotSupported,
        AddressTypeNotSupported,
        OnionDescriptorNotFound,
        OnionDescriptorInvalid,
        OnionIntroductionFailed,
        OnionRendezvousFailed,
        OnionMissingClientAuthorization,
        OnionWrongClientAuthorization,
        OnionInvalidAddress,
        OnionIntroductionTimedOut,
        Unassigned(u8),
    }

    impl ReplyCode {
        pub fn from_byte(value: u8) -> Self {
            match value {
                0 => Self::Succeeded,
                1 => Self::GeneralFailure,
                2 => Self::RulesetDenied,
                3 => Self::NetworkUnreachable,
                4 => Self::HostUnreachable,
                5 => Self::ConnectionRefused,
                6 => Self::TtlExpired,
                7 => Self::CommandNotSupported,
                8 => Self::AddressTypeNotSupported,
                0xf0 => Self::OnionDescriptorNotFound,
                0xf1 => Self::OnionDescriptorInvalid,
                0xf2 => Self::OnionIntroductionFailed,
                0xf3 => Self::OnionRendezvousFailed,
                0xf4 => Self::OnionMissingClientAuthorization,
                0xf5 => Self::OnionWrongClientAuthorization,
                0xf6 => Self::OnionInvalidAddress,
                0xf7 => Self::OnionIntroductionTimedOut,
                other => Self::Unassigned(other),
            }
        }

        pub fn as_byte(self) -> u8 {
            match self {
                Self::Succeeded => 0,
                Self::GeneralFailure => 1,
                Self::RulesetDenied => 2,
                Self::NetworkUnreachable => 3,
                Self::HostUnreachable => 4,
                Self::ConnectionRefused => 5,
                Self::TtlExpired => 6,
                Self::CommandNotSupported => 7,
                Self::AddressTypeNotSupported => 8,
                Self::OnionDescriptorNotFound => 0xf0,
                Self::OnionDescriptorInvalid => 0xf1,
                Self::OnionIntroductionFailed => 0xf2,
                Self::OnionRendezvousFailed => 0xf3,
                Self::OnionMissingClientAuthorization => 0xf4,
                Self::OnionWrongClientAuthorization => 0xf5,
                Self::OnionInvalidAddress => 0xf6,
                Self::OnionIntroductionTimedOut => 0xf7,
                Self::Unassigned(value) => value,
            }
        }
    }

    #[derive(Clone, Eq, PartialEq)]
    pub struct BoundEndpoint {
        pub address: Address,
        pub port: u16,
    }

    impl fmt::Debug for BoundEndpoint {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("BoundEndpoint([redacted])")
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct Reply {
        pub code: ReplyCode,
        pub bound: BoundEndpoint,
    }

    /// Return the exact reply length as soon as its address type/length is
    /// available. Never requires or consumes following application bytes.
    pub fn reply_frame_len(bytes: &[u8]) -> Result<Option<usize>, ProtocolError> {
        if let Some(&version) = bytes.first()
            && version != 5
        {
            return Err(ProtocolError::InvalidVersion(version));
        }
        if bytes.len() < 3 {
            return Ok(None);
        }
        if bytes[2] != 0 {
            return Err(ProtocolError::NonzeroReserved(bytes[2]));
        }
        if bytes.len() < 4 {
            return Ok(None);
        }
        match bytes[3] {
            1 => Ok(Some(10)),
            4 => Ok(Some(22)),
            3 if bytes.len() < 5 => Ok(None),
            3 if bytes[4] == 0 => Err(ProtocolError::EmptyDomain),
            3 => Ok(Some(7 + usize::from(bytes[4]))),
            other => Err(ProtocolError::UnknownAddressType(other)),
        }
    }

    /// Incremental decoder: Ok(None) means a valid prefix needs more bytes.
    /// On success, the second tuple member is the consumed frame length.
    /// Unknown reply codes remain explicit failures for transport callers.
    pub fn decode_reply(bytes: &[u8]) -> Result<Option<(Reply, usize)>, ProtocolError> {
        let Some(len) = reply_frame_len(bytes)? else {
            return Ok(None);
        };
        if bytes.len() < len {
            return Ok(None);
        }
        let address = match bytes[3] {
            1 => Address::Ipv4([bytes[4], bytes[5], bytes[6], bytes[7]]),
            4 => {
                let mut address = [0; 16];
                address.copy_from_slice(&bytes[4..20]);
                Address::Ipv6(address)
            }
            3 => Address::Domain(DomainName::new(&bytes[5..len - 2])?),
            _ => unreachable!("address type was checked by reply_frame_len"),
        };
        Ok(Some((
            Reply {
                code: ReplyCode::from_byte(bytes[1]),
                bound: BoundEndpoint {
                    address,
                    port: u16::from_be_bytes([bytes[len - 2], bytes[len - 1]]),
                },
            },
            len,
        )))
    }
}

pub mod transport {
    use super::wire::{self, Address, Authentication, BoundEndpoint, Destination, DomainName};
    use std::fmt;
    use std::io::{self, Read, Write};
    use std::net::{Shutdown, SocketAddr, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(50);
    const MAX_POLL_INTERVAL: Duration = Duration::from_secs(1);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Stage {
        ProxyConnect,
        MethodSelection,
        Authentication,
        ProxyReply,
        Resolve,
        Read,
        Write,
        Shutdown,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum ErrorKind {
        InvalidTimeout,
        InvalidProxyPort,
        Cancelled,
        TimedOut,
        Closed,
        UnexpectedEof,
        WriteZero,
        Protocol(wire::ProtocolError),
        ProxyRejected(wire::ReplyCode),
        UnexpectedResolveAddress,
        /// Deliberately retains only the std classification, never an OS
        /// message, inner error, proxy address, or destination address.
        Io(io::ErrorKind),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Error {
        pub stage: Stage,
        pub kind: ErrorKind,
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "SOCKS5 {:?}: {:?}", self.stage, self.kind)
        }
    }

    impl std::error::Error for Error {}

    fn error(stage: Stage, kind: ErrorKind) -> Error {
        Error { stage, kind }
    }

    fn protocol(stage: Stage, kind: wire::ProtocolError) -> Error {
        error(stage, ErrorKind::Protocol(kind))
    }

    #[derive(Clone, Default)]
    pub struct Cancellation(Arc<AtomicBool>);

    impl Cancellation {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn cancel(&self) {
            self.0.store(true, Ordering::Release);
        }

        pub fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
    }

    impl fmt::Debug for Cancellation {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Cancellation")
                .field("cancelled", &self.is_cancelled())
                .finish()
        }
    }

    /// One absolute deadline shared across partial I/O and every handshake
    /// stage. Progress never resets it. Create a fresh control per application
    /// operation. Cancellation during socket I/O is checked at each poll; OS
    /// timeout granularity and scheduling can add latency. Proxy TCP connect
    /// is bounded separately by `proxy_connect_timeout`, not by this poll.
    #[derive(Clone, Debug)]
    pub struct IoControl {
        deadline: Instant,
        cancellation: Cancellation,
        poll_interval: Duration,
    }

    impl IoControl {
        pub fn new(timeout: Duration, cancellation: &Cancellation) -> Result<Self, Error> {
            Self::with_poll_interval(timeout, cancellation, DEFAULT_POLL_INTERVAL)
        }

        pub fn with_poll_interval(
            timeout: Duration,
            cancellation: &Cancellation,
            poll_interval: Duration,
        ) -> Result<Self, Error> {
            if timeout.is_zero() || poll_interval.is_zero() || poll_interval > MAX_POLL_INTERVAL {
                return Err(error(Stage::ProxyConnect, ErrorKind::InvalidTimeout));
            }
            let deadline = Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| error(Stage::ProxyConnect, ErrorKind::InvalidTimeout))?;
            Ok(Self {
                deadline,
                cancellation: cancellation.clone(),
                poll_interval,
            })
        }

        fn remaining(&self, stage: Stage) -> Result<Duration, Error> {
            if self.cancellation.is_cancelled() {
                return Err(error(stage, ErrorKind::Cancelled));
            }
            self.deadline
                .checked_duration_since(Instant::now())
                .filter(|duration| !duration.is_zero())
                .ok_or_else(|| error(stage, ErrorKind::TimedOut))
        }

        fn poll_timeout(&self, stage: Stage) -> Result<Duration, Error> {
            Ok(self.remaining(stage)?.min(self.poll_interval))
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct ConnectOptions {
        /// Upper bound for cancellation latency inside OS TCP connect.
        pub proxy_connect_timeout: Duration,
        /// Total proxy connect + greeting + auth + reply deadline.
        pub total_timeout: Duration,
        pub poll_interval: Duration,
    }

    impl Default for ConnectOptions {
        fn default() -> Self {
            Self {
                proxy_connect_timeout: Duration::from_secs(3),
                total_timeout: Duration::from_secs(30),
                poll_interval: DEFAULT_POLL_INTERVAL,
            }
        }
    }

    fn io_error(stage: Stage, cause: io::Error) -> Error {
        error(stage, ErrorKind::Io(cause.kind()))
    }

    fn retryable(cause: &io::Error) -> bool {
        matches!(
            cause.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
        )
    }

    fn read_some(
        stream: &mut TcpStream,
        bytes: &mut [u8],
        control: &IoControl,
        stage: Stage,
    ) -> Result<usize, Error> {
        loop {
            stream
                .set_read_timeout(Some(control.poll_timeout(stage)?))
                .map_err(|cause| io_error(stage, cause))?;
            if bytes.is_empty() {
                return Ok(0);
            }
            match stream.read(bytes) {
                Ok(count) => return Ok(count),
                Err(cause) if retryable(&cause) => continue,
                Err(cause) => {
                    control.remaining(stage)?;
                    return Err(io_error(stage, cause));
                }
            }
        }
    }

    fn read_exact(
        stream: &mut TcpStream,
        mut bytes: &mut [u8],
        control: &IoControl,
        stage: Stage,
    ) -> Result<(), Error> {
        control.remaining(stage)?;
        while !bytes.is_empty() {
            let count = read_some(stream, bytes, control, stage)?;
            if count == 0 {
                return Err(error(stage, ErrorKind::UnexpectedEof));
            }
            bytes = &mut bytes[count..];
        }
        control.remaining(stage)?;
        Ok(())
    }

    fn write_all(
        stream: &mut TcpStream,
        mut bytes: &[u8],
        control: &IoControl,
        stage: Stage,
    ) -> Result<(), Error> {
        control.remaining(stage)?;
        while !bytes.is_empty() {
            stream
                .set_write_timeout(Some(control.poll_timeout(stage)?))
                .map_err(|cause| io_error(stage, cause))?;
            match stream.write(bytes) {
                Ok(0) => return Err(error(stage, ErrorKind::WriteZero)),
                Ok(count) => bytes = &bytes[count..],
                Err(cause) if retryable(&cause) => continue,
                Err(cause) => {
                    control.remaining(stage)?;
                    return Err(io_error(stage, cause));
                }
            }
        }
        control.remaining(stage)?;
        Ok(())
    }

    fn negotiate(
        stream: &mut TcpStream,
        authentication: Authentication<'_>,
        control: &IoControl,
    ) -> Result<(), Error> {
        write_all(
            stream,
            &authentication.greeting(),
            control,
            Stage::MethodSelection,
        )?;
        let mut selection = [0; 2];
        read_exact(stream, &mut selection[..1], control, Stage::MethodSelection)?;
        if selection[0] != 5 {
            return Err(protocol(
                Stage::MethodSelection,
                wire::ProtocolError::InvalidVersion(selection[0]),
            ));
        }
        read_exact(stream, &mut selection[1..], control, Stage::MethodSelection)?;
        authentication
            .accept_method(selection)
            .map_err(|cause| protocol(Stage::MethodSelection, cause))?;
        if let Authentication::UsernamePassword(credentials) = authentication {
            {
                let packet = wire::encode_authentication(credentials);
                write_all(stream, packet.as_bytes(), control, Stage::Authentication)?;
            }
            let mut response = [0; 2];
            read_exact(stream, &mut response[..1], control, Stage::Authentication)?;
            if response[0] != 1 {
                return Err(protocol(
                    Stage::Authentication,
                    wire::ProtocolError::InvalidAuthVersion(response[0]),
                ));
            }
            read_exact(stream, &mut response[1..], control, Stage::Authentication)?;
            wire::accept_authentication(response)
                .map_err(|cause| protocol(Stage::Authentication, cause))?;
        }
        Ok(())
    }

    fn read_reply(stream: &mut TcpStream, control: &IoControl) -> Result<wire::Reply, Error> {
        let mut buffer = [0; wire::MAX_REPLY_LEN];
        // Read only the SOCKS frame; a coalesced application banner stays in
        // the kernel receive buffer. Malformed headers fail before their body.
        read_exact(stream, &mut buffer[..1], control, Stage::ProxyReply)?;
        wire::reply_frame_len(&buffer[..1]).map_err(|cause| protocol(Stage::ProxyReply, cause))?;
        read_exact(stream, &mut buffer[1..3], control, Stage::ProxyReply)?;
        wire::reply_frame_len(&buffer[..3]).map_err(|cause| protocol(Stage::ProxyReply, cause))?;
        read_exact(stream, &mut buffer[3..4], control, Stage::ProxyReply)?;
        let mut prefix_len = 4;
        let mut length = wire::reply_frame_len(&buffer[..prefix_len])
            .map_err(|cause| protocol(Stage::ProxyReply, cause))?;
        if length.is_none() {
            read_exact(stream, &mut buffer[4..5], control, Stage::ProxyReply)?;
            prefix_len = 5;
            length = wire::reply_frame_len(&buffer[..prefix_len])
                .map_err(|cause| protocol(Stage::ProxyReply, cause))?;
        }
        let length = length.ok_or_else(|| error(Stage::ProxyReply, ErrorKind::UnexpectedEof))?;
        read_exact(
            stream,
            &mut buffer[prefix_len..length],
            control,
            Stage::ProxyReply,
        )?;
        let (reply, _) = wire::decode_reply(&buffer[..length])
            .map_err(|cause| protocol(Stage::ProxyReply, cause))?
            .ok_or_else(|| error(Stage::ProxyReply, ErrorKind::UnexpectedEof))?;
        if reply.code != wire::ReplyCode::Succeeded {
            return Err(error(
                Stage::ProxyReply,
                ErrorKind::ProxyRejected(reply.code),
            ));
        }
        Ok(reply)
    }

    fn open_proxy(
        proxy: SocketAddr,
        options: ConnectOptions,
        control: &IoControl,
    ) -> Result<TcpStream, Error> {
        if proxy.port() == 0 {
            return Err(error(Stage::ProxyConnect, ErrorKind::InvalidProxyPort));
        }
        if options.proxy_connect_timeout.is_zero() {
            return Err(error(Stage::ProxyConnect, ErrorKind::InvalidTimeout));
        }
        // Only a numeric proxy endpoint can enter std networking. Destination
        // domains never implement ToSocketAddrs and never enter this call.
        let timeout = control
            .remaining(Stage::ProxyConnect)?
            .min(options.proxy_connect_timeout);
        let result = TcpStream::connect_timeout(&proxy, timeout);
        if let Err(cause) = control.remaining(Stage::ProxyConnect) {
            if let Ok(stream) = result {
                let _ = stream.shutdown(Shutdown::Both);
            }
            return Err(cause);
        }
        let stream = result.map_err(|cause| {
            if matches!(
                cause.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) {
                error(Stage::ProxyConnect, ErrorKind::TimedOut)
            } else {
                io_error(Stage::ProxyConnect, cause)
            }
        })?;
        stream
            .set_nonblocking(false)
            .map_err(|cause| io_error(Stage::ProxyConnect, cause))?;
        stream
            .set_nodelay(true)
            .map_err(|cause| io_error(Stage::ProxyConnect, cause))?;
        Ok(stream)
    }

    fn exchange(
        proxy: SocketAddr,
        authentication: Authentication<'_>,
        request: Option<&wire::Packet>,
        options: ConnectOptions,
        cancellation: &Cancellation,
    ) -> Result<(TcpStream, Option<BoundEndpoint>), Error> {
        let control = IoControl::with_poll_interval(
            options.total_timeout,
            cancellation,
            options.poll_interval,
        )?;
        let mut stream = open_proxy(proxy, options, &control)?;
        let result = (|| {
            negotiate(&mut stream, authentication, &control)?;
            let bound = match request {
                Some(packet) => {
                    write_all(&mut stream, packet.as_bytes(), &control, Stage::ProxyReply)?;
                    Some(read_reply(&mut stream, &control)?.bound)
                }
                None => None,
            };
            control.remaining(Stage::ProxyReply)?;
            Ok(bound)
        })();
        match result {
            Ok(bound) => Ok((stream, bound)),
            Err(cause) => {
                let _ = stream.shutdown(Shutdown::Both);
                Err(cause)
            }
        }
    }

    /// Tests only SOCKS method negotiation, not Tor identity, bootstrap, or
    /// reachability. Never sends CONNECT or application data.
    pub fn probe(
        proxy: SocketAddr,
        authentication: Authentication<'_>,
        options: ConnectOptions,
        cancellation: &Cancellation,
    ) -> Result<(), Error> {
        let (stream, _) = exchange(proxy, authentication, None, options, cancellation)?;
        let _ = stream.shutdown(Shutdown::Both);
        Ok(())
    }

    /// Remote Tor lookup: one address per Tor response, with no local DNS and
    /// no assumption that a reply enumerates every address for the hostname.
    pub fn resolve(
        proxy: SocketAddr,
        domain: &DomainName,
        authentication: Authentication<'_>,
        options: ConnectOptions,
        cancellation: &Cancellation,
    ) -> Result<Address, Error> {
        let packet = wire::encode_resolve(domain);
        let (stream, bound) =
            exchange(proxy, authentication, Some(&packet), options, cancellation)?;
        let _ = stream.shutdown(Shutdown::Both);
        match bound.map(|endpoint| endpoint.address) {
            Some(address @ (Address::Ipv4(_) | Address::Ipv6(_))) => Ok(address),
            _ => Err(error(Stage::Resolve, ErrorKind::UnexpectedResolveAddress)),
        }
    }

    pub fn resolve_ptr(
        proxy: SocketAddr,
        ipv4: [u8; 4],
        authentication: Authentication<'_>,
        options: ConnectOptions,
        cancellation: &Cancellation,
    ) -> Result<DomainName, Error> {
        let packet = wire::encode_resolve_ptr(ipv4);
        let (stream, bound) =
            exchange(proxy, authentication, Some(&packet), options, cancellation)?;
        let _ = stream.shutdown(Shutdown::Both);
        match bound.map(|endpoint| endpoint.address) {
            Some(Address::Domain(domain)) => Ok(domain),
            _ => Err(error(Stage::Resolve, ErrorKind::UnexpectedResolveAddress)),
        }
    }

    struct SocketState {
        abort_stream: TcpStream,
        read_closed: AtomicBool,
        write_closed: AtomicBool,
        aborted: AtomicBool,
    }

    impl SocketState {
        fn check(&self, stage: Stage) -> Result<(), Error> {
            let closed = self.aborted.load(Ordering::Acquire)
                || match stage {
                    Stage::Read => self.read_closed.load(Ordering::Acquire),
                    Stage::Write => self.write_closed.load(Ordering::Acquire),
                    _ => false,
                };
            if closed {
                Err(error(stage, ErrorKind::Closed))
            } else {
                Ok(())
            }
        }

        fn abort(&self) {
            self.aborted.store(true, Ordering::Release);
            self.read_closed.store(true, Ordering::Release);
            self.write_closed.store(true, Ordering::Release);
            let _ = self.abort_stream.shutdown(Shutdown::Both);
        }

        fn shutdown(&self, direction: Shutdown) -> Result<(), Error> {
            if self.aborted.load(Ordering::Acquire) {
                return Ok(());
            }
            let done = match direction {
                Shutdown::Read => self.read_closed.swap(true, Ordering::AcqRel),
                Shutdown::Write => self.write_closed.swap(true, Ordering::AcqRel),
                Shutdown::Both => {
                    let read = self.read_closed.swap(true, Ordering::AcqRel);
                    let write = self.write_closed.swap(true, Ordering::AcqRel);
                    read && write
                }
            };
            if done {
                return Ok(());
            }
            match self.abort_stream.shutdown(direction) {
                // Linux can report ENOTCONN after both peers have finished their
                // halves. The requested shutdown is already complete in that case.
                Err(cause) if cause.kind() == io::ErrorKind::NotConnected => Ok(()),
                result => result.map_err(|cause| {
                    // The state remains closed even if native shutdown failed.
                    io_error(Stage::Shutdown, cause)
                }),
            }
        }
    }

    impl Drop for SocketState {
        fn drop(&mut self) {
            let _ = self.abort_stream.shutdown(Shutdown::Both);
        }
    }

    /// A wake-up handle for the host to abort established blocking I/O from a
    /// different thread. It cannot read, write, reconnect, or expose addresses.
    #[derive(Clone)]
    pub struct AbortHandle(Arc<SocketState>);

    impl AbortHandle {
        pub fn abort(&self) {
            self.0.abort();
        }
    }

    impl fmt::Debug for AbortHandle {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("AbortHandle([redacted])")
        }
    }

    /// Sole tunnel owner until explicitly split into one reader and one
    /// writer. It retains no destination or credentials. Errors in data I/O
    /// abort both directions, so a partially written request cannot be retried
    /// on this tunnel. `read` returns Ok(0) for orderly peer EOF; `read_exact`
    /// treats premature EOF as an error. Drop closes both directions.
    pub struct SocksConnection {
        stream: Option<TcpStream>,
        state: Arc<SocketState>,
        bound: BoundEndpoint,
    }

    impl fmt::Debug for SocksConnection {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("SocksConnection([redacted])")
        }
    }

    impl SocksConnection {
        pub fn connect(
            proxy: SocketAddr,
            destination: &Destination,
            authentication: Authentication<'_>,
            options: ConnectOptions,
            cancellation: &Cancellation,
        ) -> Result<Self, Error> {
            let packet = wire::encode_connect(destination);
            let (stream, bound) =
                exchange(proxy, authentication, Some(&packet), options, cancellation)?;
            let abort_stream = stream
                .try_clone()
                .map_err(|cause| io_error(Stage::ProxyConnect, cause))?;
            let state = Arc::new(SocketState {
                abort_stream,
                read_closed: AtomicBool::new(false),
                write_closed: AtomicBool::new(false),
                aborted: AtomicBool::new(false),
            });
            let bound = bound.ok_or_else(|| error(Stage::ProxyReply, ErrorKind::UnexpectedEof))?;
            Ok(Self {
                stream: Some(stream),
                state,
                bound,
            })
        }

        pub fn bound_endpoint(&self) -> &BoundEndpoint {
            &self.bound
        }

        pub fn abort_handle(&self) -> AbortHandle {
            AbortHandle(self.state.clone())
        }

        pub fn read(&mut self, bytes: &mut [u8], control: &IoControl) -> Result<usize, Error> {
            read_data(self.stream.as_mut(), &self.state, bytes, control, false)
        }

        pub fn read_exact(&mut self, bytes: &mut [u8], control: &IoControl) -> Result<(), Error> {
            read_data(self.stream.as_mut(), &self.state, bytes, control, true).map(|_| ())
        }

        pub fn write_all(&mut self, bytes: &[u8], control: &IoControl) -> Result<(), Error> {
            write_data(self.stream.as_mut(), &self.state, bytes, control)
        }

        pub fn shutdown(&self, direction: Shutdown) -> Result<(), Error> {
            self.state.shutdown(direction)
        }

        /// One owned reader and one owned writer can run concurrently. There
        /// are no public raw streams or arbitrary clone APIs that could race
        /// another reader's timeout or consume its bytes. Dropping a half shuts
        /// its direction; any data I/O error or explicit abort closes both.
        pub fn into_split(mut self) -> Result<(SocksReader, SocksWriter), Error> {
            self.state.check(Stage::Read)?;
            self.state.check(Stage::Write)?;
            let stream = self
                .stream
                .as_ref()
                .ok_or_else(|| error(Stage::Read, ErrorKind::Closed))?;
            let read_stream = stream
                .try_clone()
                .map_err(|cause| io_error(Stage::Read, cause))?;
            let write_stream = self
                .stream
                .take()
                .ok_or_else(|| error(Stage::Write, ErrorKind::Closed))?;
            Ok((
                SocksReader {
                    stream: read_stream,
                    state: self.state.clone(),
                },
                SocksWriter {
                    stream: write_stream,
                    state: self.state.clone(),
                },
            ))
        }
    }

    impl Drop for SocksConnection {
        fn drop(&mut self) {
            if self.stream.is_some() {
                self.state.abort();
            }
        }
    }

    fn read_data(
        stream: Option<&mut TcpStream>,
        state: &SocketState,
        bytes: &mut [u8],
        control: &IoControl,
        exact: bool,
    ) -> Result<usize, Error> {
        // An intentional half-close does not abort the remaining direction.
        state.check(Stage::Read)?;
        let result = (|| {
            let stream = stream.ok_or_else(|| error(Stage::Read, ErrorKind::Closed))?;
            let count = if exact {
                read_exact(stream, bytes, control, Stage::Read)?;
                bytes.len()
            } else {
                read_some(stream, bytes, control, Stage::Read)?
            };
            control.remaining(Stage::Read)?;
            Ok(count)
        })();
        let result = if state.aborted.load(Ordering::Acquire) {
            Err(error(Stage::Read, ErrorKind::Closed))
        } else {
            result
        };
        if result.is_err() {
            state.abort();
        }
        result
    }

    fn write_data(
        stream: Option<&mut TcpStream>,
        state: &SocketState,
        bytes: &[u8],
        control: &IoControl,
    ) -> Result<(), Error> {
        state.check(Stage::Write)?;
        let result = (|| {
            let stream = stream.ok_or_else(|| error(Stage::Write, ErrorKind::Closed))?;
            write_all(stream, bytes, control, Stage::Write)?;
            Ok(())
        })();
        let result = if state.aborted.load(Ordering::Acquire) {
            Err(error(Stage::Write, ErrorKind::Closed))
        } else {
            result
        };
        if result.is_err() {
            state.abort();
        }
        result
    }

    pub struct SocksReader {
        stream: TcpStream,
        state: Arc<SocketState>,
    }

    impl SocksReader {
        pub fn read(&mut self, bytes: &mut [u8], control: &IoControl) -> Result<usize, Error> {
            read_data(Some(&mut self.stream), &self.state, bytes, control, false)
        }

        pub fn read_exact(&mut self, bytes: &mut [u8], control: &IoControl) -> Result<(), Error> {
            read_data(Some(&mut self.stream), &self.state, bytes, control, true).map(|_| ())
        }

        pub fn shutdown(&self) -> Result<(), Error> {
            self.state.shutdown(Shutdown::Read)
        }
    }

    impl fmt::Debug for SocksReader {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("SocksReader([redacted])")
        }
    }

    impl Drop for SocksReader {
        fn drop(&mut self) {
            let _ = self.state.shutdown(Shutdown::Read);
        }
    }

    pub struct SocksWriter {
        stream: TcpStream,
        state: Arc<SocketState>,
    }

    impl SocksWriter {
        pub fn write_all(&mut self, bytes: &[u8], control: &IoControl) -> Result<(), Error> {
            write_data(Some(&mut self.stream), &self.state, bytes, control)
        }

        pub fn shutdown(&self) -> Result<(), Error> {
            self.state.shutdown(Shutdown::Write)
        }
    }

    impl fmt::Debug for SocksWriter {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("SocksWriter([redacted])")
        }
    }

    impl Drop for SocksWriter {
        fn drop(&mut self) {
            let _ = self.state.shutdown(Shutdown::Write);
        }
    }
}
