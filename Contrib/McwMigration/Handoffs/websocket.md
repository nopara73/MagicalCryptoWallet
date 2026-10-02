# WebSocket protocol checkpoint and Nostr integration handoff

State: **standalone WebSocket protocol ready for integration; production Nostr/NNostr replacement is not complete. The human revoked the broad Nostr expansion on 2026-10-02; its unpublished drafts are preserved and must not be registered as production. This task is awaiting its narrower bounded caller assignment.**

Worker: `01a0fc4b-c3bd-7102-9c8b-8585004d6f51`.
Implementation commit: **`51f380d8c2d2977eb2161aa7cf60a214e77acf4c`**, normally pushed to `origin/master` and verified as its ancestor on 2026-10-02.
Source checkout: `.artifacts/mcw-websocket`, initially remote `748a961c78980c42bba293ff7ad1b9ca696566ec`. Publication copied only five owned, hash-matching files into a fresh checkout of then-current remote master `0a2dd5ee50`; staging/commit/push/fetch/ancestry verification held the shared exclusive `FileShare.None` publication handle. Shared and worker indexes were empty before staging.

## Scope and ownership

This checkpoint owns `mcw/src/websocket.rs`, `mcw/tests/websocket_*`, and this document. The withdrawn expansion wrote unpublished drafts only under `.artifacts/mcw-websocket/mcw/src/nostr_service/` and `.artifacts/mcw-websocket/MagicalCryptoWallet/Mcw/Nostr/`. Those drafts are outside this published checkpoint and are preserved for review, without a production/integration claim. No existing Nostr caller, project, package, shared manifest or legacy application path was edited. Former full-service/factory/role-relocation proposals were withdrawn. Small candidate boundaries are NIP19 public-key decoding or NIP01 canonical event-ID hashing; exact scope must follow the coordinator's narrowed assignment.

QR owns shared `lib.rs`, Cargo manifests, main/CLI/bridge dispatch, managed application contracts, platform unsafe bindings/entropy, lifecycle, packaging, and the common ledger. The network owner supplies authenticated origin/proxy/isolation-bound transport, HTTP parsing/serialization, TLS, cancellation and deadlines. The wallet-crypto owner supplies actual BIP340 services. The coordinator alone dispatches incorporation when QR is idle. No additional Cargo package, shipping executable, runtime or external crate was created. All test executables are ignored tooling that compiles actual source, with no substitute implementations.

## Implemented protocol and APIs

First-party safe Rust implements the unextended [RFC 6455](https://www.rfc-editor.org/rfc/rfc6455) client wire protocol. Opening handshake fields are constructed from an explicit caller-supplied **fresh native-OS-random nonce16** and a bounded unique case-sensitive subprotocol list. Request fields contain Upgrade, Connection, Sec-WebSocket-Key, Sec-WebSocket-Version 13 and optional Sec-WebSocket-Protocol. GET/target/Host/Origin/authentication and HTTP/1.1 syntax/version belong to HTTP/network code. `ClientHandshake::{new,key,expected_accept,request_fields,validate_response}` accepts typed `Header { name, value }` fields and status, avoiding a second HTTP parser. Only status 101 succeeds; required upgrade/token/accept fields, selected subprotocol membership, CR/LF/control injection, header byte/count limits and singleton duplicates are checked. Multiple Connection fields are merged as strict token lists. Upgrade must be one websocket token. Redirect/auth/version retries are never automatic.

SHA-1 is handwritten, private, and only used for the standard public handshake digest; [RFC 3174](https://www.rfc-editor.org/rfc/rfc3174) algorithm/vector provenance is recorded here. No SHA crate or managed helper is used. Production digest input is always exactly 60 public bytes. The private fixed-size Base64 helper exists because published bitcoin_encoding has no Base64 API; no Bitcoin hex/hash/address implementation was duplicated.

`Direction`, `Opcode`, `inspect_header`, `parse_frame`, `Frame`, `ParsedFrame`, and `encode_frame` enforce all six base opcodes, FIN/control restrictions, zero RSV bits, client masking/server unmasking, minimal 7/16/64-bit lengths, 63-bit size restriction, checked platform conversions, exact one-frame consumption, complete close fields, and text UTF-8. `Frame` borrows raw wire bytes; `payload_byte` and bounded `payload` expose unmasked bytes. Continuation context belongs to the stateful client. Incomplete input returns `None`. Declared sizes are rejected before payload allocation. Every client frame, including Pong and Close, requires an explicit mask4. The host must use fresh unpredictable native entropy; zero bytes can legitimately occur in OS randomness and this codec cannot authenticate entropy provenance.

`valid_close_code`, `close_payload`, and `CloseData` accept base wire codes, assigned 1012–1014 and application/private 3000–4999. Reserved/no-wire 1004/1005/1006/1015, unassigned standard codes, code-less nonempty reasons and malformed UTF-8 reasons fail. Server 1010 is rejected. The [IANA close-code registry](https://www.iana.org/assignments/websocket) was checked on 2026-10-02. New standard codes/extensions require an explicit reviewed update.

`Limits::default` uses **64 KiB frame, 1 MiB message and 1024 fragments**. `Limits::new` allows tighter host settings within hard ceilings 16 MiB frame, 64 MiB message and 65536 fragments. Zero-fragment flooding is bounded as well as payload bytes. Handshake caps are 128 fields / 16 KiB aggregate / 32 protocols / 128 bytes per protocol. The state machine retains at most one partial input frame, one fragmented message, one immutable output frame, one control reply and **no event queue**. There can be bounded temporary decoded payload/event copies. Host event queues/relay counts and read buffers need their own caps.

`Client::{new,handshake,accept_upgrade,negotiated,state,receive,queue_frame,queue_close,queue_pending_control,outgoing,advance_written,close_sent,close_received,pending_control,transport_eof,abort}` implement Connecting/Open/Closing/Closed/Failed/Aborted state. Incremental UTF-8 handles scalars split across fragments and rejects overlong forms, surrogates, out-of-range scalars and invalid/incomplete final continuations. Ping/Pong can interrupt fragments; Ping schedules an identical-payload Pong. A pending response applies zero-consumption backpressure until output is drained and the host supplies a fresh mask. Events return individually as Text/Binary/Ping/Pong/Close.

`outgoing` borrows an immutable unwritten suffix. `advance_written` acknowledges only successful bytes, rejects over-acknowledgment, and marks Close sent only after its whole frame is written. On peer close an unstarted data/control frame is discarded; a partially written frame must finish before the echo. Local/peer/simultaneous close races preserve wire framing and emit one close. Local closing suppresses application data delivery while validating incoming data and still answering Ping until peer Close. Both Close frames are required for clean EOF. Closed denotes the completed WebSocket close exchange; host shuts down the transport.

Inbound protocol/resource errors fail terminally. Local validation and backpressure errors preserve the session. Each mutating boundary takes a caller cancellation snapshot; cancellation discards work and enters Aborted. There are no detached threads, clocks, networking or retries. Host cancellation/deadlines must wake transport I/O, be checked between bounded protocol calls, and close transport after errors/abort. An ambiguous write failure must never replay a complete request. Debug and error formatting redact all peer payloads, reasons and protocol strings.

**All extensions, including permessage-deflate, are unsupported and rejected.** No extension is offered and every Sec-WebSocket-Extensions response field is rejected, including an empty field. Compression is not silently ignored. A future extension must integrate the actual first-party content/compression implementation before negotiation is enabled.

## Host operation proposal

Reserve **`0x0C00`–`0x0CFF`**; this checkpoint changes no shared dispatch. Native Rust callers should call directly. Suggested host actions: `0x0C00` create protocol state with native nonce and upgrade fields; `0x0C01` validate upgrade/attach authenticated network origin; `0x0C02` feed bounded received bytes and return exact consumption/event; `0x0C03` queue a bounded frame with host-native mask; `0x0C04` expose/acknowledge output and flush required control replies with native entropy; `0x0C05` orderly close; `0x0C06` cancel/abort; `0x0C07` EOF/status. Reserve `0x0C40` onward for the expanded typed Nostr service rather than exposing raw managed relay implementation.

Host protocol/session IDs must be session-owned, generation-safe and bounded. Network/TLS/proxy/isolation capability remains attached to the validated origin; no caller supplies an arbitrary native handle or alternate raw socket. HTTP's exact upgrade suffix must enter `receive` without being discarded. Secure connections must validate certificates/hostname normally. No TLS bypass, live relay or wallet secret was used in testing.

## Independent evidence

Native verification on **Windows x64**, Rust **1.99.0**, edition **2024**, 2026-10-02:

- Actual source compiled as public module with `-Dwarnings`; standalone Clippy `-Dwarnings -Dclippy::all`; rustfmt check passed.
- **30 debug and 30 optimized tests passed**, none ignored. Includes RFC literal handshake/Hello/masked/fragmented/Ping vectors; SHA-1 abc/long/million-a vectors; all 65536 two-byte UTF-8 pairs; all header opcode/RSV/FIN classes; every close-code value in both directions; canonical length boundaries through 65537 bytes; every prefix/split for selected frames; fragmented UTF-8/control interleaving; malformed text; EOF/cancellation; lossless backpressure/partial-write/close races; synthetic Nostr text and redacted diagnostics.
- Deterministic adversarial stream test iterates all 65536 two-byte headers through both stateless directions and stateful client bounds.
- **148072 independent Python comparisons passed**: 139172 parse cases, 6144 independent platform SHA-1/Base64 handshake nonce cases, 2134 encodings and 622 parsed response upgrades. Expected wire bytes are formed with independent struct/codecs/hashlib/base64 code, never the Rust encoder. Exhaustive header flags/mask/length classes, arbitrary prefixes, malformed shapes, UTF-8 and limits are included.
- Oracle input SHA256 `bb344892be85314f1369718a82c50c51f2449b8716455597cbe224b498803c08`; expected and actual output SHA256 both `9d82c5405f610071676d77bb80d211cc7760dc03888d26ea5b8d784a83aa7dba`.

Reproduce with `& .\mcw\tests\websocket_verify.ps1`. The script reuses the installed shared toolchain/linker/Python, checks 2 GiB free, takes one of two exclusive heavy-build slots, builds sequentially with one codegen job, and installs nothing. Evidence directory: `C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-websocket\.artifacts\websocket-verification` (`verification.json`, `oracle.json`, debug/optimized/oracle logs). Native non-Windows execution remains unverified. Equivalent rustc commands are in the verifier; adjust installed target/native linker paths only, without introducing crates.

Verified LF source SHA256 hashes:

| File | SHA256 |
| --- | --- |
| `mcw/src/websocket.rs` | `b7cd9b613b2a277f7fe3444410822749219db520d9dd5f4653be6142456c0c20` |
| `mcw/tests/websocket_conformance.rs` | `3db7bd50447a50c7b2733b6d555809418b285ff81a246748f8669945feb628a4` |
| `mcw/tests/websocket_oracle.rs` | `6d4ba1a6d0b3f9b4d27218b756fc2d19be1b33b2e0686f12824e53e934738666` |
| `mcw/tests/websocket_reference.py` | `dc07567f87f490c55e93dd94b9c17cac5485835c13840e83980595883a4d265d` |
| `mcw/tests/websocket_verify.ps1` | `d7da6dcbb3d4bc7933955dac1976c6fc54712ff68232d49a648717f28374b40d` |

The first-party module is governed by repository `LICENSE.md` (MIT). Algorithms were written from the specifications, not copied C/Rust implementation source. RFC numbers/byte vectors are attributed above; no third-party SDK/source or license-bearing runtime is incorporated. The Python oracle and Rust drivers are test tooling only.

## Remaining real caller/package graph

The remote checkpoint's managed paths remain unchanged; no package was removed:

| Role/caller | Still retained behavior |
| --- | --- |
| Wallet `Discoverability/NostrExtensions.cs` | NNostr factory and .NET ClientWebSocket proxy configuration; actual origin, HTTP/TLS/entropy/network integration remains. The latest remote wallet `Client/Global.cs` supplies no proxy; earlier shared working edits differ, so integrate current routing policy explicitly. |
| Wallet `WebClients/CompositeNostrClient.cs` | NNostr relay clients, reconnect/state/event handling, System.Interactive.Async merge. |
| Wallet `WebClients/MagicalCryptoWalletNostrClient.cs` | NIP19 author conversion, event ID/signature verification, subscription and update-release parsing/channel. |
| Wallet `Services/UpdateManager.cs` | NNostr factory, release selection/notification and updater lifecycle; download/signature verification uses additional network/crypto dependencies outside this wire layer. |
| Coordinator `Discoverability/CoordinatorAnnouncer.cs`, `AnnouncerConfig.cs` | NNostr signing/publishing/NIP19 private keys. External coordinator role must migrate or move to a coordinator-only project boundary before the wallet core can drop NNostr. |
| `Wallets/SilentPayment/NBitcoinExtensions.cs` | Inspect current retained role: source snapshots differed; the initial shared file imported NNostr, latest remote graph did not. Never infer repository removal from one snapshot. |
| Release publisher `Contrib/Releases/Publisher/Program.cs` / `.csproj` | NNostr signing and JSON announcement preparation retained as external tool; the inspected program does not connect to a relay. Audit separately from the one wallet executable. |
| Managed tests `UnitTests/WebClients/MagicalCryptoWalletNostrClientTests.cs`, `UnitTests/Services/{UpdateManagerTests,TestReleaseAuthor}.cs` | NNostr event/key/test transport types still retained; migrate to actual Rust typed service fixtures. |

`MagicalCryptoWallet/MagicalCryptoWallet.csproj` and publisher project still directly reference **NNostr.Client 0.0.55**. Core packages.lock.json records its **LibChaCha20 1.0.1, LinqKit.Core 1.2.5, NBitcoin.Secp256k1 3.1.6 and System.Interactive.Async 6.0.1** dependencies; these and other independent callers remain. System.Net.WebSockets and managed TLS belong to the retained .NET framework, not an added Rust dependency. Removing one call path does not establish whole package or transitive removal. Nostr ID/signature, filters/relay JSON, subscriptions/reconnect/discovery/update/publishing are the expanded track, not implemented by RFC 6455 alone.

Repeat inventory at integration:

```powershell
rg -n 'NNostr|NostrClient|NostrEvent|NIP19|ClientWebSocket|System.Net.WebSockets' --glob '*.cs' --glob '*.csproj' MagicalCryptoWallet MagicalCryptoWallet.Client MagicalCryptoWallet.Tests Contrib/Releases/Publisher
rg -n 'NNostr.Client|LibChaCha20|LinqKit.Core|NBitcoin.Secp256k1|System.Interactive.Async' Directory.Packages.props --glob 'packages.lock.json'
```

Bounded protocol integration still requires a real eligible caller, actual host entropy and transport boundaries, shared registration/dispatch, and compatibility/packaging verification. NNostr's current `Action<WebSocket>` configurator does not provide an honest isolated frame-codec cutover; a codec alone is not removal of that caller's managed WebSocket implementation. The revoked full native network/TLS/Nostr/crypto migrations are not current authorization. The eventual application still requires native **Windows x64, Linux x64/ARM64 and macOS x64/ARM64** acceptance; only Windows x64 component tests are verified here. No production readiness, full Nostr replacement or dependency-removal claim is made by this checkpoint.
