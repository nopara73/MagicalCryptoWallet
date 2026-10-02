# SOCKS5 migration handoff

State: **ready for bounded host integration; the readiness caller patch is supplied separately**.
Worker: `01a0fc2a-cae8-77f2-8971-024ca40ccb64`, slug `socks5`.
Source and tests commit: **`0c36c9f8aa8689e697a2dbe46d55206b6f75a8c3`**, pushed normally to `origin/master` and verified as its ancestor.
Initial caller snapshot: `82127991068522210cdcf77080dc9b819502e486`.
Publication reconciled onto `6365f3244d` without changing another worker's files.
The unpublished initial commit `666ee0f5ed9d58678ddce8840af40804f10fe8b9` was reconciled after a stale local credential helper prevented the first push. Use the published commit above.

## Ownership and integration boundary

This track owns only:

- `mcw/src/socks5.rs`
- `mcw/src/socks5/probe_service.rs`
- `mcw/tests/socks5_wire.rs`
- `mcw/tests/socks5_transport.rs`
- `mcw/tests/socks5_verify.ps1`
- `mcw/tests/socks5_probe_*` (service tests, adapter fixture, actual-caller harness, verifier, patch generator, and integration patch)
- `Contrib/McwMigration/Handoffs/socks5.md`

The host owner in thread `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc` owns `lib.rs`, Cargo manifests, command/bridge dispatch, platform bindings, managed adapters, production callers, packaging, release checks, and the common migration ledger. The published QR foundation already declares `pub mod socks5;`. No extra Cargo package or shipping executable was created. Test executables and harnesses stay in ignored worker artifacts. The coordinator owns incorporation of the exact bounded patch; the worker does not interrupt active host work or publish shared integration files.

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
- `Cancellation::{new, from_flag, cancel, is_cancelled}`, `IoControl::{new, with_poll_interval}`, `ConnectOptions`, `AbortHandle::abort`, `Error`, `ErrorKind`, and `Stage`.

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

The host owner's published Darwin transport correction uses nonblocking established reads/writes and checks cancellation between waits bounded by the smaller of the requested polling interval and the default 50 ms. `AbortHandle` marks both directions closed and shuts down each socket half separately; pending I/O observes closure through that polling loop. OS scheduling can add latency. One writer and one reader can run concurrently after splitting; competing readers are excluded by ownership. Drop closes the connection or its owned split direction even if an abort handle outlives it. This worker consumes that published correction unchanged and does not edit the host owner's transport/deadline work.

`read` returns `Ok(0)` for orderly peer EOF while preserving the writable half. `read_exact` reports premature EOF. Explicit half-close is idempotent and preserves the other direction. A data timeout, cancellation, native I/O error or failed exact read aborts both directions; subsequent writes are rejected. A write error does not prove zero bytes were sent: the OS may already have accepted some or all of the buffer. The host must never automatically replay payments or protocol requests based solely on that error. The Windows tests accept reset/abort as well as FIN when verifying failed-handshake closure with unread bytes.

## Reserved host/bridge operation proposal

Reserve **`0x0800`–`0x08ff`**. Only the bounded `0x0805` service and a separate host/caller patch are implemented in this handoff. The other operations below remain proposals. Production Rust callers should call the module directly. A broader transitional adapter would need this host-owned design:

| Operation | Proposed host action |
| --- | --- |
| `0x0800` | CONNECT and allocate an opaque, session-owned connection ID only after success |
| `0x0801` | Bounded read, returning bytes or explicit EOF |
| `0x0802` | Bounded write with one absolute request deadline |
| `0x0803` | Half-close or full-close |
| `0x0804` | Abort an active connection or cancel an in-flight connect |
| `0x0805` | Implemented loopback no-auth readiness probe; exact incorporation patch supplied below |
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

NBitcoin **10.0.13 remains retained**, including its other Bitcoin types, P2P protocol, keys, transactions, RPC and tests. The application caller snapshot contained 194 source files importing `NBitcoin` across the core/client/UI/backend/coordinator directories. Direct PackageReferences also remain in `MagicalCryptoWallet/MagicalCryptoWallet.csproj`, `Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj`, and `ThirdParty/WabiSabi/interop/WabiSabiInterop.Tests/WabiSabiInterop.Tests.csproj`. `NBitcoin.Secp256k1` **3.1.6** remains in `ThirdParty/WabiSabi/csharp/WabiSabi/WabiSabi.csproj`. This track removes no NuGet reference and claims no whole-package removal. Use these current-source audit commands at integration because parallel scope changes can move the graph:

```powershell
rg -n 'SocksSettingsBehavior|DnsSocksResolver|GetHostAddressesAsync|NetworkCredential|WebProxy|NoAuthHandshakeMsg' MagicalCryptoWallet MagicalCryptoWallet.Client
rg -l 'using NBitcoin' MagicalCryptoWallet MagicalCryptoWallet.Client MagicalCryptoWallet.Fluent MagicalCryptoWallet.Backend MagicalCryptoWallet.Coordinator
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

1. Apply canonical LF `privacy-control-callers.patch` and `privacy-control-host.patch` from the pinned privacy revision first, then `mcw/tests/socks5_probe_caller.patch` in the host owner's isolated checkout. Preserve other caller patches and reconcile only narrow hunks when their context moves. Do not replace `TorProcessManager` as a whole.
2. Run the owned verifier against that incorporated source and prove `TorProcessManager.IsTorRunningAsync` invokes `0x0805` in the actual host. Preserve result events and cancellation propagation; reject malformed payloads/replies and emit safe failure categories.
3. Re-audit the one-package empty external Cargo graph and final Windows imports, then verify the same bounded leaf on Linux x64/ARM64 and macOS x64/ARM64. Keep the managed application and Tor explicitly transitional.
4. Keep NBitcoin, the Tor daemon/control/bootstrap, peer/crawler SOCKS behavior, remote DNS, HTTP/TLS, and WebSocket dependencies retained. Their larger migration is outside this existing bounded assignment. No Tor-rewritten or full-application-ready claim follows from this handoff.

## Bounded readiness caller follow-up

Initial bounded follow-up source commit: **`00f0cab93a4f37a69a82174f9539a9459f483fa4`**, pushed normally to `origin/master` and verified as its ancestor. Its original caller fixture/patch is superseded by the compatibility correction below; its unchanged Rust service remains the implementation.

The accepted leaf is **`TorProcessManager.IsTorRunningAsync` only**. Its managed no-auth socket handshake is replaced by `McwSocksProbe.CheckAsync`, which sends a typed request to the published `IMcwApplicationServices` boundary. The Rust service owns the TCP socket and all SOCKS bytes. This removes the caller's one-shot read assumption, handles fragmented method replies correctly, closes every probe socket, and bounds a stalled proxy. The result means SOCKS method negotiation succeeded; it proves neither Tor bootstrap nor an external connection.

The corrected integration patch is `mcw/tests/socks5_probe_caller.patch`, prepared in the private `.artifacts/mcw-socks5-privacy-check` worktree from **`52957dbc28e0f8f9b6acc65f6b6773415866988b`**, after applying the exact canonical LF privacy patches from **`ee4319b1a3ff6e456eda916273f50a12ae2ffdf6`**. Both scopes remain patch-only on `master`. The readiness patch changes only:

- `mcw/src/app.rs`: one dispatch arm for `0x0805`.
- `MagicalCryptoWallet/Mcw/Network/McwSocksProbe.cs`: the typed transitional adapter.
- `MagicalCryptoWallet/Tor/TorProcessManager.cs`: the old handshake field/import and only the readiness method.
- `MagicalCryptoWallet.Coordinator/Tor/CoordinatorTorProcessManager.cs`: explicit retained external-coordinator readiness role.
- `MagicalCryptoWallet.Coordinator/TorManagerService.cs`: one construction expression selecting that external role.

The dispatch and actual caller files were modified **only in that private verification checkout**. They are not staged as this worker's production integration. The committed service lives under this worker's `mcw/src/socks5/` ownership. The privacy/control worker owns the public optional reply-reader constructor, readonly delegate field and `InitTorControlAsync` policy; all remain byte-for-byte unchanged by readiness's hunks. Readiness fixture construction explicitly supplies `TorControlReplyReader.ReadReplyAsync`, which selects the wallet's Rust reader. There is no private-constructor/factory requirement in the pinned privacy revision. The coordinator dispatches incorporation when the QR owner is idle.

The external coordinator is unhosted and shares `TorManager`'s readiness calls. Selecting only its managed control parser would still break startup if the shared readiness method required an absent `mcw` service. Its explicit subclass overrides **only** `IsTorRunningAsync`, retains the no-auth local SOCKS probe, and supplies `CoordinatorTorControlReplyReader.ReadReplyAsync` to the unchanged base constructor. The hosted service selects that subclass explicitly; it never detects service availability or falls back from a failed Rust call. Fragmented replies, safe diagnostics, cancellation, result events and socket disposal remain covered. This preserves the coordinator's existing managed role; it does not migrate that server's transport or Tor backend.

### Exact operation contract

`0x0805` request v1 is `[1, ATYP, literal proxy octets, port big-endian]`: ATYP `1` has four IPv4 octets and an 8-byte payload; ATYP `4` has sixteen IPv6 octets and a 20-byte payload. Only loopback addresses and nonzero ports are accepted. Domains, non-loopback addresses, IPv4-mapped IPv6, scoped IPv6, truncation, and trailing bytes are rejected. The managed adapter also rejects `DnsEndPoint`, mapped/scoped IPv6, and zero ports before sending a request. Neither layer resolves DNS.

The operation sends only `[5, 1, 0]`, accepts only `[5, 0]`, and closes the socket. There is no credential, destination, CONNECT, application payload, direct fallback, retry, or persistent connection ID. Once native dispatch starts, a 125-ms TCP-connect cap and **one configured 250-ms native dispatch deadline** cover its socket operation; 10-ms I/O polling never resets that deadline. OS timing and scheduling can add latency. This budget excludes time queued behind other synchronous bridge work. The tests' less-than-one/two-second assertions are responsiveness sanity bounds and do not prove an exact 250-ms wall-clock limit.

The response is exactly `[1, ready:0/1, failure category]`. Success requires `[1,1,0]`; failure requires `ready=0` and a nonzero category. Categories are `None=0`, `Io=1`, `TimedOut=2`, `InvalidVersion=3`, `MethodRejected=4`, `UnexpectedEof=5`, `Closed=6`, `Cancelled=7`, `Protocol=8`. Invalid requests use the host error frame with code `1` and a static message; unknown service operations use code `3`. No endpoint or raw OS error enters replies or diagnostics.

The production caller publishes exactly one `TorConnectionStateChanged` event for a successful or failed probe and logs only a safe category/static service-unavailable message. Cancellation propagates without a readiness event. The typed adapter requires the application-host service binding; a missing binding is visible and never falls back to a managed socket.

### Cancellation boundary

The earlier pinned host dispatch is synchronous. Its narrow arm supplies a fresh `Cancellation` token, so bridge cancellation does **not** immediately abort that fixture's native socket. The existing managed bridge stops the caller waiting promptly and drains the late reply; after its dispatch begins, the native probe uses the 250-ms socket budget plus OS scheduling. Queued requests can wait longer, and no total queued bridge latency bound is claimed. The actual-host harness covers mid-call managed cancellation, native socket closure, absence of a readiness event, and a subsequent healthy request. The later shared-flag constructor described below enables the host owner's updated cancellation composition without changing this historical proof.

### Reproduction and evidence

In a fresh isolated checkout containing the owned service files, apply both pinned privacy patches first, then the readiness patch, or generate readiness from that prepared base with `python mcw/tests/socks5_probe_prepare.py --apply-private`. Normalize checked-out `.patch` CRLF to the canonical published LF bytes before applying; C# files use LF. The generator refuses a normal shared checkout and must run before readiness's five integration files are patched. Use `git apply --check mcw/tests/socks5_probe_caller.patch` under the shared publication lock before incorporation. Unified-diff context lines intentionally contain a leading space; validate patch applicability rather than stripping its context whitespace. Then run:

```powershell
& .\mcw\tests\socks5_probe_verify.ps1 -VerifyPrivacyControl -VerificationBase 52957dbc28e0f8f9b6acc65f6b6773415866988b -NativeLibraryPath 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\ThirdParty\WabiSabi\c\build-win\libwabisabi.dll'
```

The verifier acquires one shared build slot, checks 2 GiB free memory, uses one Cargo/MSBuild job, and installs nothing. It builds the actual native host through `Contrib/Mcw/build-windows.ps1`, checks the empty external Cargo graph and PE imports, runs strict module lint/format checks and 16 wire + 29 transport + 4 probe-service tests, compiles the real core caller and actual `ManagedApplicationHost`, and launches a test-only managed fixture under that native host. All listeners and data are synthetic loopback fixtures; no Tor process, wallet, keys, payment, or live destination is used. Temporary executable copies remain ignored test artifacts and are not shipping helpers.

The initial verifier passed at **2026-10-02T12:30:02.0948964Z**, using Rust 1.99.0 on Windows x64. All **49 Rust tests** passed (16 wire, 29 transport, 4 service; none ignored), strict formatting/warning/Clippy checks passed, and the real managed core/caller build finished with zero warnings and errors. Its **31 synthetic checks** comprise **13 fake-service adapter checks** (reply validation and invalid proxy rejection) and **18 actual Rust-host/production-boundary checks** (fragmented greeting, rejection/malformed/truncated/EOF, stalled configured-budget probe, refusal, cancellation/events, cleanup/late replies, invalid requests and subsequent healthy requests). These are not 31 real-host checks. No Tor process or wallet data was used. Diagnostic scans found no test endpoint, destination, or isolation payload.

The tested actual native host SHA256 is **`c3fe441f3ade9cfe7f6a658e53ea1e760cee5f2d2536d62781a49f9c29290416`**. The builder's full PE import audit and `dumpbin /dependents` showed only native Windows system imports (`kernel32`, `shell32`, `kernelbase`, `api-ms-win-core-synch-l1-2-0`, `ntdll`, and `WS2_32`), with no VCRUNTIME/MSVCP/MSVCR/libgcc/libstdc++/libc++ import. This is evidence for the patched Windows host fixture, not the final managed/Tor installer or four other targets. Cargo metadata contained one `mcw` package with no external normal/build/dev dependency. No manifest, toolchain, NuGet reference, or shipping executable was added by this follow-up.

Initial canonical LF fingerprints: superseded patch **`6c67298405f6dffd586f49b3b0a98a68be1fc3ece89ca2112efae3470e6c149b`**; unchanged probe service **`6a89e2ed0ca9a75dbaefb4f3a4a773c6ce92537b20b122a6eab51d45fb544e13`**; module with service declaration **`89efabe8e4e49903e0790b7a24c0a62bd140ed176699217952a78eac15d234ed`**. The tested checkout module's raw **CRLF** fingerprint is **`8e7e1acc44f6ad52bef8a1a8510f27003e336d35471287b647011895171d2683`**; the old LF label was incorrect. Initial evidence is preserved under `.artifacts/mcw-socks5-host-check/.artifacts/socks5-probe/`, including raw `source-fingerprints.json` and canonical `committed-fingerprints.json`. Four non-Windows target executions and production incorporation remain unverified by this worker.

## Composed readiness/control compatibility correction

Compatibility correction commit: **`67785fbf14292813994cd684db75594426ae9342`**, pushed normally to `origin/master` and verified as its ancestor.

The isolated candidate pins the old privacy codec/adapter/dispatcher together at `ee4319b1a3ff6e456eda916273f50a12ae2ffdf6` and the original readiness service at `00f0cab93a4f37a69a82174f9539a9459f483fa4`. No active peer checkout is edited. The privacy worker is correcting its incremental codec independently; that forthcoming adapter must be incorporated with its matching session dispatcher and separately reverified. This proof does not certify that unpublished revision or activate either patch on `master`.

The corrected readiness fixture records its 13/18 check split, explicitly selects the wallet Rust reader, and reports `nativeDispatchBudgetMs=250`, `exactDeadlineTimingProven=false`, and `bridgeQueueBudgetMs=null`. The preparer removes only the handshake field, preserving privacy's constructor/delegate fields and control-init body. Two additional owned fixtures supply and verify the retained explicit unhosted coordinator readiness role. The verifier runs privacy's unchanged published production reader probe in the retained GUI mode against the **same combined native binary**, then checks the actual external coordinator project and constructor policy in an unhosted synthetic process. All outputs remain ignored non-shipping artifacts under `.artifacts/mcw-socks5-privacy-check/.artifacts/socks5-probe/`.

The historical composed verifier passed at **2026-10-02T13:06:38.2921379Z** on Windows x64: all **49 Rust tests**, **13 fake-service adapter checks**, **18 actual Rust readiness boundary checks**, privacy's unchanged fixture's **102 assertions per GUI/daemon mode**, and **15 unhosted coordinator role checks**. Privacy counts are its fixture totals, including local/fault validation; they are not a count of Rust requests. All managed builds completed with zero warnings/errors. The external-role proof covers the actual `TorManagerService` construction, virtual readiness dispatch, retained managed parser without a host binding, fragmented/rejected/truncated/EOF replies, refusal, pre/mid-call cancellation, socket closure, and wallet no-host rejection without a managed socket or readiness event. No Tor process or wallet data was used.

That historical composed native binary for both hosted fixtures has SHA256 **`9248f939e1221e94147c9d3a33f8707ed658b7d4abd7f210ea8e08f06e06a8b4`**; its import audit contains only the native Windows DLLs listed above. Current readiness patch canonical LF SHA256 is **`4b37e974ed376624e5e7d90180d24230efb0548f1523a83103d630aaabde3182`**. Committed machine-readable evidence is `mcw/tests/socks5_probe_combined_evidence.json`, with pinned revisions, grouped counts, raw versus LF source fingerprints, unchanged privacy policy, native fingerprint and target limits. Detailed historical `verification.json`, build/diagnostic logs, GUI/daemon results, coordinator results, and `privacy-policy-preserved.txt` remain under the composed ignored evidence directory. Publication of these owned fixtures/patch/evidence does not activate the production dispatcher or callers.

The subsequent automation-removal publication removed native daemon mode. The current verifier tests and copies only the retained GUI fixture executable. The earlier dual-mode evidence remains a historical pinned fixture result; no daemon support or removed project is restored. The concurrent daemon-cleanup edits to this handoff's caller inventory/audit command are retained.

The corrected GUI-only verifier passed at **2026-10-02T13:50:50.1537326Z** in a fresh isolated checkout of **`6af4217e820dcfa689e745f011d8df88d159202e`**, with the exact pinned privacy and readiness patches applied privately. All **49 Rust tests**, **13 fake-service adapter checks**, **18 actual Rust readiness boundary checks**, **102 GUI privacy assertions**, and **15 unhosted coordinator role checks** passed. The real core, client and external coordinator builds finished with zero warnings/errors. The privacy constructor, every readonly field and complete `InitTorControlAsync` body compare byte-for-byte unchanged by readiness. The new privacy worker's session adapter/dispatcher revision remains outside this proof and must be incorporated as a matched pair.

The GUI-only composed native host SHA256 is **`80fb47a5d72d3a1aeea8f3f8ada8298347f813be29b170687c6e595d4e116a3b`**, with only native Windows system DLL imports. The current module's canonical LF SHA256 is **`c8455e3223695e81270de248b240a8e1524ab526dfd4a91febbb6f97dc721495`**, reflecting the host owner's published transport correction; the earlier `89ef...` module hash remains a historical fingerprint. The final verifier's canonical LF SHA256 is **`27723c9de54b13971c52dfd850eb4047fce0dbe279f0a64a73159b3b1aa7e88b`**. Current logs, grouped results, native imports, policy comparison and consolidated evidence are retained under `.artifacts/mcw-socks5-latest-gui/.artifacts/socks5-probe/`. The committed evidence also preserves both earlier successful pinned runs and the failed intermediate checks.

Fresh managed verification uses `-NativeLibraryPath` to supply the existing retained `libwabisabi.dll` copy prerequisite explicitly. Its SHA256 is **`fbc41759c3623e4232930b7e37541dce41cae2dad81c367d0b1e78c92711b3b3`**; all 29 wrapper C/header/CMake source files matched this checkout before use. No native dependency was downloaded or built for this follow-up, and no WabiSabi cryptography was exercised. This retained managed prerequisite is separate from the native `mcw` import audit; the full managed/Tor runtime graph remains transitional.

The earlier current-GUI attempt at `5ba626f5fcb434c293a4b7334be39a02b7b8ea00` passed the native build and 49 Rust tests but failed managed compilation in the obsolete `RpcObjectCodec` after RPC removal. Its log is preserved under `.artifacts/mcw-socks5-current-gui/.artifacts/socks5-probe/managed-build.txt`. The automation cleanup at `6af4217e...` removed that obsolete adapter; the fresh combined GUI proof above verifies the resulting managed build. No old RPC or daemon code was restored.

One rerun on the old `52957dbc...` pin timed out in `fragmented_method_auth_and_reply_with_maximum_lengths` at `ProxyReply`. Its failure log remains `.artifacts/mcw-socks5-privacy-check/.artifacts/socks5-probe/transport-tests-timeout-rerun.txt`. Inspection confirmed that the fixture sleeps after 262 single-byte reply writes within a three-second total deadline; the exact scheduler cause is unproven. A subsequent old-pin run passed at **2026-10-02T13:39:09.2095442Z**, and the current GUI-only run above also passed. This worker changed neither the shared transport nor its deadline fixture during the correction; no exact wall-clock deadline or general flakiness-resolution claim follows from those passes.

## Shared cancellation flag follow-up

`Cancellation::from_flag(flag: Arc<AtomicBool>) -> Self` wraps the exact supplied host flag without copying or resetting its current value. An already cancelled flag remains cancelled; external stores are visible to the wrapper and its clones, and `Cancellation::cancel()` updates the same flag. Existing `new`, `cancel` and `is_cancelled` semantics remain unchanged. This lets the host owner pass its per-request Inbox flag to `PROBE` so the native polling loop can observe reader-side cancellation while the synchronous dispatch is executing.

Native verification passed at **2026-10-02T16:16:51.5594738Z** against isolated base **`2889a0710d3190a175a8ca35e4e18afac835ac0e`** with Rust 1.99.0 on Windows x64: **16 wire, 30 transport and 5 probe-service tests**, **51 total**, none failed or ignored; strict module Clippy/warnings and formatting also passed. The new sharing test checks external-thread cancellation, cloned/independent wrapper observations, wrapper-to-host cancellation and an initially cancelled flag. The new loopback probe test sets the shared flag after receiving the no-auth greeting, verifies the unchanged three-byte `[1,0,Cancelled=7]` response and confirms socket EOF.

Evidence and test logs remain ignored under `.artifacts/mcw-socks5-shared-cancel/.artifacts/shared-cancel/`. Verified module canonical LF SHA256 is **`ada149ed109017d4d90cf0d3c5b3cdef25db701350291276738c5620f837f826`**. This component follow-up changes no shared app/lib/Inbox/bridge/CI file and adds no external crate or shipping executable. The host owner owns the actual five-operation Tor/readiness composition and current caller verification; this native test does not certify bridge cancellation routing, exact elapsed timing or other platforms. The prior composed GUI proofs above remain tied to their recorded revisions.
