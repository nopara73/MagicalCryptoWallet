# HTTP content-decoding workstream

Status: **active workstream; verified DEFLATE/zlib/gzip checkpoint available for
integration**, 2026-10-02, Asia/Singapore. The expanded assignment includes
first-party Brotli and the application's decoded HTTP response path. This
checkpoint does not remove System.Net.Http, Microsoft.Extensions.Http, or any
managed runtime dependency. Production response decoding remains managed until
the HTTP and application-host owners integrate and verify the service/adapters.

Worker: `compression`, thread `01a0fc45-d443-7f93-aab1-3ba11b889a0e`.
Coordinator: `01a0fc1e-7c20-76d3-bf81-cb1f68c9adb7`.
Application/QR host owner: `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc`.
HTTP/network owner: `01a0fc46-15ed-7fe3-88dc-bba09fad5d45`.

## Published checkpoint and ownership

Implementation commit: **`37e6da218cffe3055803e86f1f637b4cd690b1f2`**, normally
pushed directly to `origin/master`; subsequent fetch plus
`git merge-base --is-ancestor` and `git ls-remote` verified publication. The
implementation contains exactly these owned files:

- `mcw/src/compression.rs`
- `mcw/tests/compression_conformance.rs`
- `mcw/tests/compression_reference.py`
- `mcw/tests/compression_verify.ps1`

This document is published separately, so its own commit does not create a
self-referential implementation ID. Exact document publication ID is recorded
in the shared ignored `handoffs/compression.json`.

Isolated checkout/evidence root:
`C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-compression`.
Publication held exclusive `FileShare.None` on the shared
`.artifacts/git-publish.lock`, checked both shared/own indexes were empty and
staged only assigned paths. No shared checkout, active QR checkout, wallet,
key, live process, shared Git configuration, global runtime, or peer edit was
modified. Remote advances were fast-forwarded into this isolated checkout.

Expanded ownership declared to QR and HTTP owner before adapter/caller edits:
`mcw/src/content_service/**` and `MagicalCryptoWallet/Mcw/Content/**`, in
addition to the paths above. HTTP/network owner retains transport caller edits
under WebClients. QR retains Cargo manifests, lib/main/CLI/bridge dispatcher,
native/platform bindings, lifecycle, packaging and shared ledger. No second
Cargo package or shipping executable is added. Shared integration requests are
sent by coordinator only when QR is idle.

## Codec API and behavior

The integrator registers `pub mod compression` in the existing mcw crate.
Until then, tests compile the actual assigned source directly using ignored
rustc harnesses. `#![forbid(unsafe_code)]` is enforced; the sole import is
`std::fmt`. There are no external crates, native compression libraries, shell
calls, dynamic library loads, runtime helpers, or managed/IPC/OS-handle types.

| API | Contract |
| --- | --- |
| `Decoder::new(DecodeOptions)` | Validate policy/limits, seed explicit raw/zlib dictionary and allocate one bounded 32 KiB history. |
| `Decoder::process(input, output, end_of_input)` | Incremental parse; return exact call-local `consumed`, `written` and `NeedInput`, `NeedOutput` or `Finished`. |
| `Decoder::{total_input,total_output,total_work,members,allocated_bytes}` | Cumulative counters, including consumed bit-buffer bytes and logical allocation capacity. |
| `decode(input, options)` | Bounded collecting API; return `Decoded {bytes, consumed, members}` only after complete validation. |
| `encode(input, EncodeOptions)` | Valid bounded stored or fixed-Huffman/LZ77 streams, with zlib/gzip framing as requested. |
| `crc32`, `adler32`, `Crc32`, `Adler32` | First-party IEEE CRC-32 and Adler-32, whole-slice and incremental APIs. |

Decoder accepts RFC1951 stored, fixed and dynamic Huffman blocks, cross-block
and overlapping references, all distances through 32768 and canonical codes
through 15 bits. It validates stored LEN/NLEN, HLIT bounds, complete
code-length trees, oversubscribed/incomplete literal/distance trees (the
specified single-symbol exceptions), EOB, repeat bounds, reserved symbols and
available dictionary/history. HDIST permits the RFC's 32-symbol alphabet;
reserved distance symbols 30/31 may be defined but cannot be used.

Zlib checks CM, CINFO, FCHECK, dictionary ID and final Adler-32; its advertised
window limits distances even when a larger dictionary is supplied. Dictionary
ID covers the full dictionary; history retains only its last window. A
dictionary supplied for a zlib stream without FDICT is not used. Gzip rejects
preset dictionaries, validates fixed/reserved header fields, skips bounded
FEXTRA/FNAME/FCOMMENT without interpreting names, checks optional FHCRC, member
CRC-32 and ISIZE, and resets history/checksums between concatenated members.

`TrailingData::Reject` is default and requires EOF confirmation. `Allow`
returns exact format-boundary consumption, without crediting suffix bytes to
expansion limits. Gzip defaults to concatenation; `GzipMembers::First` is an
explicit single-member policy. A next member beginning with gzip magic must
validate completely; a lone 0x1f at EOF is truncation, including in Allow mode.
No padding bytes are silently ignored. DEFLATE's final residual bits are
ignored as specified, without fetching the next byte.

The caller must retain and resend every unconsumed suffix. At a split next
gzip magic, NeedInput can consume zero bytes. `end_of_input` means the provided
slice includes all remaining input, and stays true on NeedOutput retries.
Output is provisional until Finished; checksum/limit/truncation/trailing
errors can invalidate earlier output. An error poisons the decoder and is
returned unchanged thereafter. `Error` contains typed cause, input/output
counters and next logical byte/bit offset. Counter deltas also identify
progress preceding an error.

EncodeMethod::Stored emits <=65535-byte stored blocks; Fixed uses first-party
greedy LZ77, a 4096-entry single-candidate hash table, distances <=32768 and
matches <=258. It produces actual compression for repetitive inputs, bounds
pathological search work, and emits a fixed tree rather than optimizing a
dynamic tree. Both methods are valid in all three wrappers. Gzip metadata is
deterministic (MTIME=0, OS=255); encoder never emits a preset dictionary.

## Limits and acceptance semantics

All stream counters are cumulative and independent of input/output chunking.
Limits cover consumed input, emitted output, expansion ratio plus finite slack,
work, requested allocation capacity, dictionary length, header bytes/member
and gzip member count. Expansion uses physically consumed input and is checked
both globally and per member; a previous gzip member cannot subsidize a bomb.
Input limits count the logical compressed stream, excluding unconsumed tails.
Limit errors occur before the violating output byte is emitted.

Work charges input bytes, parsed bits, tree construction, repeat fills,
output/checksum operations and potential reallocation copying. Fixed-size
object/history initialization is bounded separately. Allocation limits charge
decoder object/history, result capacity, and encoder fixed workspace. Caller
buffers, allocator bookkeeping and transient reallocation storage are outside
that accounting; it is not an operating-system RSS limit. All dynamic reserve
operations are fallible. Collecting/encoding buffers grow geometrically with
budget checks. Streaming decoder retains no whole compressed/plaintext stream
or gzip metadata.

Default decoding limits: 16 MiB compressed, 64 MiB expanded, ratio 200 plus
1 MiB slack, 1 billion work units, 65 MiB requested allocation, 1 MiB dictionary,
128 KiB gzip header/member and 1024 members. HTTP service must choose explicit
per-route and aggregate budgets, preserve encoded Content-Length semantics,
reject invalid data visibly, and connect cancellation to bounded processing
slices. Codec default limits are not a claim of completed application policy.

## Independent verification and provenance

From this isolated checkout run:

```powershell
& .\mcw\tests\compression_verify.ps1
```

The script acquires one of the two exclusive build-slot handles, requires
>=2 GiB free memory, uses existing Rust **1.99.0** / edition **2024**, MSVC/SDK
and Python, compiles with warnings denied, overflow checks and one codegen
unit, and tests sequentially. Temporary harness executables are ignored tools,
not application deliverables. Existing native CRT/static-CRT test settings are
not a compression dependency or a packaging claim.

Final verified evidence:

- `.artifacts/compression/verification.json`: exact source hashes and run records.
- `.artifacts/compression/debug.log`: **26 passed, 0 failed**.
- `.artifacts/compression/optimized.log`: **26 passed, 0 failed**.
- `.artifacts/compression/differential.json`: **2651 passed** independent cases.
- `.artifacts/compression/reference-fixtures.jsonl`: **2651 records**, 37,375,311 bytes; deterministic request/expected-output hashes.
- `.artifacts/compression/implementation-publication.json`: exact commit and remote ancestry proof.

Python 3.14.7 stdlib `zlib` and `gzip` (zlib runtime `1.3.1.zlib-ng`) are
independent verification tools only. The corpus contains 110 synthetic
payloads, 672 Python-produced decode cases, 168 fragmented-stream cases,
660 Rust-to-Python encode cases, 64 dictionary cases, 748 invalid cases
(including 657 byte truncations), 12 concatenation/first-member cases,
27 allowed trailing-data cases and 300 random mutations. First-block coverage:
327 stored, 201 fixed and 132 dynamic streams; synchronous/full flush cases
also cross block boundaries. No private wallet material is used.

Rust tests independently build RFC1951's XY overlap example, empty/stored
streams, dynamic repeats crossing the literal/distance boundary, maximum
15-bit codes and 32 KiB distance, CRC/Adler vectors, header/trailer failures,
precise consumption, chunk-invariant work, zero output buffers, and bomb limits.
7500 additional bounded arbitrary-input decodes must not panic.

| Source/evidence | SHA-256 |
| --- | --- |
| `mcw/src/compression.rs` | `6797f3d2b1548a7a6bf78dbae2a3786fdbeb99aceed97af4ea3f47ec46bd99b8` |
| `mcw/tests/compression_conformance.rs` | `752b9b0487460a9410b261c1333533bff7a62a2c3e12f1c0a124bd9137473f5e` |
| `mcw/tests/compression_reference.py` | `79b32506f93833f9babbd49af1628f7a1eea4e54782b250eea268e8ba4dde4ca` |
| `reference-fixtures.jsonl` | `39235849470251bab2285aad941e9cc101ccfddf3bf6509bff878914b867d9ac` |
| Differential output digest | `6b702aa1d701cc89b4f3d25fecebc1e968a5abe2263c681629f99aae1bab5415` |

Implementation is independently written under repository `LICENSE.md` (MIT).
Format tables are normative protocol data. No third-party implementation or
third-party fixture source was copied. Primary specifications read directly:
[RFC1950](https://www.rfc-editor.org/rfc/rfc1950),
[RFC1951](https://www.rfc-editor.org/rfc/rfc1951),
[RFC1952](https://www.rfc-editor.org/rfc/rfc1952).
Expanded Brotli work uses [RFC7932](https://www.rfc-editor.org/rfc/rfc7932);
its normative static dictionary/tables will retain their separate provenance,
license and independently checked format CRCs.

## Concrete remaining managed callers and dependencies

Audit at the checkpoint's current remote baseline
`1228c2c589333a49a9af894da9876518739fab76`:

| Caller / surface | Remaining execution |
| --- | --- |
| `MagicalCryptoWallet/WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs:73` | `HttpClientHandler.AutomaticDecompression = DecompressionMethods.All`; actual gzip/deflate/Brotli execution still managed. Onion/direct/coordinator factories derive from this path. |
| `MagicalCryptoWallet/WabiSabi/Client/WabiSabiHttpApiClient.cs:59` | Coordinator HTTP responses -> ReadAsStringAsync -> managed JSON/WabiSabi callers. |
| `MagicalCryptoWallet/FeeRateEstimation/FeeRateProviders.cs:84` | Fee-provider HTTP responses -> ReadAsStringAsync. |
| `MagicalCryptoWallet/Wallets/Exchange/ExchangeRateProvider.cs:65` | Exchange-provider HTTP responses -> ReadAsStringAsync. |
| `MagicalCryptoWallet/Wallets/CpfpInfoProvider.cs:213` | Mempool response SendAsync/ReadAsStringAsync. |
| `MagicalCryptoWallet/Blockchain/TransactionBroadcasting/TransactionBroadcaster.cs:102` | External broadcaster HTTP response path. |
| `MagicalCryptoWallet/Services/UpdateManager.cs:223` | Installer download response -> ReadAsStreamAsync. |
| `MagicalCryptoWallet.Client/Global.cs:90,334,623,657` | Factory construction/ownership, Tor-status and coordinator setup; remains with client/network owner. |
| `MagicalCryptoWallet/MagicalCryptoWallet.csproj:28`, `Directory.Packages.props:14`, `MagicalCryptoWallet/packages.lock.json:53`, `deps.json:338` | Microsoft.Extensions.Http remains referenced/resolved. |
| `Directory.Build.props:9` | net10.0 managed runtime remains. System.Net.Http and its managed/native codec implementations are runtime dependencies, even without a dedicated NuGet PackageReference. |

Repository-wide managed-source search found the one AutomaticDecompression
factory and no direct GZipStream/ZLibStream/DeflateStream/BrotliStream caller
at this snapshot. HTTP packages also own transport/TLS, retries, response
framing and other responsibilities; a compression module alone removes none
of them. Skia/PNG is separate and is neither edited nor claimed removed here.

## Concrete integration boundary and remaining acceptance

Reserved operation proposal **0x0900-0x09FF**; allocation of exact operations
and payloads belongs to QR's existing bounded/correlated frame contract. Domain
codec/service API remains independent of bridge frames and network state.
The current 1 MiB frame ceiling requires chunked service requests/results for
larger bodies, with session/aggregate limits and cancellation. A single whole
response in one frame is not sufficient production integration.

For the expanded workstream to finish, Brotli's required prefix codes,
contexts, static dictionary/transforms and window behavior must be contained
first-party Rust; the concrete Content adapter and HTTP response path must
call it and this engine; old managed decompression must be disabled on migrated
flows. Test actual HTTP fixtures and synthetic servers through the production
response adapter, negotiated encodings, corruption/truncation, bounds,
cancellation, encoded Content-Length and visible errors. There is no silent
managed fallback for an unsupported codec. Networking/Tor/TLS state ownership
stays with HTTP/native service owners.

Required host acceptance remains module registration, service dispatcher,
managed adapter/caller wiring, capability checks, error/cancellation/lifecycle
coverage, packaged dependency audits and builds/native runtime tests for all
five desktop targets: Windows x64, Linux x64/ARM64, macOS x64/ARM64. Only the
Windows x64 standalone codec harness is verified here. Once those flows are
migrated, every remaining upstream caller and packaged reference must be
audited before final package/runtime removal. This document does not claim
native five-target execution, application HTTP decoding migration, Brotli
completion, or a production wallet release.
