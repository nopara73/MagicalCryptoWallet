# SOCKS5 migration handoff

State: **ready for host integration; production callers remain managed**.
Worker: `01a0fc2a-cae8-77f2-8971-024ca40ccb64`, slug `socks5`.
Source and tests commit: **`0c36c9f8aa8689e697a2dbe46d55206b6f75a8c3`**, pushed normally to `origin/master` and verified as its ancestor.
Initial caller snapshot: `82127991068522210cdcf77080dc9b819502e486`.
Publication reconciled onto `6365f3244d` without changing another worker's files.
The unpublished initial commit `666ee0f5ed9d58678ddce8840af40804f10fe8b9` was reconciled after a stale local credential helper prevented the first push. Use the published commit above.

## Ownership and integration boundary

This track owns only:

- `mcw/src/socks5.rs`
- `mcw/tests/socks5_wire.rs`
- `mcw/tests/socks5_transport.rs`
- `mcw/tests/socks5_verify.ps1`
- `Contrib/McwMigration/Handoffs/socks5.md`

The host owner in thread `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc` owns `lib.rs`, Cargo manifests, command/bridge dispatch, platform bindings, managed adapters, production callers, packaging, release checks, and the common migration ledger. Add `pub mod socks5;` in that owner's integration. No extra Cargo package or shipping executable was created. Temporary test executables and a metadata-only module harness stay under this worker's ignored `.artifacts/socks5-verification` directory. The coordinator owns the integration request; the worker does not interrupt active host work.

## Implemented behavior

The client implements the Tor TCP subset of [RFC1928 sections 3–6](https://www.rfc-editor.org/rfc/rfc1928) and [RFC1929](https://www.rfc-editor.org/rfc/rfc1929). It supports CONNECT with IPv4, IPv6 and domain address forms; exact big-endian ports; no-auth or username/password negotiation; complete standard reply-code classification; bounded, incremental reply decoding; and exact consumption of the reply before application bytes. GSSAPI, BIND and UDP ASSOCIATE are intentionally unsupported, matching the retained Tor use case. This is not a claim of implementing every mandatory feature of a general RFC1928 client.

[Tor's SOCKS specification](https://spec.torproject.org/socks-extensions.html) supplies remote `RESOLVE` (`0xf0`), IPv4 `RESOLVE_PTR` (`0xf1`), and named onion failures `0xf0`–`0xf7`. Every other reply byte is preserved as `Unassigned(byte)` and rejected. Remote resolve returns one address per reply, not a fabricated list of DNS results. PTR accepts IPv4 only. Unsupported reply address shapes fail closed.

The `wire` submodule uses only standard-library formatting/error traits and owned byte buffers. It has no socket, OS handle, managed dependency, IPC, DNS lookup, logging or unsafe code. Maximum domain/credential field size is 255 octets; maximum request/reply is 262 octets; maximum auth frame is 513 octets. Constructors reject empty domain names, NUL in domain names, overlong fields, CONNECT port zero, and empty credential fields. Authentication accepts arbitrary credential octets, including NUL. Domain octets are opaque; there is no UTF-8 validation, IDNA implementation, label normalization, trimming or case conversion. URI callers must supply the desired already-parsed host bytes. Scoped IPv6 addresses have no SOCKS wire field; host parsing must reject unsupported scope identifiers rather than silently discard them.

Only numeric `SocketAddr` proxy endpoints enter `TcpStream::connect_timeout`. Destination domains are forwarded unchanged with ATYP `0x03`. Destination literals enter the CONNECT frame, never another `TcpStream::connect` call. The module has no local destination resolver, automatic reconnect, destination retry or direct fallback. Supplying credentials offers **only method `0x02`**. A proxy selecting `0x00` or another unoffered method closes the socket without receiving credentials, CONNECT, or application payload. `Authentication::None` is an explicit caller policy, never a fallback. Legacy identity pairs and modern `<torS0X>0` username credentials are transmitted exactly; empty modern isolation fields are deliberately rejected. New isolation IDs must come from the host's native OS entropy source; this codec does not generate weak IDs or invent circuit identity policy.

Malformed versions/reserved fields/address types, authentication failures, reply failures, premature EOF, deadline expiration and cancellation close failed handshakes. Address/credential types and packets have redacted Debug output. Transport errors retain only a stage and safe enum classification, with no nested OS error or raw message. There are no log calls. Safe Drop overwrites credential buffers on a best-effort basis; compiler-proof erasure and erasure of caller-owned copies are not claimed.

## Public API

`mcw::socks5::wire` exposes:

- `DomainName::new(&[u8])`, `Address::{Ipv4, Ipv6, Domain}`, and `Destination::new(Address, u16)`.
- `Credentials::new(username, password)` and `Authentication::{None, UsernamePassword(&Credentials)}`. Credentials are borrowed during negotiation and are not retained by the connection.
- `Authentication::greeting` / `accept_method`, `encode_authentication` / `accept_authentication`, `encode_connect`, `encode_resolve`, and `encode_resolve_ptr`.
- `reply_frame_len` and `decode_reply`: `Ok(None)` requests more input; a completed reply returns the exact consumed length so trailing application data remains intact. Domain buffers are allocated only after bounded length validation.
- `ProtocolError`, `ReplyCode`, `Reply`, `BoundEndpoint`, `Packet`, and `AuthPacket`.

`mcw::socks5::transport` exposes:

- `SocksConnection::connect(proxy, &destination, authentication, options, &cancellation)`; the result is the tunnel owner after a successful CONNECT reply.
- `probe`, `resolve`, and `resolve_ptr` with the same proxy/auth/options/cancellation policy. `probe` checks method negotiation only; it cannot establish Tor identity, bootstrap completion or external reachability.
- `SocksConnection::{read, read_exact, write_all}` with `&IoControl`; `shutdown(Shutdown)`; `bound_endpoint`; `abort_handle`; and `into_split()` into exactly one `SocksReader` and one `SocksWriter`.
- `SocksReader::{read, read_exact, shutdown}` and `SocksWriter::{write_all, shutdown}`. They may run on separate host-owned threads. The API has no public raw stream or arbitrary reader cloning.
- `Cancellation::{new, cancel, is_cancelled}`, `IoControl::{new, with_poll_interval}`, `ConnectOptions`, `AbortHandle::abort`, `Error`, `ErrorKind`, and `Stage`.

Example of internal Rust ownership (native host integration, not a new CLI):

```rust
use mcw::socks5::{transport::*, wire::*};
use std::{net::SocketAddr, time::Duration};

let cancellation = Cancellation::new();
let credentials = Credentials::new(b"synthetic-isolation", b"synthetic-isolation")?;
let destination = Destination::new(
    Address::Domain(DomainName::new(b"example.invalid")?), 443,
)?;
let proxy: SocketAddr = "127.0.0.1:38150".parse()?;
let mut tunnel = SocksConnection::connect(
    proxy, &destination, Authentication::UsernamePassword(&credentials),
    ConnectOptions::default(), &cancellation,
)?;
let operation = IoControl::new(Duration::from_secs(10), &cancellation)?;
// Host protocol/TLS code reads/writes through tunnel with this operation budget.
// Credentials and destinations must never be put in diagnostic logs.
tunnel.shutdown(std::net::Shutdown::Both)?;
```

## Deadlines, EOF and cancellation

Default CONNECT options are a 3-second TCP proxy-connect cap, one 30-second total deadline covering TCP connect/greeting/auth/reply, and 50-ms I/O polling. Progress and stage changes never reset the absolute deadline. Timeout fields must be positive; polling is capped at 1 second. `IoControl` is an absolute application-operation deadline and can be shared across its fragments; create a new control for a new operation. Native timeout granularity and scheduling can add latency. Cancellation inside `connect_timeout` is observed immediately before and after that OS call and is bounded by the selected connect cap, **not** the polling interval. There is no hidden connection thread, detached retry, or direct connection while cancellation waits. Native nonblocking connect, if later needed for a tighter bound, belongs to the host platform layer.

Established I/O cooperatively checks cancellation at each polling timeout. `AbortHandle` also calls `Shutdown::Both` to wake a blocking socket operation from another host thread. One writer and one reader can run concurrently after splitting; same-direction timeout races and competing readers are excluded by ownership. Drop closes the connection or its owned split direction even if an abort handle outlives it.

`read` returns `Ok(0)` for orderly peer EOF while preserving the writable half. `read_exact` reports premature EOF. Explicit half-close is idempotent and preserves the other direction. A data timeout, cancellation, native I/O error or failed exact read aborts both directions; subsequent writes are rejected. A write error does not prove zero bytes were sent: the OS may already have accepted some or all of the buffer. The host must never automatically replay payments or protocol requests based solely on that error. The Windows tests accept reset/abort as well as FIN when verifying failed-handshake closure with unread bytes.

## Reserved host/bridge operation proposal

Reserve **`0x0800`–`0x08ff`**; no dispatch implementation was changed by this worker. Production Rust callers should call the module directly. A transitional managed adapter may need this host-owned design:

| Operation | Proposed host action |
| --- | --- |
| `0x0800` | CONNECT and allocate an opaque, session-owned connection ID only after success |
| `0x0801` | Bounded read, returning bytes or explicit EOF |
| `0x0802` | Bounded write with one absolute request deadline |
| `0x0803` | Half-close or full-close |
| `0x0804` | Abort an active connection or cancel an in-flight connect |
| `0x0805` | SOCKS negotiation probe |
| `0x0806` | Remote Tor RESOLVE |
| `0x0807` | Remote Tor IPv4 RESOLVE_PTR |

The host owns a bounded connection table, per-session unguessable/generation-safe IDs, per-request cancellation, one-reader/one-writer ordering, and cleanup at session exit. Cap bridge read/write frames (for example 64 KiB) and queued work before allocation; callers supply no arbitrary filesystem handle, process handle or raw socket. Keep the trusted proxy endpoint in host configuration. Credentials, destinations and returned application data are sensitive binary payloads, never diagnostic text. Error responses contain only stage/category/reply byte. The domain module must not import the bridge, C#, Avalonia, native TLS handles or platform APIs. Native TLS/protocol layers can use the bounded byte-I/O contract; this track provides a TCP tunnel and does not implement TLS, HTTP or WebSocket security.

## Remaining callers and dependencies

The following live caller graph was checked in the initial source and the reconciled remote publication snapshot. **These callers were not changed here.**

| Retained caller | Current behavior and integration/removal check |
| --- | --- |
| `MagicalCryptoWallet/Services/NodesManagement/P2pConnectionManager.cs`, `ConnectToPeerAsync` | NBitcoin `SocksSettingsBehavior`, `onlyForOnionHosts:false`, `streamIsolation:true`, then `Node.ConnectAsync`. Host must supply a native-generated fresh isolation pair per the peer connection policy and use the returned Rust tunnel for the actual peer protocol. |
| Same file, `VisitEndpointAsync` | NBitcoin `SocksSettingsBehavior`, `streamIsolation:false`, `networkCredential:null`, then `Node.ConnectAsync`. Preserve the explicit crawler policy or apply a separately authorized policy change; do not silently coalesce isolated normal peers into this group. |
| `MagicalCryptoWallet.Client/Global.cs`, P2P setup; `P2pConnectionManager.cs`, `SeedFromDnsAsync` | Constructs NBitcoin `DnsSocksResolver` with `StreamIsolation=true`; performs `GetHostAddressesAsync`, and selects up to 16 rounds for the Tor resolver. Replace with remote Rust RESOLVE and explicit isolation. Tor returns one address per exchange; retain bounded sampling/deduplication and failure behavior in the caller. |
| `MagicalCryptoWallet/BitcoinP2p/CompactFilterBehavior.cs` | Infers Tor transport from the presence of `SocksSettingsBehavior` to apply privacy-sensitive behavior. Replace with a trustworthy successfully-created tunnel/transport capability, not an unverified configuration flag. |
| `MagicalCryptoWallet/WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs`, `OnionHttpClientFactory` | .NET `HttpClientHandler` SOCKS behavior via `LoopbackBypassProxy`, `NetworkCredential(name,name)`, identity lifetime resolver and coordinator factory. Preserve group reuse/rotation and explicit pre-connection loopback routing policy; the Rust SOCKS module itself has no bypass. Migrate the actual HTTP/TLS caller before removing this path. Existing direct HTTP factories are independent policies, not SOCKS-failure fallbacks. |
| `MagicalCryptoWallet/Discoverability/NostrExtensions.cs`, `NostrClientFactory`; `MagicalCryptoWallet.Client/Global.cs` | NNostr / .NET `ClientWebSocket.Options.Proxy` uses a SOCKS URI without supplied auth. Migrate actual TLS/WebSocket ownership and honor the existing explicit proxy configuration. The coordinator announcer also uses this factory with no proxy; that separate direct-policy caller does not become a SOCKS caller. |
| `MagicalCryptoWallet/Tor/TorProcessManager.cs`, `IsTorRunningAsync`; `TorManager.cs` | Managed no-auth greeting probe. Rust `probe` can replace the wire check; retain process/control/bootstrap checks separately and remove address-bearing failure logs in the host migration. |
| `MagicalCryptoWallet/Tor/TorSettings.cs` and `MagicalCryptoWallet/MagicalCryptoWallet.csproj` | Retains Tor process, control/geoip data and bundled binaries. Current SOCKSPort enables `ExtendedErrors KeepAliveIsolateSOCKSAuth`. SOCKS does not rewrite Tor, circuit construction, its crypto, bootstrap, control authentication, or retained Tor runtime dependencies. |

NBitcoin **10.0.13 remains retained**, including its other Bitcoin types, P2P protocol, keys, transactions, RPC and tests. The application caller snapshot contained 194 source files importing `NBitcoin` across the core/client/UI/backend/coordinator/daemon directories. Direct PackageReferences also remain in `MagicalCryptoWallet/MagicalCryptoWallet.csproj`, `Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj`, and `ThirdParty/WabiSabi/interop/WabiSabiInterop.Tests/WabiSabiInterop.Tests.csproj`. `NBitcoin.Secp256k1` **3.1.6** remains in `ThirdParty/WabiSabi/csharp/WabiSabi/WabiSabi.csproj`. This track removes no NuGet reference and claims no whole-package removal. Use these current-source audit commands at integration because parallel scope changes can move the graph:

```powershell
rg -n 'SocksSettingsBehavior|DnsSocksResolver|GetHostAddressesAsync|NetworkCredential|WebProxy|NoAuthHandshakeMsg' MagicalCryptoWallet MagicalCryptoWallet.Client
rg -l 'using NBitcoin' MagicalCryptoWallet MagicalCryptoWallet.Client MagicalCryptoWallet.Fluent MagicalCryptoWallet.Backend MagicalCryptoWallet.Coordinator MagicalCryptoWallet.Daemon
rg -n 'NBitcoin' --glob '*.csproj' Directory.Packages.props
```

## Verification and runtime audit

Verified on **2026-10-02**, Rust **1.99.0**, edition **2024**, native **Windows x64**:

- `rustfmt --check`: passed.
- Public module compiled with `-Dwarnings`: passed.
- Standalone `clippy-driver -Dwarnings -Dclippy::all`: passed.
- **16 wire tests** and **29 loopback transport tests**, all passed, none ignored. Literal vectors and the fake proxy parser do not call the production request encoder to construct expectations.
- All 256 reply-code mappings, all method-selection bytes, all auth status bytes, all invalid version/reserved/type bytes, maximum lengths, every prefix of IPv4/IPv6/domain replies, and a deterministic 19,264-case adversarial parser corpus.
- Exact domain case/trailing dot and IPv4/IPv6/onion request bytes; maximum auth fields; byte-fragmented responses; every IPv6 reply split with a coalesced application banner; all 255 failure replies closing without application data; auth downgrade/rejection; proxy refusal and unavailable proxy with a listening destination proving no direct fallback.
- EOF and truncated handshakes/replies; absolute multi-stage and slow-trickle deadlines; cancellation before and during handshake/data read; write timeout with a large OS-buffered payload and no replay; abort wake-up; concurrent split reader/writer; half-close and drop; remote resolve/PTR; IPv6 numeric proxy connection; redacted formatting.

Reproduce on Windows with the installed shared compiler (the verifier takes one exclusive build-slot lock and checks at least 2 GiB free):

```powershell
& .\mcw\tests\socks5_verify.ps1
```

The verifier does not install or mutate the toolchain. It compiles test-only executables and saves `verification.json`, `wire-results.txt`, and `transport-results.txt` under `.artifacts/socks5-verification`. It validates the full public module through a metadata-only wrapper outside shipping sources. The source SHA256 for the verified LF file is `2362972123a41681b1e874b88025bea6a5ee0258d8593f5ed6f05e529bd79adb`. Publication checked that all four reconciled Git file contents match the tested commit.

Direct portable test commands (put outputs in a unique ignored directory and use the shared build-slot policy):

```text
rustc --edition=2024 --test -Dwarnings mcw/tests/socks5_wire.rs -o .artifacts/socks5_wire_tests
.artifacts/socks5_wire_tests --test-threads=1
rustc --edition=2024 --test -Dwarnings mcw/tests/socks5_transport.rs -o .artifacts/socks5_transport_tests
.artifacts/socks5_transport_tests --test-threads=1
```

There are **zero external crates or runtime helpers in this module**. Imports are exclusively Rust standard library. No Cargo manifest, dynamic binding, library-loading code, runtime installer or shipping executable was added. A read-only Cargo metadata snapshot of the host owner's current checkout showed one `mcw` package with an empty dependency list and no external build/dev dependency. That host checkout was still in progress; it is not this worker's completed source.

The temporary Windows test executable uses the installed default MSVC standard library. `dumpbin /dependents` found native Windows networking/system imports **and `VCRUNTIME140.dll`**. These ignored test executables are **not shipping artifacts** and must not be packaged. Production host packaging must use its first-party/native runtime plan and independently prove there is no VCRUNTIME/MSVCP/libgcc/libstdc++/libc++ shipping dependency. The component source's std-only design is not a production runtime audit. Linux x64/ARM64 and macOS x64/ARM64 compilation/runtime testing remain for the host's five-target integration; only the Windows target's standard library is currently installed in the shared toolchain, and this worker made no concurrent toolchain changes.

## Integration acceptance checks

1. Declare the module in the one `mcw` application; run these tests within its Cargo graph and re-audit an empty external dependency graph.
2. Route the actual retained peer, DNS, HTTP/WebSocket and readiness callers through owned Rust networking. Preserve explicit stream-isolation groups and rotation, typed destination octets, Tor capability/privacy checks, and cancellation ownership. User-visible privacy failures must identify safe categories and never trigger a direct connection or weaker auth.
3. Exercise synthetic production adapter flows against fake proxies, including downgrade, no local DNS, refused proxy, malformed/fragmented replies, cancellation, partial-data ambiguity, session cleanup, concurrent traffic and half-close. Never use live wallet identities/keys for this verification.
4. Re-run all five target builds and native network tests, then inspect final binaries/installers for non-OS runtime imports, companion helpers and external Cargo/NuGet dependencies. Keep Tor and the managed app explicitly labeled transitional while still retained.
5. Remove NBitcoin SOCKS behavior/resolver use only after all their live paths and the compact-filter transport test are replaced. Retain the NBitcoin package until its other callers are independently migrated. No Tor-rewritten or full-application-ready claim follows from this handoff.
