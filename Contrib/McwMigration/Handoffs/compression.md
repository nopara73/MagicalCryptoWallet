# Bounded HTTP content decoding

The accepted caller is **`MempoolSpace-bitcoin-fee-rate-provider`**, created by
`FeeRateProviders.MempoolSpaceAsync`. This work replaces only that small response
body's decoder. A historical native-host/retained-caller fixture passes with
test-local shared patches; its cancellation case stalls HTTP body acquisition.
Production registration, cancellation hooks and factory incorporation remain
with their owners; an unused leaf or a test-local patch is not a completed
production cutover.

Worker: `compression`, chat `01a0fc45-d443-7f93-aab1-3ba11b889a0e`.
Coordinator: `01a0fc1e-7c20-76d3-bf81-cb1f68c9adb7`.
Host/QR owner: `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc`.
Network/factory owner: `01a0fc46-15ed-7fe3-88dc-bba09fad5d45`.

Active isolated checkout:
`C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-content`.
The original `.artifacts\mcw-compression` draft and checkpoint evidence are
preserved. Original codec commit **`37e6da218cffe3055803e86f1f637b4cd690b1f2`**
was normally pushed to master and its remote ancestry verified. Exact current
leaf/document publication IDs and ancestry are recorded in the shared ignored
`handoffs/compression.json` and `.artifacts/compression-content/publication.json`.

The human revoked whole UI, Tor, storage and wallet crypto/network/CoinJoin/sync/
transaction/script rewrites. The later bounded assignment authorizes this one
response decoder and existing-host adapter. After 2026-10-03 01:00 Asia/Singapore
(2026-10-02 17:00 UTC), start no new assignment, subagent, fork, replacement task
or new dependency; finish, test, fix, publish and integrate only existing work.
No reminder or account-reset fact is inferred from that coordination cutoff.

## Owned implementation and retained behavior

Owned paths are `mcw/src/compression.rs`, `mcw/src/content_service/**`,
`mcw/tests/compression_*`, `MagicalCryptoWallet/Mcw/Content/**`, and this handoff.
QR retains lib/Cargo/app/bridge/platform/lifecycle/packaging. Network retains
factory and mechanical caller-interface changes. Prepared small shared patches
are applied only in an ignored test snapshot; coordinator requests QR
incorporation only while QR is idle. No active owner checkout is edited here.

The reviewed factory proposal caches the outer content handler for the exact named client,
with its existing transport as the inner handler. That transport alone gets
`AutomaticDecompression.None`; every other client retains `.All`. Existing
HttpClient, onion/direct routing, SOCKS identity credentials, TLS, retry,
expiration and cancellation behavior stay with the current transport. This
work introduces no general download, sink, network, socket or TLS service.

The handler privately reads one bounded encoded response, checks encoded
Content-Length against the received wire representation, and asks the existing
`IMcwApplicationServices` boundary to decode exactly once. No body escapes
until all layers validate. It removes Content-Encoding, sets decoded
Content-Length, preserves other content headers/charset, and retains encoded
length, original encoded Content-Length and per-layer proof metadata in
`McwDecodedHttpContent`. Original content is disposed after replacement;
response/content are disposed on every failure. Unsupported, corrupt,
truncated, trailing or oversized data fails visibly with no managed fallback.

`content_service::decode` handles ordered field values/comma lists, HTTP OWS,
ASCII case and x-gzip; it decodes declared layers in reverse order. HTTP
`deflate` means RFC1950 zlib, with no heuristic raw-DEFLATE retry. Each layer
must consume its complete input. Aggregate output, expansion, work and logical
allocation limits cannot reset between layers. Private intermediate buffers
are withheld on every error.

## Codec and bridge contract

The safe first-party RFC1951 engine handles stored/fixed/dynamic Huffman blocks,
canonical/repeat validation, overlapping and cross-block references, distances
through 32768, optional raw/zlib dictionaries, complete zlib/gzip checksums,
gzip optional headers/FHCRC, concatenated members and exact trailing-data
policy. Incremental `Decoder::process` reports exact consumed/written bytes and
NeedInput/NeedOutput/Finished; earlier streamed output is provisional until
Finished. Errors poison the decoder. Deterministic stored/fixed-LZ77 encoders,
CRC32 and Adler32 remain available for bounded consumers such as PNG.

The independently written RFC7932 Brotli decoder handles standard WBITS 10..24,
stored/compressed/metadata blocks, canonical simple/complex prefix trees,
block switching, all four literal contexts, RLE/MTF context maps, distance
cache/direct/postfix forms and all 121 normative dictionary transforms.
Nonstandard windows, invalid padding/codes/maps/distances/dictionary references,
lengths and trailing data fail without returning a body. Meta-block count has
its own typed limit.

Adapter operation **`0x0900`**, packet version **1**, is a single stateless
response decode using the existing <=1 MiB frame. It retains no body sessions
or sockets. Request is version:u16, timeout_ms:u16, fields:u8, reserved:u8=0,
body_len:u32, then each length:u16 + field bytes, then exactly body_len bytes.
Success returns version:u16, status:u8=0, encoded:u32, decoded:u32, layers:u8,
then each coding:u8/input:u32/consumed:u32/output:u32/work:u64 and body bytes.
Failure is a fixed 22-byte typed classification/layer/counter packet with no
plaintext. The managed leaf validates version, exact lengths, coding order,
consumption chain, counters and complete packet framing before exposing bytes.

| Selected flow limit | Value |
| --- | --- |
| Encoded and decoded body | 512 KiB each |
| Coding fields / decoding layers | 4 / 4 |
| Each field | 2048 bytes |
| Native dispatch deadline | 5000 ms |
| Expansion | ratio 200 plus 256 KiB finite slack |
| Native work | 16,000,000 units total |
| Logical decoder allocation | 4 MiB |
| Gzip header / members; Brotli meta-blocks | 16 KiB / 16; 256 |

The current host owner's bounded Inbox reads independently of dispatch, caps
queued requests at 256/16 MiB, prioritizes CANCEL and removes matching queued
work. The review-only `Inbox::is_interrupted(id, operation)` hook observes a
matching cancellation, terminal connection closure or poisoned state from the
decoder's existing checkpoint. The proposed dispatcher supplies this query to
the stateless adapter and checks host shutdown. It adds no duplicate request
registry. The previous sync_channel/32-ID Controls proposal is retired; preserved
historical snapshots retain the code and evidence they actually tested.

The 5000 ms bound starts after packet parsing, when native execution begins.
It excludes HTTP acquisition, frame transit and queue delay. The inbox/hook
component tests prove interruption during partial native decoding; real host
in-flight cancellation, EOF and saturated-queue acceptance remain unsatisfied
until the shared owner incorporates the hook and the incorporated host is tested.
Allocation counts requested live buffer capacity and codec storage, not allocator
bookkeeping, caller buffers, transient allocator copying or operating-system RSS.

Debug for dictionary policy, options and decoded Rust bodies prints lengths or
safe metadata, never payload bytes. The managed result/exception also prints only
safe metadata/classification. Actual bridge code does not log frames or bodies.

## Verification and normative assets

Run `mcw/tests/compression_verify.ps1` for the original codec regression and
`mcw/tests/compression_content_verify.ps1` for content/adapter checks. Both use
existing Rust 1.99.0, edition 2024, warnings denied, overflow checks, one codegen
unit, one test thread, synthetic inputs and independent reference oracles.
They hold one of two exclusive build-slot handles only during the run, require
at least 2 GiB free RAM, and release in finally before review/publication/waits.

- Original codec: **26 debug + 26 optimized tests**, **2651** independent Python
  zlib/gzip cases, including 657 truncations, 64 dictionary cases, fragmented
  input/output, cross-block flushing, encoding, concatenation and strict tail
  handling; 7500 additional bounded random decodes must not panic.
- Content: **26 debug + 26 optimized tests**, **6647** independent installed .NET
  10 Brotli/HTTP cases, including **2541** dictionary length/transform vectors,
  **32** hand-assembled context/RLE/MTF/block-switch vectors and **540** vectors
  covering qualities 0..11 and windows 10..24. Includes the actual HTTP/1 parser,
  synthetic TCP framing, private failure buffers, limits, cancellation, deadlines,
  stateless host checkpoints, redacted Debug and 3000 bounded random Brotli inputs.
- Managed component: **14 tests** using the actual leaf/interface and actual Rust
  payload decoder. These are component evidence, not an application-host claim.
- Actual host: **9 tests** using published host foundation
  `e64c080096a44614a1bfb4778782e8f3d3554c5b`, actual ManagedApplicationHost,
  real Core/retained fee caller/factory/retry/HTTP transport, and small test-local
  owner patches. A loopback SOCKS fixture verifies exact onion route, stream
  credentials, identity/gzip/zlib/br/layered bytes and decimal rates, typed checksum/
  unsupported failures, encoded/decoded metadata, and caller cancellation during
  HTTP body acquisition. It does not synchronize or cancel in-flight native decode.
  It never resolves/connects to the requested onion or public endpoint, starts
  no Tor/UI, and reads/writes no wallet. Native exit is **0**.

That historical Core/host snapshot compiled with **0 warnings, 0 errors**; its
native lib/bin passed Clippy with warnings denied. The actual Windows host used
`Contrib/Mcw/build.py` and its matching rebuilt standard library/native OS runtime.
PE audit reports OS imports only: KERNEL32, API-MS-WIN-CORE-SYNCH, KERNELBASE,
NTDLL and SHELL32. This is Windows x64 fixture evidence; other target/runtime
and complete product-release acceptance remain with the host owner. These facts
are bound to the recorded historical sources and binary, not current master.

`compression_content_inbox_verify.ps1 -ReviewRoot <prepared-review>` compiles exact
copied host Inbox/Frame/QR sources, the one review-only query, and the actual owned
adapter/codecs. It holds the same build-slot guard and writes fresh source-hashed
evidence. **14 debug + 14 optimized component tests** pass, including seven new
synchronized cases: active cancellation with 256 queued requests, queued cancel
removal/deduplication and sibling success, ID/operation isolation, saturated EOF,
malformed/truncated frame closure, request overload and control overload. A decoder
thread pauses at checkpoint 20, ingress is acknowledged before resume, and each
interruption requires nonzero partial-output counters below the complete size and
only the 22-byte typed failure packet. No sleep determines when work has started.
This test uses a synthetic loopback stream and never runs the real application
host; its passing results do not satisfy the real-host incorporation gates.

Evidence is under `.artifacts/compression/`, `.artifacts/compression-content/`,
and `.artifacts/compression-content/actual-host/snapshot-e/.artifacts/`.
Synchronized proof and reviewed patches are under
`.artifacts/compression-content/inbox-review-a/`; old evidence is preserved.
JSON records contain exact before/after source hashes, corpus digests, source
baseline, test-local shared patch hashes and native binary SHA256.
The Brotli corpus SHA256 is
`ba3f37cee62ce6bcfef67944749703a3329ca5e693fe920d984ea34dc651b48e`.

Normative RFC7932 dictionary: 122784 bytes, CRC32 `5136cb04`, SHA256
`20e42eb1b511c21806d4d227d07e5dd06877d8ce7b3a817f378f313653f35c70`.
Context LUT CRCs are `8e91efb7`, `d01a32f4`, `0dd7a0d6`; the 121 transforms
serialize to 648 bytes with CRC `3d965f81`. Data provenance, source/asset hashes
and required Simplified BSD RFC data license are retained alongside the assets.
Algorithms are independently written first-party MIT code; reference runtimes
are verification tools only. The owned data directory marks dictionary.bin
binary because its CRLF bytes and initially text-like prefix require exact Git
preservation. Publication verifies the staged dictionary blob SHA256 directly.

Primary specifications: [RFC1950](https://www.rfc-editor.org/rfc/rfc1950),
[RFC1951](https://www.rfc-editor.org/rfc/rfc1951),
[RFC1952](https://www.rfc-editor.org/rfc/rfc1952),
[RFC7932](https://www.rfc-editor.org/rfc/rfc7932).

## Integration acceptance and remaining dependencies

`compression_content_host_patch.py --repo <owned> --host-repo <read-only-host>
--out <fresh-owned-artifact>` generates reviewed Inbox/registration/dispatcher
and network/factory hunks plus exact originals, source roots and SHA256. It
refuses the old blocking-reader contract and never edits active owner files.
Coordinator delivers host incorporation only when QR is idle;
network applies its own factory hunks after leaf publication and host dispatch.
Run `compression_content_host_prepare.py --integrated` against exact incorporated
master after real registration, the interruption hook and selected factory
activation. It applies no local shared patches and must be followed by the actual
host verifier. Its nine retained-caller cases cover body acquisition cancellation;
meaningful synchronized native in-flight cancellation/EOF/queue proof through that
incorporated host is a separate unsatisfied gate. Prepared/tested hunks alone are
pending. The queued factory baseline differs from the tested historical baseline.

Other response clients keep managed AutomaticDecompression, including coordinator,
exchange, CPFP, broadcaster, other fee providers and installer streams.
System.Net.Http/TLS and the managed runtime remain. Microsoft.Extensions.Http
contract/package removal belongs to the separate bounded network assignment.
Managed JSON, storage, Tor, wallet/crypto, CoinJoin and UI dependencies are not
removed by this decoder. PNG/scanner ownership is separate and reuses the
published zlib engine with strict size/checksum/trailing/dictionary bounds.

For publication, hold exclusive FileShare.None on the shared git-publish.lock,
check shared/own indexes empty, stage only owned paths, commit briefly, reconcile
concurrent master in this isolated checkout, push normally to master and verify
remote ancestry/ref. Preserve peer edits, all prior drafts, processes and evidence;
never reset/stash another agent, switch the shared checkout or force-push.
