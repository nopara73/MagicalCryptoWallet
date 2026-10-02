# HTTP wire checkpoint and networking workstream

Worker `01a0fc46-15ed-7fe3-88dc-bba09fad5d45`, slug `http1`.
Verified wire implementation commit: **`05b7a3b3b5a446ff3893ac9faddb06f7be500c52`**.
Published by a normal direct push to `origin/master`; subsequent authenticated
fetch verified that the remote contains that commit. Caller inspection began at
`ff69d01b53be342ef672a5e0ebd8018217786a14`; publication reconciled in this
worker's isolated checkout onto `a96873b6a5` without staging peer files.

**This is an intermediate wire-engine checkpoint, ready for host registration.
The expanded application networking workstream is not complete. No production
HTTP caller, upstream package, TLS implementation, or Tor runtime was removed.**

## Ownership

Published checkpoint files:

- `mcw/src/http1.rs`
- `mcw/tests/http1_conformance.rs`
- `mcw/tests/http1_fixture.rs`
- `mcw/tests/http1_verify.ps1`
- this handoff

The human subsequently expanded this worker's responsibility to
`mcw/src/network_service/` and `MagicalCryptoWallet/Mcw/Network/`, secure
connections, cancellation/deadlines, proxy isolation, bounded concurrency,
redirects, HTTP orchestration and retained production HTTP flows. Exclusive
ownership of `WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs`
was proposed to QR before edits; shared/caller overlap must be resolved first.
The Content worker owns `content_service`, compression and managed Content leaves.
The Nostr worker owns its service/adapters and proposed Nostr callers including
`Services/UpdateManager.cs`; this worker will not edit those caller files.

QR thread `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc` retains shared manifests,
`lib.rs`, dispatch, platform unsafe bindings, lifecycle, packaging and the shared
ledger. This checkpoint needs that owner's `pub mod http1;` registration inside
the **one mcw package/executable**. It introduces no package, crate dependency,
runtime/library bundle, shipping helper executable or platform unsafe code.
Coordinator `01a0fc1e-7c20-76d3-bf81-cb1f68c9adb7` alone dispatches idle
incorporation requests. Scope/API messages do not constitute incorporation.

## Concrete API

`serialize_request(&Request, Limits) -> Result<EncodedRequest, Error>` validates
before returning any wire bytes. `Request` borrows method/target/header/body
octets. `RequestBody::{Empty, Bytes, Chunked{chunks,trailers}}` supports fixed and
chunked requests; the encoder emits one canonical framing field and generates
Trailer declarations. A supplied Content-Length must agree with the body;
duplicate request length fields, TE+CL, unsupported coding and caller-supplied
Trailer are rejected. Empty input chunks never emit premature terminators.
`EncodedRequest::{bytes,context,head_bytes}` permits a host to separate the head
from its body for Expect/Continue without sending or retrying anything itself.

Targets include origin, HTTP(S) absolute, CONNECT authority and OPTIONS asterisk
forms. The syntax check covers percent escapes, ASCII URI characters, ports,
IPv6/IPvFuture, duplicate Host, required HTTP/1.1 Host and absolute-form authority
agreement. It does not resolve DNS, implement IDNA, normalize URI paths or decide
whether a destination is permitted. CONNECT/TRACE reject content and chunked
request bodies. TRACE rejects known credential/cookie fields. The only supported
TE negotiation is empty or `trailers`, with `Connection: TE`; this engine never
advertises an unimplemented transfer compression coding.

`RequestContext::new(method,version,headers,limits)` supports an adapter that
already serializes requests; that adapter must independently validate its whole
outgoing request. Exact method case matters: `head` is not HEAD.

`ResponseDecoder::{new,feed,finish,abort,response,into_response,informational}`
parses one request's informational/final sequence. `feed` returns
`FeedResult{consumed,status:NeedMore|Informational|Complete}` and stops at each
informational head or final response boundary. Call again with only the unused
suffix after an informational event. A completed decoder consumes zero further
octets. It does not buffer a pipelined response or tunnel/protocol suffix.

`Response` holds informational heads, final `ResponseHead`, opaque deframed body,
separate trailers, `Framing`, `ConnectionUse` and aggregate `wire_bytes`.
`ResponseHead` preserves status/version/reason octets. `Header` preserves original
name case and the exact bytes after the colon, including OWS. Use
`trimmed_value()` for semantic interpretation and `is()` for case-insensitive
names. Repeated fields remain ordered and distinct; cookies are not joined.
Bodies can contain every octet. Content-Encoding is opaque and stays behind the
Content worker's independent size/ratio/work/checksum boundary.

`Framing::{NoBody,ContentLength(u64),Chunked,UntilClose,Upgrade,Tunnel}` and
`ConnectionUse::{Reusable,MustClose,Upgrade,Tunnel}` make the transport decision
explicit. Reusable describes only wire eligibility after the entire message;
the host must also check identity/origin, secure transport, completed writes and
liveness. Informational close is retained conservatively. Observed EOF changes
Reusable to MustClose. Upgrade requires matching offered protocol names and exact
protocol versions, HTTP/1.1, and both sides' upgrade negotiation.

Successful CONNECT follows framing precedence: it ignores even malformed CL/TE
values and detaches immediately after headers. Unsuccessful CONNECT uses normal
response framing. HEAD/204/304 end at headers; valid HEAD/304 length metadata is
not charged as a received body. 205 consumes explicit empty framing and rejects
content. Close-delimited responses complete only on `finish()`.

Call `finish()` only for verified orderly EOF. Reset, cancellation, timeout or
TLS truncation must call `abort()`; otherwise a partial close-delimited download
could be mistaken for completion. A framing/resource error poisons the decoder,
discards the message and never attempts resynchronization. The host must close
the transport on that error. Close framing cannot itself establish integrity of
an unknown-length representation; application checks remain necessary.

## Bounds and deliberate strict policy

Defaults are 8 KiB lines, 64 KiB per head, 128 fields/head, 16 MiB buffered body,
16 KiB trailers, 64 trailer fields, 8 informational heads, 1 KiB chunk lines,
65,536 chunks including the terminal chunk, and 64 MiB total wire bytes.
Absolute configurable ceilings are 64 KiB lines/chunk lines, 1 MiB heads/trailers,
4,096 fields/trailers, 256 MiB buffered body, 64 informational heads, 1,000,000
chunks and 1 GiB total wire bytes. All relevant additions/multiplications and
numeric parsing are checked; body allocation is incremental, not preallocated
from an untrusted Content-Length. Wire limits include informational heads, chunk
extensions, framing overhead and trailers, not just decoded content. Try-reserve
failures return an allocation classification. Host concurrency must also bound
the number of simultaneous decoders.

The implementation rejects bare LF/CR, obsolete folding, whitespace before a
field colon, control/NUL/DEL injection, invalid status/version, conflicting
lengths, TE+CL, unsupported/repeated/parameterized transfer codings, invalid chunk
extensions, truncated heads/bodies/chunks/trailers and count/byte abuse. Equal
response length duplicates/lists recover using one numeric length; output fields
remain untouched. It rejects framing fields prohibited in 1xx/204 rather than
leniently ignoring them. It rejects empty list elements in security/control
fields instead of repairing them. These are deliberate strict compatibility
choices; no claim is made of accepting every historically tolerated HTTP syntax.

Trailer fields cannot alter framing/routing/authentication/content controls or
connection-nominated fields. Unknown permitted syntax is stored inertly in the
separate trailer list; it is never merged into initial headers or trusted merely
because parsing succeeded. The caller must know a field's definition permits
generation/processing in trailers. Debug implementations redact peer/body/header
values; returned byte objects still require the caller's privacy discipline.

The complete serializer/decoder buffer bounded bodies. Streaming installer
downloads beyond those caps require a real host/content sink integration, not a
larger unbounded limit or fabricated streaming claim.

## Standards and source provenance

Primary references were read directly:
[RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html), especially sections
2–8, 9.2–9.3 and 11; and
[RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html), sections 5, 6.5,
7.2/7.8, 8.6, 9.3, 10.1.4 and 15. Test literals independently express those
rules; production responses are never generated by the encoder as an oracle.
Code and vectors were written first-party in this track under repository MIT
licensing. No third-party parser/implementation or extracted RFC code/pseudocode
was copied. RFC documents retain their own IETF Trust copyright/terms; they are
ignored reference evidence and are not shipped.

Retrieved plain-text source hashes:

| Source | SHA-256 |
|---|---|
| rfc9110.txt | `21c1cdce6ab0e5509b04d84a28000836c7a087cf786efe6f04877ebfff47232a` |
| rfc9112.txt | `e4f426bac6206b67fdf9e0da826154f70588db2133a0a86b15cde4ff725d8937` |

Implementation/evidence file hashes (verified LF working bytes at checkpoint):

| File | SHA-256 |
|---|---|
| mcw/src/http1.rs | `97195ccc52a5f3c2ec79b922a4c1beb4efd9c63c5501dce885d2a906bd7f3515` |
| mcw/tests/http1_conformance.rs | `8911651dc935f01758dd9ba2280f648a1d74ce3dadea844d2203c424b8678427` |
| mcw/tests/http1_fixture.rs | `d20309c414e50cb0c9dd84b10fea94c7622a754e32ebc6e5e7e584d5e8264cfc` |
| mcw/tests/http1_verify.ps1 | `6e79a65e764d162b4d5306883480d211fadd4685ce0b92e6d0216d96c2079297` |

Git may check out CRLF according to local configuration; hashes are octet-level,
so newline changes alter the hash without changing the verified source logic.

## Independent evidence

From `.artifacts/mcw-http1`:

```powershell
./mcw/tests/http1_verify.ps1
```

The verifier reuses installed Rust 1.99.0, edition 2024, holds an exclusive shared
heavy-build slot, requires 2 GiB free memory, and builds one item at a time. Its
ignored metadata harness imports the actual source, never a stub. No extra Cargo
package is created. It verifies formatting, compiler warnings denied and all
Clippy warnings denied, then compiles/runs debug conformance, loopback fixtures
and optimized conformance.

Verified Windows x64 results on 2026-10-02:

- 37 conformance tests passed in debug and optimized modes.
- 4 independent std TCP fixture tests passed; 10 synthetic exchanges exercised
  literal request comparisons, fragmented informational/chunked responses,
  opaque bodies, orderly close, HEAD, CONNECT and upgrades.
- 24,320 exhaustive single-octet mutations per conformance mode never panicked or
  overconsumed; 249 two-way partitions plus bytewise delivery covered five
  literal sequences; all 159 incomplete prefixes were rejected.
- Limits, framing conflicts, control injection, trailers, metadata lengths,
  pipelining, reuse, request serialization, negotiated upgrade and error poison
  behavior were checked separately from round trips.

Evidence resides at
`C:/Users/user/OneDrive/Documents/ChatGPT/MagicalCryptoWallet/.artifacts/mcw-http1/.artifacts/http1-verification/`:
`verification.json`, `conformance-results.txt`, `fixture-results.txt`,
`conformance-release-results.txt`, actual-source `lib.rs`, and `references/`.
Executables in that ignored directory are test tooling only.

Portable reproduction after installing the required target/toolchain through the
host's approved tooling (this worker did not install other targets):

```text
rustc --edition=2024 --test -Dwarnings mcw/tests/http1_conformance.rs -o <test-output>
<test-output> --test-threads=1 --nocapture
rustc --edition=2024 --test -Dwarnings mcw/tests/http1_fixture.rs -o <fixture-output>
<fixture-output> --test-threads=1 --nocapture
```

Only Windows x64 native runtime evidence exists for this checkpoint. Linux
x64/ARM64 and macOS x64/ARM64 native executions/packaging remain required. A
metadata compile or Windows test is not five-target runtime evidence.

## Concrete production caller and removal gate

| Current caller | Retained flow and required migration |
|---|---|
| WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs | Managed handler, proxy credentials/loopback routing, lifetime/pooling, retries, cancellation, backoff, Retry-After and automatic decompression. Replace transport with actual Rust service; coordinate Content; preserve identity/origin boundaries and no automatic payment replay. |
| WabiSabi/Client/WabiSabiHttpApiClient.cs | POST UTF-8 coordinator JSON, empty and JSON/error response bodies, HTTP/1.1, per alice/bob/satoshi identities. JSON/Content workers own semantic payload work; this service supplies actual secure transport. |
| FeeRateEstimation/FeeRateProviders.cs | Public/onion GET, User-Agent, successful UTF-8 JSON response. Must preserve explicitly selected routing and privacy policy. |
| Wallets/Exchange/ExchangeRateProvider.cs | Public GET with headers and UTF-8 JSON/error semantics. |
| Wallets/CpfpInfoProvider.cs | GET with linked caller cancellation and 20-second deadline. |
| Tor/StatusChecker/TorStatusChecker.cs | HTTPS GET and parsed status JSON; never downgrade to plaintext. |
| Blockchain/TransactionBroadcasting/TransactionBroadcaster.cs | POST transaction bytes; errors and uncertainty must not silently trigger unsafe replay. |
| Services/UpdateManager.cs | HTTP release download and content stream to disk, then signature/hash verification. Nostr worker owns this shared caller file; coordinate Rust HTTP use and a bounded streaming sink. |

These callers still run their original managed implementations at this
checkpoint. `Microsoft.Extensions.Http` remains referenced in
`MagicalCryptoWallet.csproj`, centrally versioned in `Directory.Packages.props`
and present in package locks. `System.Net.Http` remains a .NET framework/runtime
implementation and public compatibility type throughout those flows; it is not
removed merely because no explicit System.Net.Http package is listed. Transitive
Extensions options/logging/DI responsibilities and every shared factory caller
must be enumerated before removing the upstream package.

Published first-party SOCKS at `0c36c9f8aa8689e697a2dbe46d55206b6f75a8c3`
provides the actual tunnel protocol/transport for reuse. This wire engine does
not call or duplicate it. Existing Tor controller/process, bundled Tor binaries,
bootstrap/isolation policy and any broader routing runtime remain transitional;
a SOCKS codec is not a Tor replacement. HTTP/TLS sockets, trust roots, certificate
and hostname validation, crypto/native platform requirements, redirects, cookies,
authentication, retry safety, concurrency, host lifecycle and streaming content
handling remain application work. Linux TLS must have a first-party implementation
where native OS APIs do not supply one; OpenSSL/rustls/curl or plaintext/certificate
bypass are not acceptable substitutes.

## Host operation proposal and full acceptance

Reserve **`0x0B00–0x0BFF`**. Production Rust services should call this module
directly. Pure format bridge proposals, if needed during transition, are
`0x0B01` serialize, `0x0B02` start response, `0x0B03` feed, `0x0B04` orderly EOF,
`0x0B05` abort and `0x0B06` release. Host-owned handles must be typed, limited and
bound to one request/identity; arbitrary callers must not mark resets as EOF.
Full network operations in this range require a separate concrete service/frame
agreement; no dispatch or managed adapter is implemented by this checkpoint.

Completion requires real native/first-party secure connections, local synthetic
TLS/proxy/failure/privacy/cancellation/redirect tests, retained flow execution in
Rust, managed caller implementation removal from those flows, Content and Nostr
compatibility, frame fragmentation/ownership validation, one-executable packaging,
zero external Cargo/runtime audit and native runtime evidence on Windows x64,
Linux x64/ARM64 and macOS x64/ARM64. Package removal additionally requires no
remaining shared caller or packaged reference. The worker continues this expanded
workstream after publishing the wire checkpoint.
